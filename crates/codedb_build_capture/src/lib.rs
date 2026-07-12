#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::{Read, Write};
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

pub const STATUS: &str = "unsafe_build_capture_gate_available";
pub const UNSAFE_FLAG: &str = "--unsafe-execute-build";

pub type Row = BTreeMap<String, String>;

/// A caller-opened OUT_DIR reproduction root. All later traversal and
/// publication stays bound to this descriptor.
pub struct ReproductionRoot {
    directory: File,
    display: PathBuf,
}

/// Unforgeable entry token held only by this crate's trusted frontdoor.
///
/// The private field is the seal: external library callers cannot construct
/// this value and therefore cannot ask the library to self-issue authority.
pub struct TrustedExecutionFrontdoor(());

impl ReproductionRoot {
    pub fn open_existing(path: &Path) -> io::Result<Self> {
        Ok(Self {
            directory: open_directory_nofollow(path)?,
            display: path.to_path_buf(),
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct BuildCaptureRequest {
    pub repo_path: PathBuf,
    pub store_path: Option<PathBuf>,
    pub raw_log_path: PathBuf,
    pub unsafe_execute_build: bool,
    pub approver: Option<String>,
    pub task_id: Option<String>,
    pub before_state: Option<String>,
    pub cleanup_plan: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct BuildCaptureOutcome {
    pub status: BuildCaptureStatus,
    pub unsafe_execution_approval: Vec<Row>,
    pub build_script_runs: Vec<Row>,
    pub build_script_env: Vec<Row>,
    pub build_script_stdout: Vec<Row>,
    pub build_script_stderr: Vec<Row>,
    pub build_script_cargo_instructions: Vec<Row>,
    pub proc_macro_invocations: Vec<Row>,
    pub proc_macro_input_token_streams: Vec<Row>,
    pub proc_macro_output_token_streams: Vec<Row>,
    pub native_link_facts: Vec<Row>,
    pub out_dir_artifacts: Vec<Row>,
    pub toolchain_provenance: Vec<Row>,
    pub validation_errors: Vec<Row>,
    pub capture_gaps: Vec<Row>,
    pub raw_log_paths: Vec<Row>,
}

/// Host-held authority for minting request-bound execution capabilities.
///
/// The authority must stay in the trusted frontdoor. Approval-shaped request
/// fields are deliberately insufficient to invoke the dynamic runner.
struct ExecutionApprovalAuthority {
    secret: [u8; 32],
    authority_id: String,
}

/// Opaque, single-use proof that a trusted authority approved one exact
/// [`BuildCaptureRequest`].
struct ExecutionApprovalCapability {
    authority_id: String,
    request_digest: String,
    nonce: [u8; 32],
    authenticator: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCaptureStatus {
    Refused,
    ApprovedScaffold,
    Captured,
    Failed,
}

impl BuildCaptureStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "refused",
            Self::ApprovedScaffold => "approved_scaffold",
            Self::Captured => "captured",
            Self::Failed => "failed",
        }
    }
}

pub enum BuildCaptureError {
    CreateLogDir { path: PathBuf, source: io::Error },
    WriteLog { path: PathBuf, source: io::Error },
    SpawnCargo { path: PathBuf, source: io::Error },
    DisallowedEnvironment { key: String },
    AmbiguousEnvironment { reason: &'static str },
    ApprovalCapability { reason: &'static str },
    SandboxUnavailable { reason: String },
    PrepareSandbox { path: PathBuf, source: io::Error },
}

impl Debug for BuildCaptureRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildCaptureRequest")
            .field(
                "repo_path",
                &redact_text(&self.repo_path.display().to_string()),
            )
            .field(
                "store_path",
                &self
                    .store_path
                    .as_ref()
                    .map(|path| redact_text(&path.display().to_string())),
            )
            .field(
                "raw_log_path",
                &redact_text(&self.raw_log_path.display().to_string()),
            )
            .field("unsafe_execute_build", &self.unsafe_execute_build)
            .field("approver", &self.approver.as_deref().map(redact_text))
            .field("task_id", &self.task_id.as_deref().map(redact_text))
            .field(
                "before_state",
                &self.before_state.as_deref().map(redact_text),
            )
            .field(
                "cleanup_plan",
                &self.cleanup_plan.as_deref().map(redact_text),
            )
            .finish()
    }
}

impl Debug for BuildCaptureOutcome {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildCaptureOutcome")
            .field("status", &self.status)
            .field(
                "unsafe_execution_approval_rows",
                &self.unsafe_execution_approval.len(),
            )
            .field("build_script_run_rows", &self.build_script_runs.len())
            .field("build_script_env_rows", &self.build_script_env.len())
            .field("build_script_stdout_rows", &self.build_script_stdout.len())
            .field("build_script_stderr_rows", &self.build_script_stderr.len())
            .field(
                "build_script_cargo_instruction_rows",
                &self.build_script_cargo_instructions.len(),
            )
            .field(
                "proc_macro_invocation_rows",
                &self.proc_macro_invocations.len(),
            )
            .field(
                "proc_macro_input_rows",
                &self.proc_macro_input_token_streams.len(),
            )
            .field(
                "proc_macro_output_rows",
                &self.proc_macro_output_token_streams.len(),
            )
            .field("native_link_fact_rows", &self.native_link_facts.len())
            .field("out_dir_artifact_rows", &self.out_dir_artifacts.len())
            .field(
                "toolchain_provenance_rows",
                &self.toolchain_provenance.len(),
            )
            .field("validation_error_rows", &self.validation_errors.len())
            .field("capture_gap_rows", &self.capture_gaps.len())
            .field("raw_log_path_rows", &self.raw_log_paths.len())
            .finish()
    }
}

impl Debug for BuildCaptureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("BuildCaptureError")
            .field(&self.to_string())
            .finish()
    }
}

impl Display for BuildCaptureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateLogDir { path, source } => {
                write!(
                    f,
                    "failed to create log directory {}: {}",
                    redact_text(&path.display().to_string()),
                    io_error_summary(source)
                )
            }
            Self::WriteLog { path, source } => {
                write!(
                    f,
                    "failed to write raw capture log {}: {}",
                    redact_text(&path.display().to_string()),
                    io_error_summary(source)
                )
            }
            Self::SpawnCargo { path, source } => {
                write!(
                    f,
                    "failed to run cargo check in {}: {}",
                    redact_text(&path.display().to_string()),
                    io_error_summary(source)
                )
            }
            Self::DisallowedEnvironment { key } => {
                write!(
                    f,
                    "approved build capture environment key is not allowlisted: {}",
                    redact_text(key)
                )
            }
            Self::AmbiguousEnvironment { reason } => {
                write!(f, "approved build capture environment rejected: {reason}")
            }
            Self::ApprovalCapability { reason } => {
                write!(f, "execution approval capability rejected: {reason}")
            }
            Self::SandboxUnavailable { reason } => {
                write!(
                    f,
                    "mandatory Linux sandbox unavailable: {}",
                    redact_text(reason)
                )
            }
            Self::PrepareSandbox { path, source } => {
                write!(
                    f,
                    "failed to prepare mandatory sandbox at {}: {}",
                    redact_text(&path.display().to_string()),
                    io_error_summary(source)
                )
            }
        }
    }
}

impl StdError for BuildCaptureError {}

fn io_error_summary(error: &io::Error) -> String {
    format!("[io-error kind={:?}]", error.kind())
}

#[derive(Clone)]
struct BuildScriptObservation {
    package_id: String,
    out_dir: Option<PathBuf>,
    environment: Vec<(String, String)>,
    linked_libs: Vec<String>,
    linked_paths: Vec<String>,
}

#[derive(Clone)]
struct CapturedStream {
    package_id: String,
    out_dir: PathBuf,
    stream: &'static str,
    source_path: PathBuf,
    raw: String,
    redacted: String,
}

#[derive(Debug, Default)]
struct ProcMacroEvidence {
    invocations: Vec<Row>,
    inputs: Vec<Row>,
    outputs: Vec<Row>,
    log_summary: Vec<String>,
}

struct SandboxPlan {
    executable: PathBuf,
    cargo: PathBuf,
    rustc: PathBuf,
    rustdoc: PathBuf,
    linker: PathBuf,
    scratch_root: PathBuf,
    guest_source: PathBuf,
    guest_target: PathBuf,
    host_proc_macro_log: PathBuf,
    command_args: Vec<OsString>,
    environment_keys: Vec<String>,
    cleanup_armed: bool,
}

struct SandboxScratchGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for SandboxPlan {
    fn drop(&mut self) {
        if self.cleanup_armed {
            let _ = fs::remove_dir_all(&self.scratch_root);
        }
    }
}

impl SandboxScratchGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SandboxScratchGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

impl ExecutionApprovalAuthority {
    /// Create a new in-memory authority from kernel randomness.
    ///
    /// Callers must create and retain this object in the trusted approval
    /// frontdoor rather than constructing it from request/CLI data.
    fn new() -> Result<Self, BuildCaptureError> {
        let secret = kernel_random_32()?;
        Ok(Self {
            authority_id: sha256_hex(&secret),
            secret,
        })
    }

    /// Mint an opaque capability bound to every execution-relevant request
    /// field. Moving the capability into the runner makes it single-use.
    fn approve(
        &self,
        request: &BuildCaptureRequest,
        environment: &[(String, String)],
        sandbox: &SandboxPlan,
    ) -> Result<ExecutionApprovalCapability, BuildCaptureError> {
        if !request.unsafe_execute_build {
            return Err(BuildCaptureError::ApprovalCapability {
                reason: "unsafe execution was not explicitly selected",
            });
        }
        if !has_named_approver(request) || !has_complete_approval_provenance(request) {
            return Err(BuildCaptureError::ApprovalCapability {
                reason: "operator approval provenance is incomplete",
            });
        }
        let request_digest = execution_approval_digest(request, environment, sandbox);
        let nonce = kernel_random_32()?;
        let authenticator = capability_authenticator(&self.secret, &request_digest, &nonce);
        Ok(ExecutionApprovalCapability {
            authority_id: self.authority_id.clone(),
            request_digest,
            nonce,
            authenticator,
        })
    }
}

pub fn capture_build(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    if !request.unsafe_execute_build {
        return refused_capture(request);
    }
    if !has_named_approver(&request) {
        return missing_approval_capture(request);
    }
    if !has_complete_approval_provenance(&request) {
        return incomplete_approval_capture(request);
    }

    capability_required_capture(request)
}

pub fn capture_approved_fixture_build(
    request: BuildCaptureRequest,
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    capture_approved_fixture_build_with_env(request, &[])
}

pub fn capture_approved_fixture_build_with_env(
    request: BuildCaptureRequest,
    _environment: &[(&str, &str)],
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    Ok(capability_required_capture(request))
}

/// Sealed trusted-frontdoor implementation.
///
/// Library callers can submit approval-shaped data to the refusal-only public
/// API, but they cannot construct an execution authority or mint a capability:
///
/// ```compile_fail
/// use codedb_build_capture::ExecutionApprovalAuthority;
///
/// let authority = ExecutionApprovalAuthority::new().unwrap();
/// ```
fn capture_approved_fixture_build_with_capability(
    authority: &ExecutionApprovalAuthority,
    capability: ExecutionApprovalCapability,
    request: BuildCaptureRequest,
    environment: &[(&str, &str)],
    mut sandbox: SandboxPlan,
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    if !request.unsafe_execute_build {
        return Ok(refused_capture(request));
    }
    if !has_named_approver(&request) {
        return Ok(missing_approval_capture(request));
    }
    if !has_complete_approval_provenance(&request) {
        return Ok(incomplete_approval_capture(request));
    }
    let normalized_environment = normalize_approved_environment(environment)?;
    validate_execution_capability(
        authority,
        &capability,
        &request,
        &normalized_environment,
        &sandbox,
    )?;
    let log_root = ReproductionRoot::open_existing(&request.repo_path).map_err(|source| {
        BuildCaptureError::CreateLogDir {
            path: request.repo_path.clone(),
            source,
        }
    })?;
    let target_dir = sandbox.scratch_root.join("target");
    let mut command = sandbox_command(&sandbox, environment);
    let output = command
        .output()
        .map_err(|source| BuildCaptureError::SpawnCargo {
            path: request.repo_path.clone(),
            source,
        })?;

    let status = if output.status.success() {
        BuildCaptureStatus::Captured
    } else {
        BuildCaptureStatus::Failed
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut observations = build_script_observations_from_cargo_json(&stdout);
    for observation in &mut observations {
        let Some(out_dir) = observation.out_dir.as_ref() else {
            continue;
        };
        let Ok(relative) = out_dir.strip_prefix(&sandbox.guest_target) else {
            observation.out_dir = None;
            continue;
        };
        observation.out_dir = Some(target_dir.join(relative));
    }
    let streams = capture_build_script_streams(&observations);
    let mut build_script_env = build_script_env_rows(&observations, &request);
    build_script_env.extend(approved_environment_rows(environment, &request));
    let build_script_cargo_instructions = build_script_instruction_rows(&streams, &request);
    let native_link_facts = native_link_facts_from_observations_and_instructions(
        &observations,
        &build_script_cargo_instructions,
        &request,
    );
    let mut validation_errors = Vec::new();
    let mut out_dir_artifacts = Vec::new();
    let mut out_dir_capture_failed = false;
    for observation in &observations {
        let Some(out_dir) = observation.out_dir.as_deref() else {
            continue;
        };
        match capture_out_dir_artifacts(out_dir, &observation.package_id, &request) {
            Ok(mut artifacts) => out_dir_artifacts.append(&mut artifacts),
            Err(source) => {
                out_dir_capture_failed = true;
                validation_errors.push(row([
                    ("table", "validation_errors".to_string()),
                    ("code", "out_dir_artifact_capture_failed".to_string()),
                    ("package_id", observation.package_id.clone()),
                    ("out_dir", out_dir.display().to_string()),
                    ("message", redact_text(&source.to_string())),
                ]));
            }
        }
    }
    let sandbox_proc_macro_environment = environment
        .iter()
        .map(|(key, value)| {
            if *key == "CODEDB_PROC_MACRO_LOG_PATH" {
                (
                    *key,
                    sandbox.host_proc_macro_log.to_string_lossy().into_owned(),
                )
            } else {
                (*key, (*value).to_string())
            }
        })
        .collect::<Vec<_>>();
    let sandbox_proc_macro_environment = sandbox_proc_macro_environment
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect::<Vec<_>>();
    let proc_macro_evidence =
        capture_proc_macro_evidence(&sandbox_proc_macro_environment, &request);
    write_redacted_raw_log(
        &log_root,
        &request.raw_log_path,
        &request,
        &output,
        &streams,
        &proc_macro_evidence.log_summary,
    )?;

    let mut capture_gaps = Vec::new();
    if observations.is_empty() {
        capture_gaps.push(build_script_gap(&request));
    }
    if proc_macro_evidence.invocations.is_empty() {
        capture_gaps.push(proc_macro_gap(&request));
    }
    if observations.is_empty()
        || out_dir_capture_failed
        || observations
            .iter()
            .any(|observation| observation.out_dir.is_none())
    {
        capture_gaps.push(out_dir_artifact_gap(&request));
    }
    if native_link_facts.is_empty() {
        capture_gaps.push(native_link_gap(&request));
    }
    let observed_warning = stdout.contains("cargo:warning=")
        || stderr.contains("cargo:warning=")
        || stdout.contains("warning: codedb-")
        || stderr.contains("warning: codedb-")
        || stdout.contains("build-script-")
        || stderr.contains("build-script-");

    let mut outcome = BuildCaptureOutcome {
        status,
        unsafe_execution_approval: vec![approval_row(
            &request,
            "approved",
            "unsafe approval was supplied for isolated compiler/build execution capture",
        )],
        build_script_runs: build_script_run_rows(
            &observations,
            &request,
            status,
            &target_dir,
            observed_warning,
            &output,
        ),
        build_script_env,
        build_script_stdout: stream_rows(&streams, "stdout", &request),
        build_script_stderr: stream_rows(&streams, "stderr", &request),
        build_script_cargo_instructions,
        proc_macro_invocations: proc_macro_evidence.invocations,
        proc_macro_input_token_streams: proc_macro_evidence.inputs,
        proc_macro_output_token_streams: proc_macro_evidence.outputs,
        native_link_facts,
        out_dir_artifacts,
        toolchain_provenance: vec![toolchain_provenance(&target_dir, &request, Some(&sandbox))],
        validation_errors: {
            if !output.status.success() {
                validation_errors.push(row([
                    ("table", "validation_errors".to_string()),
                    ("code", "dynamic_build_capture_failed".to_string()),
                    (
                        "message",
                        first_non_empty_line(&redact_compiler_stream(&stderr))
                            .unwrap_or("cargo check failed")
                            .to_string(),
                    ),
                    ("repo_path", request.repo_path.display().to_string()),
                ]));
            }
            validation_errors
        },
        capture_gaps,
        raw_log_paths: vec![raw_log_row(&request, "written")],
    };
    match fs::remove_dir_all(&sandbox.scratch_root) {
        Ok(()) => {
            sandbox.cleanup_armed = false;
            outcome.toolchain_provenance[0]
                .insert("sandbox_cleanup".to_string(), "removed".to_string());
        }
        Err(source) => {
            outcome.status = BuildCaptureStatus::Failed;
            outcome.toolchain_provenance[0]
                .insert("sandbox_cleanup".to_string(), "failed".to_string());
            outcome.validation_errors.push(row([
                ("table", "validation_errors".to_string()),
                ("code", "sandbox_cleanup_failed".to_string()),
                (
                    "message",
                    format!(
                        "failed to remove isolated sandbox scratch: {}",
                        io_error_summary(&source)
                    ),
                ),
                (
                    "sandbox_scratch_root",
                    sandbox.scratch_root.display().to_string(),
                ),
            ]));
        }
    }
    Ok(outcome)
}

/// Execute only after crossing the crate-sealed trusted-frontdoor boundary.
///
/// ```compile_fail
/// use codedb_build_capture::TrustedExecutionFrontdoor;
///
/// let forged = TrustedExecutionFrontdoor(());
/// ```
pub fn capture_from_trusted_frontdoor(
    _frontdoor: TrustedExecutionFrontdoor,
    request: BuildCaptureRequest,
    environment: &[(&str, &str)],
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    if !request.unsafe_execute_build
        || !has_named_approver(&request)
        || !has_complete_approval_provenance(&request)
    {
        return capture_approved_fixture_build_with_env(request, environment);
    }
    let normalized_environment = normalize_approved_environment(environment)?;
    let sandbox = prepare_mandatory_sandbox(&request, environment)?;
    let authority = ExecutionApprovalAuthority::new()?;
    let capability = authority.approve(&request, &normalized_environment, &sandbox)?;
    capture_approved_fixture_build_with_capability(
        &authority,
        capability,
        request,
        environment,
        sandbox,
    )
}

fn refused_capture(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    BuildCaptureOutcome {
        status: BuildCaptureStatus::Refused,
        unsafe_execution_approval: vec![approval_row(
            &request,
            "missing",
            "dynamic build/proc-macro capture refused because unsafe approval flag is absent",
        )],
        build_script_runs: Vec::new(),
        build_script_env: Vec::new(),
        build_script_stdout: Vec::new(),
        build_script_stderr: Vec::new(),
        build_script_cargo_instructions: Vec::new(),
        proc_macro_invocations: Vec::new(),
        proc_macro_input_token_streams: Vec::new(),
        proc_macro_output_token_streams: Vec::new(),
        native_link_facts: Vec::new(),
        out_dir_artifacts: Vec::new(),
        toolchain_provenance: Vec::new(),
        validation_errors: vec![row([
            ("table", "validation_errors".to_string()),
            ("code", "unsafe_execution_refused".to_string()),
            (
                "message",
                format!("capture build requires explicit {UNSAFE_FLAG} approval"),
            ),
            ("repo_path", request.repo_path.display().to_string()),
        ])],
        capture_gaps: vec![
            row([
                ("table", "capture_gaps".to_string()),
                ("missing_truth", "build_script_execution".to_string()),
                (
                    "reason",
                    "dynamic build script execution is gated by explicit unsafe approval"
                        .to_string(),
                ),
                ("required_flag", UNSAFE_FLAG.to_string()),
            ]),
            row([
                ("table", "capture_gaps".to_string()),
                ("missing_truth", "proc_macro_execution".to_string()),
                (
                    "reason",
                    "proc-macro execution is gated by explicit unsafe approval".to_string(),
                ),
                ("required_flag", UNSAFE_FLAG.to_string()),
            ]),
            row([
                ("table", "capture_gaps".to_string()),
                ("missing_truth", "native_linker_dynamic_facts".to_string()),
                (
                    "reason",
                    "native/linker facts require approved dynamic build execution".to_string(),
                ),
                ("required_flag", UNSAFE_FLAG.to_string()),
            ]),
            row([
                ("table", "capture_gaps".to_string()),
                ("missing_truth", "out_dir_artifacts".to_string()),
                (
                    "reason",
                    "OUT_DIR artifact capture requires approved dynamic build execution"
                        .to_string(),
                ),
                ("required_flag", UNSAFE_FLAG.to_string()),
            ]),
        ],
        raw_log_paths: vec![raw_log_row(&request, "not_written")],
    }
}

fn has_named_approver(request: &BuildCaptureRequest) -> bool {
    request
        .approver
        .as_deref()
        .is_some_and(|approver| !approver.trim().is_empty())
}

fn has_complete_approval_provenance(request: &BuildCaptureRequest) -> bool {
    [
        &request.task_id,
        &request.before_state,
        &request.cleanup_plan,
    ]
    .into_iter()
    .all(|value| {
        value
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn missing_approval_capture(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    let mut outcome = refused_capture(request.clone());
    outcome.unsafe_execution_approval = vec![approval_row(
        &request,
        "missing",
        "dynamic build/proc-macro capture refused because named operator approval provenance is absent",
    )];
    outcome.validation_errors = vec![row([
        ("table", "validation_errors".to_string()),
        ("code", "approval_provenance_missing".to_string()),
        (
            "message",
            "capture build requires a non-empty approver together with explicit unsafe approval"
                .to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
    ])];
    for gap in &mut outcome.capture_gaps {
        gap.insert(
            "reason".to_string(),
            "dynamic evidence requires named operator approval provenance".to_string(),
        );
        gap.insert(
            "required_approval".to_string(),
            "named approver".to_string(),
        );
    }
    outcome
}

fn incomplete_approval_capture(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    let mut outcome = refused_capture(request.clone());
    outcome.unsafe_execution_approval = vec![approval_row(
        &request,
        "incomplete",
        "dynamic build/proc-macro capture refused because task, before-state, or cleanup provenance is absent",
    )];
    outcome.validation_errors = vec![row([
        ("table", "validation_errors".to_string()),
        ("code", "approval_provenance_incomplete".to_string()),
        (
            "message",
            "capture build requires non-empty task_id, before_state, and cleanup_plan".to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
    ])];
    for gap in &mut outcome.capture_gaps {
        gap.insert(
            "reason".to_string(),
            "dynamic evidence requires complete task, before-state, and cleanup provenance"
                .to_string(),
        );
        gap.insert(
            "required_approval".to_string(),
            "task_id, before_state, cleanup_plan".to_string(),
        );
    }
    outcome
}

fn capability_required_capture(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    if !request.unsafe_execute_build {
        return refused_capture(request);
    }
    if !has_named_approver(&request) {
        return missing_approval_capture(request);
    }
    if !has_complete_approval_provenance(&request) {
        return incomplete_approval_capture(request);
    }
    let mut outcome = refused_capture(request.clone());
    outcome.unsafe_execution_approval = vec![approval_row(
        &request,
        "capability_required",
        "approval-shaped request data cannot authorize dynamic execution",
    )];
    outcome.validation_errors = vec![row([
        ("table", "validation_errors".to_string()),
        ("code", "approval_capability_required".to_string()),
        (
            "message",
            "dynamic capture requires an opaque request-bound capability from the trusted approval authority"
                .to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
    ])];
    for gap in &mut outcome.capture_gaps {
        gap.insert(
            "reason".to_string(),
            "dynamic evidence requires a request-bound execution approval capability".to_string(),
        );
        gap.insert(
            "required_approval".to_string(),
            "opaque authority-minted capability".to_string(),
        );
    }
    outcome
}

fn approval_row(request: &BuildCaptureRequest, status: &str, note: &str) -> Row {
    row([
        ("table", "unsafe_execution_approval".to_string()),
        ("status", status.to_string()),
        ("approval_id", approval_id(request)),
        ("flag", UNSAFE_FLAG.to_string()),
        (
            "approver",
            request
                .approver
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
        ),
        ("task_id", request.task_id.clone().unwrap_or_default()),
        (
            "before_state",
            request.before_state.clone().unwrap_or_default(),
        ),
        (
            "cleanup_plan",
            request.cleanup_plan.clone().unwrap_or_default(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
        (
            "store_path",
            request
                .store_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        ),
        ("raw_log_path", request.raw_log_path.display().to_string()),
        (
            "output_artifact_path",
            isolated_target_dir(request).display().to_string(),
        ),
        ("note", note.to_string()),
    ])
}

fn raw_log_row(request: &BuildCaptureRequest, status: &str) -> Row {
    row([
        ("table", "raw_log_paths".to_string()),
        ("status", status.to_string()),
        ("approval_id", approval_id(request)),
        ("path", request.raw_log_path.display().to_string()),
        (
            "note",
            "redacted command/build evidence path; default and approval-only calls do not write it"
                .to_string(),
        ),
    ])
}

fn approval_id(request: &BuildCaptureRequest) -> String {
    approval_request_digest(request)
}

fn approval_request_digest(request: &BuildCaptureRequest) -> String {
    sha256_hex(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            request.unsafe_execute_build,
            request.task_id.as_deref().unwrap_or_default(),
            request.approver.as_deref().unwrap_or_default(),
            request.before_state.as_deref().unwrap_or_default(),
            request.repo_path.display(),
            request
                .store_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            request.raw_log_path.display(),
            request.cleanup_plan.as_deref().unwrap_or_default(),
        )
        .as_bytes(),
    )
}

fn kernel_random_32() -> Result<[u8; 32], BuildCaptureError> {
    let mut source =
        fs::File::open("/dev/urandom").map_err(|source| BuildCaptureError::ApprovalCapability {
            reason: match source.kind() {
                io::ErrorKind::NotFound => "kernel random source is unavailable",
                _ => "kernel random source could not be opened",
            },
        })?;
    let mut random = [0_u8; 32];
    source
        .read_exact(&mut random)
        .map_err(|_| BuildCaptureError::ApprovalCapability {
            reason: "kernel random source could not provide 32 bytes",
        })?;
    Ok(random)
}

fn capability_authenticator(secret: &[u8; 32], request_digest: &str, nonce: &[u8; 32]) -> [u8; 32] {
    hmac_sha256(
        secret,
        &[request_digest.as_bytes(), nonce.as_slice()].concat(),
    )
}

fn validate_execution_capability(
    authority: &ExecutionApprovalAuthority,
    capability: &ExecutionApprovalCapability,
    request: &BuildCaptureRequest,
    environment: &[(String, String)],
    sandbox: &SandboxPlan,
) -> Result<(), BuildCaptureError> {
    if capability.authority_id != authority.authority_id {
        return Err(BuildCaptureError::ApprovalCapability {
            reason: "capability was minted by a different authority",
        });
    }
    let request_digest = execution_approval_digest(request, environment, sandbox);
    if capability.request_digest != request_digest {
        return Err(BuildCaptureError::ApprovalCapability {
            reason: "capability does not match the exact build request",
        });
    }
    let expected = capability_authenticator(&authority.secret, &request_digest, &capability.nonce);
    if !constant_time_eq(&expected, &capability.authenticator) {
        return Err(BuildCaptureError::ApprovalCapability {
            reason: "capability authenticator is invalid",
        });
    }
    Ok(())
}

fn execution_approval_digest(
    request: &BuildCaptureRequest,
    environment: &[(String, String)],
    sandbox: &SandboxPlan,
) -> String {
    let mut encoded = Vec::new();
    push_digest_field(&mut encoded, approval_request_digest(request).as_bytes());
    for (key, value) in environment {
        push_digest_field(&mut encoded, key.as_bytes());
        push_digest_field(&mut encoded, value.as_bytes());
    }
    push_digest_field(
        &mut encoded,
        sandbox.executable.as_os_str().as_encoded_bytes(),
    );
    push_digest_field(&mut encoded, sandbox.cargo.as_os_str().as_encoded_bytes());
    push_digest_field(&mut encoded, sandbox.rustc.as_os_str().as_encoded_bytes());
    push_digest_field(&mut encoded, sandbox.rustdoc.as_os_str().as_encoded_bytes());
    push_digest_field(&mut encoded, sandbox.linker.as_os_str().as_encoded_bytes());
    push_digest_field(
        &mut encoded,
        sandbox.scratch_root.as_os_str().as_encoded_bytes(),
    );
    for argument in &sandbox.command_args {
        push_digest_field(&mut encoded, argument.as_os_str().as_encoded_bytes());
    }
    sha256_hex(&encoded)
}

fn push_digest_field(encoded: &mut Vec<u8>, field: &[u8]) {
    encoded.extend_from_slice(&(field.len() as u64).to_be_bytes());
    encoded.extend_from_slice(field);
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_BYTES: usize = 64;
    let mut normalized = [0_u8; BLOCK_BYTES];
    if key.len() > BLOCK_BYTES {
        normalized[..32].copy_from_slice(&sha256_bytes(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; BLOCK_BYTES];
    let mut outer_pad = [0x5c_u8; BLOCK_BYTES];
    for index in 0..BLOCK_BYTES {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }
    let inner = sha256_bytes(&[inner_pad.as_slice(), message].concat());
    sha256_bytes(&[outer_pad.as_slice(), inner.as_slice()].concat())
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn out_dir_artifact_gap(request: &BuildCaptureRequest) -> Row {
    row([
        ("table", "capture_gaps".to_string()),
        ("missing_truth", "out_dir_artifacts".to_string()),
        (
            "reason",
            "approved Cargo execution did not yield a complete readable OUT_DIR artifact manifest"
                .to_string(),
        ),
        ("required_task", "CDB080".to_string()),
        (
            "required_environment",
            "cargo build-script-executed OUT_DIR field".to_string(),
        ),
        (
            "required_provenance",
            "unsafe approval row, raw log path, and build-script execution row".to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
    ])
}

fn native_link_gap(request: &BuildCaptureRequest) -> Row {
    row([
        ("table", "capture_gaps".to_string()),
        ("missing_truth", "native_linker_dynamic_facts".to_string()),
        (
            "reason",
            "approved dynamic capture records native/linker facts only when Cargo emits build-script-executed linked_libs or linked_paths"
                .to_string(),
        ),
        ("required_task", "CDB082".to_string()),
        ("repo_path", request.repo_path.display().to_string()),
    ])
}

fn isolated_target_dir(request: &BuildCaptureRequest) -> PathBuf {
    request
        .raw_log_path
        .parent()
        .unwrap_or(&request.repo_path)
        .join("cargo-target")
}

const APPROVED_CAPTURE_ENVIRONMENT: &[&str] = &[
    "CODEDB_FIXTURE_EMIT_NATIVE_LINK",
    "CODEDB_FIXTURE_LOG_SECRET",
    "CODEDB_PROC_MACRO_LOG_PATH",
];

const SAFE_OBSERVED_ENVIRONMENT_VALUES: &[&str] = &[
    "CODEDB_FIXTURE_BUILD_SCRIPT",
    "CODEDB_FIXTURE_EMIT_NATIVE_LINK",
];

fn normalize_approved_environment(
    environment: &[(&str, &str)],
) -> Result<Vec<(String, String)>, BuildCaptureError> {
    let mut normalized = Vec::with_capacity(environment.len());
    let mut previous = None;
    for (key, value) in environment {
        if !APPROVED_CAPTURE_ENVIRONMENT.contains(key) {
            return Err(BuildCaptureError::DisallowedEnvironment {
                key: (*key).to_string(),
            });
        }
        if previous.is_some_and(|previous: &str| previous >= *key) {
            return Err(BuildCaptureError::AmbiguousEnvironment {
                reason: "keys must be unique and supplied in strict bytewise order",
            });
        }
        previous = Some(*key);
        normalized.push(((*key).to_string(), (*value).to_string()));
    }
    Ok(normalized)
}

fn observed_environment_value(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        return "[REDACTED]".to_string();
    }
    if SAFE_OBSERVED_ENVIRONMENT_VALUES.contains(&key) {
        return redact_text(value);
    }
    metadata_summary("environment-value", value.as_bytes())
}

fn approved_environment_value(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        return "[REDACTED]".to_string();
    }
    if key == "CODEDB_FIXTURE_EMIT_NATIVE_LINK" {
        return redact_text(value);
    }
    metadata_summary("approved-environment-value", value.as_bytes())
}

#[cfg(target_os = "linux")]
fn prepare_mandatory_sandbox(
    request: &BuildCaptureRequest,
    environment: &[(&str, &str)],
) -> Result<SandboxPlan, BuildCaptureError> {
    let executable = trusted_executable("bwrap")?;
    let cargo = trusted_executable("cargo")?;
    let rustc = trusted_executable("rustc")?;
    let rustdoc = trusted_executable("rustdoc")?;
    let linker = trusted_executable("cc")?;
    let source =
        request
            .repo_path
            .canonicalize()
            .map_err(|source| BuildCaptureError::PrepareSandbox {
                path: request.repo_path.clone(),
                source,
            })?;
    if !source.is_dir() {
        return Err(BuildCaptureError::SandboxUnavailable {
            reason: "capture source is not a directory".to_string(),
        });
    }

    let random_suffix = sha256_hex(&kernel_random_32()?);
    let scratch_root = std::env::temp_dir().join(format!(
        "codedb-build-sandbox-{}-{}",
        std::process::id(),
        &random_suffix[..24]
    ));
    fs::create_dir(&scratch_root).map_err(|source| BuildCaptureError::PrepareSandbox {
        path: scratch_root.clone(),
        source,
    })?;
    let mut scratch_guard = SandboxScratchGuard::new(scratch_root.clone());
    let host_source = scratch_root.join("source");
    let host_target = scratch_root.join("target");
    let host_cargo_home = scratch_root.join("cargo-home");
    let host_evidence = scratch_root.join("evidence");
    for directory in [&host_source, &host_target, &host_cargo_home, &host_evidence] {
        fs::create_dir_all(directory).map_err(|source| BuildCaptureError::PrepareSandbox {
            path: directory.to_path_buf(),
            source,
        })?;
    }
    copy_sandbox_source(
        &source,
        &host_source,
        &[&scratch_root, &isolated_target_dir(request)],
    )?;

    let guest_source = PathBuf::from("/work/source");
    let guest_target = PathBuf::from("/work/target");
    let guest_proc_macro_log = PathBuf::from("/work/evidence/proc-macro.log");
    let host_proc_macro_log = host_evidence.join("proc-macro.log");
    let mut args = vec![
        OsString::from("--die-with-parent"),
        OsString::from("--new-session"),
        OsString::from("--unshare-all"),
        OsString::from("--cap-drop"),
        OsString::from("ALL"),
        OsString::from("--clearenv"),
    ];
    for path in ["/nix/store", "/usr", "/bin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            push_mount(&mut args, "--ro-bind", Path::new(path), Path::new(path));
        }
    }
    args.extend([
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
        OsString::from("--dir"),
        OsString::from("/homeless"),
    ]);
    push_mount(&mut args, "--bind", &scratch_root, Path::new("/work"));
    bind_tool_root_if_needed(&mut args, &cargo);
    bind_tool_root_if_needed(&mut args, &rustc);
    bind_tool_root_if_needed(&mut args, &rustdoc);
    bind_tool_root_if_needed(&mut args, &linker);

    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    if let Some(cargo_home) = cargo_home {
        for name in ["registry", "git"] {
            let source = cargo_home.join(name);
            if source.exists() {
                let destination = host_cargo_home.join(name);
                fs::create_dir_all(&destination).map_err(|source| {
                    BuildCaptureError::PrepareSandbox {
                        path: destination.clone(),
                        source,
                    }
                })?;
                push_mount(
                    &mut args,
                    "--ro-bind",
                    &source,
                    &PathBuf::from("/work/cargo-home").join(name),
                );
            }
        }
    }

    let path = trusted_tool_path(&[&cargo, &rustc, &rustdoc, &linker]);
    let mut environment_keys = vec![
        "CARGO_HOME".to_string(),
        "CARGO_INCREMENTAL".to_string(),
        "CARGO_NET_OFFLINE".to_string(),
        "CARGO_TARGET_DIR".to_string(),
        "CC".to_string(),
        "HOME".to_string(),
        "LANG".to_string(),
        "LC_ALL".to_string(),
        "PATH".to_string(),
        "RUSTC".to_string(),
        "RUSTDOC".to_string(),
        "RUSTFLAGS".to_string(),
        "TMPDIR".to_string(),
    ];
    for (key, value) in [
        ("HOME", "/homeless".to_string()),
        ("CARGO_HOME", "/work/cargo-home".to_string()),
        ("CARGO_TARGET_DIR", guest_target.display().to_string()),
        ("CARGO_INCREMENTAL", "0".to_string()),
        ("CARGO_NET_OFFLINE", "true".to_string()),
        ("CC", linker.display().to_string()),
        ("RUSTC", rustc.display().to_string()),
        ("RUSTDOC", rustdoc.display().to_string()),
        ("RUSTFLAGS", format!("-C linker={}", linker.display())),
        ("PATH", path),
        ("TMPDIR", "/tmp".to_string()),
        ("LANG", "C".to_string()),
        ("LC_ALL", "C".to_string()),
    ] {
        push_setenv(&mut args, key, &value);
    }
    for key in ["NIX_CC", "NIX_CFLAGS_COMPILE", "NIX_LDFLAGS"] {
        if let Some(value) = std::env::var_os(key) {
            environment_keys.push(key.to_string());
            args.push(OsString::from("--setenv"));
            args.push(OsString::from(key));
            args.push(value);
        }
    }
    for (key, value) in environment {
        environment_keys.push((*key).to_string());
        let value = if *key == "CODEDB_PROC_MACRO_LOG_PATH" {
            guest_proc_macro_log.display().to_string()
        } else {
            (*value).to_string()
        };
        push_setenv(&mut args, key, &value);
    }
    environment_keys.sort();
    environment_keys.dedup();
    args.extend([
        OsString::from("--chdir"),
        guest_source.as_os_str().to_os_string(),
        OsString::from("--"),
        cargo.as_os_str().to_os_string(),
        OsString::from("check"),
        OsString::from("--offline"),
        OsString::from("--message-format=json"),
    ]);

    let plan = SandboxPlan {
        executable,
        cargo,
        rustc,
        rustdoc,
        linker,
        scratch_root,
        guest_source,
        guest_target,
        host_proc_macro_log,
        command_args: args,
        environment_keys,
        cleanup_armed: true,
    };
    scratch_guard.disarm();
    Ok(plan)
}

#[cfg(not(target_os = "linux"))]
fn prepare_mandatory_sandbox(
    _request: &BuildCaptureRequest,
    _environment: &[(&str, &str)],
) -> Result<SandboxPlan, BuildCaptureError> {
    Err(BuildCaptureError::SandboxUnavailable {
        reason: "dynamic build capture is supported only on Linux".to_string(),
    })
}

fn sandbox_command(plan: &SandboxPlan, _environment: &[(&str, &str)]) -> Command {
    let mut command = Command::new(&plan.executable);
    command.args(&plan.command_args).env_clear();
    command
}

#[cfg(target_os = "linux")]
fn trusted_executable(name: &str) -> Result<PathBuf, BuildCaptureError> {
    let mut candidates = vec![
        PathBuf::from("/home/flexnetos/.nix-profile/bin").join(name),
        PathBuf::from("/run/current-system/sw/bin").join(name),
        PathBuf::from("/usr/bin").join(name),
        PathBuf::from("/bin").join(name),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|directory| directory.join(name)));
    }
    candidates.sort();
    candidates.dedup();
    for candidate in candidates {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if !trusted_executable_metadata(&canonical) {
            continue;
        }
        return Ok(canonical);
    }
    Err(BuildCaptureError::SandboxUnavailable {
        reason: format!("required trusted executable is missing: {name}"),
    })
}

#[cfg(target_os = "linux")]
fn trusted_executable_metadata(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file()
        && metadata.mode() & 0o111 != 0
        && metadata.mode() & 0o022 == 0
        && (path.starts_with("/nix/store") || (path.starts_with("/usr") && metadata.uid() == 0))
}

#[cfg(target_os = "linux")]
fn copy_sandbox_source(
    source: &Path,
    destination: &Path,
    excluded: &[&Path],
) -> Result<(), BuildCaptureError> {
    copy_sandbox_source_with_hook(source, destination, excluded, &mut |_, _| {})
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SandboxCopyEvent {
    EntryScanned,
    DirectoryOpened,
    FileOpened,
}

#[cfg(target_os = "linux")]
fn copy_sandbox_source_with_hook(
    source: &Path,
    destination: &Path,
    excluded: &[&Path],
    hook: &mut dyn FnMut(SandboxCopyEvent, &Path),
) -> Result<(), BuildCaptureError> {
    let source = absolute_source_path(source)?;
    let source_directory = open_absolute_directory_handle(&source)?;
    copy_held_sandbox_directory(&source_directory, &source, destination, excluded, hook)
}

#[cfg(target_os = "linux")]
fn absolute_source_path(source: &Path) -> Result<PathBuf, BuildCaptureError> {
    if source.is_absolute() {
        return Ok(source.to_path_buf());
    }
    std::env::current_dir()
        .map(|current| current.join(source))
        .map_err(|source_error| BuildCaptureError::PrepareSandbox {
            path: source.to_path_buf(),
            source: source_error,
        })
}

#[cfg(target_os = "linux")]
fn open_absolute_directory_handle(path: &Path) -> Result<fs::File, BuildCaptureError> {
    use std::os::unix::fs::OpenOptionsExt;

    const O_DIRECTORY: i32 = 0o002_00000;
    const O_NOFOLLOW: i32 = 0o004_00000;
    const O_PATH: i32 = 0o100_00000;

    let mut current = fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_PATH | O_DIRECTORY)
        .open("/")
        .map_err(|source| BuildCaptureError::PrepareSandbox {
            path: PathBuf::from("/"),
            source,
        })?;
    let mut traversed = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => {
                traversed.push(name);
                let next = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(O_PATH | O_NOFOLLOW)
                    .open(descriptor_path(&current).join(name))
                    .map_err(|source| BuildCaptureError::PrepareSandbox {
                        path: traversed.clone(),
                        source,
                    })?;
                let metadata =
                    next.metadata()
                        .map_err(|source| BuildCaptureError::PrepareSandbox {
                            path: traversed.clone(),
                            source,
                        })?;
                if !metadata.is_dir() {
                    return Err(unsupported_sandbox_source_entry(&traversed));
                }
                current = next;
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(BuildCaptureError::SandboxUnavailable {
                    reason: format!(
                        "source tree path is not an absolute normalized directory: {}",
                        path.display()
                    ),
                });
            }
        }
    }
    Ok(current)
}

#[cfg(target_os = "linux")]
fn copy_held_sandbox_directory(
    source_directory: &fs::File,
    logical_source: &Path,
    destination: &Path,
    excluded: &[&Path],
    hook: &mut dyn FnMut(SandboxCopyEvent, &Path),
) -> Result<(), BuildCaptureError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    const O_NOFOLLOW: i32 = 0o004_00000;
    const O_PATH: i32 = 0o100_00000;

    let entries = fs::read_dir(descriptor_path(source_directory)).map_err(|source_error| {
        BuildCaptureError::PrepareSandbox {
            path: logical_source.to_path_buf(),
            source: source_error,
        }
    })?;
    let mut names = entries
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|source_error| BuildCaptureError::PrepareSandbox {
                    path: logical_source.to_path_buf(),
                    source: source_error,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();

    for name in names {
        let source_path = logical_source.join(&name);
        if excluded.iter().any(|excluded| source_path == **excluded)
            || name == ".git"
            || name == "target"
        {
            continue;
        }
        hook(SandboxCopyEvent::EntryScanned, &source_path);

        // O_PATH acquires an inert handle without reading or activating a
        // device/FIFO. O_NOFOLLOW makes a replaced symlink the held object
        // itself, which is then rejected by fstat below.
        let source_object = fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_PATH | O_NOFOLLOW)
            .open(descriptor_path(source_directory).join(&name))
            .map_err(|source| BuildCaptureError::PrepareSandbox {
                path: source_path.clone(),
                source,
            })?;
        let metadata =
            source_object
                .metadata()
                .map_err(|source| BuildCaptureError::PrepareSandbox {
                    path: source_path.clone(),
                    source,
                })?;
        let destination_path = destination.join(&name);

        if metadata.is_dir() {
            fs::create_dir(&destination_path).map_err(|source| {
                BuildCaptureError::PrepareSandbox {
                    path: destination_path.clone(),
                    source,
                }
            })?;
            hook(SandboxCopyEvent::DirectoryOpened, &source_path);
            copy_held_sandbox_directory(
                &source_object,
                &source_path,
                &destination_path,
                excluded,
                hook,
            )?;
            fs::set_permissions(&destination_path, metadata.permissions()).map_err(|source| {
                BuildCaptureError::PrepareSandbox {
                    path: destination_path.clone(),
                    source,
                }
            })?;
        } else if metadata.is_file() {
            hook(SandboxCopyEvent::FileOpened, &source_path);
            let mut source_file =
                fs::File::open(descriptor_path(&source_object)).map_err(|source| {
                    BuildCaptureError::PrepareSandbox {
                        path: source_path.clone(),
                        source,
                    }
                })?;
            let opened_metadata =
                source_file
                    .metadata()
                    .map_err(|source| BuildCaptureError::PrepareSandbox {
                        path: source_path.clone(),
                        source,
                    })?;
            if !opened_metadata.is_file()
                || opened_metadata.dev() != metadata.dev()
                || opened_metadata.ino() != metadata.ino()
            {
                return Err(BuildCaptureError::SandboxUnavailable {
                    reason: format!(
                        "held source object changed while opening data: {}",
                        source_path.display()
                    ),
                });
            }
            let mut destination_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination_path)
                .map_err(|source| BuildCaptureError::PrepareSandbox {
                    path: destination_path.clone(),
                    source,
                })?;
            io::copy(&mut source_file, &mut destination_file).map_err(|source| {
                BuildCaptureError::PrepareSandbox {
                    path: destination_path.clone(),
                    source,
                }
            })?;
            destination_file
                .set_permissions(metadata.permissions())
                .map_err(|source| BuildCaptureError::PrepareSandbox {
                    path: destination_path.clone(),
                    source,
                })?;
        } else {
            return Err(unsupported_sandbox_source_entry(&source_path));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn descriptor_path(descriptor: &fs::File) -> PathBuf {
    use std::os::fd::AsRawFd;

    PathBuf::from(format!("/proc/self/fd/{}", descriptor.as_raw_fd()))
}

#[cfg(target_os = "linux")]
fn unsupported_sandbox_source_entry(path: &Path) -> BuildCaptureError {
    BuildCaptureError::SandboxUnavailable {
        reason: format!(
            "source tree contains unsupported symlink or non-regular entry: {}",
            path.display()
        ),
    }
}

#[cfg(target_os = "linux")]
fn push_mount(args: &mut Vec<OsString>, operation: &str, source: &Path, destination: &Path) {
    args.push(OsString::from(operation));
    args.push(source.as_os_str().to_os_string());
    args.push(destination.as_os_str().to_os_string());
}

#[cfg(target_os = "linux")]
fn push_setenv(args: &mut Vec<OsString>, key: &str, value: &str) {
    args.push(OsString::from("--setenv"));
    args.push(OsString::from(key));
    args.push(OsString::from(value));
}

#[cfg(target_os = "linux")]
fn bind_tool_root_if_needed(args: &mut Vec<OsString>, executable: &Path) {
    if executable.starts_with("/nix/store")
        || executable.starts_with("/usr")
        || executable.starts_with("/bin")
    {
        return;
    }
    if let Some(root) = executable.ancestors().nth(3) {
        push_mount(args, "--ro-bind", root, root);
    }
}

#[cfg(target_os = "linux")]
fn trusted_tool_path(executables: &[&PathBuf]) -> String {
    let mut paths = executables
        .iter()
        .filter_map(|path| path.parent())
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    for path in ["/usr/bin", "/bin"] {
        if Path::new(path).exists() {
            paths.push(PathBuf::from(path));
        }
    }
    paths.sort();
    paths.dedup();
    std::env::join_paths(paths)
        .unwrap_or_else(|_| OsString::from("/usr/bin:/bin"))
        .to_string_lossy()
        .into_owned()
}

fn build_script_observations_from_cargo_json(stdout: &str) -> Vec<BuildScriptObservation> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value.get("reason").and_then(serde_json::Value::as_str) == Some("build-script-executed")
        })
        .map(|value| BuildScriptObservation {
            package_id: value
                .get("package_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown-package")
                .to_string(),
            out_dir: value
                .get("out_dir")
                .and_then(serde_json::Value::as_str)
                .map(PathBuf::from),
            environment: cargo_environment_pairs(value.get("env")),
            linked_libs: cargo_string_values(value.get("linked_libs")),
            linked_paths: cargo_string_values(value.get("linked_paths")),
        })
        .collect()
}

fn cargo_environment_pairs(value: Option<&serde_json::Value>) -> Vec<(String, String)> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let values = entry.as_array()?;
            Some((
                values.first()?.as_str()?.to_string(),
                values.get(1)?.as_str()?.to_string(),
            ))
        })
        .collect()
}

fn cargo_string_values(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}

fn build_script_run_rows(
    observations: &[BuildScriptObservation],
    request: &BuildCaptureRequest,
    status: BuildCaptureStatus,
    target_dir: &Path,
    observed_warning: bool,
    output: &std::process::Output,
) -> Vec<Row> {
    let mut rows = observations
        .iter()
        .map(|observation| {
            row([
                ("table", "build_script_runs".to_string()),
                ("approval_id", approval_id(request)),
                ("repo_path", request.repo_path.display().to_string()),
                ("package_id", observation.package_id.clone()),
                (
                    "out_dir",
                    observation
                        .out_dir
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_default(),
                ),
                ("isolated_target_dir", target_dir.display().to_string()),
                ("status", status.as_str().to_string()),
                ("exit_code", output.status.code().unwrap_or(-1).to_string()),
                ("stdout_bytes", output.stdout.len().to_string()),
                ("stderr_bytes", output.stderr.len().to_string()),
                ("observed_warning", observed_warning.to_string()),
                (
                    "provenance",
                    "cargo check --message-format=json".to_string(),
                ),
                ("approval_flag", UNSAFE_FLAG.to_string()),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() && status == BuildCaptureStatus::Failed {
        rows.push(row([
            ("table", "build_script_runs".to_string()),
            ("approval_id", approval_id(request)),
            ("repo_path", request.repo_path.display().to_string()),
            ("package_id", "unresolved-before-failure".to_string()),
            ("out_dir", String::new()),
            ("isolated_target_dir", target_dir.display().to_string()),
            ("status", status.as_str().to_string()),
            ("exit_code", output.status.code().unwrap_or(-1).to_string()),
            ("stdout_bytes", output.stdout.len().to_string()),
            ("stderr_bytes", output.stderr.len().to_string()),
            ("observed_warning", observed_warning.to_string()),
            (
                "provenance",
                "cargo check --message-format=json failure".to_string(),
            ),
            ("approval_flag", UNSAFE_FLAG.to_string()),
        ]));
    }
    rows
}

fn build_script_env_rows(
    observations: &[BuildScriptObservation],
    request: &BuildCaptureRequest,
) -> Vec<Row> {
    observations
        .iter()
        .flat_map(|observation| {
            observation.environment.iter().map(move |(key, value)| {
                row([
                    ("table", "build_script_env".to_string()),
                    ("approval_id", approval_id(request)),
                    ("status", "observed".to_string()),
                    ("package_id", observation.package_id.clone()),
                    ("key", key.clone()),
                    ("value", observed_environment_value(key, value)),
                    (
                        "out_dir",
                        observation
                            .out_dir
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default(),
                    ),
                    ("provenance", "cargo:build-script-executed.env".to_string()),
                    ("approval_flag", UNSAFE_FLAG.to_string()),
                    ("repo_path", request.repo_path.display().to_string()),
                ])
            })
        })
        .collect()
}

fn approved_environment_rows(
    environment: &[(&str, &str)],
    request: &BuildCaptureRequest,
) -> Vec<Row> {
    environment
        .iter()
        .map(|(key, value)| {
            row([
                ("table", "build_script_env".to_string()),
                ("approval_id", approval_id(request)),
                ("status", "provided".to_string()),
                ("key", (*key).to_string()),
                ("value", approved_environment_value(key, value)),
                (
                    "provenance",
                    "approved capture environment allowlist".to_string(),
                ),
                ("approval_flag", UNSAFE_FLAG.to_string()),
                ("repo_path", request.repo_path.display().to_string()),
            ])
        })
        .collect()
}

fn capture_build_script_streams(observations: &[BuildScriptObservation]) -> Vec<CapturedStream> {
    let mut streams = Vec::new();
    for observation in observations {
        let Some(out_dir) = observation.out_dir.as_deref() else {
            continue;
        };
        let Some(build_dir) = out_dir.parent() else {
            continue;
        };
        for (stream, name) in [("stdout", "output"), ("stderr", "stderr")] {
            let source_path = build_dir.join(name);
            let Ok(bytes) = fs::read(&source_path) else {
                continue;
            };
            let raw = String::from_utf8_lossy(&bytes).into_owned();
            streams.push(CapturedStream {
                package_id: observation.package_id.clone(),
                out_dir: out_dir.to_path_buf(),
                stream,
                source_path,
                redacted: redact_build_script_stream(&raw),
                raw,
            });
        }
    }
    streams
}

fn stream_rows(
    streams: &[CapturedStream],
    stream_name: &str,
    request: &BuildCaptureRequest,
) -> Vec<Row> {
    streams
        .iter()
        .filter(|stream| stream.stream == stream_name)
        .map(|stream| {
            row([
                ("table", format!("build_script_{stream_name}")),
                ("approval_id", approval_id(request)),
                ("status", "observed".to_string()),
                ("package_id", stream.package_id.clone()),
                ("out_dir", stream.out_dir.display().to_string()),
                ("source_path", stream.source_path.display().to_string()),
                ("bytes", stream.raw.len().to_string()),
                ("sha256", sha256_hex(stream.raw.as_bytes())),
                ("redacted_sha256", sha256_hex(stream.redacted.as_bytes())),
                ("redaction", "applied".to_string()),
                ("raw_log_path", request.raw_log_path.display().to_string()),
                ("approval_flag", UNSAFE_FLAG.to_string()),
            ])
        })
        .collect()
}

fn build_script_instruction_rows(
    streams: &[CapturedStream],
    request: &BuildCaptureRequest,
) -> Vec<Row> {
    streams
        .iter()
        .filter(|stream| stream.stream == "stdout")
        .flat_map(|stream| {
            stream
                .redacted
                .lines()
                .filter_map(parse_cargo_instruction)
                .map(move |(instruction, value)| {
                    row([
                        ("table", "build_script_cargo_instructions".to_string()),
                        ("approval_id", approval_id(request)),
                        ("status", "observed".to_string()),
                        ("package_id", stream.package_id.clone()),
                        ("out_dir", stream.out_dir.display().to_string()),
                        ("instruction", instruction),
                        ("value", value),
                        ("provenance", "cargo build-script output".to_string()),
                        ("raw_log_path", request.raw_log_path.display().to_string()),
                        ("approval_flag", UNSAFE_FLAG.to_string()),
                    ])
                })
        })
        .collect()
}

fn parse_cargo_instruction(line: &str) -> Option<(String, String)> {
    let payload = line
        .trim()
        .strip_prefix("cargo::")
        .or_else(|| line.trim().strip_prefix("cargo:"))?;
    let (instruction, value) = payload.split_once('=')?;
    Some((instruction.to_string(), redact_text(value)))
}

fn native_link_facts_from_observations_and_instructions(
    observations: &[BuildScriptObservation],
    instructions: &[Row],
    request: &BuildCaptureRequest,
) -> Vec<Row> {
    let mut facts = Vec::new();
    for observation in observations {
        for (fact_kind, values) in [
            ("linked_lib", &observation.linked_libs),
            ("linked_path", &observation.linked_paths),
        ] {
            for value in values {
                facts.push(row([
                    ("table", "native_link_facts".to_string()),
                    ("approval_id", approval_id(request)),
                    ("status", "observed".to_string()),
                    ("fact_kind", fact_kind.to_string()),
                    ("value", redact_text(value)),
                    ("package_id", observation.package_id.clone()),
                    (
                        "out_dir",
                        observation
                            .out_dir
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_default(),
                    ),
                    ("repo_path", request.repo_path.display().to_string()),
                    ("provenance", "cargo:build-script-executed".to_string()),
                    ("approval_flag", UNSAFE_FLAG.to_string()),
                ]));
            }
        }
    }
    for instruction in instructions {
        let Some(name) = instruction.get("instruction") else {
            continue;
        };
        if !name.starts_with("rustc-link-arg") {
            continue;
        }
        facts.push(row([
            ("table", "native_link_facts".to_string()),
            ("approval_id", approval_id(request)),
            ("status", "observed".to_string()),
            ("fact_kind", "link_arg".to_string()),
            (
                "value",
                instruction.get("value").cloned().unwrap_or_default(),
            ),
            (
                "package_id",
                instruction.get("package_id").cloned().unwrap_or_default(),
            ),
            (
                "out_dir",
                instruction.get("out_dir").cloned().unwrap_or_default(),
            ),
            ("repo_path", request.repo_path.display().to_string()),
            ("provenance", "cargo build-script output".to_string()),
            ("cargo_instruction", name.clone()),
            ("approval_flag", UNSAFE_FLAG.to_string()),
        ]));
    }
    facts
}

fn capture_out_dir_artifacts(
    out_dir: &Path,
    package_id: &str,
    request: &BuildCaptureRequest,
) -> io::Result<Vec<Row>> {
    let mut artifacts = Vec::new();
    capture_out_dir_artifacts_from(out_dir, out_dir, package_id, request, &mut artifacts)?;
    Ok(artifacts)
}

fn capture_out_dir_artifacts_from(
    root: &Path,
    directory: &Path,
    package_id: &str,
    request: &BuildCaptureRequest,
    artifacts: &mut Vec<Row>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if metadata.file_type().is_dir() {
            capture_out_dir_artifacts_from(root, &path, package_id, request, artifacts)?;
            continue;
        }

        let mut artifact = Row::new();
        artifact.insert("table".to_string(), "out_dir_artifacts".to_string());
        artifact.insert("approval_id".to_string(), approval_id(request));
        artifact.insert("status".to_string(), "observed".to_string());
        artifact.insert("package_id".to_string(), package_id.to_string());
        artifact.insert("out_dir".to_string(), root.display().to_string());
        artifact.insert("relative_path".to_string(), relative_path);
        artifact.insert("size_bytes".to_string(), metadata.len().to_string());
        artifact.insert(
            "readonly".to_string(),
            metadata.permissions().readonly().to_string(),
        );
        artifact.insert(
            "modified_unix_ms".to_string(),
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis().to_string())
                .unwrap_or_default(),
        );
        artifact.insert(
            "provenance".to_string(),
            "cargo:build-script-executed.out_dir".to_string(),
        );
        artifact.insert("approval_flag".to_string(), UNSAFE_FLAG.to_string());
        artifact.insert(
            "repo_path".to_string(),
            request.repo_path.display().to_string(),
        );
        append_platform_metadata(&mut artifact, &metadata);

        if metadata.file_type().is_file() {
            let bytes = fs::read(&path)?;
            let sha256 = sha256_hex(&bytes);
            artifact.insert("file_kind".to_string(), "file".to_string());
            artifact.insert("sha256".to_string(), sha256.clone());
            artifact.insert("content_encoding".to_string(), "hex".to_string());
            artifact.insert("content_hex".to_string(), hex_encode(&bytes));
            artifact.insert(
                "reproduction_sha256".to_string(),
                artifact_reproduction_hash(
                    artifact
                        .get("relative_path")
                        .expect("relative path inserted"),
                    "file",
                    &sha256,
                ),
            );
        } else if metadata.file_type().is_symlink() {
            let link_target = fs::read_link(&path)?.display().to_string();
            artifact.insert("file_kind".to_string(), "symlink".to_string());
            artifact.insert("link_target".to_string(), link_target.clone());
            artifact.insert(
                "materialization".to_string(),
                "metadata_only_fallback".to_string(),
            );
            artifact.insert(
                "reproduction_sha256".to_string(),
                artifact_reproduction_hash(
                    artifact
                        .get("relative_path")
                        .expect("relative path inserted"),
                    "symlink",
                    &link_target,
                ),
            );
        } else {
            artifact.insert("file_kind".to_string(), "other".to_string());
        }
        sanitize_row_values(&mut artifact);
        artifacts.push(artifact);
    }
    Ok(())
}

fn artifact_reproduction_hash(relative_path: &str, file_kind: &str, payload_id: &str) -> String {
    sha256_hex(format!("{relative_path}\0{file_kind}\0{payload_id}").as_bytes())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> io::Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(invalid_artifact("hex payload has an odd number of digits"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair)
                .map_err(|_| invalid_artifact("hex payload is not UTF-8"))?;
            u8::from_str_radix(digits, 16)
                .map_err(|_| invalid_artifact("hex payload contains a non-hex digit"))
        })
        .collect()
}

fn invalid_artifact(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn checked_relative_artifact_path(value: &str) -> io::Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(invalid_artifact(format!(
            "artifact path must be a contained relative path: {value}"
        )));
    }
    Ok(path.to_path_buf())
}

enum ReproductionPayload {
    File(Vec<u8>),
    Symlink(PathBuf),
}

struct PreparedReproduction {
    relative_path: PathBuf,
    expected_sha256: String,
    reproduction_sha256: String,
    readonly: bool,
    unix_mode: Option<u32>,
    payload: ReproductionPayload,
}

pub fn reproduce_out_dir_artifacts(
    artifacts: &[Row],
    destination: &ReproductionRoot,
) -> io::Result<Vec<Row>> {
    let mut prepared = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let relative_value = artifact
            .get("relative_path")
            .ok_or_else(|| invalid_artifact("artifact row is missing relative_path"))?;
        let relative_path = checked_relative_artifact_path(relative_value)?;
        let file_kind = artifact
            .get("file_kind")
            .ok_or_else(|| invalid_artifact("artifact row is missing file_kind"))?;
        let readonly = artifact
            .get("readonly")
            .is_some_and(|value| value == "true");
        let unix_mode = artifact
            .get("unix_mode")
            .filter(|value| !value.is_empty())
            .map(|value| {
                u32::from_str_radix(value, 8)
                    .map_err(|_| invalid_artifact("artifact unix_mode is not octal"))
            })
            .transpose()?;

        let (payload, expected_sha256, payload_id) = match file_kind.as_str() {
            "file" => {
                let content = artifact
                    .get("content_hex")
                    .ok_or_else(|| invalid_artifact("file artifact is missing content_hex"))?;
                let bytes = hex_decode(content)?;
                let actual_sha256 = sha256_hex(&bytes);
                let expected_sha256 = artifact
                    .get("sha256")
                    .ok_or_else(|| invalid_artifact("file artifact is missing sha256"))?;
                if &actual_sha256 != expected_sha256 {
                    return Err(invalid_artifact(format!(
                        "artifact payload checksum mismatch for {relative_value}"
                    )));
                }
                (
                    ReproductionPayload::File(bytes),
                    expected_sha256.clone(),
                    actual_sha256,
                )
            }
            "symlink" => {
                let target_value = artifact
                    .get("link_target")
                    .ok_or_else(|| invalid_artifact("symlink artifact is missing link_target"))?;
                let target = checked_relative_artifact_path(target_value)?;
                (
                    ReproductionPayload::Symlink(target),
                    sha256_hex(target_value.as_bytes()),
                    target_value.clone(),
                )
            }
            other => {
                return Err(invalid_artifact(format!(
                    "unsupported OUT_DIR artifact kind: {other}"
                )));
            }
        };
        let reproduction_sha256 =
            artifact_reproduction_hash(relative_value, file_kind, &payload_id);
        if artifact
            .get("reproduction_sha256")
            .is_some_and(|expected| expected != &reproduction_sha256)
        {
            return Err(invalid_artifact(format!(
                "artifact reproduction checksum mismatch for {relative_value}"
            )));
        }
        prepared.push(PreparedReproduction {
            relative_path,
            expected_sha256,
            reproduction_sha256,
            readonly,
            unix_mode,
            payload,
        });
    }

    let mut proof = Vec::with_capacity(prepared.len());
    let mut published: Vec<PathBuf> = Vec::new();
    for artifact in prepared {
        let result = match artifact.payload {
            ReproductionPayload::File(bytes) => {
                let mode = artifact
                    .unix_mode
                    .or_else(|| artifact.readonly.then_some(0o444));
                atomic_publish_bytes(destination, &artifact.relative_path, &bytes, mode)
                    .map(|()| "file")
            }
            ReproductionPayload::Symlink(target) => {
                atomic_publish_symlink(destination, &artifact.relative_path, &target)
                    .map(|()| "symlink")
            }
        };
        let file_kind = match result {
            Ok(file_kind) => file_kind,
            Err(error) => {
                for relative in published.iter().rev() {
                    let _ = remove_published_artifact(destination, relative);
                }
                return Err(error);
            }
        };
        published.push(artifact.relative_path.clone());
        proof.push(row([
            ("table", "out_dir_reproduction_proofs".to_string()),
            ("status", "verified".to_string()),
            (
                "relative_path",
                artifact.relative_path.display().to_string(),
            ),
            ("file_kind", file_kind.to_string()),
            ("sha256", artifact.expected_sha256),
            ("reproduction_sha256", artifact.reproduction_sha256),
            ("proof", "reproduced-bytes-sha256-match".to_string()),
            ("destination", destination.display.display().to_string()),
        ]));
    }
    Ok(proof)
}

#[cfg(unix)]
fn atomic_publish_symlink(
    root: &ReproductionRoot,
    relative: &Path,
    target: &Path,
) -> io::Result<()> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let (parent, final_name) = descriptor_parent(root, relative)?;
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_name = OsString::from(format!(
        ".codedb-link-{}-{sequence}.tmp",
        std::process::id()
    ));
    let ready_name = OsString::from(format!(
        ".codedb-link-{}-{sequence}.ready",
        std::process::id()
    ));
    let temp_path = descriptor_child_path(&parent, &temp_name);
    let ready_path = descriptor_child_path(&parent, &ready_name);
    let final_path = descriptor_child_path(&parent, &final_name);
    std::os::unix::fs::symlink(target, &temp_path)?;
    let mut final_published = false;
    let result = (|| {
        fs::rename(&temp_path, &ready_path)?;
        fs::hard_link(&ready_path, &final_path)?;
        final_published = true;
        parent.sync_all()?;
        fs::remove_file(&ready_path)?;
        parent.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(&ready_path);
        if final_published {
            let _ = fs::remove_file(&final_path);
            let _ = parent.sync_all();
        }
    }
    result
}

#[cfg(not(unix))]
fn atomic_publish_symlink(
    _root: &ReproductionRoot,
    _relative: &Path,
    _target: &Path,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative symlink reproduction is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn remove_published_artifact(root: &ReproductionRoot, relative: &Path) -> io::Result<()> {
    let (parent, final_name) = descriptor_parent(root, relative)?;
    fs::remove_file(descriptor_child_path(&parent, &final_name))?;
    parent.sync_all()
}

#[cfg(not(unix))]
fn remove_published_artifact(_root: &ReproductionRoot, _relative: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative rollback is unsupported on this platform",
    ))
}

#[cfg(unix)]
fn append_platform_metadata(row: &mut Row, metadata: &fs::Metadata) {
    use std::os::unix::fs::MetadataExt;

    row.insert("unix_mode".to_string(), format!("{:o}", metadata.mode()));
}

#[cfg(not(unix))]
fn append_platform_metadata(_row: &mut Row, _metadata: &fs::Metadata) {}

fn capture_proc_macro_evidence(
    environment: &[(&str, &str)],
    request: &BuildCaptureRequest,
) -> ProcMacroEvidence {
    let Some(path) = environment.iter().find_map(|(key, value)| {
        (*key == "CODEDB_PROC_MACRO_LOG_PATH").then_some(PathBuf::from(value))
    }) else {
        return ProcMacroEvidence::default();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return ProcMacroEvidence::default();
    };

    let mut evidence = ProcMacroEvidence::default();
    let mut macro_name = None;
    let mut input = None;
    let mut output = None;
    for line in content.lines().chain(std::iter::once("---")) {
        if line == "---" {
            push_proc_macro_evidence(
                &mut evidence,
                macro_name.take(),
                input.take(),
                output.take(),
                request,
            );
            continue;
        }
        if let Some(value) = line.strip_prefix("macro_name=") {
            macro_name = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("input=") {
            input = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("output=") {
            output = Some(value.to_string());
        }
    }
    let summary = if evidence.log_summary.is_empty() {
        format!(
            "proc_macro_evidence={}\n",
            metadata_summary("unrecognized-proc-macro-stream", content.as_bytes())
        )
    } else {
        format!(
            "{}\nproc_macro_source={}\n",
            evidence.log_summary.join("\n"),
            metadata_summary("proc-macro-stream", content.as_bytes())
        )
    };
    if fs::write(&path, summary).is_err() {
        let _ = fs::remove_file(&path);
        return ProcMacroEvidence::default();
    }
    evidence
}

fn push_proc_macro_evidence(
    evidence: &mut ProcMacroEvidence,
    macro_name: Option<String>,
    input: Option<String>,
    output: Option<String>,
    request: &BuildCaptureRequest,
) {
    let (Some(macro_name), Some(input), Some(output)) = (macro_name, input, output) else {
        return;
    };
    let macro_name = safe_label_or_summary("proc-macro-name", &macro_name);
    let input_sha256 = sha256_hex(input.as_bytes());
    let output_sha256 = sha256_hex(output.as_bytes());
    evidence.invocations.push(row([
        ("table", "proc_macro_invocations".to_string()),
        ("approval_id", approval_id(request)),
        ("status", "observed".to_string()),
        ("macro_name", macro_name.clone()),
        ("input_sha256", input_sha256.clone()),
        ("output_sha256", output_sha256.clone()),
        (
            "provenance",
            "compiler-executed-proc-macro-fixture".to_string(),
        ),
        ("approval_flag", UNSAFE_FLAG.to_string()),
        ("repo_path", request.repo_path.display().to_string()),
        ("capture", "hash-only".to_string()),
    ]));
    evidence.inputs.push(row([
        ("table", "proc_macro_input_token_streams".to_string()),
        ("approval_id", approval_id(request)),
        ("status", "observed".to_string()),
        ("macro_name", macro_name.clone()),
        ("sha256", input_sha256.clone()),
        ("token_count", input.split_whitespace().count().to_string()),
        ("capture", "hash-only".to_string()),
        (
            "provenance",
            "compiler-executed-proc-macro-fixture".to_string(),
        ),
        ("approval_flag", UNSAFE_FLAG.to_string()),
    ]));
    evidence.outputs.push(row([
        ("table", "proc_macro_output_token_streams".to_string()),
        ("approval_id", approval_id(request)),
        ("status", "observed".to_string()),
        ("macro_name", macro_name.clone()),
        ("sha256", output_sha256.clone()),
        ("token_count", output.split_whitespace().count().to_string()),
        ("capture", "hash-only".to_string()),
        (
            "provenance",
            "compiler-executed-proc-macro-fixture".to_string(),
        ),
        ("approval_flag", UNSAFE_FLAG.to_string()),
    ]));
    evidence.log_summary.push(format!(
        "proc_macro macro_name={macro_name} input_sha256={input_sha256} output_sha256={output_sha256} capture=hash-only"
    ));
}

fn toolchain_provenance(
    target_dir: &Path,
    request: &BuildCaptureRequest,
    sandbox: Option<&SandboxPlan>,
) -> Row {
    let mut row = Row::new();
    row.insert("table".to_string(), "toolchain_provenance".to_string());
    row.insert("approval_id".to_string(), approval_id(request));
    row.insert("provenance".to_string(), "rustc -vV".to_string());
    row.insert(
        "isolated_target_dir".to_string(),
        target_dir.display().to_string(),
    );
    let rustc = sandbox
        .map(|plan| plan.rustc.as_path())
        .unwrap_or_else(|| Path::new("rustc"));
    match Command::new(rustc).arg("-vV").output() {
        Ok(output) if output.status.success() => {
            let body = redact_text(&String::from_utf8_lossy(&output.stdout));
            row.insert("status".to_string(), "observed".to_string());
            row.insert("sha256".to_string(), sha256_hex(body.as_bytes()));
            row.insert(
                "rustc_version".to_string(),
                first_non_empty_line(&body).unwrap_or_default().to_string(),
            );
            for line in body.lines() {
                if let Some((key, value)) = line.split_once(": ") {
                    match key {
                        "host" | "release" | "commit-hash" | "commit-date" | "LLVM version" => {
                            row.insert(key.replace(' ', "_"), value.to_string());
                        }
                        _ => {}
                    }
                }
            }
            let target = std::env::var("CARGO_BUILD_TARGET")
                .ok()
                .filter(|target| !target.trim().is_empty())
                .or_else(|| row.get("host").cloned())
                .unwrap_or_default();
            row.insert("target_triple".to_string(), target);
        }
        Ok(output) => {
            row.insert("status".to_string(), "unavailable".to_string());
            row.insert(
                "message".to_string(),
                first_non_empty_line(&redact_text(&String::from_utf8_lossy(&output.stderr)))
                    .unwrap_or("rustc -vV failed")
                    .to_string(),
            );
        }
        Err(error) => {
            row.insert("status".to_string(), "unavailable".to_string());
            row.insert("message".to_string(), redact_text(&error.to_string()));
        }
    }
    let cargo = sandbox
        .map(|plan| plan.cargo.as_path())
        .unwrap_or_else(|| Path::new("cargo"));
    match Command::new(cargo).arg("-V").output() {
        Ok(output) if output.status.success() => {
            row.insert(
                "cargo_version".to_string(),
                first_non_empty_line(&redact_text(&String::from_utf8_lossy(&output.stdout)))
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        Ok(output) => {
            row.insert(
                "cargo_version".to_string(),
                first_non_empty_line(&redact_text(&String::from_utf8_lossy(&output.stderr)))
                    .unwrap_or("unavailable")
                    .to_string(),
            );
        }
        Err(_) => {
            row.insert("cargo_version".to_string(), "unavailable".to_string());
        }
    }
    if let Some(sandbox) = sandbox {
        row.insert("sandbox_backend".to_string(), "bubblewrap".to_string());
        row.insert(
            "sandbox_executable".to_string(),
            sandbox.executable.display().to_string(),
        );
        row.insert(
            "sandbox_executable_sha256".to_string(),
            fs::read(&sandbox.executable)
                .map(|bytes| sha256_hex(&bytes))
                .unwrap_or_else(|_| "unavailable".to_string()),
        );
        row.insert("network_policy".to_string(), "unshared".to_string());
        row.insert(
            "writable_root_policy".to_string(),
            "isolated-scratch-and-ephemeral-tmp".to_string(),
        );
        row.insert(
            "sandbox_provenance".to_string(),
            "kernel-enforced".to_string(),
        );
        row.insert(
            "sandbox_namespaces".to_string(),
            "user,mount,pid,ipc,uts,network,cgroup".to_string(),
        );
        row.insert(
            "sandbox_capabilities".to_string(),
            "all-dropped".to_string(),
        );
        row.insert(
            "sandbox_scratch_root".to_string(),
            sandbox.scratch_root.display().to_string(),
        );
        row.insert(
            "sandbox_guest_source".to_string(),
            sandbox.guest_source.display().to_string(),
        );
        row.insert(
            "sandbox_guest_target".to_string(),
            sandbox.guest_target.display().to_string(),
        );
        row.insert(
            "sandbox_command_sha256".to_string(),
            sha256_hex(
                &sandbox
                    .command_args
                    .iter()
                    .flat_map(|argument| {
                        let mut bytes = argument.to_string_lossy().as_bytes().to_vec();
                        bytes.push(0);
                        bytes
                    })
                    .collect::<Vec<_>>(),
            ),
        );
        row.insert(
            "sandbox_environment_keys".to_string(),
            sandbox.environment_keys.join(","),
        );
        row.insert(
            "sandbox_cargo".to_string(),
            sandbox.cargo.display().to_string(),
        );
        row.insert(
            "sandbox_rustc".to_string(),
            sandbox.rustc.display().to_string(),
        );
        row.insert(
            "sandbox_rustdoc".to_string(),
            sandbox.rustdoc.display().to_string(),
        );
        row.insert(
            "sandbox_linker".to_string(),
            sandbox.linker.display().to_string(),
        );
        row.insert(
            "environment_policy".to_string(),
            "cleared_then_minimally_allowlisted".to_string(),
        );
    } else {
        row.insert("environment_policy".to_string(), "not-executed".to_string());
    }
    sanitize_row_values(&mut row);
    row
}

#[cfg(unix)]
fn open_directory_nofollow(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_directory_nofollow(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "descriptor-relative publication is unavailable on this platform",
    ))
}

#[cfg(unix)]
fn descriptor_child_path(parent: &File, child: &OsStr) -> PathBuf {
    use std::os::fd::AsRawFd;

    PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd())).join(child)
}

#[cfg(unix)]
fn open_or_create_descriptor_directory(parent: &File, child: &OsStr) -> io::Result<File> {
    let path = descriptor_child_path(parent, child);
    match open_directory_nofollow(&path) {
        Ok(directory) => Ok(directory),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&path)?;
            open_directory_nofollow(&path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn descriptor_parent(root: &ReproductionRoot, relative: &Path) -> io::Result<(File, OsString)> {
    let mut current = root.directory.try_clone()?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(invalid_artifact("non-normal descriptor-relative path"));
        };
        if components.peek().is_none() {
            return Ok((current, name.to_os_string()));
        }
        current = open_or_create_descriptor_directory(&current, name)?;
    }
    Err(invalid_artifact(
        "descriptor-relative path has no final name",
    ))
}

#[cfg(unix)]
fn atomic_publish_bytes(
    root: &ReproductionRoot,
    relative: &Path,
    bytes: &[u8],
    unix_mode: Option<u32>,
) -> io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    const O_NOFOLLOW: i32 = 0o400000;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let (parent, final_name) = descriptor_parent(root, relative)?;
    let parent_path = descriptor_child_path(&parent, OsStr::new("."));
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_name = OsString::from(format!(
        ".codedb-write-{}-{sequence}.tmp",
        std::process::id()
    ));
    let ready_name = OsString::from(format!(
        ".codedb-write-{}-{sequence}.ready",
        std::process::id()
    ));
    let temp_path = descriptor_child_path(&parent, &temp_name);
    let ready_path = descriptor_child_path(&parent, &ready_name);
    let final_path = descriptor_child_path(&parent, &final_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(O_NOFOLLOW)
        .open(&temp_path)?;
    let mut final_published = false;
    let result = (|| {
        file.write_all(bytes)?;
        if let Some(mode) = unix_mode {
            file.set_permissions(fs::Permissions::from_mode(mode & 0o7777))?;
        }
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, &ready_path)?;
        fs::hard_link(&ready_path, &final_path)?;
        final_published = true;
        File::open(&parent_path)?.sync_all()?;
        fs::remove_file(&ready_path)?;
        File::open(&parent_path)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
        let _ = fs::remove_file(&ready_path);
        if final_published {
            let _ = fs::remove_file(&final_path);
            let _ = parent.sync_all();
        }
    }
    result
}

fn write_redacted_raw_log(
    root: &ReproductionRoot,
    path: &Path,
    request: &BuildCaptureRequest,
    output: &std::process::Output,
    streams: &[CapturedStream],
    proc_macro_log_summary: &[String],
) -> Result<(), BuildCaptureError> {
    let relative = path
        .strip_prefix(&root.display)
        .map_err(|_| BuildCaptureError::WriteLog {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::PermissionDenied,
                "raw log must be below the pre-opened trusted log root",
            ),
        })?;
    checked_relative_artifact_path(&relative.to_string_lossy()).map_err(|source| {
        BuildCaptureError::WriteLog {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let mut body = format!(
        "status={}\nexit_code={}\nredaction=applied\napproval_id={}\ntask_id={}\napprover={}\nbefore_state={}\ncleanup_plan={}\nenvironment_policy=cleared_then_minimally_allowlisted\nsandbox_backend=bubblewrap\nnetwork_policy=unshared\nwritable_root_policy=isolated-scratch-and-ephemeral-tmp\n--- cargo stdout ---\n{}\n--- cargo stderr ---\n{}\n",
        output.status,
        output.status.code().unwrap_or(-1),
        approval_id(request),
        redact_text(request.task_id.as_deref().unwrap_or_default()),
        redact_text(request.approver.as_deref().unwrap_or_default()),
        redact_text(request.before_state.as_deref().unwrap_or_default()),
        redact_text(request.cleanup_plan.as_deref().unwrap_or_default()),
        redact_cargo_json_output(&String::from_utf8_lossy(&output.stdout)),
        redact_compiler_stream(&String::from_utf8_lossy(&output.stderr))
    );
    for stream in streams {
        body.push_str(&format!(
            "--- build script {} ({}) ---\n{}\n",
            stream.stream,
            redact_text(&stream.package_id),
            stream.redacted
        ));
    }
    if !proc_macro_log_summary.is_empty() {
        body.push_str("--- proc macro evidence ---\n");
        body.push_str(&proc_macro_log_summary.join("\n"));
        body.push('\n');
    }
    atomic_publish_bytes(root, relative, body.as_bytes(), Some(0o600)).map_err(|source| {
        BuildCaptureError::WriteLog {
            path: path.to_path_buf(),
            source,
        }
    })
}

fn proc_macro_gap(request: &BuildCaptureRequest) -> Row {
    row([
        ("table", "capture_gaps".to_string()),
        ("missing_truth", "proc_macro_execution".to_string()),
        (
            "reason",
            "approved compiler run did not produce a proc-macro execution evidence log".to_string(),
        ),
        (
            "required_environment",
            "CODEDB_PROC_MACRO_LOG_PATH for an approved instrumented fixture".to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
        ("approval_flag", UNSAFE_FLAG.to_string()),
    ])
}

fn build_script_gap(request: &BuildCaptureRequest) -> Row {
    row([
        ("table", "capture_gaps".to_string()),
        ("missing_truth", "build_script_execution".to_string()),
        (
            "reason",
            "approved cargo run emitted no build-script-executed message".to_string(),
        ),
        ("repo_path", request.repo_path.display().to_string()),
        ("approval_flag", UNSAFE_FLAG.to_string()),
    ])
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    const ROUND_CONSTANTS: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];

    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());

    let mut hash = INITIAL;
    for chunk in message.chunks_exact(64) {
        let mut schedule = [0_u32; 64];
        for (index, word) in schedule.iter_mut().take(16).enumerate() {
            *word = u32::from_be_bytes(chunk[index * 4..index * 4 + 4].try_into().expect("word"));
        }
        for index in 16..64 {
            let sigma0 = schedule[index - 15].rotate_right(7)
                ^ schedule[index - 15].rotate_right(18)
                ^ (schedule[index - 15] >> 3);
            let sigma1 = schedule[index - 2].rotate_right(17)
                ^ schedule[index - 2].rotate_right(19)
                ^ (schedule[index - 2] >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(sigma0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(sigma1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = hash;
        for (index, constant) in ROUND_CONSTANTS.iter().enumerate() {
            let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(sigma1)
                .wrapping_add(choice)
                .wrapping_add(*constant)
                .wrapping_add(schedule[index]);
            let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sigma0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        hash = [
            hash[0].wrapping_add(a),
            hash[1].wrapping_add(b),
            hash[2].wrapping_add(c),
            hash[3].wrapping_add(d),
            hash[4].wrapping_add(e),
            hash[5].wrapping_add(f),
            hash[6].wrapping_add(g),
            hash[7].wrapping_add(h),
        ];
    }
    let mut digest = [0_u8; 32];
    for (index, word) in hash.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    digest
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha256_bytes(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn redact_value_for_key(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        "[REDACTED]".to_string()
    } else {
        redact_text(value)
    }
}

fn metadata_summary(kind: &str, bytes: &[u8]) -> String {
    format!(
        "[metadata-only kind={kind} bytes={} sha256={}]",
        bytes.len(),
        sha256_hex(bytes)
    )
}

fn safe_label_or_summary(kind: &str, value: &str) -> String {
    let trimmed = value.trim();
    if !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_:-.".contains(character))
        && !looks_like_bare_secret(trimmed)
        && !looks_like_jwt(trimmed)
        && !looks_like_aws_access_key(trimmed)
        && !looks_like_npm_token(trimmed)
        && !contains_database_uri(trimmed)
        && !looks_percent_encoded_credential(trimmed)
    {
        trimmed.to_string()
    } else {
        metadata_summary(kind, value.as_bytes())
    }
}

fn redact_text(value: &str) -> String {
    let mut output = Vec::new();
    let mut private_key_block = None::<String>;
    for line in value.lines() {
        if let Some(block) = private_key_block.as_mut() {
            block.push('\n');
            block.push_str(line);
            if is_private_key_end(line) {
                let block = private_key_block.take().expect("private key block");
                output.push(metadata_summary("private-key", block.as_bytes()));
            }
            continue;
        }
        if is_private_key_begin(line) {
            private_key_block = Some(line.to_string());
            if is_private_key_end(line) {
                let block = private_key_block.take().expect("private key block");
                output.push(metadata_summary("private-key", block.as_bytes()));
            }
            continue;
        }
        output.push(redact_line(line));
    }
    if let Some(block) = private_key_block {
        output.push(metadata_summary(
            "unterminated-private-key",
            block.as_bytes(),
        ));
    }
    output.join("\n")
}

fn is_private_key_begin(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    upper.contains("-----BEGIN ") && upper.contains("PRIVATE KEY-----")
}

fn is_private_key_end(line: &str) -> bool {
    let upper = line.to_ascii_uppercase();
    upper.contains("-----END ") && upper.contains("PRIVATE KEY-----")
}

fn redact_line(line: &str) -> String {
    let mut output = Vec::new();
    let mut redact_next_value = false;
    let mut database_value = false;
    let mut bearer_value = false;

    for token in line.split_whitespace() {
        let normalized = normalized_token(token);
        if bearer_value {
            output.push("[REDACTED]".to_string());
            bearer_value = false;
            continue;
        }
        if redact_next_value {
            if matches!(token, "=" | ":") {
                output.push(token.to_string());
                continue;
            }
            output.push(if database_value && contains_database_uri(token) {
                redact_database_uri_token(token)
            } else {
                "[REDACTED]".to_string()
            });
            redact_next_value = false;
            database_value = false;
            continue;
        }
        if normalized.eq_ignore_ascii_case("bearer") {
            output.push(token.to_string());
            bearer_value = true;
            continue;
        }
        if is_sensitive_key(&normalized)
            && !token.contains('=')
            && !token.contains(':')
            && !token.contains('%')
            && (is_database_key(&normalized)
                || token
                    .trim_matches(|character: char| {
                        !character.is_ascii_alphanumeric() && character != '_'
                    })
                    .chars()
                    .all(|character| {
                        !character.is_ascii_alphabetic() || character.is_ascii_uppercase()
                    }))
        {
            output.push(token.to_string());
            redact_next_value = true;
            database_value = is_database_key(&normalized);
            continue;
        }
        output.push(redact_token(token));
    }
    output.join(" ")
}

fn redact_token(token: &str) -> String {
    let unquoted = token.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
    });
    if looks_percent_encoded_credential(unquoted) {
        return "[REDACTED]".to_string();
    }
    if contains_database_uri(unquoted) {
        return redact_database_uri_token(token);
    }
    if looks_like_bare_secret(unquoted)
        || looks_like_jwt(unquoted)
        || looks_like_aws_access_key(unquoted)
        || looks_like_npm_token(unquoted)
    {
        return "[REDACTED]".to_string();
    }
    if let Some((key, value)) = token.split_once('=') {
        if is_sensitive_key(key) {
            if is_database_key(key) && contains_database_uri(value) {
                return format!("{key}={}", redact_database_uri_token(value));
            }
            return format!("{key}=[REDACTED]");
        }
        if let Some((nested_key, _nested_value)) = value.split_once('=')
            && is_sensitive_key(nested_key)
        {
            return format!("{key}={nested_key}=[REDACTED]");
        }
    }
    if let Some((key, _value)) = token.split_once(':')
        && is_sensitive_key(key)
    {
        return format!("{key}:[REDACTED]");
    }
    if let Some((prefix, _value)) = token.split_once(':')
        && prefix.eq_ignore_ascii_case("bearer")
    {
        return format!("{prefix}:[REDACTED]");
    }
    token.to_string()
}

fn normalized_token(value: &str) -> String {
    value
        .trim_matches(|character: char| {
            !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-')
        })
        .to_string()
}

fn looks_like_bare_secret(token: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "ghp_",
        "gho_",
        "github_pat_",
        "sk-",
        "sk_proj_",
        "sk-proj-",
        "xoxb-",
        "xoxp-",
    ];
    PREFIXES
        .iter()
        .any(|prefix| token.starts_with(prefix) && token.len() >= prefix.len() + 12)
}

fn looks_like_jwt(token: &str) -> bool {
    let parts = token.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            part.len() >= 8
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
        })
}

fn looks_like_aws_access_key(token: &str) -> bool {
    (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token.len() == 20
        && token
            .chars()
            .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
}

fn looks_like_npm_token(token: &str) -> bool {
    token.starts_with("npm_") && token.len() >= 16
}

fn looks_percent_encoded_credential(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    [
        "database_url%3d",
        "dsn%3d",
        "connection_string%3d",
        "password%3d",
        "passwd%3d",
        "token%3d",
        "secret%3d",
        "authorization%3a",
        "private_key%3d",
        "npm_token%3d",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || ((lower.contains("postgres%3a%2f%2f") || lower.contains("postgresql%3a%2f%2f"))
            && (lower.contains("%3a") || lower.contains("%40")))
}

fn contains_database_uri(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("postgres://") || lower.contains("postgresql://")
}

fn redact_database_uri_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    let Some(start) = ["postgresql://", "postgres://"]
        .iter()
        .filter_map(|scheme| lower.find(scheme))
        .min()
    else {
        return if looks_percent_encoded_credential(token) {
            "[REDACTED]".to_string()
        } else {
            token.to_string()
        };
    };
    let end = token[start..]
        .char_indices()
        .find_map(|(index, character)| {
            (index > 0 && matches!(character, '"' | '\'' | ')' | ']' | '}' | ',' | ';'))
                .then_some(start + index)
        })
        .unwrap_or(token.len());
    let prefix = &token[..start];
    let suffix = &token[end..];
    let uri = &token[start..end];
    format!("{prefix}{}{suffix}", redact_database_uri(uri))
}

fn redact_database_uri(uri: &str) -> String {
    let Some(scheme_end) = uri.find("://").map(|index| index + 3) else {
        return metadata_summary("malformed-database-uri", uri.as_bytes());
    };
    let authority_end = uri[scheme_end..]
        .find(['/', '?', '#'])
        .map(|index| scheme_end + index)
        .unwrap_or(uri.len());
    let authority = &uri[scheme_end..authority_end];
    let safe_authority = if let Some(at) = authority.rfind('@') {
        format!("[REDACTED]@{}", &authority[at + 1..])
    } else if let Some(at) = authority.to_ascii_lowercase().rfind("%40") {
        format!("[REDACTED]@{}", &authority[at + 3..])
    } else if let Some((host_or_user, port_or_password)) = authority.rsplit_once(':') {
        if port_or_password.parse::<u16>().is_ok() {
            format!("{host_or_user}:{port_or_password}")
        } else {
            "[REDACTED]".to_string()
        }
    } else {
        authority.to_string()
    };

    let remainder = &uri[authority_end..];
    let redacted_remainder = if let Some(query_start) = remainder.find('?') {
        let (path, query_and_fragment) = remainder.split_at(query_start + 1);
        let (query, fragment) = query_and_fragment
            .split_once('#')
            .map_or((query_and_fragment, ""), |(query, fragment)| {
                (query, fragment)
            });
        let query = query
            .split('&')
            .map(|parameter| {
                let Some((key, value)) = parameter.split_once('=') else {
                    return if is_sensitive_query_key(parameter) {
                        format!("{parameter}=[REDACTED]")
                    } else {
                        parameter.to_string()
                    };
                };
                if is_sensitive_query_key(key)
                    || looks_like_bare_secret(value)
                    || looks_like_jwt(value)
                    || looks_like_aws_access_key(value)
                    || looks_like_npm_token(value)
                    || looks_percent_encoded_credential(value)
                {
                    format!("{key}=[REDACTED]")
                } else {
                    format!("{key}={value}")
                }
            })
            .collect::<Vec<_>>()
            .join("&");
        if fragment.is_empty() {
            format!("{path}{query}")
        } else {
            format!("{path}{query}#{}", redact_text(fragment))
        }
    } else {
        remainder.to_string()
    };
    format!("{}{safe_authority}{redacted_remainder}", &uri[..scheme_end])
}

fn is_sensitive_query_key(key: &str) -> bool {
    let decoded_shape = key
        .to_ascii_lowercase()
        .replace("%5f", "_")
        .replace("%2d", "-");
    is_sensitive_key(&decoded_shape)
        || matches!(
            normalized_sensitive_key(&decoded_shape).as_str(),
            "auth" | "_auth" | "user" | "username" | "sslkey" | "passphrase"
        )
}

fn redact_cargo_json_output(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let Ok(mut message) = serde_json::from_str::<serde_json::Value>(line) else {
                return metadata_summary("unrecognized-cargo-output", line.as_bytes());
            };
            let Some(reason) = message.get("reason").and_then(serde_json::Value::as_str) else {
                return metadata_summary("unrecognized-cargo-json", line.as_bytes());
            };
            if !matches!(
                reason,
                "compiler-artifact"
                    | "compiler-message"
                    | "build-script-executed"
                    | "build-finished"
            ) {
                return metadata_summary("unrecognized-cargo-json", line.as_bytes());
            }
            if let Some(environment) = message
                .get_mut("env")
                .and_then(serde_json::Value::as_array_mut)
            {
                for entry in environment {
                    let Some(values) = entry.as_array_mut() else {
                        continue;
                    };
                    let Some(key) = values
                        .first()
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                    else {
                        continue;
                    };
                    if let Some(value) = values.get_mut(1)
                        && let Some(text) = value.as_str()
                    {
                        let redacted = if is_sensitive_key(&key) {
                            "[REDACTED]".to_string()
                        } else if SAFE_OBSERVED_ENVIRONMENT_VALUES.contains(&key.as_str()) {
                            redact_text(text)
                        } else {
                            metadata_summary("environment-value", text.as_bytes())
                        };
                        *value = serde_json::Value::String(redacted);
                    }
                }
            }
            redact_json_strings(&mut message);
            serde_json::to_string(&message)
                .unwrap_or_else(|_| metadata_summary("cargo-json-serialization", line.as_bytes()))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_json_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = redact_text(text),
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json_strings(value);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if is_sensitive_key(key) {
                    *value = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_json_value_for_key(key, value);
                }
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn redact_json_value_for_key(key: &str, value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            *text = match key {
                "stdout" => redact_build_script_stream(text),
                "stderr" => redact_compiler_stream(text),
                _ if is_allowlisted_cargo_json_string_key(key) => redact_text(text),
                _ => metadata_summary("unrecognized-cargo-json-string", text.as_bytes()),
            };
        }
        serde_json::Value::Array(values) => {
            for value in values {
                match value {
                    serde_json::Value::String(text) => {
                        *text = if is_allowlisted_cargo_json_string_key(key) {
                            redact_text(text)
                        } else {
                            metadata_summary("unrecognized-cargo-json-string", text.as_bytes())
                        };
                    }
                    _ => redact_json_strings(value),
                }
            }
        }
        serde_json::Value::Object(_)
        | serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_) => redact_json_strings(value),
    }
}

fn is_allowlisted_cargo_json_string_key(key: &str) -> bool {
    [
        "reason",
        "package_id",
        "manifest_path",
        "message",
        "rendered",
        "level",
        "code",
        "name",
        "kind",
        "crate_types",
        "src_path",
        "edition",
        "features",
        "filenames",
        "executable",
        "out_dir",
        "env",
        "linked_libs",
        "linked_paths",
        "text",
        "label",
        "suggested_replacement",
        "suggestion_applicability",
        "macro_decl_name",
        "emit",
        "debuginfo",
        "opt_level",
    ]
    .contains(&key)
}

fn is_sensitive_key(key: &str) -> bool {
    let key = normalized_sensitive_key(key);
    if key.is_empty()
        || key == "token_count"
        || key.ends_with("_sha256")
        || matches!(key.as_str(), "sha256" | "approval_id")
    {
        return false;
    }
    is_database_key(&key)
        || key == "dsn"
        || key.ends_with("_dsn")
        || matches!(key.as_str(), "auth" | "_auth")
        || [
            "secret",
            "token",
            "password",
            "passwd",
            "credential",
            "authorization",
            "api_key",
            "private_key",
            "access_key_id",
            "auth_token",
            "jwt",
            "passphrase",
            "sslkey",
        ]
        .iter()
        .any(|marker| key.contains(marker))
}

fn is_database_key(key: &str) -> bool {
    let key = normalized_sensitive_key(key);
    key == "database_url"
        || key.ends_with("_database_url")
        || key == "connection_string"
        || key.ends_with("_connection_string")
        || key == "dsn"
        || key.ends_with("_dsn")
}

fn normalized_sensitive_key(key: &str) -> String {
    let key = key.rsplit(['/', '\\', '?', '&']).next().unwrap_or(key);
    key.trim_matches(|character: char| {
        !character.is_ascii_alphanumeric() && !matches!(character, '_' | '-')
    })
    .to_ascii_lowercase()
    .replace('-', "_")
}

fn redact_build_script_stream(value: &str) -> String {
    redact_stream(value, "unrecognized-build-script-output", |line| {
        let line = line.trim_start();
        line.starts_with("cargo:") || line.starts_with("cargo::")
    })
}

fn redact_compiler_stream(value: &str) -> String {
    redact_stream(value, "unrecognized-compiler-output", |line| {
        let line = line.trim_start();
        [
            "warning:",
            "error:",
            "note:",
            "help:",
            "Caused by:",
            "Compiling ",
            "Checking ",
            "Finished ",
            "Updating ",
            "Locking ",
            "Downloaded ",
            "Downloading ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
    })
}

fn redact_stream(
    value: &str,
    unknown_kind: &str,
    allowlisted_line: impl Fn(&str) -> bool,
) -> String {
    value
        .lines()
        .map(|line| {
            let redacted = redact_text(line);
            if line.trim().is_empty()
                || allowlisted_line(line)
                || redacted.contains("[REDACTED]")
                || redacted.contains("[metadata-only")
            {
                redacted
            } else {
                metadata_summary(unknown_kind, line.as_bytes())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().find(|line| !line.trim().is_empty())
}

fn row<const N: usize>(pairs: [(&str, String); N]) -> Row {
    let mut row = pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
    sanitize_row_values(&mut row);
    row
}

fn sanitize_row_values(row: &mut Row) {
    for (key, value) in row {
        *value = redact_value_for_key(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn capture_approved_fixture_build(
        request: BuildCaptureRequest,
    ) -> Result<BuildCaptureOutcome, BuildCaptureError> {
        capture_approved_fixture_build_with_env(request, &[])
    }

    fn capture_approved_fixture_build_with_env(
        request: BuildCaptureRequest,
        environment: &[(&str, &str)],
    ) -> Result<BuildCaptureOutcome, BuildCaptureError> {
        capture_from_trusted_frontdoor(TrustedExecutionFrontdoor(()), request, environment)
    }

    // Test lane: default
    // Defends: CDB033 must refuse build/proc-macro execution unless the unsafe flag is explicit.
    #[test]
    fn capture_build_refuses_without_unsafe_flag() {
        let outcome = capture_build(request(false));

        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert_eq!(
            outcome.validation_errors[0].get("code").map(String::as_str),
            Some("unsafe_execution_refused")
        );
        assert_eq!(
            outcome.capture_gaps[0]
                .get("required_flag")
                .map(String::as_str),
            Some(UNSAFE_FLAG)
        );
        assert_eq!(
            outcome.raw_log_paths[0].get("status").map(String::as_str),
            Some("not_written")
        );
        assert!(outcome.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("out_dir_artifacts")
                && gap.get("required_flag").map(String::as_str) == Some(UNSAFE_FLAG)
        }));
    }

    // Test lane: default
    // Defends: CDB033 approval-shaped data is recorded without being promoted
    // to an approved execution capability.
    #[test]
    fn capture_build_refuses_approval_shaped_data_without_capability() {
        let outcome = capture_build(request(true));

        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert_eq!(
            outcome.unsafe_execution_approval[0]
                .get("status")
                .map(String::as_str),
            Some("capability_required")
        );
        assert_eq!(
            outcome.validation_errors[0].get("code").map(String::as_str),
            Some("approval_capability_required")
        );
        assert_eq!(
            outcome.raw_log_paths[0].get("status").map(String::as_str),
            Some("not_written")
        );
    }

    // Test lane: default
    // Defends: CDB034 approved fixture capture preserves raw logs behind the unsafe gate.
    #[test]
    fn approved_fixture_capture_writes_raw_logs() {
        let fixture = temp_dir("codedb_build_capture_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::write(
            fixture.join("Cargo.toml"),
            r#"[package]
name = "codedb-build-capture-fixture"
version = "0.1.0"
edition = "2024"
"#,
        )
        .expect("write manifest");
        fs::write(
            fixture.join("src/lib.rs"),
            "pub fn fixture() -> bool { true }\n",
        )
        .expect("write lib");
        fs::write(
            fixture.join("build.rs"),
            r#"fn main() {
    println!("cargo:warning=build-script-probe");
}
"#,
        )
        .expect("write build script");

        let raw_log_path = fixture.join("logs/raw-build.log");
        let outcome = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: fixture.clone(),
            store_path: None,
            raw_log_path: raw_log_path.clone(),
            unsafe_execute_build: true,
            approver: Some("test".to_string()),
            task_id: Some("CDB079".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("approved fixture capture should run");

        assert_eq!(outcome.status, BuildCaptureStatus::Captured);
        assert_eq!(
            outcome.build_script_runs[0]
                .get("observed_warning")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            outcome.raw_log_paths[0].get("status").map(String::as_str),
            Some("written")
        );
        let raw_log = fs::read_to_string(&raw_log_path).expect("read raw log");
        assert!(raw_log.contains("build-script-probe"));
        assert!(outcome.validation_errors.is_empty());

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: default
    // Defends: CDB034 dynamic fixture capture still refuses without unsafe approval.
    #[test]
    fn approved_fixture_capture_still_refuses_without_unsafe_flag() {
        let outcome = capture_approved_fixture_build(request(false))
            .expect("refusal does not need to run cargo");

        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert!(outcome.build_script_runs.is_empty());
        assert_eq!(
            outcome.validation_errors[0].get("code").map(String::as_str),
            Some("unsafe_execution_refused")
        );
    }

    // Test lane: default plus approved dynamic execution
    // Defends: CDB078 keeps the default refusal while an approved compiler run
    // records an actual proc-macro invocation and output hash.
    #[test]
    fn proc_macro_execution_gate_refuses_default_and_captures_compiler_evidence() {
        let refused = capture_build(request(false));
        assert!(refused.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("proc_macro_execution")
                && gap.get("required_flag").map(String::as_str) == Some(UNSAFE_FLAG)
        }));
        assert!(refused.proc_macro_invocations.is_empty());

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let fixture = temp_dir("codedb_proc_macro_gate_fixture");
        copy_fixture_tree(
            &repo_root.join("fixtures/proc_macro_consumer"),
            &fixture,
            &[
                "Cargo.toml",
                "crates/consumer/Cargo.toml",
                "crates/consumer/src/lib.rs",
                "crates/demo_macro/Cargo.toml",
                "crates/demo_macro/src/lib.rs",
            ],
        );
        let proc_macro_log = fixture.join("logs/proc-macro.log");
        let proc_macro_log_value = proc_macro_log.display().to_string();
        let approved = capture_approved_fixture_build_with_env(
            BuildCaptureRequest {
                repo_path: fixture.clone(),
                store_path: None,
                raw_log_path: fixture.join("logs/cargo.log"),
                unsafe_execute_build: true,
                approver: Some("proc-macro-test".to_string()),
                task_id: Some("CDB078".to_string()),
                before_state: Some("fixture-source-copied-and-unchanged".to_string()),
                cleanup_plan: Some(
                    "remove isolated fixture and cargo target after proof".to_string(),
                ),
            },
            &[("CODEDB_PROC_MACRO_LOG_PATH", proc_macro_log_value.as_str())],
        )
        .expect("approved proc-macro capture should run");

        assert_eq!(approved.status, BuildCaptureStatus::Captured);
        assert!(approved.proc_macro_invocations.iter().any(|row| {
            row.get("status").map(String::as_str) == Some("observed")
                && row.get("macro_name").map(String::as_str) == Some("demo_attr")
                && row.get("provenance").map(String::as_str)
                    == Some("compiler-executed-proc-macro-fixture")
        }));
        assert!(approved.proc_macro_input_token_streams.iter().any(|row| {
            row.get("status").map(String::as_str) == Some("observed")
                && row.get("sha256").is_some_and(|value| value.len() == 64)
                && row.get("capture").map(String::as_str) == Some("hash-only")
        }));
        assert!(approved.proc_macro_output_token_streams.iter().any(|row| {
            row.get("status").map(String::as_str) == Some("observed")
                && row.get("sha256").is_some_and(|value| value.len() == 64)
        }));
        assert!(!approved.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("proc_macro_execution")
        }));

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: default
    // Defends: CDB079 build-script execution is refused by default and approved runs capture provenance/logs.
    #[test]
    fn build_script_execution_gate_refuses_default_and_captures_approved_logs() {
        let refused = capture_build(request(false));
        assert!(refused.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("build_script_execution")
                && gap.get("required_flag").map(String::as_str) == Some(UNSAFE_FLAG)
        }));
        assert!(refused.build_script_runs.is_empty());

        let fixture = temp_dir("codedb_build_script_gate_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::write(
            fixture.join("Cargo.toml"),
            r#"[package]
name = "codedb-build-script-gate-fixture"
version = "0.1.0"
edition = "2024"
"#,
        )
        .expect("write manifest");
        fs::write(fixture.join("src/lib.rs"), "pub fn fixture() {}\n").expect("write lib");
        fs::write(
            fixture.join("build.rs"),
            r#"fn main() {
    println!("cargo:warning=build-script-provenance");
}
"#,
        )
        .expect("write build script");

        let raw_log_path = fixture.join("logs/raw-build.log");
        let approved = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: fixture.clone(),
            store_path: None,
            raw_log_path: raw_log_path.clone(),
            unsafe_execute_build: true,
            approver: Some("build-script-test".to_string()),
            task_id: Some("CDB079".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("approved fixture capture should run");

        assert_eq!(approved.status, BuildCaptureStatus::Captured);
        assert_eq!(
            approved.unsafe_execution_approval[0]
                .get("approver")
                .map(String::as_str),
            Some("build-script-test")
        );
        assert_eq!(
            approved.unsafe_execution_approval[0]
                .get("task_id")
                .map(String::as_str),
            Some("CDB079")
        );
        assert_eq!(
            approved.unsafe_execution_approval[0]
                .get("before_state")
                .map(String::as_str),
            Some("fixture-source-copied-and-unchanged")
        );
        assert_eq!(
            approved.unsafe_execution_approval[0]
                .get("cleanup_plan")
                .map(String::as_str),
            Some("remove isolated fixture and cargo target after proof")
        );
        let approval_id = approved.unsafe_execution_approval[0]
            .get("approval_id")
            .expect("approval id");
        assert_eq!(approval_id.len(), 64);
        assert!(
            approved
                .build_script_runs
                .iter()
                .all(|row| row.get("approval_id") == Some(approval_id))
        );
        assert_eq!(
            approved.build_script_runs[0]
                .get("observed_warning")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            approved.raw_log_paths[0].get("status").map(String::as_str),
            Some("written")
        );
        let raw_log = fs::read_to_string(&raw_log_path).expect("read raw log");
        assert!(raw_log.contains("build-script-provenance"));

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: default
    // Defends: CDB082 native/link facts are captured only under approved dynamic build execution.
    #[test]
    fn native_linker_facts_require_approved_dynamic_capture() {
        let refused = capture_build(request(false));
        assert!(refused.native_link_facts.is_empty());
        assert!(refused.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("native_linker_dynamic_facts")
                && gap.get("required_flag").map(String::as_str) == Some(UNSAFE_FLAG)
        }));

        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let fixture = temp_dir("codedb_native_link_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::copy(
            repo_root.join("fixtures/native_link/Cargo.toml"),
            fixture.join("Cargo.toml"),
        )
        .expect("copy fixture manifest");
        fs::copy(
            repo_root.join("fixtures/native_link/build.rs"),
            fixture.join("build.rs"),
        )
        .expect("copy fixture build script");
        fs::copy(
            repo_root.join("fixtures/native_link/src/lib.rs"),
            fixture.join("src/lib.rs"),
        )
        .expect("copy fixture lib");

        let raw_log_path = fixture.join("logs/raw-build.log");
        let approved = capture_approved_fixture_build_with_env(
            BuildCaptureRequest {
                repo_path: fixture.clone(),
                store_path: None,
                raw_log_path: raw_log_path.clone(),
                unsafe_execute_build: true,
                approver: Some("native-link-test".to_string()),
                task_id: Some("CDB082".to_string()),
                before_state: Some("fixture-source-copied-and-unchanged".to_string()),
                cleanup_plan: Some(
                    "remove isolated fixture and cargo target after proof".to_string(),
                ),
            },
            &[("CODEDB_FIXTURE_EMIT_NATIVE_LINK", "1")],
        )
        .expect("approved native link fixture capture should run");

        assert_eq!(approved.status, BuildCaptureStatus::Captured);
        assert!(approved.native_link_facts.iter().any(|fact| {
            fact.get("fact_kind").map(String::as_str) == Some("linked_lib")
                && fact.get("value").map(String::as_str) == Some("static=codedb_fixture_native")
                && fact.get("provenance").map(String::as_str) == Some("cargo:build-script-executed")
                && fact.get("approval_flag").map(String::as_str) == Some(UNSAFE_FLAG)
        }));
        assert!(approved.native_link_facts.iter().any(|fact| {
            fact.get("fact_kind").map(String::as_str) == Some("linked_path")
                && fact.get("value").map(String::as_str) == Some("native=vendor/native")
        }));
        assert!(approved.native_link_facts.iter().any(|fact| {
            fact.get("fact_kind").map(String::as_str) == Some("link_arg")
                && fact.get("value").map(String::as_str) == Some("-Wl,--as-needed")
                && fact.get("provenance").map(String::as_str) == Some("cargo build-script output")
        }));
        assert!(!approved.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("native_linker_dynamic_facts")
        }));

        let raw_log = fs::read_to_string(&raw_log_path).expect("read raw log");
        assert!(raw_log.contains("linked_libs"));
        assert!(raw_log.contains("codedb_fixture_native"));

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: approved dynamic execution
    // Defends: CDB080 captures reproducible OUT_DIR paths, hashes, metadata,
    // toolchain provenance, and safe symlink materialization facts.
    #[test]
    fn out_dir_artifact_reproduction_captures_manifest() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");
        let fixture = temp_dir("codedb_out_dir_generator_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::copy(
            repo_root.join("fixtures/out_dir_generator/Cargo.toml"),
            fixture.join("Cargo.toml"),
        )
        .expect("copy fixture manifest");
        fs::copy(
            repo_root.join("fixtures/out_dir_generator/build.rs"),
            fixture.join("build.rs"),
        )
        .expect("copy fixture build script");
        fs::copy(
            repo_root.join("fixtures/out_dir_generator/src/lib.rs"),
            fixture.join("src/lib.rs"),
        )
        .expect("copy fixture lib");
        let raw_log_path = fixture.join("logs/raw-build.log");
        let outcome = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: fixture.clone(),
            store_path: None,
            raw_log_path: raw_log_path.clone(),
            unsafe_execute_build: true,
            approver: Some("out-dir-test".to_string()),
            task_id: Some("CDB080".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("approved out-dir fixture capture should run");

        assert_eq!(outcome.status, BuildCaptureStatus::Captured);
        assert!(outcome.out_dir_artifacts.iter().any(|artifact| {
            artifact.get("relative_path").map(String::as_str) == Some("generated.rs")
                && artifact.get("file_kind").map(String::as_str) == Some("file")
                && artifact
                    .get("sha256")
                    .is_some_and(|sha256| sha256.len() == 64)
                && artifact.get("size_bytes").is_some()
                && artifact.get("modified_unix_ms").is_some()
        }));
        #[cfg(unix)]
        assert!(outcome.out_dir_artifacts.iter().any(|artifact| {
            artifact.get("relative_path").map(String::as_str) == Some("generated-link.rs")
                && artifact.get("file_kind").map(String::as_str) == Some("symlink")
                && artifact.get("link_target").map(String::as_str) == Some("generated.rs")
                && artifact.get("materialization").map(String::as_str)
                    == Some("metadata_only_fallback")
        }));
        assert!(outcome.toolchain_provenance.iter().any(|provenance| {
            provenance.get("status").map(String::as_str) == Some("observed")
                && provenance.get("rustc_version").is_some()
                && provenance.get("host").is_some()
                && provenance.get("target_triple").is_some()
                && provenance.get("cargo_version").is_some()
                && provenance.get("environment_policy").map(String::as_str)
                    == Some("cleared_then_minimally_allowlisted")
        }));
        assert!(!outcome.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("out_dir_artifacts")
        }));

        let reproduced = fixture.join("reproduced-out-dir");
        fs::create_dir(&reproduced).expect("create reproduction root");
        let reproduction_root =
            ReproductionRoot::open_existing(&reproduced).expect("open reproduction root");
        let reproduction_proof =
            reproduce_out_dir_artifacts(&outcome.out_dir_artifacts, &reproduction_root)
                .expect("reproduce captured OUT_DIR");
        assert_eq!(
            fs::read(reproduced.join("generated.rs")).expect("read reproduced generated.rs"),
            b"pub const GENERATED_VALUE: &str = \"generated\";\n"
        );
        assert!(
            reproduction_proof
                .iter()
                .all(|row| { row.get("status").map(String::as_str) == Some("verified") })
        );
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(reproduced.join("generated-link.rs"))
                .expect("read reproduced generated symlink"),
            PathBuf::from("generated.rs")
        );

        let second_fixture = temp_dir("codedb_out_dir_generator_repeat_fixture");
        copy_fixture_tree(
            &repo_root.join("fixtures/out_dir_generator"),
            &second_fixture,
            &["Cargo.toml", "build.rs", "src/lib.rs"],
        );
        let second = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: second_fixture.clone(),
            store_path: None,
            raw_log_path: second_fixture.join("logs/raw-build.log"),
            unsafe_execute_build: true,
            approver: Some("out-dir-repeat-test".to_string()),
            task_id: Some("CDB080".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("repeat approved out-dir fixture capture");
        let stable_projection = |rows: &[Row]| {
            rows.iter()
                .map(|row| {
                    (
                        row["relative_path"].clone(),
                        row["file_kind"].clone(),
                        row.get("sha256").cloned().unwrap_or_default(),
                        row.get("link_target").cloned().unwrap_or_default(),
                        row["reproduction_sha256"].clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            stable_projection(&outcome.out_dir_artifacts),
            stable_projection(&second.out_dir_artifacts)
        );

        let raw_log = fs::read_to_string(&raw_log_path).expect("read raw log");
        assert!(
            raw_log.contains("generated.rs")
                || raw_log.contains("codedb_fixture_out_dir_generator")
        );

        let _ = fs::remove_dir_all(fixture);
        let _ = fs::remove_dir_all(second_fixture);
    }

    // Test lane: approved dynamic execution
    // Defends: CDB078-CDB082 require compiler/build-observed evidence rather
    // than approval scaffolding or GAP-only rows.
    #[test]
    fn approved_execution_capture_records_complete_compiler_evidence() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("repo root");

        let proc_macro_fixture = temp_dir("codedb_proc_macro_execution_fixture");
        copy_fixture_tree(
            &repo_root.join("fixtures/proc_macro_consumer"),
            &proc_macro_fixture,
            &[
                "Cargo.toml",
                "crates/consumer/Cargo.toml",
                "crates/consumer/src/lib.rs",
                "crates/demo_macro/Cargo.toml",
                "crates/demo_macro/src/lib.rs",
            ],
        );
        let proc_macro_log_path = proc_macro_fixture.join("logs/proc-macro.log");
        let proc_macro_log_value = proc_macro_log_path.display().to_string();
        let proc_macro_outcome = capture_approved_fixture_build_with_env(
            BuildCaptureRequest {
                repo_path: proc_macro_fixture.clone(),
                store_path: None,
                raw_log_path: proc_macro_fixture.join("logs/cargo.log"),
                unsafe_execute_build: true,
                approver: Some("execution-evidence-test".to_string()),
                task_id: Some("CDB078".to_string()),
                before_state: Some("fixture-source-copied-and-unchanged".to_string()),
                cleanup_plan: Some(
                    "remove isolated fixture and cargo target after proof".to_string(),
                ),
            },
            &[("CODEDB_PROC_MACRO_LOG_PATH", proc_macro_log_value.as_str())],
        )
        .expect("approved proc-macro capture should run");

        assert_eq!(proc_macro_outcome.status, BuildCaptureStatus::Captured);
        assert!(proc_macro_outcome.proc_macro_invocations.iter().any(|row| {
            row.get("status").map(String::as_str) == Some("observed")
                && row.get("provenance").map(String::as_str)
                    == Some("compiler-executed-proc-macro-fixture")
                && row.get("macro_name").map(String::as_str) == Some("demo_attr")
        }));
        assert!(
            proc_macro_outcome
                .proc_macro_output_token_streams
                .iter()
                .any(|row| {
                    row.get("status").map(String::as_str) == Some("observed")
                        && row.get("sha256").is_some_and(|value| value.len() == 64)
                        && row.get("capture").map(String::as_str) == Some("hash-only")
                })
        );
        assert!(!proc_macro_outcome.capture_gaps.iter().any(|row| {
            row.get("missing_truth").map(String::as_str) == Some("proc_macro_execution")
        }));

        let build_script_fixture = temp_dir("codedb_build_script_execution_fixture");
        copy_fixture_tree(
            &repo_root.join("fixtures/build_script"),
            &build_script_fixture,
            &["Cargo.toml", "build.rs", "src/lib.rs"],
        );
        let build_script_outcome = capture_approved_fixture_build_with_env(
            BuildCaptureRequest {
                repo_path: build_script_fixture.clone(),
                store_path: None,
                raw_log_path: build_script_fixture.join("logs/cargo.log"),
                unsafe_execute_build: true,
                approver: Some("execution-evidence-test".to_string()),
                task_id: Some("CDB079".to_string()),
                before_state: Some("fixture-source-copied-and-unchanged".to_string()),
                cleanup_plan: Some(
                    "remove isolated fixture and cargo target after proof".to_string(),
                ),
            },
            &[(
                "CODEDB_FIXTURE_LOG_SECRET",
                "fixture-secret-should-not-leak",
            )],
        )
        .expect("approved build-script capture should run");

        assert!(build_script_outcome.build_script_env.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("CODEDB_FIXTURE_BUILD_SCRIPT")
                && row.get("value").map(String::as_str) == Some("observed")
        }));
        assert!(build_script_outcome.build_script_env.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("CODEDB_FIXTURE_API_TOKEN")
                && row.get("value").map(String::as_str) == Some("[REDACTED]")
        }));
        assert!(build_script_outcome.build_script_env.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("CODEDB_FIXTURE_LOG_SECRET")
                && row.get("value").map(String::as_str) == Some("[REDACTED]")
                && row.get("status").map(String::as_str) == Some("provided")
        }));
        assert!(
            build_script_outcome
                .build_script_cargo_instructions
                .iter()
                .any(|row| {
                    row.get("instruction").map(String::as_str) == Some("rerun-if-changed")
                        && row.get("value").map(String::as_str) == Some("build.rs")
                })
        );
        assert!(
            build_script_outcome
                .build_script_stdout
                .iter()
                .any(|row| row.get("redaction").map(String::as_str) == Some("applied"))
        );
        let redacted_log = fs::read_to_string(build_script_fixture.join("logs/cargo.log"))
            .expect("read redacted cargo log");
        assert!(!redacted_log.contains("fixture-secret-should-not-leak"));
        assert!(redacted_log.contains("[REDACTED]"));

        let out_dir_fixture = temp_dir("codedb_out_dir_execution_fixture");
        copy_fixture_tree(
            &repo_root.join("fixtures/out_dir_generator"),
            &out_dir_fixture,
            &["Cargo.toml", "build.rs", "src/lib.rs"],
        );
        let out_dir_outcome = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: out_dir_fixture.clone(),
            store_path: None,
            raw_log_path: out_dir_fixture.join("logs/cargo.log"),
            unsafe_execute_build: true,
            approver: Some("execution-evidence-test".to_string()),
            task_id: Some("CDB080".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("approved OUT_DIR capture should run");

        assert!(out_dir_outcome.out_dir_artifacts.iter().any(|row| {
            row.get("relative_path").map(String::as_str) == Some("generated.rs")
                && row.get("file_kind").map(String::as_str) == Some("file")
                && row.get("sha256").is_some_and(|value| value.len() == 64)
                && row.get("out_dir").is_some()
        }));
        assert!(out_dir_outcome.toolchain_provenance.iter().any(|row| {
            row.get("rustc_version").is_some()
                && row.get("host").is_some()
                && row.get("provenance").map(String::as_str) == Some("rustc -vV")
        }));
        #[cfg(unix)]
        assert!(out_dir_outcome.out_dir_artifacts.iter().any(|row| {
            row.get("relative_path").map(String::as_str) == Some("generated-link.rs")
                && row.get("file_kind").map(String::as_str) == Some("symlink")
                && row.get("link_target").map(String::as_str) == Some("generated.rs")
                && row.get("materialization").map(String::as_str) == Some("metadata_only_fallback")
        }));

        let _ = fs::remove_dir_all(proc_macro_fixture);
        let _ = fs::remove_dir_all(build_script_fixture);
        let _ = fs::remove_dir_all(out_dir_fixture);
    }

    // Test lane: default refusal
    // Defends: approved execution requires named operator provenance, not just
    // the unsafe boolean.
    #[test]
    fn approved_execution_refuses_missing_approver_before_spawning_cargo() {
        let mut missing_approver = request(true);
        missing_approver.approver = None;

        let outcome = capture_approved_fixture_build(missing_approver)
            .expect("missing approval provenance must fail closed without spawning cargo");

        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert!(outcome.build_script_runs.is_empty());
        assert!(outcome.validation_errors.iter().any(|row| {
            row.get("code").map(String::as_str) == Some("approval_provenance_missing")
        }));
    }

    // Test lane: mandatory execution approval
    // Defends: caller-controlled booleans and operator/provenance strings are
    // data, not an execution capability, and therefore cannot authorize Cargo.
    #[test]
    fn plain_boolean_and_operator_strings_cannot_forge_execution_approval() {
        let scaffold = capture_build(request(true));
        assert_eq!(scaffold.status, BuildCaptureStatus::Refused);
        assert!(scaffold.validation_errors.iter().any(|row| {
            row.get("code").map(String::as_str) == Some("approval_capability_required")
        }));

        let outcome = super::capture_approved_fixture_build(request(true))
            .expect("untrusted approval-shaped data must fail closed");

        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert!(outcome.build_script_runs.is_empty());
        assert!(outcome.validation_errors.iter().any(|row| {
            row.get("code").map(String::as_str) == Some("approval_capability_required")
        }));
    }

    // Test lane: request-bound capability
    // Defends: a real capability cannot be replayed for a request whose paths
    // or other execution-relevant fields changed after operator approval.
    #[test]
    fn execution_capability_is_bound_to_the_exact_request() {
        let authority = ExecutionApprovalAuthority::new().expect("approval authority");
        let fixture = temp_dir("codedb_capability_binding");
        fs::create_dir_all(&fixture).expect("create fixture");
        let mut approved = request(true);
        approved.repo_path = fixture.clone();
        approved.raw_log_path = fixture.join("capture.log");
        let environment = normalize_approved_environment(&[]).expect("normalize environment");
        let sandbox = prepare_mandatory_sandbox(&approved, &[]).expect("prepare sandbox");
        let capability = authority
            .approve(&approved, &environment, &sandbox)
            .expect("mint capability");
        let mut changed = approved;
        changed.raw_log_path = PathBuf::from("/tmp/forged-after-approval.log");

        let error = super::capture_approved_fixture_build_with_capability(
            &authority,
            capability,
            changed,
            &[],
            sandbox,
        )
        .expect_err("changed request must invalidate the capability before sandbox launch");
        assert!(matches!(
            error,
            BuildCaptureError::ApprovalCapability {
                reason: "capability does not match the exact build request"
            }
        ));
        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: fail-closed sandbox discovery
    // Defends: missing repo/Nix-owned isolation tooling is a hard error, never
    // a signal to fall back to direct Cargo execution.
    #[cfg(target_os = "linux")]
    #[test]
    fn missing_sandbox_backend_fails_closed() {
        let error = trusted_executable("codedb-deliberately-missing-sandbox-backend")
            .expect_err("missing sandbox backend must be rejected");
        assert!(matches!(
            error,
            BuildCaptureError::SandboxUnavailable { .. }
        ));
    }

    // Test lane: source-copy containment
    // Defends: replacing a scanned final file with a symlink cannot redirect
    // the sandbox snapshot read outside the approved source tree.
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_source_copy_rejects_final_component_replacement() {
        use std::os::unix::fs::symlink;

        let fixture = temp_dir("codedb_sandbox_copy_final_replacement");
        let source = fixture.join("source");
        let destination = fixture.join("destination");
        let outside = fixture.join("outside");
        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(source.join("victim.txt"), b"approved-source").expect("write source file");
        fs::write(outside.join("secret.txt"), b"outside-secret").expect("write outside file");

        let victim = source.join("victim.txt");
        let outside_secret = outside.join("secret.txt");
        let mut replaced = false;
        let error =
            copy_sandbox_source_with_hook(&source, &destination, &[], &mut |event, path| {
                if !replaced && event == SandboxCopyEvent::EntryScanned && path == victim {
                    fs::remove_file(&victim).expect("remove scanned source file");
                    symlink(&outside_secret, &victim)
                        .expect("replace final component with symlink");
                    replaced = true;
                }
            })
            .expect_err("symlink replacement must fail closed");

        assert!(replaced, "adversarial replacement hook did not run");
        assert!(matches!(
            error,
            BuildCaptureError::SandboxUnavailable { .. }
        ));
        assert!(
            !destination.join("victim.txt").exists(),
            "outside bytes must never be copied through a replacement symlink"
        );

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: source-copy containment
    // Defends: data is read from the held regular-file object rather than by
    // reopening its pathname after validation.
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_source_copy_holds_final_file_across_path_replacement() {
        use std::os::unix::fs::symlink;

        let fixture = temp_dir("codedb_sandbox_copy_held_final");
        let source = fixture.join("source");
        let destination = fixture.join("destination");
        let outside = fixture.join("outside");
        let detached = fixture.join("detached-approved-file.txt");
        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(source.join("input.txt"), b"approved-source").expect("write approved source");
        fs::write(outside.join("input.txt"), b"outside-secret").expect("write outside source");

        let input = source.join("input.txt");
        let outside_input = outside.join("input.txt");
        let mut replaced = false;
        copy_sandbox_source_with_hook(&source, &destination, &[], &mut |event, path| {
            if !replaced && event == SandboxCopyEvent::FileOpened && path == input {
                fs::rename(&input, &detached).expect("detach opened source file");
                symlink(&outside_input, &input).expect("replace source path with symlink");
                replaced = true;
            }
        })
        .expect("held regular-file read must not follow replacement path");

        assert!(replaced, "adversarial replacement hook did not run");
        assert_eq!(
            fs::read(destination.join("input.txt")).expect("read copied source"),
            b"approved-source"
        );

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: source-copy containment
    // Defends: once an ancestor directory is opened, traversal and data reads
    // remain bound to that held object even if its pathname is replaced.
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_source_copy_holds_ancestor_across_path_replacement() {
        use std::os::unix::fs::symlink;

        let fixture = temp_dir("codedb_sandbox_copy_ancestor_replacement");
        let source = fixture.join("source");
        let destination = fixture.join("destination");
        let outside = fixture.join("outside");
        let detached = fixture.join("detached-approved-ancestor");
        fs::create_dir_all(source.join("ancestor")).expect("create source ancestor");
        fs::create_dir_all(&destination).expect("create destination");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(source.join("ancestor/input.txt"), b"approved-source")
            .expect("write approved source");
        fs::write(outside.join("input.txt"), b"outside-secret").expect("write outside source");

        let ancestor = source.join("ancestor");
        let mut replaced = false;
        copy_sandbox_source_with_hook(&source, &destination, &[], &mut |event, path| {
            if !replaced && event == SandboxCopyEvent::DirectoryOpened && path == ancestor {
                fs::rename(&ancestor, &detached).expect("detach opened ancestor");
                symlink(&outside, &ancestor).expect("replace ancestor path with symlink");
                replaced = true;
            }
        })
        .expect("held ancestor traversal must not follow replacement path");

        assert!(replaced, "adversarial replacement hook did not run");
        assert_eq!(
            fs::read(destination.join("ancestor/input.txt")).expect("read copied source"),
            b"approved-source"
        );
        assert_ne!(
            fs::read(destination.join("ancestor/input.txt")).expect("read copied source"),
            b"outside-secret"
        );

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: source-copy containment
    // Defends: inert handle inspection rejects special files without opening
    // them for device, socket, or FIFO I/O.
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_source_copy_rejects_special_files() {
        use std::os::unix::net::UnixListener;

        let fixture = temp_dir("codedb_sandbox_copy_special");
        let source = fixture.join("source");
        let destination = fixture.join("destination");
        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&destination).expect("create destination");
        let socket_path = source.join("build.sock");
        let _listener = UnixListener::bind(&socket_path).expect("create source socket");

        let error = copy_sandbox_source(&source, &destination, &[])
            .expect_err("special source entries must fail closed");
        assert!(matches!(
            error,
            BuildCaptureError::SandboxUnavailable { .. }
        ));
        assert!(!destination.join("build.sock").exists());

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: source-copy policy
    // Defends: descriptor-relative traversal retains the established .git,
    // target, and caller-supplied exact-path exclusions.
    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_source_copy_preserves_exclusions() {
        let fixture = temp_dir("codedb_sandbox_copy_exclusions");
        let source = fixture.join("source");
        let destination = fixture.join("destination");
        let excluded = source.join("generated");
        for directory in [
            source.join(".git"),
            source.join("target"),
            excluded.clone(),
            source.join("src"),
        ] {
            fs::create_dir_all(directory).expect("create source directory");
        }
        fs::create_dir_all(&destination).expect("create destination");
        fs::write(source.join(".git/config"), b"git").expect("write git metadata");
        fs::write(source.join("target/artifact"), b"target").expect("write target artifact");
        fs::write(excluded.join("artifact"), b"generated").expect("write excluded artifact");
        fs::write(source.join("src/lib.rs"), b"pub fn copied() {}\n")
            .expect("write included source");

        copy_sandbox_source(&source, &destination, &[excluded.as_path()])
            .expect("copy source with exclusions");

        assert!(!destination.join(".git").exists());
        assert!(!destination.join("target").exists());
        assert!(!destination.join("generated").exists());
        assert_eq!(
            fs::read(destination.join("src/lib.rs")).expect("read included source"),
            b"pub fn copied() {}\n"
        );

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: mandatory execution isolation
    // Defends: an approved build/proc-macro execution is not valid evidence
    // unless the runner proves network isolation, bounded writable roots,
    // ambient-sensitive-environment clearing, and the sandbox implementation.
    #[test]
    fn approved_execution_requires_recorded_mandatory_sandbox_provenance() {
        let fixture = temp_dir("codedb_mandatory_sandbox_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::write(
            fixture.join("Cargo.toml"),
            r#"[package]
name = "codedb-mandatory-sandbox-fixture"
version = "0.1.0"
edition = "2024"
"#,
        )
        .expect("write manifest");
        fs::write(fixture.join("src/lib.rs"), "pub fn fixture() {}\n").expect("write lib");
        let host_sentinel = PathBuf::from("/tmp").join(format!(
            "{}-host-write-sentinel",
            fixture
                .file_name()
                .expect("fixture file name")
                .to_string_lossy()
        ));
        fs::write(
            fixture.join("build.rs"),
            format!(
                r#"fn main() {{
    let routes = std::fs::read_to_string("/proc/net/route").unwrap_or_default();
    assert!(
        !routes.lines().skip(1).any(|line| {{
            line.split_whitespace().nth(1) == Some("00000000")
        }}),
        "sandbox exposed a default network route"
    );
    for key in [
        "AWS_SECRET_ACCESS_KEY",
        "GITHUB_TOKEN",
        "OPENROUTER_API_KEY",
        "SSH_AUTH_SOCK",
    ] {{
        assert!(std::env::var_os(key).is_none(), "sensitive ambient env survived: {{key}}");
    }}
    assert_eq!(std::env::var("HOME").as_deref(), Ok("/homeless"));
    assert!(!std::path::Path::new("/home/flexnetos/.ssh").exists());
    std::fs::write({:?}, b"sandbox-only").expect("write sandbox-local sentinel");
    println!("cargo:warning=mandatory-sandbox-probe");
}}
"#,
                host_sentinel
            ),
        )
        .expect("write build script");

        let outcome = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: fixture.clone(),
            store_path: None,
            raw_log_path: fixture.join("logs/cargo.log"),
            unsafe_execute_build: true,
            approver: Some("sandbox-test".to_string()),
            task_id: Some("CDB106".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("mandatory sandbox should either execute or fail closed");
        let log = fs::read_to_string(fixture.join("logs/cargo.log")).unwrap_or_default();
        assert_eq!(
            outcome.status,
            BuildCaptureStatus::Captured,
            "sandbox log:\n{log}\nvalidation={:?}\nprovenance={:?}",
            outcome.validation_errors,
            outcome.toolchain_provenance
        );

        let provenance = outcome
            .toolchain_provenance
            .first()
            .expect("successful execution must record sandbox provenance");
        assert_eq!(
            provenance.get("sandbox_backend").map(String::as_str),
            Some("bubblewrap")
        );
        assert_eq!(
            provenance.get("network_policy").map(String::as_str),
            Some("unshared")
        );
        assert_eq!(
            provenance.get("writable_root_policy").map(String::as_str),
            Some("isolated-scratch-and-ephemeral-tmp")
        );
        assert_eq!(
            provenance.get("environment_policy").map(String::as_str),
            Some("cleared_then_minimally_allowlisted")
        );
        assert_eq!(
            provenance.get("sandbox_provenance").map(String::as_str),
            Some("kernel-enforced")
        );
        assert_eq!(
            provenance.get("sandbox_cleanup").map(String::as_str),
            Some("removed")
        );
        let scratch_root = provenance
            .get("sandbox_scratch_root")
            .expect("sandbox scratch root provenance");
        assert!(
            !Path::new(scratch_root).exists(),
            "sandbox scratch root must be removed after evidence capture"
        );
        assert!(
            !Path::new(scratch_root).starts_with(&fixture),
            "sandbox scratch must never be created inside the source checkout"
        );
        for key in ["sandbox_cargo", "sandbox_rustc", "sandbox_rustdoc"] {
            let executable = provenance
                .get(key)
                .unwrap_or_else(|| panic!("missing {key} provenance"));
            assert!(
                executable.starts_with("/nix/store/"),
                "{key} must resolve to the profile-owned Nix toolchain: {executable}"
            );
        }
        assert!(
            !host_sentinel.exists(),
            "sandboxed build wrote outside its host-visible writable roots"
        );

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: default refusal
    // Defends: unsafe approval is incomplete without the selected task,
    // before-state evidence, and a cleanup plan.
    #[test]
    fn approved_execution_refuses_incomplete_approval_provenance() {
        for incomplete in [
            {
                let mut request = request(true);
                request.task_id = None;
                request
            },
            {
                let mut request = request(true);
                request.before_state = None;
                request
            },
            {
                let mut request = request(true);
                request.cleanup_plan = None;
                request
            },
        ] {
            let outcome = capture_approved_fixture_build(incomplete)
                .expect("incomplete approval must fail closed before cargo");
            assert_eq!(outcome.status, BuildCaptureStatus::Refused);
            assert!(outcome.validation_errors.iter().any(|row| {
                row.get("code").map(String::as_str) == Some("approval_provenance_incomplete")
            }));
        }
    }

    // Test lane: approved dynamic execution
    // Defends: caller-provided build environment is an explicit CodeDB
    // allowlist and cannot inject loader/compiler control variables.
    #[test]
    fn approved_execution_rejects_non_allowlisted_environment() {
        let error = capture_approved_fixture_build_with_env(
            request(true),
            &[("LD_PRELOAD", "/tmp/not-allowed.so")],
        )
        .expect_err("non-allowlisted environment must be rejected before cargo");

        assert!(matches!(
            error,
            BuildCaptureError::DisallowedEnvironment { ref key } if key == "LD_PRELOAD"
        ));
    }

    // Test lane: approved failure evidence
    // Defends: a failed build still emits a run row and a redacted failure log.
    #[test]
    fn failed_build_preserves_redacted_failure_run() {
        let fixture = temp_dir("codedb_failed_build_capture_fixture");
        fs::create_dir_all(fixture.join("src")).expect("create fixture src");
        fs::write(
            fixture.join("Cargo.toml"),
            r#"[package]
name = "codedb-failed-build-capture-fixture"
version = "0.1.0"
edition = "2024"
"#,
        )
        .expect("write manifest");
        fs::write(fixture.join("src/lib.rs"), "pub fn fixture() {}\n").expect("write lib");
        fs::write(
            fixture.join("build.rs"),
            r#"fn main() {
    panic!("password=failure-log-secret");
}
"#,
        )
        .expect("write failing build script");

        let raw_log_path = fixture.join("logs/cargo.log");
        let outcome = capture_approved_fixture_build(BuildCaptureRequest {
            repo_path: fixture.clone(),
            store_path: None,
            raw_log_path: raw_log_path.clone(),
            unsafe_execute_build: true,
            approver: Some("failure-test".to_string()),
            task_id: Some("CDB079".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        })
        .expect("cargo failure is captured as evidence");

        assert_eq!(outcome.status, BuildCaptureStatus::Failed);
        assert_eq!(outcome.build_script_runs.len(), 1);
        assert_eq!(
            outcome.build_script_runs[0]
                .get("status")
                .map(String::as_str),
            Some("failed")
        );
        assert!(outcome.validation_errors.iter().any(|row| {
            row.get("code").map(String::as_str) == Some("dynamic_build_capture_failed")
        }));
        let log = fs::read_to_string(raw_log_path).expect("read failure log");
        assert!(!log.contains("failure-log-secret"));
        assert!(log.contains("password=[REDACTED]"));

        let _ = fs::remove_dir_all(fixture);
    }

    // Test lane: deterministic OUT_DIR reproduction
    // Defends: CDB080 stores exact regular-file payloads, deterministically
    // orders the manifest, and reproduces a checksum-identical artifact tree.
    #[test]
    fn out_dir_manifest_is_deterministic_and_reproduces_exact_bytes() {
        let source = temp_dir("codedb_out_dir_manifest_source");
        fs::create_dir_all(source.join("nested")).expect("create source");
        fs::write(source.join("z.rs"), b"pub const Z: u8 = 26;\n").expect("write z");
        fs::write(source.join("nested/a.bin"), [0_u8, 1, 2, 0xff]).expect("write binary");
        let request = BuildCaptureRequest {
            repo_path: source.clone(),
            store_path: None,
            raw_log_path: source.join("capture.log"),
            unsafe_execute_build: true,
            approver: Some("reproduction-test".to_string()),
            task_id: Some("CDB080".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        };

        let first =
            capture_out_dir_artifacts(&source, "fixture 0.1.0", &request).expect("first capture");
        let second =
            capture_out_dir_artifacts(&source, "fixture 0.1.0", &request).expect("second capture");

        let stable_projection = |rows: &[Row]| {
            rows.iter()
                .map(|row| {
                    (
                        row["relative_path"].clone(),
                        row["sha256"].clone(),
                        row["content_hex"].clone(),
                        row["reproduction_sha256"].clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(stable_projection(&first), stable_projection(&second));
        assert_eq!(
            first
                .iter()
                .map(|row| row["relative_path"].as_str())
                .collect::<Vec<_>>(),
            vec!["nested/a.bin", "z.rs"]
        );

        let destination = temp_dir("codedb_out_dir_manifest_destination");
        fs::create_dir(&destination).expect("create destination");
        let destination_root =
            ReproductionRoot::open_existing(&destination).expect("open destination");
        let proof =
            reproduce_out_dir_artifacts(&first, &destination_root).expect("reproduce artifacts");
        assert_eq!(proof.len(), 2);
        assert_eq!(
            fs::read(destination.join("nested/a.bin")).expect("read reproduced binary"),
            [0_u8, 1, 2, 0xff]
        );
        assert_eq!(
            sha256_hex(&fs::read(destination.join("z.rs")).expect("read reproduced rust")),
            first
                .iter()
                .find(|row| row["relative_path"] == "z.rs")
                .expect("z row")["sha256"]
        );
        assert!(proof.iter().all(|row| {
            row.get("status").map(String::as_str) == Some("verified")
                && row.get("proof").map(String::as_str) == Some("reproduced-bytes-sha256-match")
        }));

        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(destination);
    }

    // Test lane: reproduction containment
    // Defends: a forged artifact row cannot escape the reproduction root.
    #[test]
    fn out_dir_reproduction_rejects_escaping_paths() {
        let destination = temp_dir("codedb_out_dir_escape_destination");
        fs::create_dir(&destination).expect("create destination");
        let destination_root =
            ReproductionRoot::open_existing(&destination).expect("open destination");
        let forged = row([
            ("relative_path", "../escape.rs".to_string()),
            ("file_kind", "file".to_string()),
            ("content_hex", "657363617065".to_string()),
            ("sha256", sha256_hex(b"escape")),
        ]);

        let error = reproduce_out_dir_artifacts(&[forged], &destination_root)
            .expect_err("escaping artifact path must be rejected");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            !destination
                .parent()
                .expect("parent")
                .join("escape.rs")
                .exists()
        );
    }

    // Test lane: redaction
    // Defends: common credential-shaped bare tokens are redacted even when a
    // tool prints them without a key=value label.
    #[test]
    fn redaction_covers_bare_secret_tokens() {
        let redacted = redact_text(
            "bearer ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD sk-proj-abcdefghijklmnop",
        );

        assert!(!redacted.contains("ghp_"));
        assert!(!redacted.contains("sk-proj-"));
        assert_eq!(redacted, "bearer [REDACTED] [REDACTED]");
    }

    // Test lane: redaction
    // Defends: secrets nested in Cargo JSON diagnostics are recursively
    // redacted, not only build-script env arrays.
    #[test]
    fn cargo_json_redaction_covers_nested_diagnostic_strings() {
        let input = serde_json::json!({
            "reason": "compiler-message",
            "message": {
                "rendered": "warning: token=nested-json-secret ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD"
            }
        })
        .to_string();

        let redacted = redact_cargo_json_output(&input);
        assert!(!redacted.contains("nested-json-secret"));
        assert!(!redacted.contains("ghp_"));
        assert!(redacted.contains("[REDACTED]"));
    }

    // Test lane: redaction
    // Defends: database credentials and common CI/package credential families
    // never survive build/compiler stream sanitization, including encoded forms.
    #[test]
    fn redaction_covers_database_uri_and_ci_token_families() {
        let input = concat!(
            "DATABASE_URL=postgresql://dbuser:db-password-sentinel@db.example/app?sslmode=require&token=query-token-sentinel\n",
            "DSN = postgres://encoded:p%40ss-encoded-sentinel@db.example/app?password=query-password-sentinel\n",
            "dsn = opaque-dsn-sentinel\n",
            "connection_string: host=db.example password=connection-password-sentinel\n",
            "Authorization: Bearer bearer-token-sentinel\n",
            "jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJqd3Qtc2VudGluZWwifQ.jwt-signature-sentinel\n",
            "AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE AWS_SECRET_ACCESS_KEY=aws-secret-sentinel\n",
            "npm_token=npm_npm-token-sentinel\n",
            "DATABASE_URL%3Dpostgresql%3A%2F%2Fencoded%3Apercent-credential-sentinel%40db.example%2Fapp\n",
            "-----BEGIN PRIVATE KEY-----\nprivate-key-sentinel\n-----END PRIVATE KEY-----\n",
        );

        let redacted = redact_text(input);

        assert_no_sentinels(&redacted);
        assert!(redacted.contains("db.example"));
        assert!(redacted.contains("[REDACTED]"));
    }

    // Test lane: redaction
    // Defends: valid Cargo JSON is recursively key-aware while malformed or
    // unrecognized lines are represented only by non-reversible metadata.
    #[test]
    fn cargo_json_redaction_is_key_aware_and_malformed_lines_are_metadata_only() {
        let valid = serde_json::json!({
            "reason": "compiler-message",
            "DATABASE_URL": "postgresql://user:json-database-sentinel@db.example/app",
            "message": {
                "rendered": "warning: Authorization: Bearer json-bearer-sentinel",
                "children": [{
                    "message": "linker -Wl,-rpath,postgresql://user:json-linker-sentinel@db.example/app"
                }]
            },
            "env": [
                ["DSN", "postgres://user:json-dsn-sentinel@db.example/app"],
                ["CODEDB_FIXTURE_BUILD_SCRIPT", "observed"],
                ["UNRECOGNIZED_BUILD_VALUE", "json-env-opaque-sentinel"]
            ]
        })
        .to_string();
        let malformed =
            r#"{"reason":"compiler-message","message":"DATABASE_URL=malformed-json-sentinel""#;
        let input = format!("{valid}\n{malformed}");

        let redacted = redact_cargo_json_output(&input);

        assert_no_sentinels(&redacted);
        assert!(redacted.contains("observed"));
        assert!(redacted.contains("metadata-only"));
        assert!(redacted.contains("sha256="));
    }

    // Test lane: redaction
    // Defends: Cargo warnings plus build-script stdout/compiler stderr retain
    // recognized safe evidence, redact credential forms, and hash unknown lines.
    #[test]
    fn cargo_warning_stdout_stderr_and_unknown_streams_are_fail_closed() {
        let stdout = redact_build_script_stream(concat!(
            "cargo:warning=safe-build-warning\n",
            "cargo:warning=DATABASE_URL=postgresql://user:stdout-database-sentinel@db.example/app\n",
            "opaque-build-output-sentinel\n",
        ));
        let stderr = redact_compiler_stream(concat!(
            "warning: safe-compiler-warning\n",
            "error: Authorization: Bearer stderr-bearer-sentinel\n",
            "opaque-compiler-output-sentinel\n",
        ));
        let json = redact_cargo_json_output(
            &serde_json::json!({
                "reason": "compiler-message",
                "mystery_stream": "opaque-json-stream-sentinel"
            })
            .to_string(),
        );
        let rendered = format!("{stdout}\n{stderr}\n{json}");

        assert_no_sentinels(&rendered);
        assert!(rendered.contains("safe-build-warning"));
        assert!(rendered.contains("safe-compiler-warning"));
        assert!(rendered.contains("metadata-only"));
        assert!(rendered.contains("sha256="));
    }

    // Test lane: redaction
    // Defends: only explicitly safe observed environment values retain their
    // value; unknown values become metadata-only and credential keys redact.
    #[test]
    fn environment_rows_preserve_allowlisted_safe_values_and_summarize_unknown_values() {
        let observations = vec![BuildScriptObservation {
            package_id: "fixture 0.1.0".to_string(),
            out_dir: None,
            environment: vec![
                (
                    "CODEDB_FIXTURE_BUILD_SCRIPT".to_string(),
                    "observed".to_string(),
                ),
                (
                    "DATABASE_URL".to_string(),
                    "postgresql://user:env-database-sentinel@db.example/app".to_string(),
                ),
                (
                    "UNRECOGNIZED_BUILD_VALUE".to_string(),
                    "opaque-env-sentinel".to_string(),
                ),
            ],
            linked_libs: Vec::new(),
            linked_paths: Vec::new(),
        }];

        let rows = build_script_env_rows(&observations, &request(true));
        let rendered = format!("{rows:?}");

        assert_no_sentinels(&rendered);
        assert!(rows.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("CODEDB_FIXTURE_BUILD_SCRIPT")
                && row.get("value").map(String::as_str) == Some("observed")
        }));
        assert!(rows.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("DATABASE_URL")
                && row.get("value").map(String::as_str) == Some("[REDACTED]")
        }));
        assert!(rows.iter().any(|row| {
            row.get("key").map(String::as_str) == Some("UNRECOGNIZED_BUILD_VALUE")
                && row
                    .get("value")
                    .is_some_and(|value| value.contains("metadata-only"))
        }));
    }

    // Test lane: redaction
    // Defends: linker evidence keeps benign positive facts while credentials
    // embedded in Cargo linker observations or rustc-link-arg values disappear.
    #[test]
    fn linker_rows_preserve_safe_evidence_without_embedded_credentials() {
        let request = request(true);
        let observations = vec![BuildScriptObservation {
            package_id: "fixture 0.1.0".to_string(),
            out_dir: None,
            environment: Vec::new(),
            linked_libs: vec![
                "static=codedb_fixture_native".to_string(),
                "postgresql://user:linked-lib-sentinel@db.example/app".to_string(),
            ],
            linked_paths: Vec::new(),
        }];
        let instructions = vec![row([
            ("instruction", "rustc-link-arg".to_string()),
            (
                "value",
                "-Wl,-rpath,postgresql://user:link-arg-sentinel@db.example/app?token=link-query-sentinel"
                    .to_string(),
            ),
            ("package_id", "fixture 0.1.0".to_string()),
            ("out_dir", String::new()),
        ])];

        let facts = native_link_facts_from_observations_and_instructions(
            &observations,
            &instructions,
            &request,
        );
        let rendered = format!("{facts:?}");

        assert_no_sentinels(&rendered);
        assert!(facts.iter().any(|fact| {
            fact.get("value").map(String::as_str) == Some("static=codedb_fixture_native")
        }));
    }

    // Test lane: redaction
    // Defends: operator-controlled approval fields and public Debug/Error
    // formatting cannot become a side channel for sentinels.
    #[test]
    fn approval_rows_and_debug_error_formatting_never_expose_sentinels() {
        let mut request = request(true);
        request.approver = Some("Bearer approval-bearer-sentinel".to_string());
        request.task_id = Some("DATABASE_URL=approval-task-sentinel".to_string());
        request.before_state = Some("npm_approval-before-sentinel".to_string());
        request.cleanup_plan =
            Some("postgresql://user:approval-cleanup-sentinel@db.example/app".to_string());

        let request_debug = format!("{request:?}");
        let outcome = capture_build(request);
        let outcome_debug = format!("{outcome:?}");
        let outcome_rows = format!("{:?}", outcome.unsafe_execution_approval);
        let error = BuildCaptureError::DisallowedEnvironment {
            key: "DATABASE_URL=error-format-sentinel".to_string(),
        };
        let io_error = BuildCaptureError::SpawnCargo {
            path: PathBuf::from("/tmp/DATABASE_URL=path-error-sentinel"),
            source: io::Error::other("opaque-io-error-sentinel"),
        };
        let error_rendered = format!("{error} {error:?} {io_error} {io_error:?}");

        assert_no_sentinels(&request_debug);
        assert_no_sentinels(&outcome_debug);
        assert_no_sentinels(&outcome_rows);
        assert_no_sentinels(&error_rendered);
        assert_eq!(outcome.status, BuildCaptureStatus::Refused);
        assert_eq!(
            outcome.unsafe_execution_approval[0]
                .get("status")
                .map(String::as_str),
            Some("capability_required")
        );
    }

    // Test lane: redaction
    // Defends: the persisted raw evidence log applies the same policy to
    // approval provenance, Cargo output, and captured build-script streams.
    #[test]
    fn raw_log_never_exposes_approval_or_stream_sentinels() {
        let directory = temp_dir("codedb_raw_log_redaction");
        fs::create_dir_all(&directory).expect("create raw-log root");
        let raw_log_path = directory.join("capture.log");
        let mut request = request(true);
        request.raw_log_path = raw_log_path.clone();
        request.approver = Some("Bearer approval-bearer-sentinel".to_string());
        request.task_id = Some("DATABASE_URL=approval-task-sentinel".to_string());
        request.before_state = Some("npm_approval-before-sentinel".to_string());
        request.cleanup_plan =
            Some("postgresql://user:approval-cleanup-sentinel@db.example/app".to_string());
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("rustc output for raw-log redaction fixture");
        let stream_raw = concat!(
            "cargo:warning=Authorization: Bearer stdout-stream-bearer-sentinel\n",
            "opaque-raw-stream-sentinel\n",
        )
        .to_string();
        let streams = vec![CapturedStream {
            package_id: "fixture 0.1.0".to_string(),
            out_dir: directory.join("out"),
            stream: "stdout",
            source_path: directory.join("output"),
            redacted: redact_build_script_stream(&stream_raw),
            raw: stream_raw,
        }];

        let root = ReproductionRoot::open_existing(&directory).expect("open trusted log root");
        write_redacted_raw_log(&root, &raw_log_path, &request, &output, &streams, &[])
            .expect("write redacted raw log");
        let log = fs::read_to_string(&raw_log_path).expect("read redacted raw log");

        assert_no_sentinels(&log);
        assert!(log.contains("metadata-only"));
        assert!(log.contains("[REDACTED]"));

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn approval_binds_exact_environment_and_rejects_duplicate_or_ambiguous_order() {
        assert!(matches!(
            normalize_approved_environment(&[
                ("CODEDB_FIXTURE_LOG_SECRET", "a"),
                ("CODEDB_FIXTURE_LOG_SECRET", "b"),
            ]),
            Err(BuildCaptureError::AmbiguousEnvironment { .. })
        ));
        assert!(matches!(
            normalize_approved_environment(&[
                ("CODEDB_PROC_MACRO_LOG_PATH", "a"),
                ("CODEDB_FIXTURE_LOG_SECRET", "b"),
            ]),
            Err(BuildCaptureError::AmbiguousEnvironment { .. })
        ));

        let fixture = temp_dir("codedb_environment_binding");
        fs::create_dir_all(&fixture).expect("create fixture");
        let mut approved = request(true);
        approved.repo_path = fixture.clone();
        approved.raw_log_path = fixture.join("capture.log");
        let approved_env = [("CODEDB_FIXTURE_LOG_SECRET", "approved")];
        let changed_env = [("CODEDB_FIXTURE_LOG_SECRET", "changed")];
        let normalized =
            normalize_approved_environment(&approved_env).expect("normalize approved env");
        let sandbox = prepare_mandatory_sandbox(&approved, &approved_env).expect("prepare sandbox");
        let authority = ExecutionApprovalAuthority::new().expect("authority");
        let capability = authority
            .approve(&approved, &normalized, &sandbox)
            .expect("approve exact plan");
        let error = super::capture_approved_fixture_build_with_capability(
            &authority,
            capability,
            approved,
            &changed_env,
            sandbox,
        )
        .expect_err("changed environment must invalidate capability");
        assert!(matches!(
            error,
            BuildCaptureError::ApprovalCapability {
                reason: "capability does not match the exact build request"
            }
        ));
        let _ = fs::remove_dir_all(fixture);
    }

    #[cfg(unix)]
    #[test]
    fn raw_log_publication_is_descriptor_bound_no_replace_and_no_symlink() {
        use std::os::unix::fs::symlink;

        let root_path = temp_dir("codedb_raw_log_root");
        let outside = temp_dir("codedb_raw_log_outside");
        fs::create_dir_all(&root_path).expect("create root");
        fs::create_dir_all(&outside).expect("create outside");
        symlink(&outside, root_path.join("logs")).expect("create ancestor symlink");
        let root = ReproductionRoot::open_existing(&root_path).expect("open root");
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("rustc output");
        let mut request = request(true);
        request.repo_path = root_path.clone();
        request.raw_log_path = root_path.join("logs/capture.log");
        assert!(
            write_redacted_raw_log(&root, &request.raw_log_path, &request, &output, &[], &[])
                .is_err()
        );
        assert!(!outside.join("capture.log").exists());

        fs::remove_file(root_path.join("logs")).expect("remove symlink");
        fs::create_dir(root_path.join("logs")).expect("create logs");
        fs::write(root_path.join("logs/capture.log"), b"trusted-existing")
            .expect("write existing log");
        assert!(
            write_redacted_raw_log(&root, &request.raw_log_path, &request, &output, &[], &[])
                .is_err()
        );
        assert_eq!(
            fs::read(root_path.join("logs/capture.log")).expect("read existing"),
            b"trusted-existing"
        );
        let _ = fs::remove_dir_all(root_path);
        let _ = fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn out_dir_batch_rolls_back_and_rejects_existing_or_symlinked_destinations() {
        use std::os::unix::fs::symlink;

        let destination = temp_dir("codedb_reproduction_security");
        let outside = temp_dir("codedb_reproduction_outside");
        fs::create_dir_all(&destination).expect("create destination");
        fs::create_dir_all(&outside).expect("create outside");
        fs::write(destination.join("collision"), b"existing").expect("write collision");
        let root = ReproductionRoot::open_existing(&destination).expect("open root");
        let artifact = |path: &str, bytes: &[u8]| {
            row([
                ("relative_path", path.to_string()),
                ("file_kind", "file".to_string()),
                ("content_hex", hex_encode(bytes)),
                ("sha256", sha256_hex(bytes)),
            ])
        };
        let error = reproduce_out_dir_artifacts(
            &[artifact("first", b"first"), artifact("collision", b"new")],
            &root,
        )
        .expect_err("collision must roll back the whole batch");
        assert!(matches!(
            error.kind(),
            io::ErrorKind::AlreadyExists | io::ErrorKind::Other
        ));
        assert!(!destination.join("first").exists());
        assert_eq!(
            fs::read(destination.join("collision")).unwrap(),
            b"existing"
        );

        symlink(&outside, destination.join("ancestor")).expect("create ancestor symlink");
        assert!(
            reproduce_out_dir_artifacts(&[artifact("ancestor/escape", b"escape")], &root).is_err()
        );
        assert!(!outside.join("escape").exists());

        symlink("missing", destination.join("final-link")).expect("create final symlink");
        assert!(reproduce_out_dir_artifacts(&[artifact("final-link", b"replace")], &root).is_err());
        assert_eq!(
            fs::read_link(destination.join("final-link")).expect("read final symlink"),
            PathBuf::from("missing")
        );
        let _ = fs::remove_dir_all(destination);
        let _ = fs::remove_dir_all(outside);
    }

    // Test lane: redaction
    // Defends: proc-macro evidence remains hash-only when names or token
    // streams contain credentials, while safe macro names remain useful.
    #[test]
    fn proc_macro_rows_and_rewritten_logs_never_expose_sentinels() {
        let directory = temp_dir("codedb_proc_macro_redaction");
        fs::create_dir_all(&directory).expect("create proc-macro redaction fixture");
        let log_path = directory.join("proc-macro.log");
        fs::write(
            &log_path,
            concat!(
                "macro_name=Bearer proc-macro-name-sentinel\n",
                "input=DATABASE_URL=proc-macro-input-sentinel\n",
                "output=npm_proc-macro-output-sentinel\n",
                "---\n",
                "malformed=private-key-malformed-sentinel\n",
            ),
        )
        .expect("write proc-macro evidence");
        let log_value = log_path.display().to_string();

        let evidence = capture_proc_macro_evidence(
            &[("CODEDB_PROC_MACRO_LOG_PATH", log_value.as_str())],
            &request(true),
        );
        let rendered = format!("{evidence:?}");
        let rewritten = fs::read_to_string(&log_path).expect("read rewritten proc-macro log");

        assert_no_sentinels(&rendered);
        assert_no_sentinels(&rewritten);
        assert!(
            evidence
                .inputs
                .iter()
                .all(|row| row.get("capture").map(String::as_str) == Some("hash-only"))
        );
        assert!(rewritten.contains("sha256="));

        let _ = fs::remove_dir_all(directory);
    }

    fn assert_no_sentinels(value: &str) {
        for sentinel in [
            "db-password-sentinel",
            "p%40ss-encoded-sentinel",
            "query-token-sentinel",
            "query-password-sentinel",
            "opaque-dsn-sentinel",
            "connection-password-sentinel",
            "bearer-token-sentinel",
            "jwt-signature-sentinel",
            "aws-secret-sentinel",
            "npm-token-sentinel",
            "percent-credential-sentinel",
            "private-key-sentinel",
            "json-database-sentinel",
            "json-bearer-sentinel",
            "json-linker-sentinel",
            "json-dsn-sentinel",
            "json-env-opaque-sentinel",
            "malformed-json-sentinel",
            "stdout-database-sentinel",
            "opaque-build-output-sentinel",
            "stderr-bearer-sentinel",
            "opaque-compiler-output-sentinel",
            "opaque-json-stream-sentinel",
            "env-database-sentinel",
            "opaque-env-sentinel",
            "linked-lib-sentinel",
            "link-arg-sentinel",
            "link-query-sentinel",
            "approval-bearer-sentinel",
            "approval-task-sentinel",
            "approval-before-sentinel",
            "approval-cleanup-sentinel",
            "error-format-sentinel",
            "path-error-sentinel",
            "opaque-io-error-sentinel",
            "stdout-stream-bearer-sentinel",
            "opaque-raw-stream-sentinel",
            "proc-macro-name-sentinel",
            "proc-macro-input-sentinel",
            "proc-macro-output-sentinel",
            "private-key-malformed-sentinel",
        ] {
            assert!(
                !value.contains(sentinel),
                "redaction leaked sentinel {sentinel}: {value}"
            );
        }
    }

    fn copy_fixture_tree(source: &Path, destination: &Path, files: &[&str]) {
        for relative_path in files {
            let destination_path = destination.join(relative_path);
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).expect("create fixture parent");
            }
            fs::copy(source.join(relative_path), destination_path).expect("copy fixture file");
        }
    }

    #[test]
    fn artifact_hashes_use_standard_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn request(unsafe_execute_build: bool) -> BuildCaptureRequest {
        BuildCaptureRequest {
            repo_path: PathBuf::from("/tmp/codedb-fixture"),
            store_path: None,
            raw_log_path: PathBuf::from("/tmp/codedb-build-capture.log"),
            unsafe_execute_build,
            approver: Some("test".to_string()),
            task_id: Some("CDB078,CDB079,CDB080,CDB082".to_string()),
            before_state: Some("fixture-source-copied-and-unchanged".to_string()),
            cleanup_plan: Some("remove isolated fixture and cargo target after proof".to_string()),
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{suffix}"))
    }
}

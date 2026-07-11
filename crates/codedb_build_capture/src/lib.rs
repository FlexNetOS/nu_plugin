#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

pub const STATUS: &str = "unsafe_build_capture_gate_available";
pub const UNSAFE_FLAG: &str = "--unsafe-execute-build";

pub type Row = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCaptureRequest {
    pub repo_path: PathBuf,
    pub store_path: Option<PathBuf>,
    pub raw_log_path: PathBuf,
    pub unsafe_execute_build: bool,
    pub approver: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCaptureOutcome {
    pub status: BuildCaptureStatus,
    pub unsafe_execution_approval: Vec<Row>,
    pub build_script_runs: Vec<Row>,
    pub build_script_env: Vec<Row>,
    pub build_script_stdout: Vec<Row>,
    pub build_script_stderr: Vec<Row>,
    pub build_script_cargo_instructions: Vec<Row>,
    pub proc_macro_invocations: Vec<Row>,
    pub proc_macro_output_token_streams: Vec<Row>,
    pub native_link_facts: Vec<Row>,
    pub out_dir_artifacts: Vec<Row>,
    pub toolchain_provenance: Vec<Row>,
    pub validation_errors: Vec<Row>,
    pub capture_gaps: Vec<Row>,
    pub raw_log_paths: Vec<Row>,
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

#[derive(Debug)]
pub enum BuildCaptureError {
    CreateLogDir { path: PathBuf, source: io::Error },
    WriteLog { path: PathBuf, source: io::Error },
    SpawnCargo { path: PathBuf, source: io::Error },
}

impl Display for BuildCaptureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CreateLogDir { path, source } => {
                write!(
                    f,
                    "failed to create log directory {}: {source}",
                    path.display()
                )
            }
            Self::WriteLog { path, source } => {
                write!(
                    f,
                    "failed to write raw capture log {}: {source}",
                    path.display()
                )
            }
            Self::SpawnCargo { path, source } => {
                write!(
                    f,
                    "failed to run cargo check in {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl StdError for BuildCaptureError {}

#[derive(Debug, Clone)]
struct BuildScriptObservation {
    package_id: String,
    out_dir: Option<PathBuf>,
    environment: Vec<(String, String)>,
    linked_libs: Vec<String>,
    linked_paths: Vec<String>,
}

#[derive(Debug, Clone)]
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
    outputs: Vec<Row>,
    log_summary: Vec<String>,
}

pub fn capture_build(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    if !request.unsafe_execute_build {
        return refused_capture(request);
    }

    approved_scaffold(request)
}

pub fn capture_approved_fixture_build(
    request: BuildCaptureRequest,
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    capture_approved_fixture_build_with_env(request, &[])
}

pub fn capture_approved_fixture_build_with_env(
    request: BuildCaptureRequest,
    environment: &[(&str, &str)],
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    if !request.unsafe_execute_build {
        return Ok(refused_capture(request));
    }

    let target_dir = isolated_target_dir(&request);
    let mut command = Command::new("cargo");
    command
        .args(["check", "--message-format=json"])
        .current_dir(&request.repo_path);
    for (key, value) in environment {
        command.env(key, value);
    }
    command
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("CARGO_INCREMENTAL", "0");
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
    let observations = build_script_observations_from_cargo_json(&stdout);
    let streams = capture_build_script_streams(&observations);
    let mut build_script_env = build_script_env_rows(&observations, &request);
    build_script_env.extend(approved_environment_rows(environment, &request));
    let build_script_cargo_instructions = build_script_instruction_rows(&streams, &request);
    let native_link_facts = native_link_facts_from_observations(&observations, &request);
    let mut validation_errors = Vec::new();
    let mut out_dir_artifacts = Vec::new();
    for observation in &observations {
        let Some(out_dir) = observation.out_dir.as_deref() else {
            continue;
        };
        match capture_out_dir_artifacts(out_dir, &observation.package_id, &request) {
            Ok(mut artifacts) => out_dir_artifacts.append(&mut artifacts),
            Err(source) => validation_errors.push(row([
                ("table", "validation_errors".to_string()),
                ("code", "out_dir_artifact_capture_failed".to_string()),
                ("package_id", observation.package_id.clone()),
                ("out_dir", out_dir.display().to_string()),
                ("message", redact_text(&source.to_string())),
            ])),
        }
    }
    let proc_macro_evidence = capture_proc_macro_evidence(environment, &request);
    write_redacted_raw_log(
        &request.raw_log_path,
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
    if observations
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

    Ok(BuildCaptureOutcome {
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
        proc_macro_output_token_streams: proc_macro_evidence.outputs,
        native_link_facts,
        out_dir_artifacts,
        toolchain_provenance: vec![toolchain_provenance(&target_dir)],
        validation_errors: {
            if !output.status.success() {
                validation_errors.push(row([
                    ("table", "validation_errors".to_string()),
                    ("code", "dynamic_build_capture_failed".to_string()),
                    (
                        "message",
                        first_non_empty_line(&redact_text(&stderr))
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
    })
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
        ],
        raw_log_paths: vec![raw_log_row(&request, "not_written")],
    }
}

fn approved_scaffold(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    BuildCaptureOutcome {
        status: BuildCaptureStatus::ApprovedScaffold,
        unsafe_execution_approval: vec![approval_row(
            &request,
            "approved",
            "unsafe approval was supplied; this approval-only API does not execute Cargo",
        )],
        build_script_runs: Vec::new(),
        build_script_env: Vec::new(),
        build_script_stdout: Vec::new(),
        build_script_stderr: Vec::new(),
        build_script_cargo_instructions: Vec::new(),
        proc_macro_invocations: Vec::new(),
        proc_macro_output_token_streams: Vec::new(),
        native_link_facts: Vec::new(),
        out_dir_artifacts: Vec::new(),
        toolchain_provenance: Vec::new(),
        validation_errors: Vec::new(),
        capture_gaps: vec![row([
            ("table", "capture_gaps".to_string()),
            ("missing_truth", "dynamic_capture_runner".to_string()),
            (
                "reason",
                "call capture_approved_fixture_build to run the approved compiler/build capture"
                    .to_string(),
            ),
            ("required_task", "CDB034".to_string()),
        ])],
        raw_log_paths: vec![raw_log_row(&request, "reserved")],
    }
}

fn approval_row(request: &BuildCaptureRequest, status: &str, note: &str) -> Row {
    row([
        ("table", "unsafe_execution_approval".to_string()),
        ("status", status.to_string()),
        ("flag", UNSAFE_FLAG.to_string()),
        (
            "approver",
            request
                .approver
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
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
        ("note", note.to_string()),
    ])
}

fn raw_log_row(request: &BuildCaptureRequest, status: &str) -> Row {
    row([
        ("table", "raw_log_paths".to_string()),
        ("status", status.to_string()),
        ("path", request.raw_log_path.display().to_string()),
        (
            "note",
            "redacted command/build evidence path; default and approval-only calls do not write it"
                .to_string(),
        ),
    ])
}

fn out_dir_artifact_gap(request: &BuildCaptureRequest) -> Row {
    row([
        ("table", "capture_gaps".to_string()),
        ("missing_truth", "out_dir_artifacts".to_string()),
        (
            "reason",
            "Cargo reported a build-script execution without an observable OUT_DIR path"
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
    observations
        .iter()
        .map(|observation| {
            row([
                ("table", "build_script_runs".to_string()),
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
        .collect()
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
                    ("status", "observed".to_string()),
                    ("package_id", observation.package_id.clone()),
                    ("key", key.clone()),
                    ("value", redact_value_for_key(key, value)),
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
                ("status", "provided".to_string()),
                ("key", (*key).to_string()),
                ("value", redact_value_for_key(key, value)),
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
                redacted: redact_text(&raw),
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

fn native_link_facts_from_observations(
    observations: &[BuildScriptObservation],
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
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
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
            artifact.insert("file_kind".to_string(), "file".to_string());
            artifact.insert("sha256".to_string(), sha256_hex(&fs::read(&path)?));
        } else if metadata.file_type().is_symlink() {
            artifact.insert("file_kind".to_string(), "symlink".to_string());
            artifact.insert(
                "link_target".to_string(),
                fs::read_link(&path)?.display().to_string(),
            );
            artifact.insert(
                "materialization".to_string(),
                "metadata_only_fallback".to_string(),
            );
        } else {
            artifact.insert("file_kind".to_string(), "other".to_string());
        }
        artifacts.push(artifact);
    }
    Ok(())
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
        "proc_macro_evidence=unrecognized\n".to_string()
    } else {
        format!("{}\n", evidence.log_summary.join("\n"))
    };
    if fs::write(&path, summary).is_err() {
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
    let input_sha256 = sha256_hex(input.as_bytes());
    let output_sha256 = sha256_hex(output.as_bytes());
    evidence.invocations.push(row([
        ("table", "proc_macro_invocations".to_string()),
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
    evidence.outputs.push(row([
        ("table", "proc_macro_output_token_streams".to_string()),
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

fn toolchain_provenance(target_dir: &Path) -> Row {
    let mut row = Row::new();
    row.insert("table".to_string(), "toolchain_provenance".to_string());
    row.insert("provenance".to_string(), "rustc -vV".to_string());
    row.insert(
        "isolated_target_dir".to_string(),
        target_dir.display().to_string(),
    );
    match Command::new("rustc").arg("-vV").output() {
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
    row
}

fn write_redacted_raw_log(
    path: &Path,
    output: &std::process::Output,
    streams: &[CapturedStream],
    proc_macro_log_summary: &[String],
) -> Result<(), BuildCaptureError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| BuildCaptureError::CreateLogDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut body = format!(
        "status={}\nexit_code={}\nredaction=applied\n--- cargo stdout ---\n{}\n--- cargo stderr ---\n{}\n",
        output.status,
        output.status.code().unwrap_or(-1),
        redact_cargo_json_output(&String::from_utf8_lossy(&output.stdout)),
        redact_text(&String::from_utf8_lossy(&output.stderr))
    );
    for stream in streams {
        body.push_str(&format!(
            "--- build script {} ({}) ---\n{}\n",
            stream.stream, stream.package_id, stream.redacted
        ));
    }
    if !proc_macro_log_summary.is_empty() {
        body.push_str("--- proc macro evidence ---\n");
        body.push_str(&proc_macro_log_summary.join("\n"));
        body.push('\n');
    }
    fs::write(path, body).map_err(|source| BuildCaptureError::WriteLog {
        path: path.to_path_buf(),
        source,
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

fn sha256_hex(bytes: &[u8]) -> String {
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
    hash.iter().map(|word| format!("{word:08x}")).collect()
}

fn redact_value_for_key(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        "[REDACTED]".to_string()
    } else {
        redact_text(value)
    }
}

fn redact_text(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(redact_token)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_token(token: &str) -> String {
    if let Some((key, value)) = token.split_once('=') {
        if is_sensitive_key(key) {
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
    token.to_string()
}

fn redact_cargo_json_output(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            let Ok(mut message) = serde_json::from_str::<serde_json::Value>(line) else {
                return redact_text(line);
            };
            if let Some(environment) = message
                .get_mut("env")
                .and_then(serde_json::Value::as_array_mut)
            {
                for entry in environment {
                    let Some(values) = entry.as_array_mut() else {
                        continue;
                    };
                    let Some(key) = values.first().and_then(serde_json::Value::as_str) else {
                        continue;
                    };
                    if is_sensitive_key(key)
                        && let Some(value) = values.get_mut(1)
                    {
                        *value = serde_json::Value::String("[REDACTED]".to_string());
                    }
                }
            }
            serde_json::to_string(&message).unwrap_or_else(|_| redact_text(line))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "secret",
        "token",
        "password",
        "credential",
        "authorization",
        "api_key",
        "private_key",
    ]
    .iter()
    .any(|marker| key.contains(marker))
}

fn first_non_empty_line(value: &str) -> Option<&str> {
    value.lines().find(|line| !line.trim().is_empty())
}

fn row<const N: usize>(pairs: [(&str, String); N]) -> Row {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    }

    // Test lane: default
    // Defends: CDB033 approval scaffold records approval without claiming execution.
    #[test]
    fn capture_build_records_approval_scaffold_with_unsafe_flag() {
        let outcome = capture_build(request(true));

        assert_eq!(outcome.status, BuildCaptureStatus::ApprovedScaffold);
        assert!(outcome.validation_errors.is_empty());
        assert_eq!(
            outcome.unsafe_execution_approval[0]
                .get("status")
                .map(String::as_str),
            Some("approved")
        );
        assert_eq!(
            outcome.capture_gaps[0]
                .get("required_task")
                .map(String::as_str),
            Some("CDB034")
        );
        assert_eq!(
            outcome.raw_log_paths[0].get("status").map(String::as_str),
            Some("reserved")
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
        }));
        assert!(!outcome.capture_gaps.iter().any(|gap| {
            gap.get("missing_truth").map(String::as_str) == Some("out_dir_artifacts")
        }));

        let raw_log = fs::read_to_string(&raw_log_path).expect("read raw log");
        assert!(
            raw_log.contains("generated.rs")
                || raw_log.contains("codedb_fixture_out_dir_generator")
        );

        let _ = fs::remove_dir_all(fixture);
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

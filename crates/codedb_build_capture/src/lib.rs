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
    pub proc_macro_invocations: Vec<Row>,
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

pub fn capture_build(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    if !request.unsafe_execute_build {
        return refused_capture(request);
    }

    approved_scaffold(request)
}

pub fn capture_approved_fixture_build(
    request: BuildCaptureRequest,
) -> Result<BuildCaptureOutcome, BuildCaptureError> {
    if !request.unsafe_execute_build {
        return Ok(refused_capture(request));
    }

    let output = Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(&request.repo_path)
        .output()
        .map_err(|source| BuildCaptureError::SpawnCargo {
            path: request.repo_path.clone(),
            source,
        })?;

    write_raw_log(&request.raw_log_path, &output)?;
    let status = if output.status.success() {
        BuildCaptureStatus::Captured
    } else {
        BuildCaptureStatus::Failed
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let observed_warning =
        stdout.contains("build-script-probe") || stderr.contains("build-script-probe");

    Ok(BuildCaptureOutcome {
        status,
        unsafe_execution_approval: vec![approval_row(
            &request,
            "approved",
            "unsafe approval was supplied for approved fixture dynamic capture",
        )],
        build_script_runs: vec![row([
            ("table", "build_script_runs".to_string()),
            ("repo_path", request.repo_path.display().to_string()),
            ("status", status.as_str().to_string()),
            (
                "exit_code",
                output.status.code().unwrap_or(-1).to_string(),
            ),
            (
                "stdout_bytes",
                output.stdout.len().to_string(),
            ),
            (
                "stderr_bytes",
                output.stderr.len().to_string(),
            ),
            (
                "observed_warning",
                observed_warning.to_string(),
            ),
        ])],
        proc_macro_invocations: vec![row([
            ("table", "proc_macro_invocations".to_string()),
            ("status", "not_observed".to_string()),
            (
                "reason",
                "approved fixture dynamic capture did not include a proc-macro crate".to_string(),
            ),
        ])],
        validation_errors: if output.status.success() {
            Vec::new()
        } else {
            vec![row([
                ("table", "validation_errors".to_string()),
                ("code", "dynamic_build_capture_failed".to_string()),
                (
                    "message",
                    first_non_empty_line(&stderr)
                        .unwrap_or("cargo check failed")
                        .to_string(),
                ),
                ("repo_path", request.repo_path.display().to_string()),
            ])]
        },
        capture_gaps: vec![row([
            ("table", "capture_gaps".to_string()),
            ("missing_truth", "proc_macro_execution".to_string()),
            (
                "reason",
                "CDB034 fixture captures build logs; proc-macro execution remains represented as a gap unless fixture includes proc macros"
                    .to_string(),
            ),
            ("required_task", "CDB045".to_string()),
        ])],
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
        proc_macro_invocations: Vec::new(),
        validation_errors: vec![row([
            ("table", "validation_errors".to_string()),
            ("code", "unsafe_execution_refused".to_string()),
            (
                "message",
                format!("capture build requires explicit {UNSAFE_FLAG} approval"),
            ),
            ("repo_path", request.repo_path.display().to_string()),
        ])],
        capture_gaps: vec![row([
            ("table", "capture_gaps".to_string()),
            ("missing_truth", "build_script_execution".to_string()),
            (
                "reason",
                "dynamic build script and proc-macro execution is gated by explicit unsafe approval"
                    .to_string(),
            ),
            ("required_flag", UNSAFE_FLAG.to_string()),
        ])],
        raw_log_paths: vec![raw_log_row(&request, "not_written")],
    }
}

fn approved_scaffold(request: BuildCaptureRequest) -> BuildCaptureOutcome {
    BuildCaptureOutcome {
        status: BuildCaptureStatus::ApprovedScaffold,
        unsafe_execution_approval: vec![approval_row(
            &request,
            "approved",
            "unsafe approval was supplied; CDB033 records approval but does not execute yet",
        )],
        build_script_runs: Vec::new(),
        proc_macro_invocations: Vec::new(),
        validation_errors: Vec::new(),
        capture_gaps: vec![row([
            ("table", "capture_gaps".to_string()),
            ("missing_truth", "dynamic_capture_runner".to_string()),
            (
                "reason",
                "CDB033 is the approval scaffold; CDB034 owns optional dynamic capture execution"
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
            "path recorded for future dynamic capture; CDB033 does not write raw execution logs"
                .to_string(),
        ),
    ])
}

fn write_raw_log(path: &Path, output: &std::process::Output) -> Result<(), BuildCaptureError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| BuildCaptureError::CreateLogDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let body = format!(
        "status={}\nexit_code={}\n--- stdout ---\n{}\n--- stderr ---\n{}\n",
        output.status,
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(path, body).map_err(|source| BuildCaptureError::WriteLog {
        path: path.to_path_buf(),
        source,
    })
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
            store_path: Some(fixture.join("codedb.redb")),
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

    fn request(unsafe_execute_build: bool) -> BuildCaptureRequest {
        BuildCaptureRequest {
            repo_path: PathBuf::from("/tmp/codedb-fixture"),
            store_path: Some(PathBuf::from("/tmp/codedb.redb")),
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

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoContextRequest {
    pub manifest_path: PathBuf,
    pub target_triple: String,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            success: true,
            status: 0,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }
}

pub trait CommandRunner {
    fn output(
        &self,
        program: &str,
        args: &[String],
        current_dir: &Path,
    ) -> Result<CommandOutput, ContextError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn output(
        &self,
        program: &str,
        args: &[String],
        current_dir: &Path,
    ) -> Result<CommandOutput, ContextError> {
        let output = Command::new(program)
            .args(args)
            .current_dir(current_dir)
            .output()
            .map_err(|source| ContextError::Spawn {
                program: program.to_string(),
                source,
            })?;
        Ok(CommandOutput {
            success: output.status.success(),
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedCargoContext {
    pub cargo_version: String,
    pub rustc_version: String,
    pub host_triple: String,
    pub target_triple: String,
    pub target_cfgs: Vec<String>,
    pub requested_features: Vec<String>,
    pub resolved_features: BTreeMap<String, Vec<String>>,
    pub profile: String,
    pub cargo_lock_sha256: String,
    pub cargo_metadata_json: String,
    pub context_id: String,
}

#[derive(Debug)]
pub enum ContextError {
    MissingManifest { path: PathBuf },
    MissingLockfile { path: PathBuf },
    InvalidManifestPath { path: PathBuf },
    InvalidFeatureSelection,
    EmptyTargetTriple,
    EmptyProfile,
    ReadFile { path: PathBuf, source: io::Error },
    Spawn { program: String, source: io::Error },
    CommandFailed {
        program: String,
        status: i32,
        stderr: String,
    },
    MissingHostTriple,
    InvalidMetadata { message: &'static str },
    ParseMetadata { source: serde_json::Error },
}

impl fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManifest { path } => {
                write!(formatter, "Cargo manifest does not exist: {}", path.display())
            }
            Self::MissingLockfile { path } => {
                write!(formatter, "Cargo.lock is required for reproducible capture: {}", path.display())
            }
            Self::InvalidManifestPath { path } => {
                write!(formatter, "Cargo manifest has no parent directory: {}", path.display())
            }
            Self::InvalidFeatureSelection => {
                write!(formatter, "--all-features and --no-default-features cannot be combined")
            }
            Self::EmptyTargetTriple => write!(formatter, "target triple cannot be empty"),
            Self::EmptyProfile => write!(formatter, "Cargo profile cannot be empty"),
            Self::ReadFile { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Spawn { program, source } => {
                write!(formatter, "failed to start {program}: {source}")
            }
            Self::CommandFailed {
                program,
                status,
                stderr,
            } => write!(formatter, "{program} failed with status {status}: {stderr}"),
            Self::MissingHostTriple => write!(formatter, "rustc -vV did not report a host triple"),
            Self::InvalidMetadata { message } => {
                write!(formatter, "cargo metadata is missing {message}")
            }
            Self::ParseMetadata { source } => write!(formatter, "invalid cargo metadata JSON: {source}"),
        }
    }
}

impl Error for ContextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFile { source, .. } | Self::Spawn { source, .. } => Some(source),
            Self::ParseMetadata { source } => Some(source),
            _ => None,
        }
    }
}

pub fn capture_context(request: &CargoContextRequest) -> Result<CapturedCargoContext, ContextError> {
    capture_context_with_runner(request, &SystemCommandRunner)
}

pub fn capture_context_with_runner<R: CommandRunner + ?Sized>(
    request: &CargoContextRequest,
    runner: &R,
) -> Result<CapturedCargoContext, ContextError> {
    if request.target_triple.trim().is_empty() {
        return Err(ContextError::EmptyTargetTriple);
    }
    if request.profile.trim().is_empty() {
        return Err(ContextError::EmptyProfile);
    }
    if request.all_features && request.no_default_features {
        return Err(ContextError::InvalidFeatureSelection);
    }
    if !request.manifest_path.is_file() {
        return Err(ContextError::MissingManifest {
            path: request.manifest_path.clone(),
        });
    }
    let current_dir = request
        .manifest_path
        .parent()
        .ok_or_else(|| ContextError::InvalidManifestPath {
            path: request.manifest_path.clone(),
        })?;
    let lockfile_path = current_dir.join("Cargo.lock");
    if !lockfile_path.is_file() {
        return Err(ContextError::MissingLockfile {
            path: lockfile_path,
        });
    }
    let lockfile = fs::read(&lockfile_path).map_err(|source| ContextError::ReadFile {
        path: lockfile_path.clone(),
        source,
    })?;
    let cargo_lock_sha256 = sha256_hex(&lockfile);

    let cargo_version_output = checked_output(
        runner,
        "cargo",
        &["--version".to_string()],
        current_dir,
    )?;
    let cargo_version = first_nonempty_line(&cargo_version_output.stdout).to_string();

    let rustc_verbose = checked_output(runner, "rustc", &["-vV".to_string()], current_dir)?;
    let rustc_version = first_nonempty_line(&rustc_verbose.stdout).to_string();
    let host_triple = rustc_verbose
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .ok_or(ContextError::MissingHostTriple)?
        .to_string();

    let cfg_output = checked_output(
        runner,
        "rustc",
        &[
            "--print".to_string(),
            "cfg".to_string(),
            "--target".to_string(),
            request.target_triple.clone(),
        ],
        current_dir,
    )?;
    let mut target_cfgs = cfg_output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    target_cfgs.sort();
    target_cfgs.dedup();

    let mut requested_features = request.features.clone();
    requested_features.retain(|feature| !feature.trim().is_empty());
    requested_features.sort();
    requested_features.dedup();

    let mut metadata_args = vec![
        "metadata".to_string(),
        "--format-version".to_string(),
        "1".to_string(),
        "--manifest-path".to_string(),
        request.manifest_path.to_string_lossy().into_owned(),
        "--locked".to_string(),
        "--filter-platform".to_string(),
        request.target_triple.clone(),
    ];
    if request.all_features {
        metadata_args.push("--all-features".to_string());
    }
    if request.no_default_features {
        metadata_args.push("--no-default-features".to_string());
    }
    if !requested_features.is_empty() {
        metadata_args.push("--features".to_string());
        metadata_args.push(requested_features.join(","));
    }
    let metadata_output = checked_output(runner, "cargo", &metadata_args, current_dir)?;
    let resolved_features = parse_resolved_features(&metadata_output.stdout)?;

    let context_id = context_identity(
        &cargo_version,
        &rustc_version,
        &host_triple,
        &request.target_triple,
        &target_cfgs,
        &requested_features,
        &resolved_features,
        &request.profile,
        &cargo_lock_sha256,
    );

    Ok(CapturedCargoContext {
        cargo_version,
        rustc_version,
        host_triple,
        target_triple: request.target_triple.clone(),
        target_cfgs,
        requested_features,
        resolved_features,
        profile: request.profile.clone(),
        cargo_lock_sha256,
        cargo_metadata_json: metadata_output.stdout,
        context_id,
    })
}

fn checked_output<R: CommandRunner + ?Sized>(
    runner: &R,
    program: &str,
    args: &[String],
    current_dir: &Path,
) -> Result<CommandOutput, ContextError> {
    let output = runner.output(program, args, current_dir)?;
    if output.success {
        Ok(output)
    } else {
        Err(ContextError::CommandFailed {
            program: format!("{program} {}", args.join(" ")),
            status: output.status,
            stderr: output.stderr,
        })
    }
}

fn first_nonempty_line(value: &str) -> &str {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

fn parse_resolved_features(
    metadata_json: &str,
) -> Result<BTreeMap<String, Vec<String>>, ContextError> {
    let metadata: Value = serde_json::from_str(metadata_json)
        .map_err(|source| ContextError::ParseMetadata { source })?;
    let nodes = metadata
        .pointer("/resolve/nodes")
        .and_then(Value::as_array)
        .ok_or(ContextError::InvalidMetadata {
            message: "resolve.nodes",
        })?;
    let mut resolved = BTreeMap::new();
    for node in nodes {
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ContextError::InvalidMetadata {
                message: "resolve.nodes[].id",
            })?;
        let feature_values = node
            .get("features")
            .and_then(Value::as_array)
            .ok_or(ContextError::InvalidMetadata {
                message: "resolve.nodes[].features",
            })?;
        let mut features = feature_values
            .iter()
            .map(|feature| {
                feature
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or(ContextError::InvalidMetadata {
                        message: "string resolve.nodes[].features[]",
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        features.sort();
        features.dedup();
        resolved.insert(id.to_string(), features);
    }
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
fn context_identity(
    cargo_version: &str,
    rustc_version: &str,
    host_triple: &str,
    target_triple: &str,
    target_cfgs: &[String],
    requested_features: &[String],
    resolved_features: &BTreeMap<String, Vec<String>>,
    profile: &str,
    cargo_lock_sha256: &str,
) -> String {
    let mut digest = Sha256::new();
    digest_field(&mut digest, "codedb-context-v1");
    digest_field(&mut digest, cargo_version);
    digest_field(&mut digest, rustc_version);
    digest_field(&mut digest, host_triple);
    digest_field(&mut digest, target_triple);
    digest_field(&mut digest, profile);
    digest_field(&mut digest, cargo_lock_sha256);
    for cfg in target_cfgs {
        digest_field(&mut digest, cfg);
    }
    for feature in requested_features {
        digest_field(&mut digest, feature);
    }
    for (package_id, features) in resolved_features {
        digest_field(&mut digest, package_id);
        for feature in features {
            digest_field(&mut digest, feature);
        }
    }
    format!("{:x}", digest.finalize())
}

fn digest_field(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

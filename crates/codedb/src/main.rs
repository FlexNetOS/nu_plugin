use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use codedb_build_capture::{
    BuildCaptureRequest, ReproductionRoot, capture_approved_build,
    capture_build as refuse_build_capture, reproduce_out_dir_artifacts,
};
use codedb_cargo::{CargoMetadataCapture, capture_cargo_metadata_json};
use codedb_context::{
    CapturedCargoContext, CargoContextRequest, capture_context, detect_host_triple,
};
use codedb_core::capture_policy::{
    ExactSourceRequirement, RawPersistenceAuthorization, RawPersistenceDecision,
    authorize_raw_persistence, load_external_policy,
};
use codedb_core::store::{
    BlobStore, ContainedDirectory, MaterializedFileRollback, MaterializedSymlinkRollback,
    materialize_symlink, platform_symlink_materialization_status, prepare_materialization_path,
    rollback_materialized_file, rollback_materialized_symlink, take_materialized_file_rollback,
    take_materialized_symlink_rollback,
};
use codedb_core::store_spec::{StoreBackend, StoreSpec};
use codedb_core::{
    FilesystemEntry, NU_PLUGIN_PROTOCOL_VERSION, SourceBlobMetadata, SymlinkMaterializationStatus,
    TableRow, capture_gaps, capture_source_metadata_from_bytes, prove_no_mutation, scan_filesystem,
    schema_rows, table_inventory, validation_errors,
};
use codedb_rust_static::capture_rust_items;
use codedb_rust_static::{
    ApprovedCompilerEvidenceOutcome, ApprovedCompilerEvidenceRequest, CompilerEvidenceOptions,
    capture_approved_compiler_evidence, capture_compiler_evidence,
};
use codedb_store_redb::{CaptureBatcher, StoreInitContext, initialize_store};
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

mod ingest;

type Row = BTreeMap<String, String>;

#[derive(Clone)]
struct PluginRecord {
    name: String,
    version: String,
    owner: String,
    source_path: PathBuf,
}

const CODEDB_INIT_TEMPLATE: &str = include_str!("../../../templates/nushell/codedb_init.nu");
const CODEDB_EXTERN_TEMPLATE: &str = include_str!("../../../templates/nushell/codedb_extern.nu");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Nuon,
    Csv,
}

#[derive(Debug)]
enum CliError {
    Message(String),
    Core(Box<dyn StdError>),
}

impl Display for CliError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(message) => write!(f, "{message}"),
            Self::Core(source) => write!(f, "{source}"),
        }
    }
}

impl StdError for CliError {}

fn capture_repo_cargo(
    repo_path: &Path,
) -> Result<(CapturedCargoContext, CargoMetadataCapture), CliError> {
    let target_triple = detect_host_triple().map_err(|source| CliError::Core(Box::new(source)))?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(|source| CliError::Core(Box::new(source)))?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json)
        .map_err(|source| CliError::Core(Box::new(source)))?;
    Ok((context, metadata))
}

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("codedb: {error}");
        std::process::exit(2);
    }
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    let Some(command) = args.first().map(String::as_str) else {
        print_rows(table_rows(table_inventory()), OutputFormat::Csv)?;
        return Ok(());
    };

    match command {
        "mcp" => run_mcp_frontdoor(&args),
        "scan" => {
            let selection =
                repo_selection(&args, 1, "scan requires <repo_path> or --repo-path <path>")?;
            let format = parse_format(&args)?;
            let rows = scan_rows(&selection)?;
            print_rows(rows, format)
        }
        // capture = scan + PERSIST: every regular file's exact bytes land in the
        // selected CodeDB store as a content-addressed blob (sha256) with its relative path
        // and unix mode; anything unpersistable becomes a capture_gaps row —
        // silent omission is failure (PRD CDB015/017/018 wiring).
        "capture" if args.get(1).map(String::as_str) == Some("compiler") => {
            let format = parse_format(&args)?;
            let rows = compiler_capture_rows(&args)?;
            print_rows(rows, format)
        }
        "capture" if args.get(1).map(String::as_str) == Some("build") => {
            let format = parse_format(&args)?;
            let rows = build_capture_rows(&args)?;
            print_rows(rows, format)
        }
        "capture" => {
            let selection = repo_selection(
                &args,
                1,
                "capture requires <repo_path> or --repo-path <path>",
            )?;
            let format = parse_format(&args)?;
            let config = CaptureConfig::from_args(&args)?;
            let rows = capture_rows(&selection, &config, &args)?;
            print_rows(rows, format)
        }
        // materialize = re-emit captured trees byte-for-byte from the store
        // (whole tree, or one --path), restoring unix modes; the byte-parity
        // acceptance surface.
        "materialize" => {
            let store = option_value(&args, "--store")
                .ok_or_else(|| CliError::Message("materialize requires --store <path>".into()))?
                .to_string();
            let out_dir = option_value(&args, "--out-dir")
                .map(PathBuf::from)
                .ok_or_else(|| CliError::Message("materialize requires --out-dir <path>".into()))?;
            let only = option_value(&args, "--path").map(str::to_string);
            let format = parse_format(&args)?;
            let rows = materialize_rows(&store, &out_dir, only.as_deref(), &args)?;
            print_rows(rows, format)
        }
        "reproduce" => {
            let rows = reproduce_build_artifacts_rows(&args)?;
            print_rows(rows, parse_format(&args)?)
        }
        // store-report = the store's own metadata/toolchain/validation rows.
        "store-report" => {
            let store = option_value(&args, "--store")
                .ok_or_else(|| CliError::Message("store-report requires --store <path>".into()))?
                .to_string();
            let backend = open_store_readonly(&store, &args)?;
            let rows = backend
                .store_metadata_rows()
                .map_err(|e| CliError::Message(format!("store report failed: {e}")))?
                .into_iter()
                .map(|m| row([("table", m.table), ("key", m.key), ("value", m.value)]))
                .collect();
            print_rows(rows, parse_format(&args)?)
        }
        "export" => {
            let table = positional(&args, 1, "export requires <table>")?;
            let selection = repo_selection(
                &args,
                2,
                "export requires --repo-path <path> or --repo <path>",
            )?;
            let format = parse_format(&args)?;
            let harness_home_path = option_value(&args, "--home-path").map(PathBuf::from);
            let rows = export_rows(table, &selection, harness_home_path.as_deref(), &args)?;
            print_rows(rows, format)
        }
        // merge-plan = classify every source file across two repo roots as
        // identical / divergent / unique, and flag crate-name collisions — the
        // surgical worklist for reconciling divergent forks. Source-only: target/
        // .git/vendor/generated are skipped (they regenerate, they don't merge).
        "merge-plan" => {
            let repo_a = positional(&args, 1, "merge-plan requires <repo_a> <repo_b>")?.to_string();
            let repo_b = positional(&args, 2, "merge-plan requires <repo_a> <repo_b>")?.to_string();
            let detail = args.iter().any(|a| a == "--files");
            let rows = merge_plan_rows(Path::new(&repo_a), Path::new(&repo_b), detail)?;
            print_rows(rows, parse_format(&args)?)
        }
        "schema" => print_rows(table_rows(schema_rows()), parse_format(&args)?),
        "tables" => print_rows(table_rows(table_inventory()), parse_format(&args)?),
        "gaps" => print_rows(table_rows(capture_gaps()), parse_format(&args)?),
        "validation-errors" => print_rows(table_rows(validation_errors()), parse_format(&args)?),
        "doctor" => print_rows(doctor_rows(&args)?, parse_format(&args)?),
        "generate-yazelix-bridge" => {
            let out_dir = option_value(&args, "--out-dir")
                .map(PathBuf::from)
                .ok_or_else(|| {
                    CliError::Message(
                        "generate-yazelix-bridge requires --out-dir <path>".to_string(),
                    )
                })?;
            let rows = generate_yazelix_bridge_rows(&out_dir)?;
            print_rows(rows, parse_format(&args)?)
        }
        "--version" | "-V" => {
            println!("{}", codedb_core::VERSION);
            Ok(())
        }
        _ => Err(CliError::Message(format!(
            "unsupported command: {command}; supported commands: mcp serve, scan, capture, capture build, capture compiler, materialize, reproduce, merge-plan, store-report, export, schema, tables, gaps, validation-errors, doctor, generate-yazelix-bridge, --version"
        ))),
    }
}

fn build_capture_rows(args: &[String]) -> Result<Vec<Row>, CliError> {
    let repo = positional(args, 2, "capture build requires <repo_path>")?;
    if repo.starts_with("--") {
        return Err(CliError::Message(
            "capture build requires <repo_path> before options".to_string(),
        ));
    }
    let repo_path = fs::canonicalize(repo).map_err(|source| CliError::Core(Box::new(source)))?;
    if !repo_path.is_dir() {
        return Err(CliError::Message(
            "capture build repository path must be a directory".to_string(),
        ));
    }

    let raw_log = strict_option_value(args, "--raw-log")?;
    let store = strict_option_value(args, "--store")?;
    let unsafe_execute_build = has_flag(args, "--unsafe-execute-build");
    let approver = strict_option_value(args, "--approver")?;
    let task_id = strict_option_value(args, "--task-id")?;
    let before_state = strict_option_value(args, "--before-state")?;
    let cleanup_plan = strict_option_value(args, "--cleanup-plan")?;

    if unsafe_execute_build {
        let mut missing = Vec::new();
        for (name, value) in [
            ("--raw-log", raw_log),
            ("--approver", approver),
            ("--task-id", task_id),
            ("--before-state", before_state),
            ("--cleanup-plan", cleanup_plan),
        ] {
            if value.is_none_or(|value| value.trim().is_empty()) {
                missing.push(name);
            }
        }
        if !missing.is_empty() {
            return Err(CliError::Message(format!(
                "approved capture build requires complete provenance: {}",
                missing.join(", ")
            )));
        }
        if let Some(store) = store {
            parse_store_spec(store, args)?;
        }
    }

    let raw_log_path = match raw_log {
        Some(path) => absolute_cli_path(path)?,
        None => repo_path.with_extension("codedb-build-capture-refused.log"),
    };
    let request = BuildCaptureRequest {
        repo_path,
        store_path: store.map(PathBuf::from),
        raw_log_path,
        unsafe_execute_build,
        approver: approver.map(str::to_string),
        task_id: task_id.map(str::to_string),
        before_state: before_state.map(str::to_string),
        cleanup_plan: cleanup_plan.map(str::to_string),
    };

    if !unsafe_execute_build {
        return Ok(refuse_build_capture(request).into_rows());
    }

    let outcome =
        capture_approved_build(request).map_err(|source| CliError::Core(Box::new(source)))?;
    let mut rows = outcome.into_rows();
    if let Some(store) = store {
        let receipt = persist_build_capture_receipt(&rows, store, args)?;
        rows.push(receipt);
    }
    Ok(rows)
}

fn absolute_cli_path(path: &str) -> Result<PathBuf, CliError> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(env::current_dir()
            .map_err(|source| CliError::Core(Box::new(source)))?
            .join(path))
    }
}

fn persist_build_capture_receipt(
    rows: &[Row],
    store: &str,
    args: &[String],
) -> Result<Row, CliError> {
    let approval_id = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("unsafe_execution_approval"))
        .and_then(|row| row.get("approval_id"))
        .ok_or_else(|| {
            CliError::Message("approved build capture omitted its approval identifier".to_string())
        })?;
    let relative_path = format!("dynamic-build-captures/{approval_id}.json");
    let bytes = serde_json::to_vec(rows)
        .map_err(|source| CliError::Message(format!("build receipt encoding failed: {source}")))?;
    let (mut backend, store_identity) = open_store_for_capture(store, args)?;
    let persisted = backend
        .persist_batch(&[(relative_path.clone(), bytes)])
        .map_err(|source| {
            CliError::Message(format!("build receipt persistence failed: {source}"))
        })?;
    let persisted = persisted.into_iter().next().ok_or_else(|| {
        CliError::Message("build receipt persistence returned no receipt".to_string())
    })?;
    Ok(row([
        ("table", "build_capture_receipts".to_string()),
        ("approval_id", approval_id.clone()),
        ("relative_path", relative_path),
        ("blob_ref", persisted.blob_ref),
        ("sha256", persisted.sha256),
        ("bytes", persisted.bytes.to_string()),
        ("store_identity", store_identity),
        ("status", "persisted".to_string()),
    ]))
}

fn reproduce_build_artifacts_rows(args: &[String]) -> Result<Vec<Row>, CliError> {
    let store = strict_option_value(args, "--store")?
        .ok_or_else(|| CliError::Message("reproduce requires --store <path>".to_string()))?;
    let approval_id = strict_option_value(args, "--approval-id")?.ok_or_else(|| {
        CliError::Message("reproduce requires --approval-id <sha256>".to_string())
    })?;
    if approval_id.len() != 64 || !approval_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CliError::Message(
            "reproduce --approval-id must be a 64-character SHA-256 identifier".to_string(),
        ));
    }
    let artifact_dir = strict_option_value(args, "--artifact-dir")?
        .ok_or_else(|| CliError::Message("reproduce requires --artifact-dir <path>".to_string()))?;
    let selected_package_id = strict_option_value(args, "--package-id")?;
    let selected_artifact_group = strict_option_value(args, "--artifact-group")?;
    let artifact_dir = absolute_cli_path(artifact_dir)?;
    if artifact_dir.exists() {
        return Err(CliError::Message(
            "reproduce artifact directory must not already exist".to_string(),
        ));
    }

    let backend = open_store_readonly(store, args)?;
    let receipt_path = format!("dynamic-build-captures/{approval_id}.json");
    let receipt = backend
        .read_source_file_blob(&receipt_path)
        .map_err(|source| CliError::Message(format!("build receipt read failed: {source}")))?
        .ok_or_else(|| {
            CliError::Message(format!(
                "build receipt not found for approval identifier {approval_id}"
            ))
        })?;
    let receipt_rows: Vec<Row> = serde_json::from_slice(&receipt)
        .map_err(|source| CliError::Message(format!("build receipt decode failed: {source}")))?;
    let mut artifacts = receipt_rows
        .into_iter()
        .filter(|row| row.get("table").map(String::as_str) == Some("out_dir_artifacts"))
        .collect::<Vec<_>>();
    if artifacts.is_empty() {
        return Err(CliError::Message(
            "build receipt contains no OUT_DIR artifacts to reproduce".to_string(),
        ));
    }
    for artifact in &mut artifacts {
        if artifact
            .get("artifact_group_id")
            .is_some_and(|value| !value.is_empty())
        {
            continue;
        }
        let package_id = artifact.get("package_id").cloned().ok_or_else(|| {
            CliError::Message("build receipt OUT_DIR artifact is missing package_id".to_string())
        })?;
        let out_dir = artifact.get("out_dir").cloned().ok_or_else(|| {
            CliError::Message("build receipt OUT_DIR artifact is missing out_dir".to_string())
        })?;
        artifact.insert(
            "artifact_group_id".to_string(),
            format!(
                "legacy-sha256:{}",
                sha256_hex(format!("{package_id}\0{out_dir}").as_bytes())
            ),
        );
    }
    let package_ids = artifacts
        .iter()
        .map(|artifact| {
            artifact
                .get("package_id")
                .filter(|package_id| !package_id.is_empty())
                .cloned()
                .ok_or_else(|| {
                    CliError::Message(
                        "build receipt OUT_DIR artifact is missing package_id".to_string(),
                    )
                })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let artifacts = if let Some(selected_package_id) = selected_package_id {
        if !package_ids.contains(selected_package_id) {
            return Err(CliError::Message(format!(
                "reproduce --package-id did not match an OUT_DIR package: {selected_package_id}"
            )));
        }
        artifacts
            .into_iter()
            .filter(|artifact| {
                artifact.get("package_id").map(String::as_str) == Some(selected_package_id)
            })
            .collect::<Vec<_>>()
    } else if package_ids.len() > 1 {
        return Err(CliError::Message(format!(
            "build receipt contains OUT_DIR artifacts from {} packages; reproduce requires --package-id <exact-captured-package-id>",
            package_ids.len()
        )));
    } else {
        artifacts
    };
    let artifact_groups = artifacts
        .iter()
        .map(|artifact| {
            artifact
                .get("artifact_group_id")
                .filter(|group| !group.is_empty())
                .cloned()
                .ok_or_else(|| {
                    CliError::Message(
                        "build receipt OUT_DIR artifact is missing artifact_group_id".to_string(),
                    )
                })
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let artifacts = if let Some(selected_artifact_group) = selected_artifact_group {
        if !artifact_groups.contains(selected_artifact_group) {
            return Err(CliError::Message(format!(
                "reproduce --artifact-group did not match an OUT_DIR execution group: {selected_artifact_group}"
            )));
        }
        artifacts
            .into_iter()
            .filter(|artifact| {
                artifact.get("artifact_group_id").map(String::as_str)
                    == Some(selected_artifact_group)
            })
            .collect::<Vec<_>>()
    } else if artifact_groups.len() > 1 {
        return Err(CliError::Message(format!(
            "selected package contains OUT_DIR artifacts from {} execution groups; reproduce requires --artifact-group <exact-captured-artifact-group-id>",
            artifact_groups.len()
        )));
    } else {
        artifacts
    };

    if let Some(parent) = artifact_dir.parent() {
        fs::create_dir_all(parent).map_err(|source| CliError::Core(Box::new(source)))?;
    }
    fs::create_dir(&artifact_dir).map_err(|source| CliError::Core(Box::new(source)))?;
    let destination = ReproductionRoot::open_existing(&artifact_dir)
        .map_err(|source| CliError::Core(Box::new(source)))?;
    reproduce_out_dir_artifacts(&artifacts, &destination)
        .map_err(|source| CliError::Core(Box::new(source)))
}

fn compiler_capture_rows(args: &[String]) -> Result<Vec<Row>, CliError> {
    let source = positional(args, 2, "capture compiler requires <source.rs>")?;
    if source.starts_with("--") {
        return Err(CliError::Message(
            "capture compiler requires <source.rs> before options".to_string(),
        ));
    }
    let source_path =
        fs::canonicalize(source).map_err(|source| CliError::Core(Box::new(source)))?;
    if !source_path.is_file() {
        return Err(CliError::Message(
            "capture compiler source must be a regular file".to_string(),
        ));
    }
    if !has_flag(args, "--unsafe-execute-build") {
        let report = capture_compiler_evidence(&source_path, CompilerEvidenceOptions::default());
        let mut rows = vec![row([
            ("table", "unsafe_execution_approval".to_string()),
            ("status", "missing".to_string()),
            ("flag", "--unsafe-execute-build".to_string()),
            ("source_path", source_path.display().to_string()),
            (
                "note",
                "compiler-observed capture refused without explicit approval".to_string(),
            ),
        ])];
        rows.push(row([
            ("table", "validation_errors".to_string()),
            ("code", "unsafe_execution_refused".to_string()),
            (
                "message",
                "capture compiler requires --unsafe-execute-build".to_string(),
            ),
            ("source_path", source_path.display().to_string()),
        ]));
        rows.extend(report.gaps.into_iter().map(|gap| {
            row([
                ("table", "capture_gaps".to_string()),
                ("missing_truth", "compiler_observed_evidence".to_string()),
                (
                    "artifact_kind",
                    gap.artifact
                        .map(|artifact| artifact.as_str().to_string())
                        .unwrap_or_default(),
                ),
                ("reason", gap.reason),
            ])
        }));
        rows.push(row([
            ("table", "raw_log_paths".to_string()),
            ("status", "not_written".to_string()),
            ("path", String::new()),
            (
                "note",
                "refusal-only compiler capture creates no evidence directory".to_string(),
            ),
        ]));
        return Ok(rows);
    }

    let repo = strict_option_value(args, "--repo-path")?;
    let evidence_dir = strict_option_value(args, "--evidence-dir")?;
    let store = strict_option_value(args, "--store")?;
    let approver = strict_option_value(args, "--approver")?;
    let task_id = strict_option_value(args, "--task-id")?;
    let before_state = strict_option_value(args, "--before-state")?;
    let cleanup_plan = strict_option_value(args, "--cleanup-plan")?;
    let mut missing = Vec::new();
    for (name, value) in [
        ("--repo-path", repo),
        ("--evidence-dir", evidence_dir),
        ("--store", store),
        ("--approver", approver),
        ("--task-id", task_id),
        ("--before-state", before_state),
        ("--cleanup-plan", cleanup_plan),
    ] {
        if value.is_none_or(|value| value.trim().is_empty()) {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        return Err(CliError::Message(format!(
            "approved capture compiler requires complete provenance: {}",
            missing.join(", ")
        )));
    }
    let repo_path = fs::canonicalize(repo.expect("validated repo option"))
        .map_err(|source| CliError::Core(Box::new(source)))?;
    let evidence_dir = absolute_cli_path(evidence_dir.expect("validated evidence option"))?;
    if evidence_dir.exists() {
        return Err(CliError::Message(
            "compiler evidence directory must not already exist".to_string(),
        ));
    }
    let evidence_parent = evidence_dir.parent().ok_or_else(|| {
        CliError::Message("compiler evidence directory must have a parent".to_string())
    })?;
    fs::create_dir_all(evidence_parent).map_err(|source| CliError::Core(Box::new(source)))?;
    let evidence_parent = evidence_parent
        .canonicalize()
        .map_err(|source| CliError::Core(Box::new(source)))?;
    let evidence_dir = evidence_parent.join(evidence_dir.file_name().ok_or_else(|| {
        CliError::Message("compiler evidence directory must have a final name".to_string())
    })?);
    let store = store.expect("validated store option");
    parse_store_spec(store, args)?;
    let edition = strict_option_value(args, "--edition")?.unwrap_or("2024");
    if !matches!(edition, "2015" | "2018" | "2021" | "2024") {
        return Err(CliError::Message(
            "capture compiler --edition must be 2015, 2018, 2021, or 2024".to_string(),
        ));
    }
    let options = CompilerEvidenceOptions {
        enabled: true,
        edition: edition.to_string(),
        crate_name: strict_option_value(args, "--crate-name")?.map(str::to_string),
        ..CompilerEvidenceOptions::default()
    };
    let outcome = capture_approved_compiler_evidence(ApprovedCompilerEvidenceRequest {
        repo_path,
        source_path,
        evidence_dir,
        approver: approver.expect("validated approver").to_string(),
        task_id: task_id.expect("validated task id").to_string(),
        before_state: before_state.expect("validated before state").to_string(),
        cleanup_plan: cleanup_plan.expect("validated cleanup plan").to_string(),
        options,
    })
    .map_err(CliError::Message)?;
    persist_compiler_evidence(outcome, store, args)
}

fn compiler_artifact_filename(kind: codedb_rust_static::CompilerEvidenceArtifactKind) -> String {
    let extension = match kind.as_str() {
        "macro_resolution" => "rmeta",
        "rustdoc_public_api" => "json",
        _ => "txt",
    };
    format!("{}.{}", kind.as_str(), extension)
}

fn write_new_evidence_file(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|source| CliError::Core(Box::new(source)))?;
    file.write_all(bytes)
        .map_err(|source| CliError::Core(Box::new(source)))?;
    file.sync_all()
        .map_err(|source| CliError::Core(Box::new(source)))
}

fn persist_compiler_evidence(
    outcome: ApprovedCompilerEvidenceOutcome,
    store: &str,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    fs::create_dir(&outcome.evidence_dir).map_err(|source| CliError::Core(Box::new(source)))?;
    let result = persist_compiler_evidence_in_created_dir(&outcome, store, args);
    if result.is_err() {
        let _ = fs::remove_dir_all(&outcome.evidence_dir);
    }
    result
}

fn persist_compiler_evidence_in_created_dir(
    outcome: &ApprovedCompilerEvidenceOutcome,
    store: &str,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    let report = &outcome.report;
    let mut rows = vec![row([
        ("table", "unsafe_execution_approval".to_string()),
        ("status", "approved".to_string()),
        ("flag", "--unsafe-execute-build".to_string()),
        ("approval_id", outcome.approval_id.clone()),
        ("approver", outcome.approver.clone()),
        ("task_id", outcome.task_id.clone()),
        ("before_state", outcome.before_state.clone()),
        ("cleanup_plan", outcome.cleanup_plan.clone()),
        ("repo_path", outcome.repo_path.display().to_string()),
        ("source_path", report.source_path.display().to_string()),
        ("evidence_dir", outcome.evidence_dir.display().to_string()),
    ])];
    if let Some(toolchain) = &report.toolchain {
        rows.push(row([
            ("table", "compiler_toolchain_provenance".to_string()),
            ("approval_id", outcome.approval_id.clone()),
            ("status", "compiler_observed".to_string()),
            ("rustc_path", toolchain.rustc_path.display().to_string()),
            ("rustc_version", toolchain.rustc_version.clone()),
            ("rustdoc_path", toolchain.rustdoc_path.display().to_string()),
            ("rustdoc_version", toolchain.rustdoc_version.clone()),
            ("sysroot", toolchain.sysroot.display().to_string()),
            (
                "target_libdir",
                toolchain.target_libdir.display().to_string(),
            ),
            ("host", toolchain.host.clone()),
            ("toolchain_sha256", toolchain.toolchain_sha256.clone()),
        ]));
    }
    if let Some(context) = &report.context {
        rows.push(row([
            ("table", "compiler_context_provenance".to_string()),
            ("approval_id", outcome.approval_id.clone()),
            ("source_path", context.source_path.display().to_string()),
            ("source_sha256", context.source_sha256.clone()),
            ("crate_name", context.crate_name.clone()),
            ("crate_type", context.crate_type.clone()),
            ("edition", context.edition.clone()),
            ("target", context.target.clone()),
            ("context_sha256", context.context_sha256.clone()),
        ]));
    }

    let mut store_files = Vec::new();
    for artifact in &report.artifacts {
        let filename = compiler_artifact_filename(artifact.kind);
        let evidence_path = outcome.evidence_dir.join(&filename);
        let store_relative_path = format!("compiler-evidence/{}/{}", outcome.approval_id, filename);
        if let Some(payload) = &artifact.payload {
            write_new_evidence_file(&evidence_path, payload.as_bytes())?;
            store_files.push((store_relative_path.clone(), payload.as_bytes().to_vec()));
        }
        rows.push(row([
            ("table", "compiler_artifacts".to_string()),
            ("approval_id", outcome.approval_id.clone()),
            ("artifact_kind", artifact.kind.as_str().to_string()),
            ("status", artifact.status.as_str().to_string()),
            (
                "evidence_path",
                artifact
                    .payload
                    .as_ref()
                    .map(|_| evidence_path.display().to_string())
                    .unwrap_or_default(),
            ),
            ("store_relative_path", store_relative_path),
            (
                "payload_kind",
                artifact
                    .payload
                    .as_ref()
                    .map(|payload| payload.kind().to_string())
                    .unwrap_or_default(),
            ),
            (
                "evidence_sha256",
                artifact.evidence_sha256.clone().unwrap_or_default(),
            ),
            (
                "evidence_bytes",
                artifact
                    .evidence_bytes
                    .map(|bytes| bytes.to_string())
                    .unwrap_or_default(),
            ),
            (
                "context_sha256",
                artifact.context_sha256.clone().unwrap_or_default(),
            ),
            (
                "toolchain_sha256",
                artifact.toolchain_sha256.clone().unwrap_or_default(),
            ),
            (
                "pin_sha256",
                artifact.pin_sha256.clone().unwrap_or_default(),
            ),
            ("diagnostic", artifact.diagnostic.clone()),
        ]));
    }
    if let (Some(semantic_hash), Some(public_api_hash)) =
        (&report.semantic_hash, &report.public_api_hash)
    {
        rows.push(row([
            ("table", "compiler_semantic_hashes".to_string()),
            ("approval_id", outcome.approval_id.clone()),
            ("status", report.collection_status.as_str().to_string()),
            ("semantic_hash", semantic_hash.clone()),
            ("public_api_hash", public_api_hash.clone()),
            (
                "context_sha256",
                report
                    .context
                    .as_ref()
                    .map(|context| context.context_sha256.clone())
                    .unwrap_or_default(),
            ),
        ]));
    }
    rows.extend(report.gaps.iter().map(|gap| {
        row([
            ("table", "capture_gaps".to_string()),
            ("approval_id", outcome.approval_id.clone()),
            ("missing_truth", "compiler_observed_evidence".to_string()),
            (
                "artifact_kind",
                gap.artifact
                    .map(|artifact| artifact.as_str().to_string())
                    .unwrap_or_default(),
            ),
            ("reason", gap.reason.clone()),
        ])
    }));

    let raw_log_path = outcome.evidence_dir.join("compiler.log");
    let raw_log = format!(
        "status={}\napproval_id={}\nsource_sha256={}\nsemantic_hash={}\npublic_api_hash={}\nartifact_count={}\ngap_count={}\n",
        report.collection_status.as_str(),
        outcome.approval_id,
        report.source_sha256.as_deref().unwrap_or_default(),
        report.semantic_hash.as_deref().unwrap_or_default(),
        report.public_api_hash.as_deref().unwrap_or_default(),
        report.artifacts.len(),
        report.gaps.len(),
    );
    write_new_evidence_file(&raw_log_path, raw_log.as_bytes())?;
    let raw_log_store_path = format!("compiler-evidence/{}/compiler.log", outcome.approval_id);
    store_files.push((raw_log_store_path, raw_log.into_bytes()));
    rows.push(row([
        ("table", "raw_log_paths".to_string()),
        ("approval_id", outcome.approval_id.clone()),
        ("status", "written".to_string()),
        ("path", raw_log_path.display().to_string()),
        (
            "note",
            "compiler artifact hashes and collection status; full artifacts are sibling files"
                .to_string(),
        ),
    ]));

    let (mut backend, store_identity) = open_store_for_capture(store, args)?;
    let persisted = backend.persist_batch(&store_files).map_err(|source| {
        CliError::Message(format!("compiler evidence persistence failed: {source}"))
    })?;
    let persisted = persisted
        .into_iter()
        .map(|receipt| (receipt.relative_path.clone(), receipt))
        .collect::<BTreeMap<_, _>>();
    for artifact in rows
        .iter_mut()
        .filter(|row| row.get("table").map(String::as_str) == Some("compiler_artifacts"))
    {
        if let Some(receipt) = artifact
            .get("store_relative_path")
            .and_then(|path| persisted.get(path))
        {
            artifact.insert("blob_ref".to_string(), receipt.blob_ref.clone());
        }
    }
    let receipt_path = format!("compiler-evidence/{}/receipt.json", outcome.approval_id);
    let receipt_bytes = serde_json::to_vec(&rows).map_err(|source| {
        CliError::Message(format!("compiler receipt encoding failed: {source}"))
    })?;
    let receipt = backend
        .persist_batch(&[(receipt_path.clone(), receipt_bytes)])
        .map_err(|source| {
            CliError::Message(format!("compiler receipt persistence failed: {source}"))
        })?
        .into_iter()
        .next()
        .ok_or_else(|| {
            CliError::Message("compiler receipt persistence returned no receipt".to_string())
        })?;
    rows.push(row([
        ("table", "compiler_capture_receipts".to_string()),
        ("approval_id", outcome.approval_id.clone()),
        ("status", "persisted".to_string()),
        ("relative_path", receipt_path),
        ("blob_ref", receipt.blob_ref),
        ("sha256", receipt.sha256),
        ("bytes", receipt.bytes.to_string()),
        ("store_identity", store_identity),
    ]));
    Ok(rows)
}

fn run_mcp_frontdoor(args: &[String]) -> Result<(), CliError> {
    if args.get(1).map(String::as_str) != Some("serve") {
        return Err(CliError::Message(
            "mcp requires the read-only serve subcommand".to_string(),
        ));
    }
    let repo_path = option_value(args, "--repo-path")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Message("mcp serve requires --repo-path <path>".to_string()))?;
    let allowed_root =
        fs::canonicalize(&repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    if !allowed_root.is_dir() {
        return Err(CliError::Message(
            "mcp serve repository path must be a directory".to_string(),
        ));
    }
    let store = option_value(args, "--store")
        .ok_or_else(|| CliError::Message("mcp serve requires --store <selector>".to_string()))?;
    parse_store_spec(store, args)?;
    let current_exe = env::current_exe().map_err(|source| CliError::Core(Box::new(source)))?;
    let server = sibling_mcp_server_path(&current_exe)?;

    let mut command = mcp_server_command(&server, &allowed_root, store, args);
    let status = command
        .status()
        .map_err(|_| CliError::Message("failed to launch codedb-mcp server".to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Message(
            "codedb-mcp server exited unsuccessfully".to_string(),
        ))
    }
}

fn sibling_mcp_server_path(current_exe: &Path) -> Result<PathBuf, CliError> {
    let sibling = current_exe.with_file_name(format!("codedb-mcp{}", env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(CliError::Message(
            "trusted sibling codedb-mcp executable is unavailable".to_string(),
        ))
    }
}

fn mcp_server_command(server: &Path, allowed_root: &Path, store: &str, args: &[String]) -> Command {
    let mut command = Command::new(server);
    command
        .env_clear()
        .env("CODEDB_MCP_ALLOWED_ROOT", allowed_root)
        .env("CODEDB_MCP_STORE", store)
        .env("CODEDB_MCP_RAW_SOURCE_ENABLED", "false");
    if store == "pg"
        && let Some(pg_conn) = external_pg_conn_string()
    {
        command.env("CODEDB_PG_CONN", pg_conn);
    }
    if let Some(default_limit) = option_value(args, "--default-limit") {
        command.env("CODEDB_MCP_DEFAULT_ROW_LIMIT", default_limit);
    }
    if let Some(max_bytes) = option_value(args, "--max-bytes") {
        command
            .env("CODEDB_MCP_DEFAULT_MAX_BYTES", max_bytes)
            .env("CODEDB_MCP_MAX_RESPONSE_BYTES", max_bytes);
    }
    if let Some(max_requests) = option_value(args, "--max-requests") {
        command.env("CODEDB_MCP_MAX_REQUESTS", max_requests);
    }
    if let Some(pg_table) = option_value(args, "--pg-table") {
        command.env("CODEDB_MCP_PG_TABLE", pg_table);
    }
    command
}

#[derive(Debug, Clone)]
struct RepoSelection {
    repo_id: String,
    repo_path: PathBuf,
    store_path: String,
    selection_source: String,
}

/// First line of `<tool> --version`, or an explicit gap value — never silence.
fn probe_tool_version(tool: &str) -> String {
    Command::new(tool)
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| {
            String::from_utf8(out.stdout)
                .ok()
                .and_then(|s| s.lines().next().map(str::to_string))
        })
        .unwrap_or_else(|| format!("gap_tool_not_available:{tool}"))
}

/// Phased-capture tunables. Defaults commit durable batches without any flag;
/// flags only adjust cadence (batch size, time budget, resume) — never *what* is
/// captured, so tuning can never downgrade the imported set.
struct CaptureConfig {
    batch_files: usize,
    batch_bytes: u64,
    time_budget: Option<Duration>,
    resume: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CapturePolicySelection {
    DefaultDeny,
    BuiltInSafeSourceClasses,
    ExternalOperatorPolicy(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositorySnapshot {
    binding: String,
    exact_sources: BTreeMap<String, ExactSourceRequirement>,
}

impl CaptureConfig {
    fn from_args(args: &[String]) -> Result<Self, CliError> {
        let batch_files = match option_value(args, "--batch-files") {
            Some(v) => v
                .parse::<usize>()
                .map_err(|_| CliError::Message(format!("invalid --batch-files: {v}")))?
                .max(1),
            None => 512,
        };
        let batch_bytes = match option_value(args, "--batch-bytes") {
            Some(v) => parse_byte_size(v)?,
            None => 64 * 1024 * 1024,
        };
        let time_budget = match option_value(args, "--time-budget") {
            Some(v) => Some(parse_time_budget(v)?),
            None => None,
        };
        Ok(Self {
            batch_files,
            batch_bytes,
            time_budget,
            resume: has_flag(args, "--resume"),
        })
    }
}

/// Parse the deliberately narrow raw-persistence authorization surface.
///
/// Absence means default deny. The only built-in positive authority is the
/// exact literal `safe-source`; external policy contents never enter argv.
fn capture_policy_selection(args: &[String]) -> Result<CapturePolicySelection, CliError> {
    let built_in = strict_option_value(args, "--raw-persistence")?;
    let external = strict_option_value(args, "--raw-persistence-policy")?;
    if built_in.is_some() && external.is_some() {
        return Err(CliError::Message(
            "--raw-persistence and --raw-persistence-policy are mutually exclusive".to_string(),
        ));
    }
    if let Some(value) = built_in {
        return if value == "safe-source" {
            Ok(CapturePolicySelection::BuiltInSafeSourceClasses)
        } else {
            Err(CliError::Message(
                "--raw-persistence accepts only the built-in safe-source authority".to_string(),
            ))
        };
    }
    if let Some(path) = external {
        return Ok(CapturePolicySelection::ExternalOperatorPolicy(
            PathBuf::from(path),
        ));
    }
    Ok(CapturePolicySelection::DefaultDeny)
}

fn strict_option_value<'a>(args: &'a [String], option: &str) -> Result<Option<&'a str>, CliError> {
    let mut found = None;
    for (index, arg) in args.iter().enumerate() {
        if arg != option {
            continue;
        }
        if found.is_some() {
            return Err(CliError::Message(format!(
                "{option} may be specified only once"
            )));
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| CliError::Message(format!("{option} requires one value")))?;
        found = Some(value.as_str());
    }
    Ok(found)
}

/// Parse a byte size: plain bytes or a K/M/G (binary) suffix, e.g. `64M`.
fn parse_byte_size(s: &str) -> Result<u64, CliError> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix(['G', 'g']) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix(['M', 'm']) {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix(['K', 'k']) {
        (n, 1024)
    } else {
        (s, 1)
    };
    num.trim()
        .parse::<u64>()
        .map(|v| v * mult)
        .map_err(|_| CliError::Message(format!("invalid --batch-bytes: {s}")))
}

/// Parse a time budget: plain seconds or an s/m/h suffix, e.g. `90s`, `15m`.
fn parse_time_budget(s: &str) -> Result<Duration, CliError> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix(['h', 'H']) {
        (n, 3600)
    } else if let Some(n) = s.strip_suffix(['m', 'M']) {
        (n, 60)
    } else if let Some(n) = s.strip_suffix(['s', 'S']) {
        (n, 1)
    } else {
        (s, 1)
    };
    num.trim()
        .parse::<u64>()
        .map(|v| Duration::from_secs(v * mult))
        .map_err(|_| CliError::Message(format!("invalid --time-budget: {s}")))
}

/// scan + persist: exact bytes of every regular file into the selected CodeDB store
/// (content-addressed sha256 blobs + relative-path rows + unix modes); every
/// non-file, non-directory entry becomes a capture_gaps row. Read-only on the
/// scanned tree; the ONLY write target is the store path. Persisted in durable
/// batches (see [`CaptureConfig`]) so a full-repo import is checkpointed and
/// resumable rather than one fsync per file.
// `batch_bytes` is a flush-and-reset accumulator; the reset after the final flush
// is intentionally not read again.
/// Resolve the only CLI-approved PostgreSQL DSN source.
///
/// PostgreSQL connection strings are inherited through `CODEDB_PG_CONN` so
/// they never enter process arguments. Ambient `DATABASE_URL` is deliberately
/// ignored because it is not a CodeDB-scoped authorization boundary.
fn external_pg_conn_string() -> Option<String> {
    env::var("CODEDB_PG_CONN")
        .ok()
        .filter(|dsn| !dsn.trim().is_empty())
}

/// Parse the user-provided store selector without opening a backend or touching
/// the filesystem. Unknown URI schemes fail closed instead of becoming redb
/// pathnames.
fn parse_store_spec(store_spec: &str, args: &[String]) -> Result<StoreSpec, CliError> {
    if option_value(args, "--pg-conn").is_some() {
        return Err(CliError::Message(
            "--pg-conn is forbidden because DSNs must not enter process arguments; use --store pg with inherited CODEDB_PG_CONN"
                .to_string(),
        ));
    }
    if store_spec.split_once(':').is_some_and(|(scheme, _)| {
        matches!(
            scheme.to_ascii_lowercase().as_str(),
            "postgres" | "postgresql"
        )
    }) {
        return Err(CliError::Message(
            "PostgreSQL URLs are forbidden in --store; use --store pg with inherited CODEDB_PG_CONN"
                .to_string(),
        ));
    }
    let external_dsn = external_pg_conn_string();
    if store_spec == "pg" && external_dsn.is_none() {
        return Err(CliError::Message(
            "PostgreSQL DSN is required: use --store pg with inherited CODEDB_PG_CONN".to_string(),
        ));
    }
    StoreSpec::parse(store_spec, external_dsn.as_deref())
        .map_err(|error| CliError::Message(error.to_string()))
}

/// Resolve the PostgreSQL table name: `--pg-table`, then `CODEDB_PG_TABLE`, then
/// the crate default `codebase_codedb` (a dedicated table, never production
/// `codebase`).
fn pg_table_name(args: &[String]) -> String {
    option_value(args, "--pg-table")
        .map(str::to_string)
        .or_else(|| env::var("CODEDB_PG_TABLE").ok())
        .unwrap_or_else(|| codedb_store_pg::DEFAULT_TABLE.to_string())
}

/// Open the capture-side backend selected by the backend-neutral `StoreSpec`.
/// The parser runs before filesystem/database effects, so a misspelled URI
/// cannot silently create a redb file.
fn open_store_for_capture(
    store_spec: &str,
    args: &[String],
) -> Result<(Box<dyn BlobStore>, String), CliError> {
    let store_spec = parse_store_spec(store_spec, args)?;
    match store_spec.backend() {
        StoreBackend::PostgreSql => {
            let conn = store_spec
                .connection_string()
                .expect("PostgreSQL StoreSpec has a connection string");
            let table = pg_table_name(args);
            let store = codedb_store_pg::PgStore::initialize(conn, &table)
                .map_err(|e| CliError::Message(format!("pg store connect failed: {e}")))?;
            Ok((Box::new(store), format!("postgresql:{table}")))
        }
        StoreBackend::Redb => {
            let store = store_spec
                .redb_path()
                .expect("redb StoreSpec has a filesystem path");
            if let Some(parent) = store.parent().filter(|p| !p.as_os_str().is_empty()) {
                fs::create_dir_all(parent)
                    .map_err(|e| CliError::Message(format!("creating store parent: {e}")))?;
            }
            if !store.exists() {
                let rustc_version = probe_tool_version("rustc");
                let cargo_version = probe_tool_version("cargo");
                initialize_store(
                    store,
                    &StoreInitContext {
                        codedb_version: codedb_core::VERSION,
                        toolchain: "host-default",
                        rustc_version: &rustc_version,
                        cargo_version: &cargo_version,
                    },
                )
                .map_err(|e| CliError::Message(format!("store init failed: {e}")))?;
            }
            let batcher = CaptureBatcher::open(store)
                .map_err(|e| CliError::Message(format!("opening store for capture: {e}")))?;
            Ok((Box::new(batcher), store_spec.redacted()))
        }
    }
}

/// Open the read-side backend for a store spec (materialize / store-report).
/// redb: open the existing file (must already exist). pg: connect.
fn open_store_readonly(store_spec: &str, args: &[String]) -> Result<Box<dyn BlobStore>, CliError> {
    let store_spec = parse_store_spec(store_spec, args)?;
    open_parsed_store_readonly(&store_spec, args)
}

fn open_parsed_store_readonly(
    store_spec: &StoreSpec,
    args: &[String],
) -> Result<Box<dyn BlobStore>, CliError> {
    match store_spec.backend() {
        StoreBackend::PostgreSql => {
            let conn = store_spec
                .connection_string()
                .expect("PostgreSQL StoreSpec has a connection string");
            let table = pg_table_name(args);
            let store = codedb_store_pg::PgStore::open_existing(conn, &table)
                .map_err(|e| CliError::Message(format!("pg store connect failed: {e}")))?;
            Ok(Box::new(store))
        }
        StoreBackend::Redb => {
            let path = store_spec
                .redb_path()
                .expect("redb StoreSpec has a filesystem path");
            let batcher = CaptureBatcher::open(path)
                .map_err(|e| CliError::Message(format!("opening store: {e}")))?;
            Ok(Box::new(batcher))
        }
    }
}

fn materialize_store_identity(store_spec: &StoreSpec, args: &[String]) -> String {
    match store_spec.backend() {
        StoreBackend::PostgreSql => format!("postgresql:{}", pg_table_name(args)),
        StoreBackend::Redb => store_spec.redacted(),
    }
}

fn capture_store_selector(selection: &RepoSelection) -> String {
    if selection.store_path.is_empty() {
        selection
            .repo_path
            .join(".codedb/store.redb")
            .display()
            .to_string()
    } else {
        selection.store_path.clone()
    }
}

fn capture_snapshot_exclusions(
    repo_path: &Path,
    store_selector: &str,
    args: &[String],
) -> Result<BTreeSet<String>, CliError> {
    let store_spec = parse_store_spec(store_selector, args)?;
    let mut exclusions = BTreeSet::new();
    if store_spec.backend() != StoreBackend::Redb {
        return Ok(exclusions);
    }
    let store_path = store_spec
        .redb_path()
        .expect("redb StoreSpec has a filesystem path");
    let absolute_store = if store_path.is_absolute() {
        store_path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|source| CliError::Core(Box::new(source)))?
            .join(store_path)
    };
    let Ok(relative_store) = absolute_store.strip_prefix(repo_path) else {
        return Ok(exclusions);
    };
    let relative_store = relative_store.to_string_lossy().replace('\\', "/");
    if relative_store.is_empty() {
        return Err(CliError::Message(
            "capture store cannot replace the repository root".to_string(),
        ));
    }
    exclusions.insert(relative_store.clone());
    let mut parent = Path::new(&relative_store).parent();
    while let Some(path) = parent {
        let value = path.to_string_lossy().replace('\\', "/");
        if value.is_empty() {
            break;
        }
        exclusions.insert(value);
        parent = path.parent();
    }
    Ok(exclusions)
}

fn repository_snapshot(
    contained_repo: &ContainedDirectory,
    entries: &[FilesystemEntry],
    exclusions: &BTreeSet<String>,
) -> Result<RepositorySnapshot, CliError> {
    let mut hasher = Sha256::new();
    hash_snapshot_field(&mut hasher, "codedb.repository-snapshot.v1");
    let mut exact_sources = BTreeMap::new();
    for entry in entries {
        if exclusions.contains(&entry.relative_path) {
            continue;
        }
        hash_snapshot_field(&mut hasher, &entry.relative_path);
        hash_snapshot_field(&mut hasher, entry.kind.as_str());
        if entry.kind.as_str() == "file" && !entry.is_symlink {
            let contained_file = contained_repo
                .read_regular_file(&entry.relative_path)
                .map_err(|error| {
                    CliError::Message(format!(
                        "contained snapshot read failed for {}: {error}",
                        entry.relative_path
                    ))
                })?;
            let requirement = ExactSourceRequirement {
                relative_path: entry.relative_path.clone(),
                byte_len: contained_file.bytes.len() as u64,
                sha256: sha256_hex(&contained_file.bytes),
            };
            hash_snapshot_field(&mut hasher, &requirement.byte_len.to_string());
            hash_snapshot_field(&mut hasher, &requirement.sha256);
            exact_sources.insert(entry.relative_path.clone(), requirement);
        } else {
            hash_snapshot_field(&mut hasher, entry.symlink_target.as_deref().unwrap_or(""));
        }
    }
    Ok(RepositorySnapshot {
        binding: format!("sha256:{:x}", hasher.finalize()),
        exact_sources,
    })
}

fn hash_snapshot_field(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value.as_bytes());
}

fn resolve_capture_authorization(
    repo_path: &Path,
    repository_binding: &str,
    selection: &CapturePolicySelection,
) -> Result<Option<RawPersistenceAuthorization>, CliError> {
    match selection {
        CapturePolicySelection::DefaultDeny => Ok(None),
        CapturePolicySelection::BuiltInSafeSourceClasses => {
            Ok(Some(RawPersistenceAuthorization::BuiltInSafeSourceClasses))
        }
        CapturePolicySelection::ExternalOperatorPolicy(policy_path) => {
            load_external_policy(repo_path, policy_path, repository_binding)
                .map(RawPersistenceAuthorization::External)
                .map(Some)
                .map_err(|error| CliError::Message(format!("capture policy rejected: {error}")))
        }
    }
}

fn capture_policy_context_row(
    repository_snapshot: &RepositorySnapshot,
    authorization: Option<&RawPersistenceAuthorization>,
) -> Row {
    let context = authorize_raw_persistence(
        "src/__codedb_policy_context.rs",
        b"",
        &repository_snapshot.binding,
        authorization,
    );
    row([
        ("table", "capture_policy_context".to_string()),
        (
            "repository_snapshot_id",
            repository_snapshot.binding.clone(),
        ),
        ("policy_id", context.policy.policy_id),
        ("policy_digest", context.policy.policy_digest),
        ("policy_binding_digest", context.policy.binding_digest),
        ("policy_authority", context.policy.authority),
        (
            "policy_authority_source",
            context.policy.authority_source.as_str().to_string(),
        ),
        (
            "policy_location",
            if context.policy.external_policy_path.is_some() {
                "external-to-repository"
            } else {
                "codedb-core-built-in"
            }
            .to_string(),
        ),
        (
            "reproduction_contract",
            "exact-source-sha256-and-byte-length".to_string(),
        ),
        ("raw_source_bytes_emitted", "false".to_string()),
    ])
}

fn source_policy_decision_row(
    source_metadata: &SourceBlobMetadata,
    decision: &RawPersistenceDecision,
) -> Row {
    let evidence = decision
        .classifier_evidence
        .iter()
        .map(|item| item.as_str())
        .collect::<Vec<_>>()
        .join(",");
    row([
        ("table", "source_policy".to_string()),
        ("relative_path", source_metadata.relative_path.clone()),
        ("sha256", source_metadata.sha256.clone()),
        ("bytes", source_metadata.byte_len.to_string()),
        (
            "exact_source_sha256",
            format!("sha256:{}", decision.exact_source.sha256),
        ),
        (
            "exact_source_bytes",
            decision.exact_source.byte_len.to_string(),
        ),
        ("source_class", decision.source_class.as_str().to_string()),
        ("mode", source_metadata.default_mode.as_str().to_string()),
        (
            "persistence_disposition",
            decision.disposition.as_str().to_string(),
        ),
        (
            "has_secret_like_material",
            source_metadata.has_secret_like_material.to_string(),
        ),
        (
            "classification_status",
            decision.classifier_status.as_str().to_string(),
        ),
        (
            "classification_evidence",
            if evidence.is_empty() {
                "none".to_string()
            } else {
                evidence
            },
        ),
        (
            "raw_blob_persisted",
            decision.raw_persistence_allowed().to_string(),
        ),
        ("reason", decision.reason.as_str().to_string()),
        ("policy_id", decision.policy.policy_id.clone()),
        ("policy_digest", decision.policy.policy_digest.clone()),
        (
            "policy_binding_digest",
            decision.policy.binding_digest.clone(),
        ),
        ("policy_authority", decision.policy.authority.clone()),
        (
            "policy_authority_source",
            decision.policy.authority_source.as_str().to_string(),
        ),
        (
            "repository_snapshot_id",
            decision.policy.repository_binding.clone(),
        ),
        (
            "policy_location",
            if decision.policy.external_policy_path.is_some() {
                "external-to-repository"
            } else {
                "codedb-core-built-in"
            }
            .to_string(),
        ),
        (
            "reproduction_contract",
            "operator-must-supply-exact-bytes-matching-sha256-and-byte-length".to_string(),
        ),
        ("raw_source_bytes_emitted", "false".to_string()),
    ])
}

fn capture_rows(
    selection: &RepoSelection,
    config: &CaptureConfig,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    capture_rows_after_scan(selection, config, args, || {})
}

#[allow(unused_assignments)]
fn capture_rows_after_scan<F>(
    selection: &RepoSelection,
    config: &CaptureConfig,
    args: &[String],
    after_scan: F,
) -> Result<Vec<Row>, CliError>
where
    F: FnOnce(),
{
    let repo_path = selection.repo_path.as_path();
    let store_spec = capture_store_selector(selection);
    let policy_selection = capture_policy_selection(args)?;
    let snapshot_exclusions = capture_snapshot_exclusions(repo_path, &store_spec, args)?;
    let contained_repo = ContainedDirectory::open_existing(repo_path)
        .map_err(|error| CliError::Message(format!("opening contained repository: {error}")))?;
    let entries = scan_filesystem(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    after_scan();
    let repository_snapshot = repository_snapshot(&contained_repo, &entries, &snapshot_exclusions)?;
    let authorization =
        resolve_capture_authorization(repo_path, &repository_snapshot.binding, &policy_selection)?;
    // One store open for the whole import; each batch is a durable commit. The
    // repository snapshot and policy are validated before either backend opens.
    let (mut store, store_path) = open_store_for_capture(&store_spec, args)?;
    // Resume: skip paths already durably captured by a prior (possibly interrupted)
    // run so an import continues from its last checkpoint instead of restarting.
    let already = if config.resume {
        store
            .captured_paths()
            .map_err(|e| CliError::Message(format!("reading resume checkpoint: {e}")))?
    } else {
        std::collections::BTreeSet::new()
    };

    let mut rows: Vec<Row> = Vec::new();
    rows.push(row([
        ("table", "meta_repo_selection".to_string()),
        ("repo_id", selection.repo_id.clone()),
        ("repo_path", repo_path.display().to_string()),
        ("store_path", store_path.clone()),
        ("selection_source", selection.selection_source.clone()),
        (
            "mutation_policy",
            "read_only_scan_store_is_only_write_target".to_string(),
        ),
    ]));
    rows.push(capture_policy_context_row(
        &repository_snapshot,
        authorization.as_ref(),
    ));

    let started = Instant::now();
    let mut captured = 0usize;
    let mut captured_bytes = 0u64;
    let mut metadata_only = 0usize;
    let mut directories = 0usize;
    let mut gaps = 0usize;
    let mut symlinks = 0usize;
    let mut resumed = 0usize;
    let mut batches = 0usize;
    let mut stopped_early = false;
    let mut batch: Vec<(String, Vec<u8>)> = Vec::new();
    let mut batch_bytes = 0u64;

    // Commit the pending batch as one durable checkpoint (fsync) and emit its rows.
    // A macro (not a closure) so it can borrow the surrounding mutable state inline.
    macro_rules! flush_batch {
        () => {{
            if !batch.is_empty() {
                let persisted = store
                    .persist_batch(&batch)
                    .map_err(|e| CliError::Message(format!("persist batch failed: {e}")))?;
                batches += 1;
                for blob in persisted {
                    captured += 1;
                    captured_bytes += blob.bytes;
                    rows.push(row([
                        ("table", "source_blobs".to_string()),
                        ("relative_path", blob.relative_path),
                        ("blob_ref", blob.blob_ref),
                        ("sha256", blob.sha256),
                        ("bytes", blob.bytes.to_string()),
                        ("status", "captured".to_string()),
                    ]));
                }
                batch.clear();
                batch_bytes = 0;
            }
        }};
    }

    for entry in entries {
        if snapshot_exclusions.contains(&entry.relative_path) {
            continue;
        }
        let kind = entry.kind.as_str();
        if kind == "directory" && !entry.is_symlink {
            directories += 1;
            continue;
        }
        if kind == "file" && !entry.is_symlink {
            if config.resume && already.contains(&entry.relative_path) {
                resumed += 1;
                continue;
            }
            let contained_file = contained_repo
                .read_regular_file(&entry.relative_path)
                .map_err(|error| {
                    CliError::Message(format!(
                        "contained read failed for {}: {error}",
                        entry.relative_path
                    ))
                })?;
            let bytes = contained_file.bytes;
            let source_metadata =
                capture_source_metadata_from_bytes(entry.relative_path.clone(), &bytes);
            let decision = authorize_raw_persistence(
                &entry.relative_path,
                &bytes,
                &repository_snapshot.binding,
                authorization.as_ref(),
            );
            let expected = repository_snapshot
                .exact_sources
                .get(&entry.relative_path)
                .ok_or_else(|| {
                    CliError::Message(format!(
                        "repository snapshot omitted regular file {}",
                        entry.relative_path
                    ))
                })?;
            if decision.exact_source != *expected {
                return Err(CliError::Message(format!(
                    "repository changed after policy snapshot for {}",
                    entry.relative_path
                )));
            }
            rows.push(source_policy_decision_row(&source_metadata, &decision));
            if !decision.raw_persistence_allowed() {
                metadata_only += 1;
                continue;
            }
            let len = bytes.len() as u64;
            // Adaptive: a file at/above the batch-bytes budget gets its own singleton
            // batch so one large blob can't stall a batch of small files.
            if len >= config.batch_bytes {
                flush_batch!();
                batch.push((entry.relative_path.clone(), bytes));
                flush_batch!();
            } else {
                batch.push((entry.relative_path.clone(), bytes));
                batch_bytes += len;
                if batch.len() >= config.batch_files || batch_bytes >= config.batch_bytes {
                    flush_batch!();
                }
            }
            // Timer: once a checkpoint is durable, stop cleanly if over budget. The
            // store keeps every committed batch; a re-run resumes from here.
            if config
                .time_budget
                .is_some_and(|budget| started.elapsed() >= budget)
            {
                stopped_early = true;
                break;
            }
        } else if entry.is_symlink {
            if config.resume && already.contains(&entry.relative_path) {
                resumed += 1;
                continue;
            }
            flush_batch!();
            let Some(target) = entry.symlink_target else {
                gaps += 1;
                rows.push(row([
                    ("table", "capture_gaps".to_string()),
                    ("relative_path", entry.relative_path),
                    ("kind", kind.to_string()),
                    ("gap", "non_utf8_symlink_target".to_string()),
                    ("status", "gap".to_string()),
                ]));
                continue;
            };
            let persisted = store
                .persist_symlink(&entry.relative_path, &target)
                .map_err(|error| {
                    CliError::Message(format!(
                        "persist symlink metadata failed for {}: {error}",
                        entry.relative_path
                    ))
                })?;
            symlinks += 1;
            rows.push(row([
                ("table", "source_symlinks".to_string()),
                ("relative_path", persisted.relative_path),
                ("target", persisted.target),
                ("target_sha256", persisted.target_sha256),
                ("status", "captured".to_string()),
            ]));
        } else {
            // Special files remain explicit gaps; symbolic links are first-class
            // checksum-bound metadata and never enter the regular-file blob path.
            gaps += 1;
            rows.push(row([
                ("table", "capture_gaps".to_string()),
                ("relative_path", entry.relative_path),
                ("kind", kind.to_string()),
                ("gap", format!("unsupported_entry_kind:{kind}")),
                ("status", "gap".to_string()),
            ]));
        }
    }
    // Final checkpoint for the trailing partial batch (also runs when the timer
    // fired mid-batch, so buffered files are never lost).
    flush_batch!();

    let status = if stopped_early {
        "time_budget_reached_resumable"
    } else if gaps == 0 {
        "complete"
    } else {
        "complete_with_gaps"
    };
    rows.push(row([
        ("table", "capture_summary".to_string()),
        ("store_path", store_path),
        ("files_captured", captured.to_string()),
        ("bytes_captured", captured_bytes.to_string()),
        ("files_metadata_only", metadata_only.to_string()),
        ("symlinks_captured", symlinks.to_string()),
        ("directories_walked", directories.to_string()),
        ("capture_gaps", gaps.to_string()),
        ("files_resumed_skipped", resumed.to_string()),
        ("batches_committed", batches.to_string()),
        ("elapsed_ms", started.elapsed().as_millis().to_string()),
        ("status", status.to_string()),
    ]));
    Ok(rows)
}

/// Compare two repo roots' source files by content hash and emit the merge plan:
/// a `merge_summary` row, one `crate_collision` row per shared package name, and
/// one `divergent` row per conflicting file (the surgical worklist). `--files`
/// additionally emits a `file` row per path with its per-repo sha.
fn merge_plan_rows(repo_a: &Path, repo_b: &Path, detail: bool) -> Result<Vec<Row>, CliError> {
    let plan = codedb_core::merge::merge_plan(repo_a, repo_b)
        .map_err(|e| CliError::Message(format!("merge-plan scan failed: {e}")))?;
    let mut rows: Vec<Row> = Vec::new();
    rows.push(row([
        ("table", "merge_summary".to_string()),
        ("repo_a", repo_a.display().to_string()),
        ("repo_b", repo_b.display().to_string()),
        ("identical", plan.identical.to_string()),
        ("divergent", plan.divergent.to_string()),
        ("unique_a", plan.unique_a.to_string()),
        ("unique_b", plan.unique_b.to_string()),
        ("crate_collisions", plan.crate_collisions.len().to_string()),
    ]));
    for name in &plan.crate_collisions {
        rows.push(row([
            ("table", "crate_collision".to_string()),
            ("package", name.clone()),
        ]));
    }
    for path in &plan.divergent_paths {
        rows.push(row([
            ("table", "divergent".to_string()),
            ("relative_path", path.clone()),
        ]));
    }
    if detail {
        for f in &plan.files {
            rows.push(row([
                ("table", "file".to_string()),
                ("relative_path", f.relative_path.clone()),
                ("status", f.status.as_str().to_string()),
                ("sha_a", f.sha_a.clone().unwrap_or_default()),
                ("sha_b", f.sha_b.clone().unwrap_or_default()),
            ]));
        }
    }
    Ok(rows)
}

/// Re-emit captured files byte-for-byte from the store (whole tree or one
/// --path), restoring unix modes; every row re-checksums the materialized file.
fn materialize_rows(
    store_spec: &str,
    out_dir: &Path,
    only: Option<&str>,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    let parsed_store = parse_store_spec(store_spec, args)?;
    let store_identity = materialize_store_identity(&parsed_store, args);
    let store = open_parsed_store_readonly(&parsed_store, args)?;
    let files = store
        .list_source_files()
        .map_err(|e| CliError::Message(format!("listing store files: {e}")))?;
    let symlinks = store
        .list_source_symlinks()
        .map_err(|e| CliError::Message(format!("listing store symlinks: {e}")))?;
    let selected_symlinks = symlinks
        .into_iter()
        .filter(|link| only.is_none_or(|filter| link.relative_path == filter))
        .collect::<Vec<_>>();
    // Validate every stored target and portable path before creating any output.
    // Metadata-only mode exercises the exact same target grammar without touching
    // the destination tree, so one unsafe link rejects the batch fail-closed.
    for link in &selected_symlinks {
        link.verify()
            .map_err(|error| CliError::Message(error.to_string()))?;
        materialize_symlink(
            out_dir,
            &link.relative_path,
            &link.target,
            SymlinkMaterializationStatus::MetadataOnlyFallback,
        )
        .map_err(|error| {
            CliError::Message(format!(
                "unsafe symlink materialization target {} -> {}: {error}",
                link.relative_path, link.target
            ))
        })?;
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut count = 0usize;
    let mut bytes = 0u64;
    let mut published_paths = Vec::new();
    for file in files {
        if only.is_some_and(|filter| file.relative_path != filter) {
            continue;
        }
        let out_path = match prepare_materialization_path(out_dir, &file.relative_path) {
            Ok(path) => path,
            Err(error) => {
                rollback_materialized_files(published_paths)?;
                return Err(CliError::Message(format!(
                    "unsafe materialization path {}: {error}",
                    file.relative_path
                )));
            }
        };
        let report = match store.materialize_source_file(&file.relative_path, &out_path) {
            Ok(report) => report,
            Err(error) => {
                rollback_materialized_files(published_paths)?;
                return Err(CliError::Message(format!(
                    "materialize failed for {}: {error}",
                    file.relative_path
                )));
            }
        };
        let rollback = match take_materialized_file_rollback(&report.path) {
            Ok(rollback) => rollback,
            Err(error) => {
                rollback_materialized_files(published_paths)?;
                return Err(CliError::Message(format!(
                    "materialization publication identity unavailable for {}; residual requires audit: {error}",
                    report.path.display()
                )));
            }
        };
        published_paths.push(rollback);
        let roundtrip_ok = report.sha256 == file.sha256;
        count += 1;
        bytes += report.bytes;
        rows.push(row([
            ("table", "materialized_files".to_string()),
            ("relative_path", file.relative_path),
            ("path", report.path.display().to_string()),
            ("sha256", report.sha256),
            ("bytes", report.bytes.to_string()),
            (
                "status",
                if roundtrip_ok {
                    "sha256_roundtrip_ok".to_string()
                } else {
                    "sha256_roundtrip_mismatch".to_string()
                },
            ),
        ]));
        if !roundtrip_ok {
            rollback_materialized_files(published_paths)?;
            return Err(CliError::Message(format!(
                "sha256 roundtrip mismatch materializing {}",
                rows.last()
                    .and_then(|r| r.get("relative_path").cloned())
                    .unwrap_or_default()
            )));
        }
    }
    let symlink_status = platform_symlink_materialization_status();
    let mut symlinks_materialized = 0usize;
    let mut symlinks_metadata_only = 0usize;
    let mut published_symlinks = Vec::new();
    for link in selected_symlinks {
        let report =
            match materialize_symlink(out_dir, &link.relative_path, &link.target, symlink_status) {
                Ok(report) => report,
                Err(error) => {
                    let rollback =
                        rollback_materialized_publications(published_paths, published_symlinks);
                    return Err(CliError::Message(format!(
                        "materialize symlink failed for {} -> {}: {error}{}",
                        link.relative_path,
                        link.target,
                        rollback
                            .err()
                            .map(|rollback_error| format!(
                                "; rollback conflict/residual audit: {rollback_error}"
                            ))
                            .unwrap_or_default()
                    )));
                }
            };
        if report.link_created {
            let rollback = match take_materialized_symlink_rollback(&report.path) {
                Ok(rollback) => rollback,
                Err(error) => {
                    let prior_rollback =
                        rollback_materialized_publications(published_paths, published_symlinks);
                    return Err(CliError::Message(format!(
                        "symlink materialization publication identity unavailable for {}; current residual requires audit: {error}{}",
                        report.path.display(),
                        prior_rollback
                            .err()
                            .map(|rollback_error| format!(
                                "; prior rollback conflict/residual audit: {rollback_error}"
                            ))
                            .unwrap_or_default()
                    )));
                }
            };
            published_symlinks.push(rollback);
            symlinks_materialized += 1;
        } else {
            symlinks_metadata_only += 1;
        }
        rows.push(row([
            ("table", "materialized_symlinks".to_string()),
            ("relative_path", link.relative_path),
            ("path", report.path.display().to_string()),
            ("target", report.target),
            ("target_sha256", link.target_sha256),
            ("platform_status", report.status.as_str().to_string()),
            ("link_created", report.link_created.to_string()),
            (
                "status",
                if report.link_created {
                    "symlink_roundtrip_ok".to_string()
                } else {
                    "metadata_only_fallback".to_string()
                },
            ),
        ]));
    }
    rows.push(row([
        ("table", "materialize_summary".to_string()),
        ("store_path", store_identity),
        ("out_dir", out_dir.display().to_string()),
        ("files_materialized", count.to_string()),
        ("bytes_materialized", bytes.to_string()),
        ("symlinks_materialized", symlinks_materialized.to_string()),
        ("symlinks_metadata_only", symlinks_metadata_only.to_string()),
        ("status", "complete".to_string()),
    ]));
    Ok(rows)
}

fn rollback_materialized_files(
    publications: Vec<MaterializedFileRollback>,
) -> Result<(), CliError> {
    let failures = publications
        .into_iter()
        .rev()
        .filter_map(|publication| rollback_materialized_file(publication).err())
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(CliError::Message(format!(
            "materialization rollback conflict/residual audit: {}",
            failures.join("; ")
        )))
    }
}

fn rollback_materialized_publications(
    files: Vec<MaterializedFileRollback>,
    symlinks: Vec<MaterializedSymlinkRollback>,
) -> Result<(), CliError> {
    let failures = symlinks
        .into_iter()
        .rev()
        .filter_map(|publication| rollback_materialized_symlink(publication).err())
        .chain(
            files
                .into_iter()
                .rev()
                .filter_map(|publication| rollback_materialized_file(publication).err()),
        )
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(CliError::Message(failures.join("; ")))
    }
}

fn scan_rows(selection: &RepoSelection) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    let filesystem_entries =
        scan_filesystem(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    let rust_items = rust_item_rows(repo_path)?;
    let manifest_path = repo_path.join("Cargo.toml");
    let cargo_capture = if manifest_path.exists() {
        Some(capture_repo_cargo(repo_path)?)
    } else {
        None
    };

    let mut rows = Vec::new();
    rows.push(meta_repo_selection_row(selection));
    rows.push(summary_row(
        "filesystem_entries",
        "available",
        filesystem_entries.len(),
        "read-only filesystem scan completed",
    ));
    rows.push(summary_row(
        "rust_items",
        "available",
        rust_items.len(),
        "static syntax item scan completed",
    ));
    if let Some((context, cargo_metadata)) = cargo_capture {
        rows.push(row([
            ("table", "codedb_contexts".to_string()),
            ("context_id", context.context_id),
            ("cargo_version", context.cargo_version),
            ("rustc_version", context.rustc_version),
            ("host_triple", context.host_triple),
            ("target_triple", context.target_triple),
            ("target_cfgs", context.target_cfgs.join(";")),
            ("requested_features", context.requested_features.join(";")),
            ("all_features", context.all_features.to_string()),
            (
                "no_default_features",
                context.no_default_features.to_string(),
            ),
            ("profile", context.profile),
            ("cargo_lock_sha256", context.cargo_lock_sha256),
            (
                "resolved_package_count",
                context.resolved_features.len().to_string(),
            ),
            ("status", "available".to_string()),
        ]));
        rows.push(summary_row(
            "cargo_packages",
            "available",
            cargo_metadata.packages.len(),
            "cargo metadata package rows captured",
        ));
        rows.push(summary_row(
            "cargo_dependencies",
            "available",
            cargo_metadata.dependencies.len(),
            "cargo metadata dependency rows captured",
        ));
        rows.push(summary_row(
            "cargo_sources",
            "available",
            cargo_metadata.sources.len(),
            "cargo source provenance rows captured",
        ));
    } else {
        rows.push(summary_row(
            "cargo_packages",
            "degraded",
            0,
            "Cargo.toml not found",
        ));
    }
    Ok(rows)
}

fn export_rows(
    table: &str,
    selection: &RepoSelection,
    harness_home_path: Option<&Path>,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    match table {
        "meta_repo_selection" | "repo_selection" => Ok(vec![meta_repo_selection_row(selection)]),
        "envctl" | "envctl_export" => envctl_export_rows(selection, args),
        "agent_harness" | "agent_harness_export" => {
            let rows = agent_harness_export_rows(
                selection,
                &resolve_harness_home_path(harness_home_path)?,
            )?;
            Ok(rows)
        }
        "agent_harness_manifests"
        | "agent_harness_sources"
        | "agent_harness_codex_settings"
        | "agent_harness_mcp_servers"
        | "agent_harness_plugins"
        | "agent_harness_plugin_skills"
        | "agent_harness_prompts"
        | "agent_harness_hooks"
        | "agent_harness_env"
        | "agent_harness_policy_rows"
        | "agent_harness_validation_errors"
        | "agent_harness_export_manifests"
        | "agent_harness_materialization_plan" => {
            let rows = agent_harness_export_rows(
                selection,
                &resolve_harness_home_path(harness_home_path)?,
            )?;
            Ok(rows
                .into_iter()
                .filter(|row| row.get("table").is_some_and(|value| value == table))
                .collect())
        }
        "codedb_tool_versions" | "tool_versions" => Ok(codedb_tool_version_rows()),
        "codedb_database_endpoints" | "database_endpoints" => {
            with_envctl_store_contract(codedb_database_endpoint_rows(repo_path), selection, args)
        }
        "codedb_capture_status" | "capture_status" => codedb_capture_status_rows(repo_path),
        "codedb_table_checksums" | "table_checksums" => codedb_table_checksum_rows(repo_path),
        "codedb_validation_errors" => Ok(envctl_validation_error_rows()),
        "codedb_cache_dirs" | "cache_dirs" => Ok(codedb_cache_dir_rows(repo_path)),
        "codedb_log_dirs" | "log_dirs" => Ok(codedb_log_dir_rows(repo_path)),
        "codedb_release_artifacts" | "release_artifacts" => {
            Ok(codedb_release_artifact_rows(repo_path))
        }
        "codedb_source_root_hashes" | "source_root_hashes" => {
            codedb_source_root_hash_rows(repo_path)
        }
        "codedb_materialization_targets" | "materialization_targets" => with_envctl_store_contract(
            codedb_materialization_target_rows(repo_path)?,
            selection,
            args,
        ),
        "codedb_export_manifests" | "export_manifests" => codedb_export_manifest_rows(repo_path),
        "codedb_runtime_integration" | "runtime_integration" => {
            Ok(codedb_runtime_integration_rows(repo_path))
        }
        "runner_proof_manifest" | "codedb_runner_proof_manifest" | "runner_proof" => {
            runner_proof_manifest_rows(selection)
        }
        "schema" | "schema_rows" => Ok(table_rows(schema_rows())),
        "tables" => Ok(table_rows(table_inventory())),
        "capture_gaps" | "gaps" => Ok(table_rows(capture_gaps())),
        "validation_errors" | "validation-errors" => Ok(table_rows(validation_errors())),
        "filesystem_entries" | "fs_entries" => filesystem_rows(repo_path),
        "rust_items" => rust_item_rows(repo_path),
        "cargo_packages" => cargo_package_rows(repo_path),
        "cargo_dependencies" | "cargo_deps" => cargo_dependency_rows(repo_path),
        "cargo_sources" => cargo_source_rows(repo_path),
        _ => Err(CliError::Message(format!(
            "unsupported export table: {table}; supported tables: meta_repo_selection, envctl, agent_harness, runner_proof_manifest, codedb_tool_versions, codedb_database_endpoints, codedb_capture_status, codedb_table_checksums, codedb_validation_errors, codedb_cache_dirs, codedb_log_dirs, codedb_release_artifacts, codedb_source_root_hashes, codedb_materialization_targets, codedb_export_manifests, codedb_runtime_integration, schema, tables, filesystem_entries, rust_items, cargo_packages, cargo_dependencies, cargo_sources, capture_gaps, validation_errors"
        ))),
    }
}

fn resolve_harness_home_path(explicit: Option<&Path>) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CliError::Message("agent harness export requires --home-path <path> or HOME".to_string())
    })
}

fn agent_harness_export_rows(
    selection: &RepoSelection,
    home_path: &Path,
) -> Result<Vec<Row>, CliError> {
    let mut rows = Vec::new();
    let mut validations = Vec::new();
    let mut source_ids = Vec::new();
    let codex_dir = home_path.join(".codex");
    let config_path = codex_dir.join("config.toml");
    let prompts_dir = codex_dir.join("prompts");
    let skills_dir = codex_dir.join("skills");
    let plugins_dir = codex_dir.join("plugins");
    let auth_path = codex_dir.join("auth.json");
    let yazelix_nushell_dir = home_path.join(".local/share/yazelix/initializers/nushell");
    let repo_path = selection.repo_path.as_path();
    let repo_kb_config = repo_path.join(".kb/config.toml");
    let repo_kb_agents = repo_path.join(".kb/AGENTS.md");
    let repo_kb_skills_dir = repo_path.join(".kb/skills");
    let repo_agents = repo_path.join("AGENTS.md");
    let manifest_id = format!(
        "agent_harness:{}:{}",
        selection.repo_id,
        sha256_hex(format!("{}::{}", repo_path.display(), home_path.display()).as_bytes())
    );

    for (source_id, path, source_class, owner_boundary) in [
        (
            "codex_config",
            config_path.as_path(),
            "codex_user_config",
            "user_local",
        ),
        (
            "repo_kb_config",
            repo_kb_config.as_path(),
            "repo_kb_config",
            "repo_local",
        ),
        (
            "repo_kb_agents",
            repo_kb_agents.as_path(),
            "repo_kb_agents",
            "repo_local",
        ),
        (
            "repo_agents",
            repo_agents.as_path(),
            "repo_agents",
            "repo_local",
        ),
        (
            "codex_auth_file",
            auth_path.as_path(),
            "codex_auth_file",
            "user_local",
        ),
    ] {
        if path.exists() {
            rows.push(agent_harness_source_row(
                source_id,
                path,
                source_class,
                owner_boundary,
            )?);
            rows.push(agent_harness_file_row(
                &manifest_id,
                source_id,
                path,
                source_class,
                owner_boundary,
            )?);
            source_ids.push(source_id.to_string());
        }
    }

    if config_path.exists() {
        let raw =
            fs::read_to_string(&config_path).map_err(|source| CliError::Core(Box::new(source)))?;
        let parsed: TomlValue = raw.parse().map_err(|source| {
            CliError::Message(format!(
                "failed to parse {}: {source}",
                config_path.display()
            ))
        })?;
        let Some(config_table) = parsed.as_table() else {
            return Err(CliError::Message(format!(
                "expected {} to be a TOML table",
                config_path.display()
            )));
        };

        for (key, value) in config_table {
            if key == "mcp_servers" || key == "hooks" {
                continue;
            }
            if let Some(value_string) = toml_scalar_string(value) {
                let (rendered_value, value_redacted, secret_ref) =
                    redacted_value(key, &value_string);
                rows.push(row([
                    ("table", "agent_harness_codex_settings".to_string()),
                    ("manifest_id", manifest_id.clone()),
                    ("key", key.clone()),
                    ("value", rendered_value),
                    ("value_redacted", value_redacted.to_string()),
                    ("secret_ref", secret_ref),
                    ("source_path", config_path.display().to_string()),
                    ("owner_boundary", "user_local".to_string()),
                    ("source_hash", sha256_hex(raw.as_bytes())),
                ]));
            }
        }

        if let Some(mcp_servers) = config_table
            .get("mcp_servers")
            .and_then(TomlValue::as_table)
        {
            let mut signatures: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (server_id, server_value) in mcp_servers {
                let Some(server_table) = server_value.as_table() else {
                    continue;
                };
                let command = server_table
                    .get("command")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default()
                    .to_string();
                let args = server_table
                    .get("args")
                    .and_then(TomlValue::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(toml_scalar_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let signature = format!("{command}::{args}");
                signatures
                    .entry(signature)
                    .or_default()
                    .push(server_id.clone());
                rows.push(row([
                    ("table", "agent_harness_mcp_servers".to_string()),
                    ("manifest_id", manifest_id.clone()),
                    ("server_id", server_id.clone()),
                    ("command", command),
                    ("args", args),
                    ("source_path", config_path.display().to_string()),
                    ("owner_boundary", "user_local".to_string()),
                ]));
            }
            for (signature, server_ids) in signatures {
                if server_ids.len() > 1 {
                    validations.push(agent_harness_validation_row(
                        &manifest_id,
                        "duplicate_mcp_command",
                        &format!(
                            "multiple MCP server entries share the same command signature: {}",
                            server_ids.join(",")
                        ),
                        &config_path,
                        Some(signature),
                    ));
                }
            }
        }

        if let Some(hooks) = config_table.get("hooks").and_then(TomlValue::as_table) {
            for (hook_id, hook_value) in hooks {
                let Some(hook_table) = hook_value.as_table() else {
                    continue;
                };
                let hook_enabled = hook_table
                    .get("enabled")
                    .and_then(TomlValue::as_bool)
                    .unwrap_or(true);
                let configured_command = hook_table
                    .get("command")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default()
                    .to_string();
                let fallback_script = repo_path
                    .join(".codex/hooks")
                    .join(format!("{}.sh", hook_id.replace('_', "-")));
                let resolved_path = if Path::new(&configured_command).is_file() {
                    PathBuf::from(&configured_command)
                } else if fallback_script.is_file() {
                    fallback_script.clone()
                } else {
                    PathBuf::from(&configured_command)
                };
                let hook_exists = resolved_path.is_file();
                rows.push(row([
                    ("table", "agent_harness_hooks".to_string()),
                    ("manifest_id", manifest_id.clone()),
                    ("hook_id", hook_id.clone()),
                    ("configured_command", configured_command),
                    ("enabled", hook_enabled.to_string()),
                    ("resolved_path", resolved_path.display().to_string()),
                    ("exists", hook_exists.to_string()),
                    (
                        "owner_boundary",
                        if resolved_path.starts_with(repo_path) {
                            "repo_local".to_string()
                        } else {
                            "user_local".to_string()
                        },
                    ),
                ]));
                if hook_exists {
                    rows.push(agent_harness_source_row(
                        &format!("hook:{hook_id}"),
                        &resolved_path,
                        "hook_entrypoint",
                        if resolved_path.starts_with(repo_path) {
                            "repo_local"
                        } else {
                            "user_local"
                        },
                    )?);
                    rows.push(agent_harness_file_row(
                        &manifest_id,
                        &format!("hook:{hook_id}"),
                        &resolved_path,
                        "hook_entrypoint",
                        if resolved_path.starts_with(repo_path) {
                            "repo_local"
                        } else {
                            "user_local"
                        },
                    )?);
                    source_ids.push(format!("hook:{hook_id}"));
                } else {
                    validations.push(agent_harness_validation_row(
                        &manifest_id,
                        "missing_hook_script",
                        &format!("configured hook {hook_id} does not resolve to a local script"),
                        &config_path,
                        Some(resolved_path.display().to_string()),
                    ));
                }
                if !hook_enabled {
                    validations.push(agent_harness_validation_row(
                        &manifest_id,
                        "disabled_hook",
                        &format!("configured hook {hook_id} is disabled and will not run"),
                        &config_path,
                        Some(hook_id.clone()),
                    ));
                }
            }
        }
    }

    for prompt_path in collect_files_recursive(&prompts_dir)? {
        if prompt_path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let prompt_name = prompt_path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("prompt")
            .to_string();
        rows.push(agent_harness_source_row(
            &format!("prompt:{prompt_name}"),
            &prompt_path,
            "codex_prompt",
            "user_local",
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("prompt:{prompt_name}"),
            &prompt_path,
            "codex_prompt",
            "user_local",
        )?);
        rows.push(row([
            ("table", "agent_harness_prompts".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("prompt_name", prompt_name.clone()),
            ("source_path", prompt_path.display().to_string()),
            (
                "source_hash",
                sha256_hex(
                    &fs::read(&prompt_path).map_err(|source| CliError::Core(Box::new(source)))?,
                ),
            ),
            ("owner_boundary", "user_local".to_string()),
        ]));
        source_ids.push(format!("prompt:{prompt_name}"));
    }

    for skill_path in collect_files_recursive(&skills_dir)? {
        if skill_path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") {
            continue;
        }
        let skill_name = skill_path
            .parent()
            .and_then(|value| value.file_name())
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
            .to_string();
        rows.push(agent_harness_source_row(
            &format!("skill:{skill_name}"),
            &skill_path,
            "codex_skill",
            "user_local",
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("skill:{skill_name}"),
            &skill_path,
            "codex_skill",
            "user_local",
        )?);
        rows.push(row([
            ("table", "agent_harness_plugin_skills".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("plugin_name", "codex_user_skills".to_string()),
            ("skill_name", skill_name.clone()),
            ("source_path", skill_path.display().to_string()),
            ("owner_boundary", "user_local".to_string()),
        ]));
        source_ids.push(format!("skill:{skill_name}"));
    }

    for skill_path in collect_files_recursive(&repo_kb_skills_dir)? {
        if skill_path.file_name().and_then(|value| value.to_str()) != Some("SKILL.md") {
            continue;
        }
        let skill_name = skill_path
            .parent()
            .and_then(|value| value.file_name())
            .and_then(|value| value.to_str())
            .unwrap_or("skill")
            .to_string();
        rows.push(agent_harness_source_row(
            &format!("repo_skill:{skill_name}"),
            &skill_path,
            "repo_kb_skill",
            "repo_local",
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("repo_skill:{skill_name}"),
            &skill_path,
            "repo_kb_skill",
            "repo_local",
        )?);
        rows.push(row([
            ("table", "agent_harness_plugin_skills".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("plugin_name", "repo_kb_skills".to_string()),
            ("skill_name", skill_name.clone()),
            ("source_path", skill_path.display().to_string()),
            ("owner_boundary", "repo_local".to_string()),
        ]));
        source_ids.push(format!("repo_skill:{skill_name}"));
    }

    let mut plugins_by_name: BTreeMap<String, Vec<PluginRecord>> = BTreeMap::new();
    for plugin_path in collect_files_recursive(&plugins_dir)? {
        if plugin_path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw =
            fs::read_to_string(&plugin_path).map_err(|source| CliError::Core(Box::new(source)))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|source| {
            CliError::Message(format!(
                "failed to parse {}: {source}",
                plugin_path.display()
            ))
        })?;
        let plugin_name = parsed
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown-plugin")
            .to_string();
        let plugin_version = parsed
            .get("version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let plugin_owner = parsed
            .get("owner")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown-owner")
            .to_string();
        plugins_by_name
            .entry(plugin_name.clone())
            .or_default()
            .push(PluginRecord {
                name: plugin_name.clone(),
                version: plugin_version.clone(),
                owner: plugin_owner.clone(),
                source_path: plugin_path.clone(),
            });
        rows.push(agent_harness_source_row(
            &format!("plugin:{plugin_name}"),
            &plugin_path,
            "codex_plugin_metadata",
            "user_local",
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("plugin:{plugin_name}"),
            &plugin_path,
            "codex_plugin_metadata",
            "user_local",
        )?);
        rows.push(row([
            ("table", "agent_harness_plugins".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("plugin_name", plugin_name.clone()),
            ("plugin_version", plugin_version),
            ("plugin_owner", plugin_owner),
            ("source_path", plugin_path.display().to_string()),
            ("owner_boundary", "user_local".to_string()),
        ]));
        source_ids.push(format!("plugin:{plugin_name}"));
    }

    for records in plugins_by_name.values() {
        let owner_set = records
            .iter()
            .map(|record| record.owner.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if records.len() > 1 && owner_set.len() > 1 {
            validations.push(agent_harness_validation_row(
                &manifest_id,
                "duplicate_plugin_ownership",
                &format!(
                    "plugin {} has conflicting owners across {}",
                    records[0].name,
                    records
                        .iter()
                        .map(|record| record.source_path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                &records[0].source_path,
                Some(records[0].name.clone()),
            ));
        }
        for record in records {
            if let Some(parent_version) = version_dir_name(&record.source_path)
                && !record.version.is_empty()
                && parent_version != record.version
            {
                validations.push(agent_harness_validation_row(
                    &manifest_id,
                    "stale_plugin_metadata",
                    &format!(
                        "plugin {} metadata version {} does not match cache path {}",
                        record.name, record.version, parent_version
                    ),
                    &record.source_path,
                    Some(record.name.clone()),
                ));
            }
        }
    }

    rows.push(row([
        ("table", "agent_harness_env".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("env_key", "CODEX_HOME".to_string()),
        ("env_value", codex_dir.display().to_string()),
        ("value_redacted", "false".to_string()),
        ("target_class", "user_local".to_string()),
    ]));
    rows.push(row([
        ("table", "agent_harness_env".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("env_key", "CODEDB_REPO_ROOT".to_string()),
        ("env_value", repo_path.display().to_string()),
        ("value_redacted", "false".to_string()),
        ("target_class", "repo_local".to_string()),
    ]));
    for (env_key, env_value) in secret_env_entries() {
        let (rendered_value, value_redacted, secret_ref) = redacted_value(&env_key, &env_value);
        rows.push(row([
            ("table", "agent_harness_env".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("env_key", env_key),
            ("env_value", rendered_value),
            ("value_redacted", value_redacted.to_string()),
            ("secret_ref", secret_ref),
            ("target_class", "private_env".to_string()),
        ]));
    }
    if yazelix_nushell_dir.exists() {
        rows.push(row([
            ("table", "agent_harness_env".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("env_key", "YAZELIX_NUSHELL_INIT_DIR".to_string()),
            ("env_value", yazelix_nushell_dir.display().to_string()),
            ("value_redacted", "false".to_string()),
            ("target_class", "generated_state".to_string()),
        ]));
        for file_name in ["codedb_init.nu", "codedb_extern.nu"] {
            let bridge_path = yazelix_nushell_dir.join(file_name);
            if bridge_path.is_file() {
                rows.push(agent_harness_source_row(
                    &format!("generated:{file_name}"),
                    &bridge_path,
                    "yazelix_generated_bridge",
                    "generated_state",
                )?);
                rows.push(agent_harness_file_row(
                    &manifest_id,
                    &format!("generated:{file_name}"),
                    &bridge_path,
                    "yazelix_generated_bridge",
                    "generated_state",
                )?);
                source_ids.push(format!("generated:{file_name}"));
            } else {
                validations.push(agent_harness_validation_row(
                    &manifest_id,
                    "generated_state_missing",
                    &format!("expected Yazelix generated bridge file {file_name} is missing"),
                    &bridge_path,
                    Some(file_name.to_string()),
                ));
            }
        }
        let init_path = yazelix_nushell_dir.join("codedb_init.nu");
        if init_path.is_file() {
            let init_raw = fs::read_to_string(&init_path)
                .map_err(|source| CliError::Core(Box::new(source)))?;
            if !init_raw.contains("CODEDB_YAZELIX_BRIDGE_MODE = \"generated-state\"") {
                validations.push(agent_harness_validation_row(
                    &manifest_id,
                    "generated_state_stale",
                    "Yazelix generated bridge is missing the generated-state mode marker",
                    &init_path,
                    Some("codedb_init.nu".to_string()),
                ));
            }
        }
    }
    rows.push(row([
        ("table", "agent_harness_policy_rows".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("policy_id", "secret_redaction".to_string()),
        ("policy_value", "hash_secret_like_values".to_string()),
        ("source_authority", "codedb".to_string()),
    ]));
    rows.push(row([
        ("table", "agent_harness_policy_rows".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("policy_id", "materialization_gate".to_string()),
        (
            "policy_value",
            "approval_required_no_live_overwrite".to_string(),
        ),
        ("source_authority", "codedb".to_string()),
    ]));

    let mut materialization_rows = Vec::new();
    for harness_row in rows.iter().filter(|row| {
        row.get("table").is_some_and(|table| {
            matches!(
                table.as_str(),
                "agent_harness_sources"
                    | "agent_harness_prompts"
                    | "agent_harness_plugin_skills"
                    | "agent_harness_plugins"
                    | "agent_harness_hooks"
            )
        })
    }) {
        let source_path = harness_row
            .get("source_path")
            .cloned()
            .or_else(|| harness_row.get("resolved_path").cloned())
            .unwrap_or_default();
        let target_class = if Path::new(&source_path).starts_with(home_path) {
            "user_local"
        } else {
            "repo_local"
        };
        materialization_rows.push(row([
            ("table", "agent_harness_materialization_plan".to_string()),
            ("manifest_id", manifest_id.clone()),
            ("target_path", source_path.clone()),
            ("target_class", target_class.to_string()),
            ("mutation_allowed", "false".to_string()),
            ("apply_mode", "approval_required".to_string()),
            (
                "reproduction_policy",
                if target_class == "user_local" {
                    "explicit_user_approval"
                } else {
                    "repo_worktree_proposal"
                }
                .to_string(),
            ),
        ]));
    }
    rows.extend(materialization_rows);

    rows.append(&mut validations);
    let component_count = source_ids.len()
        + rows
            .iter()
            .filter(|row| {
                row.get("table").is_some_and(|table| {
                    matches!(
                        table.as_str(),
                        "agent_harness_codex_settings"
                            | "agent_harness_mcp_servers"
                            | "agent_harness_hooks"
                            | "agent_harness_env"
                            | "agent_harness_policy_rows"
                    )
                })
            })
            .count();
    let validation_count = rows
        .iter()
        .filter(|row| {
            row.get("table")
                .is_some_and(|table| table == "agent_harness_validation_errors")
        })
        .count();
    rows.push(row([
        ("table", "agent_harness_manifests".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("repo_id", selection.repo_id.clone()),
        ("repo_path", repo_path.display().to_string()),
        ("home_path", home_path.display().to_string()),
        ("component_count", component_count.to_string()),
        ("validation_count", validation_count.to_string()),
        ("generated_at", export_timestamp()),
    ]));
    rows.push(row([
        ("table", "agent_harness_export_manifests".to_string()),
        ("manifest_id", manifest_id.clone()),
        ("export_format_set", "json;nuon;csv".to_string()),
        (
            "plan_table",
            "agent_harness_materialization_plan".to_string(),
        ),
        (
            "validation_table",
            "agent_harness_validation_errors".to_string(),
        ),
        (
            "materialization_policy",
            "bounded_non_mutating_plan_only".to_string(),
        ),
        ("row_checksum", rows_checksum("agent_harness_export", &rows)),
    ]));

    Ok(rows)
}

fn agent_harness_source_row(
    source_id: &str,
    path: &Path,
    source_class: &str,
    owner_boundary: &str,
) -> Result<Row, CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::Core(Box::new(source)))?;
    Ok(row([
        ("table", "agent_harness_sources".to_string()),
        ("source_id", source_id.to_string()),
        ("source_path", path.display().to_string()),
        ("source_class", source_class.to_string()),
        ("owner_boundary", owner_boundary.to_string()),
        ("source_hash", sha256_hex(&bytes)),
        ("byte_len", bytes.len().to_string()),
    ]))
}

fn agent_harness_file_row(
    manifest_id: &str,
    source_id: &str,
    path: &Path,
    source_class: &str,
    owner_boundary: &str,
) -> Result<Row, CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::Core(Box::new(source)))?;
    Ok(row([
        ("table", "agent_harness_files".to_string()),
        ("manifest_id", manifest_id.to_string()),
        ("source_id", source_id.to_string()),
        ("source_path", path.display().to_string()),
        (
            "file_name",
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
        ),
        ("source_class", source_class.to_string()),
        ("owner_boundary", owner_boundary.to_string()),
        ("source_hash", sha256_hex(&bytes)),
        ("byte_len", bytes.len().to_string()),
    ]))
}

fn agent_harness_validation_row(
    manifest_id: &str,
    code: &str,
    message: &str,
    source_path: &Path,
    detail: Option<String>,
) -> Row {
    row([
        ("table", "agent_harness_validation_errors".to_string()),
        ("manifest_id", manifest_id.to_string()),
        ("code", code.to_string()),
        ("message", message.to_string()),
        ("source_path", source_path.display().to_string()),
        ("detail", detail.unwrap_or_default()),
    ])
}

fn version_dir_name(path: &Path) -> Option<String> {
    for ancestor in path.ancestors().skip(1) {
        let Some(name) = ancestor.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.contains('.') && name.chars().any(|ch| ch.is_ascii_digit()) {
            return Some(name.to_string());
        }
    }
    None
}

fn collect_files_recursive(root: &Path) -> Result<Vec<PathBuf>, CliError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::metadata(&path).map_err(|source| CliError::Core(Box::new(source)))?;
        if metadata.is_dir() {
            let mut entries = fs::read_dir(&path)
                .map_err(|source| CliError::Core(Box::new(source)))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|source| CliError::Core(Box::new(source)))?;
            entries.sort_by_key(|entry| entry.path());
            for entry in entries.into_iter().rev() {
                stack.push(entry.path());
            }
        } else if metadata.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn toml_scalar_string(value: &TomlValue) -> Option<String> {
    match value {
        TomlValue::String(value) => Some(value.clone()),
        TomlValue::Integer(value) => Some(value.to_string()),
        TomlValue::Float(value) => Some(value.to_string()),
        TomlValue::Boolean(value) => Some(value.to_string()),
        TomlValue::Datetime(value) => Some(value.to_string()),
        TomlValue::Array(values) => Some(
            values
                .iter()
                .filter_map(toml_scalar_string)
                .collect::<Vec<_>>()
                .join(";"),
        ),
        TomlValue::Table(_) => None,
    }
}

fn redacted_value(key: &str, value: &str) -> (String, bool, String) {
    if secret_like_key(key) || secret_like_value(value) {
        (
            "[redacted]".to_string(),
            true,
            format!("sha256:{}", sha256_hex(value.as_bytes())),
        )
    } else {
        (value.to_string(), false, String::new())
    }
}

fn secret_like_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    ["token", "secret", "api_key", "apikey", "password", "auth"]
        .iter()
        .any(|candidate| normalized.contains(candidate))
}

fn secret_like_value(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    normalized.contains("sk-")
        || normalized.contains("ghp_")
        || normalized.contains("github_pat_")
        || normalized.contains("token")
        || normalized.contains("secret")
}

fn secret_env_entries() -> Vec<(String, String)> {
    let mut entries = env::vars()
        .filter(|(key, _)| secret_like_key(key))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn envctl_store_spec(selection: &RepoSelection, args: &[String]) -> Result<StoreSpec, CliError> {
    let selector = if selection.store_path.is_empty() {
        selection
            .repo_path
            .join(".codedb/store.redb")
            .display()
            .to_string()
    } else {
        selection.store_path.clone()
    };
    parse_store_spec(&selector, args)
}

fn with_envctl_store_contract(
    rows: Vec<Row>,
    selection: &RepoSelection,
    args: &[String],
) -> Result<Vec<Row>, CliError> {
    let store_spec = envctl_store_spec(selection, args)?;
    let (store_backend, store_identity) = match store_spec.backend() {
        StoreBackend::Redb => ("redb", format!("redb:{}", store_spec.redacted())),
        StoreBackend::PostgreSql => ("postgresql", format!("postgresql:{}", pg_table_name(args))),
    };

    Ok(rows
        .into_iter()
        .map(|mut row| {
            let backend_relevant = matches!(
                row.get("table").map(String::as_str),
                Some(
                    "meta_repo_selection"
                        | "codedb_database_endpoints"
                        | "codedb_materialization_targets"
                )
            );
            let meta_selection = row
                .get("table")
                .is_some_and(|table| table == "meta_repo_selection");
            if backend_relevant {
                row.insert("store_backend".to_string(), store_backend.to_string());
                row.insert("store_identity".to_string(), store_identity.clone());
                row.insert(
                    "backend_internal_access".to_string(),
                    "forbidden".to_string(),
                );
            }
            if meta_selection {
                row.insert("store_path".to_string(), store_identity.clone());
            }
            row
        })
        .collect())
}

fn envctl_export_rows(selection: &RepoSelection, args: &[String]) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    let mut rows = Vec::new();
    rows.push(meta_repo_selection_row(selection));
    rows.extend(codedb_export_manifest_rows(repo_path)?);
    rows.extend(codedb_tool_version_rows());
    rows.extend(codedb_database_endpoint_rows(repo_path));
    rows.extend(codedb_capture_status_rows(repo_path)?);
    rows.extend(codedb_table_checksum_rows(repo_path)?);
    rows.extend(envctl_validation_error_rows());
    rows.extend(table_rows(capture_gaps()).into_iter().map(|mut row| {
        row.insert("table".to_string(), "codedb_capture_gaps".to_string());
        row
    }));
    rows.extend(codedb_source_root_hash_rows(repo_path)?);
    rows.extend(codedb_materialization_target_rows(repo_path)?);
    rows.extend(codedb_cache_dir_rows(repo_path));
    rows.extend(codedb_log_dir_rows(repo_path));
    rows.extend(codedb_release_artifact_rows(repo_path));
    rows.extend(codedb_runtime_integration_rows(repo_path));
    with_envctl_store_contract(rows, selection, args)
}

fn codedb_export_manifest_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let checksum_rows = codedb_table_checksum_rows(repo_path)?;
    let checksum = rows_checksum("codedb_table_checksums", &checksum_rows);
    Ok(vec![envctl_row(
        "codedb_export_manifests",
        "codedb_export_manifests:package:envctl",
        [
            ("artifact_id", "codedb_envctl_export".to_string()),
            ("format_set", "json;nuon;csv".to_string()),
            ("generator", "codedb export envctl".to_string()),
            ("source_table", "codedb_table_checksums".to_string()),
            ("source_table_checksum", checksum),
            ("generation_timestamp", export_timestamp()),
            (
                "redaction_policy",
                "no_raw_secrets;paths_allowed".to_string(),
            ),
            ("manual_edits_allowed", "false".to_string()),
            ("header_status", "structured_rows".to_string()),
            ("secret_policy", "secret_refs_only".to_string()),
            ("authority", "codedb".to_string()),
            ("consumer", "envctl".to_string()),
            (
                "declared_runtime_table",
                "codedb_runtime_integration".to_string(),
            ),
        ],
    )])
}

fn codedb_tool_version_rows() -> Vec<Row> {
    vec![
        envctl_row(
            "codedb_tool_versions",
            "codedb_tool_versions:codedb:cli",
            [
                ("tool_name", "codedb".to_string()),
                ("detected_version", codedb_core::VERSION.to_string()),
                ("expected_version", codedb_core::VERSION.to_string()),
                ("install_source", "cargo_workspace".to_string()),
                ("version_command", "codedb --version".to_string()),
                ("package_source", "crates/codedb".to_string()),
                ("install_status", "available".to_string()),
                ("release_manifest", "true".to_string()),
            ],
        ),
        envctl_row(
            "codedb_tool_versions",
            "codedb_tool_versions:codedb:nu_plugin_codedb",
            [
                ("tool_name", "nu_plugin_codedb".to_string()),
                ("detected_version", codedb_core::VERSION.to_string()),
                ("expected_version", codedb_core::VERSION.to_string()),
                ("install_source", "cargo_workspace".to_string()),
                (
                    "version_command",
                    "nu plugin metadata nu_plugin_codedb".to_string(),
                ),
                ("package_source", "crates/nu_plugin_codedb".to_string()),
                ("install_status", "build_required".to_string()),
                ("release_manifest", "true".to_string()),
            ],
        ),
    ]
}

fn codedb_database_endpoint_rows(repo_path: &Path) -> Vec<Row> {
    vec![envctl_row(
        "codedb_database_endpoints",
        "codedb_database_endpoints:codedb:export_only",
        [
            ("endpoint_kind", "export_only".to_string()),
            ("repo_path", repo_path.display().to_string()),
            ("direct_storage_access", "forbidden".to_string()),
            (
                "export_surface",
                "codedb export <table> --format json|nuon|csv".to_string(),
            ),
            (
                "validation_message",
                "envctl consumes exported datatables and never reads CodeDB backend internals"
                    .to_string(),
            ),
        ],
    )]
}

fn codedb_capture_status_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let filesystem_rows = filesystem_rows(repo_path)?;
    let rust_item_rows = rust_item_rows(repo_path)?;
    let mut rows = vec![
        envctl_row(
            "codedb_capture_status",
            "codedb_capture_status:filesystem_entries",
            [
                ("capture_kind", "filesystem_entries".to_string()),
                ("row_count", filesystem_rows.len().to_string()),
                ("status", "available".to_string()),
                (
                    "note",
                    "CodeDB file-to-datatable inventory is authoritative".to_string(),
                ),
            ],
        ),
        envctl_row(
            "codedb_capture_status",
            "codedb_capture_status:rust_items",
            [
                ("capture_kind", "rust_items".to_string()),
                ("row_count", rust_item_rows.len().to_string()),
                ("status", "available".to_string()),
                (
                    "note",
                    "CodeDB Rust semantic rows are authoritative for envctl consumers".to_string(),
                ),
            ],
        ),
    ];
    if repo_path.join("Cargo.toml").exists() {
        rows.push(envctl_row(
            "codedb_capture_status",
            "codedb_capture_status:cargo_metadata",
            [
                ("capture_kind", "cargo_metadata".to_string()),
                (
                    "row_count",
                    cargo_package_rows(repo_path)?.len().to_string(),
                ),
                ("status", "available".to_string()),
                (
                    "note",
                    "CodeDB crate metadata rows are authoritative for envctl consumers".to_string(),
                ),
            ],
        ));
    }
    Ok(rows)
}

fn codedb_table_checksum_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let mut checksummed_tables = vec![
        ("schema", table_rows(schema_rows())),
        ("tables", table_rows(table_inventory())),
        ("capture_gaps", table_rows(capture_gaps())),
        ("validation_errors", table_rows(validation_errors())),
        ("filesystem_entries", filesystem_rows(repo_path)?),
        ("rust_items", rust_item_rows(repo_path)?),
        (
            "codedb_runtime_integration",
            codedb_runtime_integration_rows(repo_path),
        ),
    ];
    if repo_path.join("Cargo.toml").exists() {
        checksummed_tables.push(("cargo_packages", cargo_package_rows(repo_path)?));
        checksummed_tables.push(("cargo_dependencies", cargo_dependency_rows(repo_path)?));
        checksummed_tables.push(("cargo_sources", cargo_source_rows(repo_path)?));
    }

    Ok(checksummed_tables
        .into_iter()
        .map(|(source_table, rows)| {
            envctl_row(
                "codedb_table_checksums",
                &format!("codedb_table_checksums:{source_table}"),
                [
                    ("source_table", source_table.to_string()),
                    ("row_count", rows.len().to_string()),
                    ("sha256", rows_checksum(source_table, &rows)),
                    ("checksum_format", "codedb_row_stream_v1".to_string()),
                    (
                        "authority",
                        "codedb_authoritative_datatable_export".to_string(),
                    ),
                ],
            )
        })
        .collect())
}

fn envctl_validation_error_rows() -> Vec<Row> {
    let source_rows = table_rows(validation_errors());
    if source_rows.is_empty() {
        return vec![envctl_row(
            "codedb_validation_errors",
            "codedb_validation_errors:none",
            [
                ("error_table", "validation_errors".to_string()),
                ("error_count", "0".to_string()),
                ("validation_status", "ok".to_string()),
                (
                    "validation_message",
                    "no CodeDB validation errors are currently reported".to_string(),
                ),
            ],
        )];
    }

    source_rows
        .into_iter()
        .enumerate()
        .map(|(index, source_row)| {
            let mut row = envctl_row(
                "codedb_validation_errors",
                &format!("codedb_validation_errors:{index}"),
                [
                    ("error_table", "validation_errors".to_string()),
                    ("validation_status", "warning".to_string()),
                    (
                        "validation_message",
                        "CodeDB exports validation errors as datatable rows for envctl".to_string(),
                    ),
                ],
            );
            for (key, value) in source_row {
                row.insert(format!("source_{key}"), value);
            }
            row
        })
        .collect()
}

fn codedb_cache_dir_rows(repo_path: &Path) -> Vec<Row> {
    vec![envctl_row(
        "codedb_cache_dirs",
        "codedb_cache_dirs:codedb:default",
        [
            (
                "path",
                repo_path.join(".codedb/cache").display().to_string(),
            ),
            ("path_kind", "cache".to_string()),
            ("owner", "codedb".to_string()),
            ("managed_by", "codedb".to_string()),
            ("validation_status", "deferred".to_string()),
            (
                "deferred_reason",
                "cache directory materialization is outside CDB035".to_string(),
            ),
        ],
    )]
}

fn codedb_log_dir_rows(repo_path: &Path) -> Vec<Row> {
    vec![envctl_row(
        "codedb_log_dirs",
        "codedb_log_dirs:codedb:default",
        [
            ("path", repo_path.join(".codedb/logs").display().to_string()),
            ("path_kind", "log".to_string()),
            ("owner", "codedb".to_string()),
            ("managed_by", "codedb".to_string()),
            ("validation_status", "deferred".to_string()),
            (
                "deferred_reason",
                "log directory materialization is outside CDB035".to_string(),
            ),
        ],
    )]
}

fn codedb_release_artifact_rows(repo_path: &Path) -> Vec<Row> {
    vec![
        envctl_row(
            "codedb_release_artifacts",
            "codedb_release_artifacts:codedb:cli",
            [
                ("artifact_id", "codedb_cli".to_string()),
                ("artifact_kind", "binary".to_string()),
                ("path", "target/debug/codedb".to_string()),
                ("source_path", "crates/codedb".to_string()),
                ("repo_path", repo_path.display().to_string()),
                ("validation_status", "unknown".to_string()),
            ],
        ),
        envctl_row(
            "codedb_release_artifacts",
            "codedb_release_artifacts:codedb:nu_plugin",
            [
                ("artifact_id", "nu_plugin_codedb".to_string()),
                ("artifact_kind", "nu_plugin_binary".to_string()),
                ("path", "target/debug/nu_plugin_codedb".to_string()),
                ("source_path", "crates/nu_plugin_codedb".to_string()),
                ("repo_path", repo_path.display().to_string()),
                ("validation_status", "unknown".to_string()),
            ],
        ),
    ]
}

fn codedb_source_root_hash_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let rows = filesystem_rows(repo_path)?;
    Ok(vec![envctl_row(
        "codedb_source_root_hashes",
        "codedb_source_root_hashes:codedb:filesystem_entries",
        [
            ("repo_path", repo_path.display().to_string()),
            ("source_table", "filesystem_entries".to_string()),
            (
                "source_table_checksum",
                rows_checksum("filesystem_entries", &rows),
            ),
            ("row_count", rows.len().to_string()),
            ("hash_scope", "file_identity_table".to_string()),
        ],
    )])
}

fn codedb_materialization_target_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let filesystem = filesystem_rows(repo_path)?;
    let checksum = rows_checksum("filesystem_entries", &filesystem);
    Ok(vec![
        envctl_row(
            "codedb_materialization_targets",
            "codedb_materialization_targets:envctl:source_files",
            [
                ("repo_path", repo_path.display().to_string()),
                ("target_table", "source_files".to_string()),
                ("source_table", "filesystem_entries".to_string()),
                ("source_table_checksum", checksum.clone()),
                ("materialization_owner", "envctl".to_string()),
                ("materialization_mode", "explicit_request_only".to_string()),
                ("write_policy", "refuse_unauthorized_paths".to_string()),
                (
                    "roundtrip_status",
                    "declared_and_checksum_bound".to_string(),
                ),
                (
                    "validation_message",
                    "envctl may materialize files only from exported rows and checksums"
                        .to_string(),
                ),
            ],
        ),
        envctl_row(
            "codedb_materialization_targets",
            "codedb_materialization_targets:codedb:blob_refs",
            [
                ("repo_path", repo_path.display().to_string()),
                ("target_table", "source_blobs".to_string()),
                ("source_table", "codedb_source_root_hashes".to_string()),
                ("source_table_checksum", checksum),
                ("materialization_owner", "codedb_selected_store".to_string()),
                (
                    "materialization_mode",
                    "sha256_blob_ref_roundtrip".to_string(),
                ),
                ("write_policy", "hash_addressed_content_only".to_string()),
                (
                    "roundtrip_status",
                    "store_restore_materialize_proven".to_string(),
                ),
                (
                    "validation_message",
                    "the selected CodeDB store restores source blobs by hash before envctl consumes file rows"
                        .to_string(),
                ),
            ],
        ),
    ])
}

fn codedb_runtime_integration_rows(repo_path: &Path) -> Vec<Row> {
    vec![
        envctl_row(
            "codedb_runtime_integration",
            "codedb_runtime_integration:envctl:authority_boundary",
            [
                ("repo_path", repo_path.display().to_string()),
                ("runtime_surface", "envctl".to_string()),
                ("integration_owner", "envctl".to_string()),
                ("source_authority", "codedb_export_rows".to_string()),
                (
                    "materialization_owner",
                    "envctl_or_yazelix_when_requested".to_string(),
                ),
                ("envctl_role", "consume_exports_materialize_files_when_requested".to_string()),
                ("backend_internal_access", "forbidden".to_string()),
                ("native_nu_file_tables", "interactive_edge_only".to_string()),
                (
                    "accuracy_basis",
                    "codedb_typed_rows_blob_semantics_rust_crate_facts".to_string(),
                ),
                ("tool_table_ref", "codedb_tool_versions".to_string()),
                ("checksum_table_ref", "codedb_table_checksums".to_string()),
                ("runtime_status", "declared".to_string()),
                (
                    "validation_message",
                    "envctl consumes CodeDB datatable exports and does not derive Rust/crate facts"
                        .to_string(),
                ),
            ],
        ),
        envctl_row(
            "codedb_runtime_integration",
            "codedb_runtime_integration:yazelix:generated_bridge",
            [
                ("repo_path", repo_path.display().to_string()),
                ("runtime_surface", "yazelix_generated_bridge".to_string()),
                ("integration_owner", "yazelix_envctl".to_string()),
                ("source_authority", "codedb_bridge_templates".to_string()),
                ("materialization_owner", "yazelix_or_envctl_state_generation".to_string()),
                ("envctl_role", "materialize_declared_bridge_files_when_requested".to_string()),
                ("backend_internal_access", "forbidden".to_string()),
                ("bridge_manifest_ref", "codedb_yazelix_bridge_artifacts".to_string()),
                (
                    "source_template_ref",
                    "templates/nushell/codedb_init.nu;templates/nushell/codedb_extern.nu"
                        .to_string(),
                ),
                ("generated_artifact_policy", "generated_state_only".to_string()),
                ("plugin_registry_mutation", "forbidden_by_default".to_string()),
                ("runtime_status", "declared".to_string()),
                (
                    "validation_message",
                    "generated bridge rows describe materialization inputs without editing tracked Yazelix config"
                        .to_string(),
                ),
            ],
        ),
        envctl_row(
            "codedb_runtime_integration",
            "codedb_runtime_integration:codedb:runtime_tools",
            [
                ("repo_path", repo_path.display().to_string()),
                ("runtime_surface", "codedb_cli_and_nu_plugin".to_string()),
                ("integration_owner", "codedb".to_string()),
                ("source_authority", "codedb_runtime_tool_package".to_string()),
                ("materialization_owner", "yazelix_runtime_package".to_string()),
                ("envctl_role", "consume_tool_and_checksum_rows".to_string()),
                ("backend_internal_access", "forbidden".to_string()),
                ("tool_table_ref", "codedb_tool_versions".to_string()),
                ("release_artifact_ref", "codedb_release_artifacts".to_string()),
                ("runtime_tool_metadata_ref", "share/codedb/runtime-tool-metadata.json".to_string()),
                ("runtime_status", "declared".to_string()),
                (
                    "validation_message",
                    "runtime tools are package inputs; CodeDB remains the authoritative datatable store"
                        .to_string(),
                ),
            ],
        ),
        envctl_row(
            "codedb_runtime_integration",
            "codedb_runtime_integration:checksums:runtime_contract",
            [
                ("repo_path", repo_path.display().to_string()),
                ("runtime_surface", "checksum_manifest".to_string()),
                ("integration_owner", "codedb".to_string()),
                ("source_authority", "codedb_table_checksums".to_string()),
                ("materialization_owner", "envctl_or_yazelix_when_requested".to_string()),
                ("envctl_role", "verify_rows_before_file_materialization".to_string()),
                ("backend_internal_access", "forbidden".to_string()),
                ("checksum_format", "codedb_row_stream_v1".to_string()),
                (
                    "checksum_source_tables",
                    "codedb_table_checksums;codedb_export_manifests;codedb_runtime_integration"
                        .to_string(),
                ),
                ("runtime_status", "declared".to_string()),
                (
                    "validation_message",
                    "checksum rows are the envctl proof surface for runtime materialization"
                        .to_string(),
                ),
            ],
        ),
    ]
}

fn runner_proof_manifest_rows(selection: &RepoSelection) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    let schema = table_rows(schema_rows());
    let table_inventory_rows = table_rows(table_inventory());
    let gaps = table_rows(capture_gaps());
    let validation = table_rows(validation_errors());
    let table_checksums = codedb_table_checksum_rows(repo_path)?;
    let scan = scan_rows(selection)?;
    let no_mutation = prove_no_mutation(repo_path, "codedb_runner_proof_scan", || {
        let _ = scan_filesystem(repo_path);
    })
    .map_err(|source| CliError::Core(Box::new(source)))?;

    let no_mutation_status = match no_mutation.status.as_str() {
        "proven" => "satisfied",
        "mutated" => "failed",
        "degraded" => "degraded",
        _ => "pending",
    };

    let mut rows = vec![
        runner_proof_row(
            "runner_manifest",
            "satisfied",
            "codedb export runner_proof_manifest",
            "runner-readable proof manifest emitted by CodeDB",
            "logs/CDB039-runner.log",
            [
                ("repo_id", selection.repo_id.clone()),
                ("repo_path", repo_path.display().to_string()),
                ("store_path", selection.store_path.clone()),
                ("schema_version", "codedb.runner_proof.v1".to_string()),
            ],
        ),
        runner_proof_row(
            "scan_succeeds",
            "satisfied",
            "codedb scan",
            "scan rows emitted for selected repo",
            "logs/CDB039-runner.log",
            [
                ("row_count", scan.len().to_string()),
                ("repo_id", selection.repo_id.clone()),
                ("repo_path", repo_path.display().to_string()),
                ("source_table_checksum", rows_checksum("scan", &scan)),
            ],
        ),
        runner_proof_row(
            "schema_introspection",
            "satisfied",
            "codedb schema; codedb tables",
            "schema and table inventory rows emitted",
            "logs/CDB039-runner.log",
            [
                ("schema_row_count", schema.len().to_string()),
                ("table_row_count", table_inventory_rows.len().to_string()),
                ("schema_checksum", rows_checksum("schema", &schema)),
                (
                    "table_inventory_checksum",
                    rows_checksum("tables", &table_inventory_rows),
                ),
            ],
        ),
        runner_proof_row(
            "export_checksums_recorded",
            "satisfied",
            "codedb export codedb_table_checksums",
            "table checksum rows emitted for runner provenance",
            "logs/CDB039-runner.log",
            [
                ("checksum_row_count", table_checksums.len().to_string()),
                (
                    "source_table_checksum",
                    rows_checksum("codedb_table_checksums", &table_checksums),
                ),
            ],
        ),
        runner_proof_row(
            "capture_gaps_recorded",
            if gaps.is_empty() {
                "satisfied"
            } else {
                "degraded"
            },
            "codedb gaps",
            "capture gaps are explicit rows",
            "logs/CDB039-runner.log",
            [
                ("gap_row_count", gaps.len().to_string()),
                (
                    "source_table_checksum",
                    rows_checksum("capture_gaps", &gaps),
                ),
            ],
        ),
        runner_proof_row(
            "validation_errors_recorded",
            if validation.is_empty() {
                "satisfied"
            } else {
                "degraded"
            },
            "codedb validation-errors",
            "validation errors are explicit rows",
            "logs/CDB039-runner.log",
            [
                ("validation_error_row_count", validation.len().to_string()),
                (
                    "source_table_checksum",
                    rows_checksum("validation_errors", &validation),
                ),
            ],
        ),
        runner_proof_row(
            "no_mutation_proof",
            no_mutation_status,
            "codedb_core::prove_no_mutation",
            "read-only scan proof records before/after git state",
            "logs/CDB039-runner.log",
            [
                ("operation", no_mutation.operation),
                ("proof_status", no_mutation.status.as_str().to_string()),
                (
                    "pre_existing_dirty",
                    no_mutation.pre_existing_dirty.to_string(),
                ),
                (
                    "mutation_detected",
                    no_mutation.mutation_detected.to_string(),
                ),
                (
                    "degradation_reason",
                    no_mutation.degradation_reason.unwrap_or_default(),
                ),
            ],
        ),
        runner_proof_row(
            "bounded_mcp_status",
            "satisfied",
            "codedb-mcp",
            "MCP defaults are bounded, read-only, and raw-source-disabled by design",
            "logs/CDB032-mcp.log",
            [
                ("mcp_status", "bounded_read_only_mcp_available".to_string()),
                ("default_row_limit", "50".to_string()),
                ("max_row_limit", "200".to_string()),
                ("default_max_bytes", "65536".to_string()),
            ],
        ),
        runner_proof_row(
            "unsafe_capture_default",
            "satisfied",
            "codedb-build-capture",
            "unsafe build capture refuses unless explicitly approved",
            "logs/CDB033-unsafe-gate.log;logs/CDB034-build-capture.log",
            [
                ("default_policy", "refuse_without_unsafe_flag".to_string()),
                ("mcp_dynamic_execution", "blocked".to_string()),
            ],
        ),
        runner_proof_row(
            "redb_backup_restore",
            "satisfied",
            "codedb-store-redb tests",
            "redb backup/restore smoke evidence is recorded by CDB016",
            "logs/CDB016-redb-restore.log",
            [("storage_engine", "redb".to_string())],
        ),
        runner_proof_row(
            "fixture_matrix",
            "pending",
            "future fixture task block",
            "full fixture matrix is not completed by CDB039",
            "",
            [("blocks_release_readiness", "true".to_string())],
        ),
        runner_proof_row(
            "generated_artifact_reproduction",
            "pending",
            "future reproduction mode",
            "generated artifact trees compile only when reproduction mode is enabled",
            "",
            [("blocks_release_readiness", "true".to_string())],
        ),
        {
            let gate = bidirectional_release_gate_summary();
            runner_proof_row(
                "bidirectional_issue_212",
                gate.status,
                "execution/BIDIRECTIONAL_TASK_GRAPH.csv;current-head capability receipts",
                &gate.note,
                "logs/CDB090-release-gate.log",
                [
                    ("task_range", "CDB070-CDB090".to_string()),
                    ("task_count", gate.task_count.to_string()),
                    ("active_task_count", gate.incomplete_task_count.to_string()),
                    ("read_only_defaults", "proven".to_string()),
                    ("hidden_mutation", "forbidden".to_string()),
                ],
            )
        },
    ];
    rows.push(runner_proof_row(
        "release_readiness",
        "pending",
        "runner/fxrun",
        "runner owns final release readiness; pending rows block release",
        "logs/CDB039-runner.log",
        [
            ("release_without_provenance", "forbidden".to_string()),
            ("runner_owner", "true".to_string()),
        ],
    ));
    Ok(rows)
}

struct BidirectionalReleaseGateSummary {
    status: &'static str,
    task_count: usize,
    incomplete_task_count: usize,
    note: String,
}

fn bidirectional_release_gate_summary() -> BidirectionalReleaseGateSummary {
    const TASK_GRAPH: &str = include_str!("../../../execution/BIDIRECTIONAL_TASK_GRAPH.csv");
    let mut task_count = 0;
    let mut incomplete_task_count = 0;

    for line in TASK_GRAPH
        .lines()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
    {
        let mut fields = line.split(',');
        let task_id = fields.next().unwrap_or_default();
        if !matches!(
            task_id
                .strip_prefix("CDB")
                .and_then(|value| value.parse::<u16>().ok()),
            Some(70..=90)
        ) {
            continue;
        }

        task_count += 1;
        let status = line.rsplit(',').next().unwrap_or_default().trim();
        if status != "complete" {
            incomplete_task_count += 1;
        }
    }

    let status = if task_count == 21 && incomplete_task_count == 0 {
        "satisfied"
    } else {
        "pending"
    };
    let note = if status == "satisfied" {
        "all CDB070-CDB090 rows are complete; release still requires current-head capability receipts"
            .to_string()
    } else {
        format!(
            "{incomplete_task_count} of {task_count} CDB070-CDB090 tasks remain incomplete; GAP, planned, or refusal-only evidence cannot satisfy this release gate"
        )
    };

    BidirectionalReleaseGateSummary {
        status,
        task_count,
        incomplete_task_count,
        note,
    }
}

fn runner_proof_row<const N: usize>(
    gate_id: &str,
    status: &str,
    evidence: &str,
    note: &str,
    raw_log_path: &str,
    pairs: [(&str, String); N],
) -> Row {
    let mut row = row([
        ("table", "runner_proof_manifest".to_string()),
        ("gate_id", gate_id.to_string()),
        ("status", status.to_string()),
        ("evidence", evidence.to_string()),
        ("note", note.to_string()),
        ("raw_log_path", raw_log_path.to_string()),
        ("release_without_provenance", "forbidden".to_string()),
    ]);
    for (key, value) in pairs {
        row.insert(key.to_string(), value);
    }
    row
}

fn meta_repo_selection_row(selection: &RepoSelection) -> Row {
    row([
        ("table", "meta_repo_selection".to_string()),
        ("repo_id", selection.repo_id.clone()),
        ("repo_path", selection.repo_path.display().to_string()),
        ("store_path", selection.store_path.clone()),
        ("selection_source", selection.selection_source.clone()),
        ("mutation_policy", "read_only_no_meta_mutation".to_string()),
        (
            "note",
            "meta selects repo/project inputs; CodeDB scans only the explicit repo path"
                .to_string(),
        ),
    ])
}

fn filesystem_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let entries = scan_filesystem(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            row([
                ("table", "filesystem_entries".to_string()),
                ("relative_path", entry.relative_path),
                ("kind", entry.kind.as_str().to_string()),
                ("classification", entry.classification.as_str().to_string()),
                ("size_bytes", entry.size_bytes.to_string()),
                ("readonly", entry.readonly.to_string()),
                ("is_symlink", entry.is_symlink.to_string()),
                ("symlink_target", entry.symlink_target.unwrap_or_default()),
            ])
        })
        .collect())
}

fn rust_item_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let entries = scan_filesystem(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    let mut rows = Vec::new();
    for entry in entries {
        if entry.classification.as_str() != "rust_source" {
            continue;
        }
        let path = repo_path.join(&entry.relative_path);
        let items = capture_rust_items(repo_path, &path, "cli-static")
            .map_err(|source| CliError::Core(Box::new(source)))?;
        rows.extend(items.into_iter().map(|item| {
            row([
                ("table", "rust_items".to_string()),
                ("stable_id", item.stable_id),
                ("relative_path", item.relative_path),
                ("module_path", item.module_path),
                ("item_kind", item.item_kind.as_str().to_string()),
                ("name", item.name),
                ("visibility", item.visibility.as_str().to_string()),
                ("confidence", item.confidence.as_str().to_string()),
            ])
        }));
    }
    Ok(rows)
}

fn cargo_package_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let (_context, metadata) = capture_repo_cargo(repo_path)?;
    Ok(metadata
        .packages
        .into_iter()
        .map(|package| {
            row([
                ("table", "cargo_packages".to_string()),
                ("package_id", package.package_id),
                ("name", package.name),
                ("version", package.version),
                ("edition", package.edition),
                ("manifest_path", package.manifest_path),
                ("source", package.source.unwrap_or_default()),
            ])
        })
        .collect())
}

fn cargo_dependency_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let (_context, metadata) = capture_repo_cargo(repo_path)?;
    Ok(metadata
        .dependencies
        .into_iter()
        .map(|dependency| {
            row([
                ("table", "cargo_dependencies".to_string()),
                ("package_id", dependency.package_id),
                ("name", dependency.name),
                ("req", dependency.req),
                ("kind", dependency.kind.unwrap_or_default()),
                ("target", dependency.target.unwrap_or_default()),
                ("optional", dependency.optional.to_string()),
                (
                    "uses_default_features",
                    dependency.uses_default_features.to_string(),
                ),
                ("features", dependency.features.join(";")),
                ("source", dependency.source.unwrap_or_default()),
            ])
        })
        .collect())
}

fn cargo_source_rows(repo_path: &Path) -> Result<Vec<Row>, CliError> {
    let (_context, metadata) = capture_repo_cargo(repo_path)?;
    Ok(metadata
        .sources
        .into_iter()
        .map(|source| {
            row([
                ("table", "cargo_sources".to_string()),
                ("owner_package_id", source.owner_package_id),
                ("source_name", source.source_name),
                ("kind", source.kind.as_str().to_string()),
                ("source", source.source.unwrap_or_default()),
                ("observed_from", format!("{:?}", source.observed_from)),
            ])
        })
        .collect())
}

fn doctor_rows(args: &[String]) -> Result<Vec<Row>, CliError> {
    let requested_scope = ["--nu", "--yazelix", "--codex", "--meta", "--envctl"]
        .iter()
        .any(|flag| has_flag(args, flag));
    let include_all = !requested_scope;
    let mut rows = Vec::new();

    if include_all || has_flag(args, "--nu") {
        rows.extend(nu_runtime_doctor_rows("host_nu", find_on_path("nu"))?);
    }
    if include_all || has_flag(args, "--yazelix") {
        rows.extend(yazelix_runtime_doctor_rows()?);
    }
    if include_all || has_flag(args, "--codex") {
        rows.extend(tool_doctor_rows(
            "codex",
            find_on_path("codex"),
            "Codex CLI is optional; use codedb CLI/MCP directly if Codex is unavailable",
        )?);
    }
    if include_all || has_flag(args, "--meta") {
        rows.extend(tool_doctor_rows(
            "meta",
            find_on_path("meta"),
            "meta is optional in V1.1; pass explicit repo paths when meta is unavailable",
        )?);
    }
    if include_all || has_flag(args, "--envctl") {
        rows.extend(tool_doctor_rows(
            "envctl",
            find_on_path("envctl"),
            "envctl should consume CodeDB exports; do not read redb internals",
        )?);
    }

    Ok(rows)
}

fn nu_runtime_doctor_rows(component: &str, nu_path: Option<PathBuf>) -> Result<Vec<Row>, CliError> {
    let Some(nu_path) = nu_path else {
        return Ok(vec![doctor_row(
            component,
            "nu_path",
            "degraded",
            "",
            "nu executable not found",
            &format!(
                "install Nushell {NU_PLUGIN_PROTOCOL_VERSION} or pass a runtime-specific registration command"
            ),
        )]);
    };

    let path_value = nu_path.display().to_string();
    let version = command_stdout(&nu_path, &["--version"])?;
    let compatibility_status = if version.trim() == NU_PLUGIN_PROTOCOL_VERSION {
        "available"
    } else {
        "degraded"
    };
    let compatibility_note = if compatibility_status == "available" {
        format!(
            "runtime Nu version matches nu-plugin/nu-protocol handshake {NU_PLUGIN_PROTOCOL_VERSION}"
        )
    } else {
        format!(
            "runtime Nu version differs from nu-plugin/nu-protocol handshake {NU_PLUGIN_PROTOCOL_VERSION}"
        )
    };
    let plugin_path = plugin_binary_path();
    let registration_command = plugin_path
        .as_ref()
        .map(|path| format!("plugin add {}", path.display()))
        .unwrap_or_else(|| "build nu_plugin_codedb before registration".to_string());
    let plugin_status = if plugin_path.as_ref().is_some_and(|path| path.exists()) {
        "available"
    } else {
        "degraded"
    };

    Ok(vec![
        doctor_row(
            component,
            "nu_path",
            "available",
            &path_value,
            "Nu executable discovered without mutating plugin registries",
            "use this path for runtime-specific plugin registration",
        ),
        doctor_row(
            component,
            "nu_version",
            "available",
            version.trim(),
            "Nu version command completed",
            "compare against the plugin protocol version before registration",
        ),
        doctor_row(
            component,
            "plugin_protocol_compatibility",
            compatibility_status,
            &format!(
                "nu-plugin={NU_PLUGIN_PROTOCOL_VERSION};nu-protocol={NU_PLUGIN_PROTOCOL_VERSION};nu-plugin-protocol={NU_PLUGIN_PROTOCOL_VERSION}"
            ),
            &compatibility_note,
            "rebuild the plugin against the target Nu protocol if degraded",
        ),
        doctor_row(
            component,
            "plugin_binary_path",
            plugin_status,
            &plugin_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            "expected sibling nu_plugin_codedb binary next to codedb",
            "run cargo build -p nu_plugin_codedb before registration",
        ),
        doctor_row(
            component,
            "plugin_registration_status",
            "degraded",
            "unknown",
            "doctor does not mutate or inspect user plugin registries in CDB031",
            &registration_command,
        ),
    ])
}

fn yazelix_runtime_doctor_rows() -> Result<Vec<Row>, CliError> {
    let candidates = [
        env::var_os("YAZELIX_NU_BIN").map(PathBuf::from),
        env::var_os("YAZELIX_NU_PATH").map(PathBuf::from),
        env::var_os("YAZELIX_RUNTIME_NU").map(PathBuf::from),
        env::var_os("YZX_NU").map(PathBuf::from),
        env::var_os("YAZELIX_TOOLBIN").map(|path| PathBuf::from(path).join("nu")),
    ];
    let nu_path = candidates.into_iter().flatten().find(|path| path.exists());
    if let Some(nu_path) = nu_path {
        nu_runtime_doctor_rows("yazelix_nu", Some(nu_path))
    } else {
        Ok(vec![doctor_row(
            "yazelix_nu",
            "runtime_nu_path",
            "degraded",
            "",
            "Yazelix runtime Nu was not discoverable from explicit Yazelix environment variables",
            "set YAZELIX_NU_BIN, YAZELIX_NU_PATH, or YAZELIX_RUNTIME_NU for runtime-specific doctor checks",
        )])
    }
}

fn tool_doctor_rows(
    component: &str,
    path: Option<PathBuf>,
    fallback_note: &str,
) -> Result<Vec<Row>, CliError> {
    let Some(path) = path else {
        return Ok(vec![doctor_row(
            component,
            "tool_path",
            "degraded",
            "",
            fallback_note,
            "install the tool or use CodeDB's direct CLI/export surface",
        )]);
    };

    Ok(vec![
        doctor_row(
            component,
            "tool_path",
            "available",
            &path.display().to_string(),
            "tool discovered on PATH",
            "use explicit CodeDB exports for integration boundaries",
        ),
        doctor_row(
            component,
            "integration_boundary",
            "available",
            "codedb exports",
            fallback_note,
            "consume JSON/NUON/CSV output; do not read redb internals",
        ),
    ])
}

fn generate_yazelix_bridge_rows(out_dir: &Path) -> Result<Vec<Row>, CliError> {
    fs::create_dir_all(out_dir).map_err(|source| CliError::Core(Box::new(source)))?;

    let init_path = out_dir.join("codedb_init.nu");
    let extern_path = out_dir.join("codedb_extern.nu");
    let manifest_path = out_dir.join("codedb_bridge_manifest.json");

    let init_content = render_bridge_template(CODEDB_INIT_TEMPLATE);
    let extern_content = render_bridge_template(CODEDB_EXTERN_TEMPLATE);
    fs::write(&init_path, init_content.as_bytes())
        .map_err(|source| CliError::Core(Box::new(source)))?;
    fs::write(&extern_path, extern_content.as_bytes())
        .map_err(|source| CliError::Core(Box::new(source)))?;

    let init_checksum = sha256_hex(init_content.as_bytes());
    let extern_checksum = sha256_hex(extern_content.as_bytes());
    let rows = vec![
        bridge_artifact_row(
            "codedb_init",
            &init_path,
            &init_checksum,
            "generated initializer; no plugin registry mutation",
        ),
        bridge_artifact_row(
            "codedb_extern",
            &extern_path,
            &extern_checksum,
            "generated extern declarations for CLI bridge",
        ),
    ];
    let manifest_checksum = rows_checksum("codedb_yazelix_bridge_manifest", &rows);
    let manifest = serde_json::json!({
        "schema_version": 1,
        "generator": "codedb generate-yazelix-bridge",
        "generated_at": export_timestamp(),
        "artifacts": [
            {
                "artifact": "codedb_init",
                "path": init_path.display().to_string(),
                "sha256": init_checksum,
                "kind": "initializer",
                "mutates_plugin_registry": false,
            },
            {
                "artifact": "codedb_extern",
                "path": extern_path.display().to_string(),
                "sha256": extern_checksum,
                "kind": "extern",
                "mutates_plugin_registry": false,
            },
        ],
        "manifest_sha256": manifest_checksum,
        "source_templates": [
            "templates/nushell/codedb_init.nu",
            "templates/nushell/codedb_extern.nu",
        ],
    });
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)
            .map_err(|source| CliError::Core(Box::new(source)))?,
    )
    .map_err(|source| CliError::Core(Box::new(source)))?;

    let mut out_rows = rows;
    out_rows.push(bridge_artifact_row(
        "codedb_bridge_manifest",
        &manifest_path,
        &sha256_hex(
            fs::read(&manifest_path)
                .map_err(|source| CliError::Core(Box::new(source)))?
                .as_slice(),
        ),
        "generated manifest with artifact checksums",
    ));
    Ok(out_rows)
}

fn render_bridge_template(template: &str) -> String {
    template.replace("@CODEDB_VERSION@", codedb_core::VERSION)
}

fn bridge_artifact_row(artifact: &str, path: &Path, checksum: &str, note: &str) -> Row {
    row([
        ("table", "codedb_yazelix_bridge_artifacts".to_string()),
        ("artifact", artifact.to_string()),
        ("path", path.display().to_string()),
        ("sha256", checksum.to_string()),
        ("generated", "true".to_string()),
        ("manual_edits_allowed", "false".to_string()),
        ("mutates_plugin_registry", "false".to_string()),
        ("source_truth", "templates".to_string()),
        ("note", note.to_string()),
    ])
}

fn plugin_binary_path() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let binary_name = if cfg!(windows) {
        "nu_plugin_codedb.exe"
    } else {
        "nu_plugin_codedb"
    };
    Some(exe.with_file_name(binary_name))
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .map(|directory| directory.join(binary_name))
            .find(|candidate| candidate.is_file())
    })
}

fn command_stdout(command: &Path, args: &[&str]) -> Result<String, CliError> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|source| CliError::Core(Box::new(source)))?;
    if !output.status.success() {
        return Err(CliError::Message(format!(
            "{} {} failed with status {}: {}",
            command.display(),
            args.join(" "),
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn envctl_row<const N: usize>(table: &str, row_id: &str, pairs: [(&str, String); N]) -> Row {
    let mut row = row([
        ("table", table.to_string()),
        ("table_name", table.to_string()),
        ("row_id", row_id.to_string()),
        ("schema_version", "codedb.envctl_export.v1".to_string()),
        ("owner", "codedb".to_string()),
        ("source_role", "canonical".to_string()),
        ("source_path", "".to_string()),
        ("source_format", "derived".to_string()),
        ("source_checksum", "".to_string()),
        ("source_row_ref", "".to_string()),
        ("scope", "repo".to_string()),
        ("precedence", "0".to_string()),
        ("sensitive", "false".to_string()),
        ("secret_ref", "".to_string()),
        ("generated", "false".to_string()),
        ("manual_override", "false".to_string()),
        ("override_reason", "".to_string()),
        ("review_required", "false".to_string()),
        ("validation_status", "ok".to_string()),
        ("validation_message", "".to_string()),
        ("conflict_id", "".to_string()),
        ("deferred_reason", "".to_string()),
    ]);
    for (key, value) in pairs {
        row.insert(key.to_string(), value);
    }
    row
}

fn rows_checksum(table: &str, rows: &[Row]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(table.as_bytes());
    hasher.update(b"\0");
    for row in rows {
        for (key, value) in row {
            hasher.update(key.as_bytes());
            hasher.update(b"\0");
            hasher.update(value.as_bytes());
            hasher.update(b"\0");
        }
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn export_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    format!("unix:{seconds}")
}

fn doctor_row(
    component: &str,
    check: &str,
    status: &str,
    value: &str,
    note: &str,
    action: &str,
) -> Row {
    row([
        ("table", "doctor_checks".to_string()),
        ("component", component.to_string()),
        ("check", check.to_string()),
        ("status", status.to_string()),
        ("value", value.to_string()),
        ("note", note.to_string()),
        ("action", action.to_string()),
    ])
}

fn table_rows(rows: Vec<TableRow>) -> Vec<Row> {
    rows.into_iter()
        .map(|table_row| {
            row([
                ("table", table_row.table.to_string()),
                ("status", table_row.status.to_string()),
                ("rows", table_row.rows.to_string()),
                ("note", table_row.note.to_string()),
            ])
        })
        .collect()
}

fn summary_row(table: &str, status: &str, rows: usize, note: &str) -> Row {
    row([
        ("table", table.to_string()),
        ("status", status.to_string()),
        ("rows", rows.to_string()),
        ("note", note.to_string()),
    ])
}

fn row<const N: usize>(pairs: [(&str, String); N]) -> Row {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn print_rows(rows: Vec<Row>, format: OutputFormat) -> Result<(), CliError> {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&rows)
                    .map_err(|source| CliError::Core(Box::new(source)))?
            );
        }
        OutputFormat::Nuon => print_nuon(&rows),
        OutputFormat::Csv => print_csv(&rows),
    }
    Ok(())
}

fn print_nuon(rows: &[Row]) {
    print!("[");
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            print!(", ");
        }
        print!("{{");
        for (field_index, (key, value)) in row.iter().enumerate() {
            if field_index > 0 {
                print!(", ");
            }
            print!("{key}: {}", nuon_string(value));
        }
        print!("}}");
    }
    println!("]");
}

fn print_csv(rows: &[Row]) {
    let headers = csv_headers(rows);
    println!(
        "{}",
        headers
            .iter()
            .map(|value| csv_cell(value))
            .collect::<Vec<_>>()
            .join(",")
    );
    for row in rows {
        println!(
            "{}",
            headers
                .iter()
                .map(|header| csv_cell(row.get(header).map(String::as_str).unwrap_or("")))
                .collect::<Vec<_>>()
                .join(",")
        );
    }
}

fn csv_headers(rows: &[Row]) -> Vec<String> {
    let mut headers = Vec::new();
    for row in rows {
        for key in row.keys() {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
    }
    headers
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn nuon_string(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn parse_format(args: &[String]) -> Result<OutputFormat, CliError> {
    match option_value(args, "--format").unwrap_or("csv") {
        "json" => Ok(OutputFormat::Json),
        "nuon" => Ok(OutputFormat::Nuon),
        "csv" => Ok(OutputFormat::Csv),
        other => Err(CliError::Message(format!(
            "unsupported format: {other}; expected json, nuon, or csv"
        ))),
    }
}

fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.windows(2).find_map(|window| {
        if window[0] == option {
            Some(window[1].as_str())
        } else {
            None
        }
    })
}

fn repo_selection(
    args: &[String],
    positional_index: usize,
    missing_message: &str,
) -> Result<RepoSelection, CliError> {
    let explicit_repo_path =
        option_value(args, "--repo-path").or_else(|| option_value(args, "--repo"));
    let positional_repo_path = args
        .get(positional_index)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str);

    let repo_path = match (explicit_repo_path, positional_repo_path) {
        (Some(explicit), Some(positional)) if explicit != positional => {
            return Err(CliError::Message(format!(
                "conflicting repo selection: positional repo path {positional} differs from explicit repo path {explicit}"
            )));
        }
        (Some(explicit), _) => explicit,
        (None, Some(positional)) => positional,
        (None, None) if positional_index == 2 => ".",
        (None, None) => return Err(CliError::Message(missing_message.to_string())),
    };

    let repo_id = option_value(args, "--repo-id")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| repo_path_id(repo_path));
    let store_path = option_value(args, "--store").unwrap_or("").to_string();
    let selection_source = if option_value(args, "--repo-path").is_some() {
        "explicit_repo_path"
    } else if option_value(args, "--repo").is_some() {
        "explicit_repo"
    } else if positional_repo_path.is_some() {
        "positional_repo_path"
    } else {
        "default_repo_path"
    };
    let canonical_repo_path =
        fs::canonicalize(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;

    Ok(RepoSelection {
        repo_id,
        repo_path: canonical_repo_path,
        store_path,
        selection_source: selection_source.to_string(),
    })
}

fn repo_path_id(repo_path: &str) -> String {
    let normalized = repo_path.trim_end_matches('/');
    Path::new(normalized)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("repo")
        .to_string()
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn positional<'a>(args: &'a [String], index: usize, message: &str) -> Result<&'a str, CliError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| CliError::Message(message.to_string()))
}

#[allow(dead_code)]
fn _repo_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locked_package_version(package_name: &str) -> String {
        let lock: TomlValue = toml::from_str(include_str!("../../../Cargo.lock"))
            .expect("workspace Cargo.lock must be valid TOML");
        let versions = lock
            .get("package")
            .and_then(TomlValue::as_array)
            .expect("Cargo.lock must contain package entries")
            .iter()
            .filter(|package| package.get("name").and_then(TomlValue::as_str) == Some(package_name))
            .filter_map(|package| package.get("version").and_then(TomlValue::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            versions.len(),
            1,
            "expected exactly one locked {package_name} package, found {versions:?}"
        );
        versions[0].to_string()
    }

    #[test]
    fn locked_nu_packages_share_the_plugin_handshake_version() {
        let plugin = locked_package_version("nu-plugin");
        let protocol = locked_package_version("nu-plugin-protocol");
        let nu_protocol = locked_package_version("nu-protocol");

        assert_eq!(plugin, protocol);
        assert_eq!(nu_protocol, protocol);
        assert_eq!(NU_PLUGIN_PROTOCOL_VERSION, protocol);
    }

    #[cfg(unix)]
    #[test]
    fn doctor_protocol_metadata_matches_the_locked_plugin_handshake() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_repo();
        let nu = root.join("nu");
        let protocol = locked_package_version("nu-plugin-protocol");
        fs::write(&nu, format!("#!/bin/sh\nprintf '%s\\n' '{protocol}'\n"))
            .expect("write fake Nu version command");
        fs::set_permissions(&nu, fs::Permissions::from_mode(0o700))
            .expect("make fake Nu executable");

        let rows = nu_runtime_doctor_rows("host_nu", Some(nu)).expect("run Nu doctor checks");
        let compatibility = rows
            .iter()
            .find(|row| {
                row.get("check")
                    .is_some_and(|check| check == "plugin_protocol_compatibility")
            })
            .expect("plugin protocol compatibility row");

        assert_eq!(
            compatibility.get("status").map(String::as_str),
            Some("available")
        );
        assert_eq!(
            compatibility.get("value"),
            Some(&format!(
                "nu-plugin={protocol};nu-protocol={protocol};nu-plugin-protocol={protocol}"
            ))
        );
        assert!(
            compatibility
                .get("note")
                .is_some_and(|note| note.contains(&protocol))
        );

        fs::remove_dir_all(root).expect("remove fake Nu directory");
    }

    #[test]
    fn doctor_missing_nu_guidance_uses_the_locked_plugin_handshake() {
        let protocol = locked_package_version("nu-plugin-protocol");
        let rows = nu_runtime_doctor_rows("host_nu", None).expect("run missing Nu doctor check");
        let remediation = rows[0].get("action").expect("missing Nu remediation");

        assert_eq!(
            remediation,
            &format!("install Nushell {protocol} or pass a runtime-specific registration command")
        );
    }

    #[test]
    fn materialize_postgresql_identity_hides_url_credentials() {
        let sentinel = "CODEDB_CREDENTIAL_SENTINEL";
        let spec = StoreSpec::parse(
            format!("postgresql://codedb:{sentinel}@db.example.test/codedb"),
            None,
        )
        .expect("parse PostgreSQL selector");
        let identity = materialize_store_identity(&spec, &[]);

        assert_eq!(identity, "postgresql:codebase_codedb");
        assert!(!identity.contains(sentinel));
        assert!(!identity.contains('@'));
    }

    #[test]
    fn materialize_postgresql_identity_hides_query_values() {
        let sentinel = "CODEDB_QUERY_SENTINEL";
        let spec = StoreSpec::parse(
            format!(
                "postgresql://codedb@db.example.test/codedb?application_name={sentinel}&sslpassword=hidden"
            ),
            None,
        )
        .expect("parse PostgreSQL selector");
        let identity = materialize_store_identity(&spec, &[]);

        assert_eq!(identity, "postgresql:codebase_codedb");
        assert!(!identity.contains(sentinel));
        assert!(!identity.contains('?'));
    }

    #[test]
    fn materialize_postgresql_identity_hides_percent_encoded_sentinels() {
        let sentinel = "%43%4f%44%45%44%42%5f%53%45%4e%54%49%4e%45%4c";
        let spec = StoreSpec::parse(
            format!("postgresql://codedb@db.example.test/codedb?options={sentinel}"),
            None,
        )
        .expect("parse PostgreSQL selector");
        let identity = materialize_store_identity(&spec, &[]);

        assert_eq!(identity, "postgresql:codebase_codedb");
        assert!(!identity.contains(sentinel));
        assert!(!identity.contains('%'));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn multi_file_materialization_rolls_back_new_files_after_late_failure() {
        let repo = temp_repo();
        let store = repo.with_extension("rollback.redb");
        let output = repo.with_extension("materialized");
        fs::write(repo.join("a.rs"), b"pub fn newly_published() {}\n").expect("write first source");
        fs::write(repo.join("z.rs"), b"pub fn must_not_replace() {}\n")
            .expect("write second source");
        let selection = RepoSelection {
            repo_id: "rollback".to_string(),
            repo_path: repo.clone(),
            store_path: store.display().to_string(),
            selection_source: "test".to_string(),
        };
        capture_rows(
            &selection,
            &CaptureConfig {
                batch_files: 32,
                batch_bytes: 1024 * 1024,
                time_budget: None,
                resume: false,
            },
            &safe_source_policy_args(),
        )
        .expect("capture rollback fixture");
        fs::create_dir_all(&output).expect("create output");
        fs::write(output.join("z.rs"), b"pre-existing").expect("write protected destination");

        let error = materialize_rows(&store.display().to_string(), &output, None, &[])
            .expect_err("late no-replace conflict must fail the whole materialization");

        assert!(error.to_string().contains("no-replace") || error.to_string().contains("exists"));
        assert!(
            !output.join("a.rs").exists(),
            "file published earlier in the failed attempt was not rolled back"
        );
        assert_eq!(
            fs::read(output.join("z.rs")).expect("read protected destination"),
            b"pre-existing"
        );
        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(output);
        let _ = fs::remove_file(store);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn replacement_before_late_failure_is_preserved_and_audited_as_residual() {
        use codedb_core::store::atomic_materialize_file;

        let root = temp_repo();
        let output = root.join("published.rs");
        let published = b"pub fn published_by_attempt() {}\n";
        let replacement = b"pub fn concurrent_replacement() {}\n";
        let sha256 = format!("{:x}", Sha256::digest(published));
        atomic_materialize_file(&output, published, &sha256, None)
            .expect("publish earlier batch entry");
        let rollback =
            take_materialized_file_rollback(&output).expect("retain batch rollback identity");
        fs::remove_file(&output).expect("remove published entry before replacement");
        fs::write(&output, replacement).expect("install deterministic concurrent replacement");

        let error = rollback_materialized_files(vec![rollback])
            .expect_err("late failure rollback must report the replacement conflict");

        assert!(
            error.to_string().contains("conflict/residual audit"),
            "unexpected rollback audit: {error}"
        );
        assert_eq!(
            fs::read(&output).expect("replacement remains"),
            replacement,
            "batch rollback deleted a concurrently replaced path"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mcp_frontdoor_rejects_postgresql_argv_sentinels_without_echoing_them() {
        let root = temp_repo();
        let credential_sentinel = "CODEDB_ARGV_CREDENTIAL_SENTINEL";
        let query_sentinel = "CODEDB_ARGV_QUERY_SENTINEL";
        let percent_sentinel = "%43%4f%44%45%44%42";
        let selector = format!(
            "postgresql://codedb:{credential_sentinel}@db.example.test/codedb?password={query_sentinel}&options={percent_sentinel}"
        );
        let error = run_mcp_frontdoor(&[
            "mcp".to_string(),
            "serve".to_string(),
            "--repo-path".to_string(),
            root.display().to_string(),
            "--store".to_string(),
            selector,
        ])
        .expect_err("PostgreSQL URL selectors must be rejected before launch");
        let diagnostic = error.to_string();

        assert!(diagnostic.contains("use --store pg"));
        assert!(!diagnostic.contains(credential_sentinel));
        assert!(!diagnostic.contains(query_sentinel));
        assert!(!diagnostic.contains(percent_sentinel));
        assert!(!diagnostic.contains("postgresql://codedb:"));

        let uppercase_error = run_mcp_frontdoor(&[
            "mcp".to_string(),
            "serve".to_string(),
            "--repo-path".to_string(),
            root.display().to_string(),
            "--store".to_string(),
            format!(
                "PostgreSQL://codedb:{credential_sentinel}@db.example.test/codedb?password={query_sentinel}"
            ),
        ])
        .expect_err("PostgreSQL URL scheme matching must be case-insensitive");
        let uppercase_diagnostic = uppercase_error.to_string();
        assert!(uppercase_diagnostic.contains("use --store pg"));
        assert!(!uppercase_diagnostic.contains(credential_sentinel));
        assert!(!uppercase_diagnostic.contains(query_sentinel));

        fs::remove_dir_all(root).expect("remove MCP root");
    }

    #[cfg(unix)]
    #[test]
    fn mcp_frontdoor_ignores_arbitrary_server_override_without_executing_marker() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = TEST_ENV_LOCK.lock().expect("lock test environment");
        let root = temp_repo();
        let marker = root.join("override-executed");
        let override_bin = root.join("forbidden-override.sh");
        fs::write(
            &override_bin,
            format!("#!/bin/sh\nprintf executed > '{}'\n", marker.display()),
        )
        .expect("write override marker");
        fs::set_permissions(&override_bin, fs::Permissions::from_mode(0o700))
            .expect("make override executable");
        let _override_guard =
            TestEnvGuard::set("CODEDB_MCP_SERVER_BIN", &override_bin.display().to_string());

        let result = run_mcp_frontdoor(&[
            "mcp".to_string(),
            "serve".to_string(),
            "--repo-path".to_string(),
            root.display().to_string(),
            "--store".to_string(),
            root.join("store.redb").display().to_string(),
        ]);

        assert!(result.is_err(), "missing sibling server must fail closed");
        assert!(!marker.exists(), "arbitrary MCP override executed");

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn mcp_child_environment_is_minimal_and_pg_conn_is_backend_scoped() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = TEST_ENV_LOCK.lock().expect("lock test environment");
        let root = temp_repo();
        let ambient_sentinel = "CODEDB_MCP_AMBIENT_SECRET_SENTINEL";
        let pg_sentinel = "CODEDB_MCP_PG_CONN_SENTINEL";
        let _ambient_guard = TestEnvGuard::set("CODEDB_MCP_ENV_SENTINEL", ambient_sentinel);
        let _pg_guard = TestEnvGuard::set(
            "CODEDB_PG_CONN",
            &format!("postgresql://codedb:{pg_sentinel}@db.example.test/codedb"),
        );

        let write_probe = |name: &str| {
            let output = root.join(format!("{name}.env"));
            let script = root.join(format!("{name}.sh"));
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\nprintf '%s\\n%s\\n' \"${{CODEDB_MCP_ENV_SENTINEL-unset}}\" \"${{CODEDB_PG_CONN-unset}}\" > '{}'\n",
                    output.display()
                ),
            )
            .expect("write environment probe");
            fs::set_permissions(&script, fs::Permissions::from_mode(0o700))
                .expect("make environment probe executable");
            (script, output)
        };

        let (redb_probe, redb_output) = write_probe("redb");
        let mut redb_command = mcp_server_command(&redb_probe, &root, "store.redb", &[]);
        assert!(redb_command.status().expect("run redb probe").success());
        let redb_env = fs::read_to_string(redb_output).expect("read redb child env");
        assert_eq!(redb_env, "unset\nunset\n");

        let (pg_probe, pg_output) = write_probe("pg");
        let mut pg_command = mcp_server_command(&pg_probe, &root, "pg", &[]);
        assert!(pg_command.status().expect("run PostgreSQL probe").success());
        let pg_env = fs::read_to_string(pg_output).expect("read PostgreSQL child env");
        assert_eq!(
            pg_env,
            format!("unset\npostgresql://codedb:{pg_sentinel}@db.example.test/codedb\n")
        );

        let _ = fs::remove_dir_all(root);
    }

    use std::sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP_REPO_ID: AtomicU64 = AtomicU64::new(0);
    static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn temp_repo() -> PathBuf {
        for _ in 0..1024 {
            let suffix = NEXT_TEMP_REPO_ID.fetch_add(1, Ordering::Relaxed);
            let path =
                env::temp_dir().join(format!("codedb-cli-test-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return path,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create unique test repository {}: {error}", path.display()),
            }
        }
        panic!("unable to reserve a unique CodeDB CLI test repository");
    }

    fn test_capture_config() -> CaptureConfig {
        CaptureConfig {
            batch_files: 32,
            batch_bytes: 1024 * 1024,
            time_budget: None,
            resume: false,
        }
    }

    fn safe_source_policy_args() -> Vec<String> {
        vec!["--raw-persistence".to_string(), "safe-source".to_string()]
    }

    fn external_policy_args(path: &Path) -> Vec<String> {
        vec![
            "--raw-persistence-policy".to_string(),
            path.display().to_string(),
        ]
    }

    fn external_policy_document(repository_binding: &str, allow: &str) -> String {
        format!(
            "version=codedb.raw-persistence-policy.v1\n\
             policy_id=operator-reviewed-source\n\
             authority=operator:local-user\n\
             repository_binding={repository_binding}\n\
             allow={allow}\n"
        )
    }

    fn source_policy_row_for<'a>(rows: &'a [Row], relative_path: &str) -> &'a Row {
        rows.iter()
            .find(|row| {
                row.get("table")
                    .is_some_and(|table| table == "source_policy")
                    && row
                        .get("relative_path")
                        .is_some_and(|path| path == relative_path)
            })
            .unwrap_or_else(|| panic!("missing source_policy row for {relative_path}"))
    }

    fn assert_prefixed_sha256(row: &Row, field: &str) {
        let value = row
            .get(field)
            .unwrap_or_else(|| panic!("missing {field} in {row:?}"));
        assert_eq!(value.len(), "sha256:".len() + 64, "{field}: {value}");
        assert!(value.starts_with("sha256:"), "{field}: {value}");
        assert!(
            value["sha256:".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
            "{field}: {value}"
        );
    }

    #[test]
    fn temp_repo_reserves_exclusive_directories_for_parallel_tests() {
        let first = temp_repo();
        let second = temp_repo();

        assert_ne!(first, second);
        fs::write(first.join("only-first"), "sentinel").expect("write first sentinel");
        assert!(!second.join("only-first").exists());

        fs::remove_dir_all(first).expect("remove first temp repo");
        fs::remove_dir_all(second).expect("remove second temp repo");
    }

    #[test]
    fn cli_store_selection_rejects_unknown_uri_schemes_before_opening_a_backend() {
        let error = parse_store_spec("postgre://user:password@localhost/codedb", &[])
            .expect_err("misspelled PostgreSQL scheme must be rejected");

        assert!(error.to_string().contains("unsupported store URI scheme"));
    }

    #[test]
    fn cli_store_selection_rejects_postgresql_argv_credentials_without_echoing_them() {
        let credential_sentinel = "CODEDB_CLI_ARGV_CREDENTIAL_SENTINEL";
        let query_sentinel = "CODEDB_CLI_ARGV_QUERY_SENTINEL";
        let pg_conn_args = vec![
            "--pg-conn".to_string(),
            format!(
                "postgresql://codedb:{credential_sentinel}@db.example.test/codedb?sslpassword={query_sentinel}"
            ),
        ];
        let pg_conn_error = parse_store_spec("pg", &pg_conn_args)
            .expect_err("--pg-conn must be rejected before opening a backend");
        let pg_conn_diagnostic = pg_conn_error.to_string();
        assert!(pg_conn_diagnostic.contains("CODEDB_PG_CONN"));
        assert!(!pg_conn_diagnostic.contains(credential_sentinel));
        assert!(!pg_conn_diagnostic.contains(query_sentinel));

        let url_error = parse_store_spec(
            &format!(
                "postgresql://codedb:{credential_sentinel}@db.example.test/codedb?sslpassword={query_sentinel}"
            ),
            &[],
        )
        .expect_err("PostgreSQL --store URLs must be rejected before opening a backend");
        let url_diagnostic = url_error.to_string();
        assert!(url_diagnostic.contains("use --store pg"));
        assert!(!url_diagnostic.contains(credential_sentinel));
        assert!(!url_diagnostic.contains(query_sentinel));
    }

    #[test]
    fn cli_pg_selector_uses_only_codedb_pg_conn_not_ambient_database_url() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("lock test environment");
        let database_url_sentinel = "CODEDB_DATABASE_URL_SENTINEL";
        let codedb_pg_sentinel = "CODEDB_PG_CONN_SENTINEL";

        {
            let _codedb_pg_guard = TestEnvGuard::remove("CODEDB_PG_CONN");
            let _database_url_guard = TestEnvGuard::set(
                "DATABASE_URL",
                &format!("postgresql://codedb:{database_url_sentinel}@db.example.test/codedb"),
            );
            let error = parse_store_spec("pg", &[])
                .expect_err("ambient DATABASE_URL must not enable PostgreSQL");
            let diagnostic = error.to_string();
            assert!(diagnostic.contains("CODEDB_PG_CONN"));
            assert!(!diagnostic.contains(database_url_sentinel));
        }

        {
            let _codedb_pg_guard = TestEnvGuard::set(
                "CODEDB_PG_CONN",
                &format!("postgresql://codedb:{codedb_pg_sentinel}@db.example.test/codedb"),
            );
            let spec = parse_store_spec("pg", &[])
                .expect("inherited CODEDB_PG_CONN is the sole PostgreSQL positive path");
            assert_eq!(spec.backend(), StoreBackend::PostgreSql);
            assert!(!spec.redacted().contains(codedb_pg_sentinel));
        }
    }

    #[test]
    fn default_capture_persists_metadata_only_even_for_classifier_clear_source() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn safe() {}\n").expect("safe source");
        fs::write(repo.join(".env"), "OPENAI_API_KEY=sk-test-secret\n").expect("secret fixture");
        let store_path = repo.with_extension("default-deny.redb");
        let selection = RepoSelection {
            repo_id: "secret-policy".to_string(),
            repo_path: repo.clone(),
            store_path: store_path.display().to_string(),
            selection_source: "test".to_string(),
        };
        let config = CaptureConfig {
            batch_files: 32,
            batch_bytes: 1024 * 1024,
            time_budget: None,
            resume: false,
        };

        let rows = capture_rows(&selection, &config, &[]).expect("capture rows");
        let store = CaptureBatcher::open(&store_path).expect("open captured store");
        let paths = store.captured_paths().expect("captured paths");

        assert!(
            paths.is_empty(),
            "default capture persisted raw bytes: {paths:?}"
        );
        assert!(!paths.contains(".env"));
        for relative_path in ["src/lib.rs", ".env"] {
            let policy = source_policy_row_for(&rows, relative_path);
            assert_eq!(
                policy.get("raw_blob_persisted").map(String::as_str),
                Some("false")
            );
            assert_eq!(
                policy.get("policy_authority_source").map(String::as_str),
                Some("default-deny")
            );
            assert_eq!(
                policy.get("persistence_disposition").map(String::as_str),
                Some("metadata-only")
            );
            assert_prefixed_sha256(policy, "repository_snapshot_id");
            assert_prefixed_sha256(policy, "policy_digest");
            assert_prefixed_sha256(policy, "policy_binding_digest");
            assert_prefixed_sha256(policy, "exact_source_sha256");
            assert!(!format!("{policy:?}").contains("sk-test-secret"));
        }

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_file(store_path);
    }

    #[test]
    fn explicit_safe_source_policy_persists_source_but_never_secret_bytes() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn safe() {}\n").expect("safe source");
        fs::write(
            repo.join("src/secret.rs"),
            "const CLIENT_SECRET: &str = \"do-not-persist\";\n",
        )
        .expect("secret source fixture");
        let store_path = repo.with_extension("safe-source.redb");
        let selection = RepoSelection {
            repo_id: "safe-source-policy".to_string(),
            repo_path: repo.clone(),
            store_path: store_path.display().to_string(),
            selection_source: "test".to_string(),
        };

        let rows = capture_rows(
            &selection,
            &test_capture_config(),
            &safe_source_policy_args(),
        )
        .expect("safe-source capture");
        let store = CaptureBatcher::open(&store_path).expect("open captured store");
        let paths = store.captured_paths().expect("captured paths");

        assert!(paths.contains("src/lib.rs"));
        assert!(!paths.contains("src/secret.rs"));
        let safe_policy = source_policy_row_for(&rows, "src/lib.rs");
        assert_eq!(
            safe_policy
                .get("policy_authority_source")
                .map(String::as_str),
            Some("built-in-safe-source-classes")
        );
        assert_eq!(
            safe_policy
                .get("persistence_disposition")
                .map(String::as_str),
            Some("persist-raw")
        );
        assert_eq!(
            safe_policy.get("raw_blob_persisted").map(String::as_str),
            Some("true")
        );
        let denied_policy = source_policy_row_for(&rows, "src/secret.rs");
        assert_eq!(
            denied_policy.get("reason").map(String::as_str),
            Some("classifier-secret-detected")
        );
        assert_eq!(
            denied_policy
                .get("persistence_disposition")
                .map(String::as_str),
            Some("metadata-only")
        );
        assert!(!format!("{rows:?}").contains("do-not-persist"));

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_file(store_path);
    }

    #[test]
    fn external_policy_requires_outside_path_and_exact_repository_snapshot_binding() {
        let repo = temp_repo();
        let policy_home = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn reviewed() {}\n").expect("source");
        let probe_store = repo.with_extension("policy-probe.redb");
        let probe_selection = RepoSelection {
            repo_id: "external-policy".to_string(),
            repo_path: repo.clone(),
            store_path: probe_store.display().to_string(),
            selection_source: "test".to_string(),
        };

        let probe_rows =
            capture_rows(&probe_selection, &test_capture_config(), &[]).expect("binding probe");
        let repository_binding = source_policy_row_for(&probe_rows, "src/lib.rs")
            .get("repository_snapshot_id")
            .expect("repository snapshot binding")
            .clone();
        let policy_path = policy_home.join("capture.policy");
        fs::write(
            &policy_path,
            external_policy_document(&repository_binding, "source-code"),
        )
        .expect("write external policy");

        let external_store = repo.with_extension("external-policy.redb");
        let external_selection = RepoSelection {
            store_path: external_store.display().to_string(),
            ..probe_selection.clone()
        };
        let rows = capture_rows(
            &external_selection,
            &test_capture_config(),
            &external_policy_args(&policy_path),
        )
        .expect("external policy capture");
        let store = CaptureBatcher::open(&external_store).expect("open external-policy store");
        assert!(
            store
                .captured_paths()
                .expect("captured paths")
                .contains("src/lib.rs")
        );
        let provenance = source_policy_row_for(&rows, "src/lib.rs");
        assert_eq!(
            provenance
                .get("policy_authority_source")
                .map(String::as_str),
            Some("external-operator-policy")
        );
        assert_eq!(
            provenance.get("repository_snapshot_id"),
            Some(&repository_binding)
        );
        assert!(!format!("{rows:?}").contains("pub fn reviewed"));

        let in_repo_policy = repo.join("capture.policy");
        fs::write(
            &in_repo_policy,
            external_policy_document(&repository_binding, "source-code"),
        )
        .expect("write repository-controlled policy");
        let in_repo_error = capture_rows(
            &external_selection,
            &test_capture_config(),
            &external_policy_args(&in_repo_policy),
        )
        .expect_err("repository-controlled policy must fail closed");
        assert!(
            in_repo_error
                .to_string()
                .contains("must be external to the repository")
        );

        fs::write(
            &policy_path,
            external_policy_document(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "source-code",
            ),
        )
        .expect("write binding mismatch policy");
        let mismatch_error = capture_rows(
            &external_selection,
            &test_capture_config(),
            &external_policy_args(&policy_path),
        )
        .expect_err("repository binding mismatch must fail closed");
        assert!(
            mismatch_error
                .to_string()
                .contains("repository binding mismatch")
        );

        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(policy_home);
        let _ = fs::remove_file(probe_store);
        let _ = fs::remove_file(external_store);
    }

    #[test]
    fn capture_policy_cli_construction_is_backend_neutral_and_rejects_secret_bearing_values() {
        let redb_args = vec![
            "capture".to_string(),
            ".".to_string(),
            "--store".to_string(),
            "capture.redb".to_string(),
            "--raw-persistence".to_string(),
            "safe-source".to_string(),
        ];
        let pg_args = vec![
            "capture".to_string(),
            ".".to_string(),
            "--store".to_string(),
            "pg".to_string(),
            "--raw-persistence".to_string(),
            "safe-source".to_string(),
        ];
        assert_eq!(
            capture_policy_selection(&redb_args).expect("redb policy"),
            capture_policy_selection(&pg_args).expect("PostgreSQL policy")
        );

        let sentinel = "RAW_POLICY_SECRET_SENTINEL";
        let error = capture_policy_selection(&[
            "--raw-persistence".to_string(),
            format!("safe-source={sentinel}"),
        ])
        .expect_err("unbounded policy values must be rejected");
        assert!(error.to_string().contains("safe-source"));
        assert!(!error.to_string().contains(sentinel));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_reads_remain_bound_to_scanned_root_after_path_symlink_swap() {
        use std::os::unix::fs::symlink;

        let repo = temp_repo();
        let held_repo = repo.with_extension("held-original");
        let outside = repo.with_extension("outside");
        let store_path = repo.with_extension("capture.redb");
        fs::create_dir_all(repo.join("src")).expect("create repo src");
        fs::create_dir_all(outside.join("src")).expect("create outside src");
        fs::write(repo.join("src/lib.rs"), b"inside payload\n").expect("write inside source");
        fs::write(outside.join("src/lib.rs"), b"outside payload\n").expect("write outside source");

        let selection = RepoSelection {
            repo_id: "contained-capture".to_string(),
            repo_path: repo.clone(),
            store_path: store_path.display().to_string(),
            selection_source: "test".to_string(),
        };
        let config = CaptureConfig {
            batch_files: 32,
            batch_bytes: 1024 * 1024,
            time_budget: None,
            resume: false,
        };

        capture_rows_after_scan(&selection, &config, &safe_source_policy_args(), || {
            fs::rename(&repo, &held_repo).expect("hold scanned repository");
            symlink(&outside, &repo).expect("replace repository path with outside symlink");
        })
        .expect("capture remains bound to scanned root");

        let store = CaptureBatcher::open(&store_path).expect("open captured store");
        let captured = store
            .read_source_file_blob("src/lib.rs")
            .expect("read captured source")
            .expect("captured source exists");
        assert_eq!(captured, b"inside payload\n");
        assert_ne!(captured, b"outside payload\n");

        fs::remove_file(&repo).expect("remove replacement symlink");
        fs::rename(&held_repo, &repo).expect("restore repository path");
        let _ = fs::remove_dir_all(repo);
        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_file(store_path);
    }

    // Test lane: default
    // Defends: codedb export envctl includes checksum-bound materialization rows.
    #[test]
    fn envctl_export_includes_materialization_targets() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").expect("source");
        let selection = RepoSelection {
            repo_id: "test".to_string(),
            repo_path: repo.clone(),
            store_path: repo.join(".codedb/store.redb").display().to_string(),
            selection_source: "test".to_string(),
        };

        let rows = envctl_export_rows(&selection, &[]).expect("envctl rows");
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|table| table == "codedb_materialization_targets")
                && row
                    .get("roundtrip_status")
                    .is_some_and(|status| status == "store_restore_materialize_proven")
        }));
        assert!(rows.iter().any(|row| {
            row.get("backend_internal_access")
                .is_some_and(|access| access == "forbidden")
        }));

        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default (the PostgreSQL selector is parsed but no backend is opened).
    // Defends: envctl export has one backend-neutral observable contract, carries
    // explicit safe backend identity, forbids internal access, and never emits a DSN.
    #[test]
    fn envctl_export_normalizes_redb_and_postgresql_store_contracts_without_dsn_leakage() {
        let _env_lock = TEST_ENV_LOCK.lock().expect("lock test environment");
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").expect("source");

        let redb_selection = RepoSelection {
            repo_id: "test".to_string(),
            repo_path: repo.clone(),
            store_path: repo.join(".codedb/contract.redb").display().to_string(),
            selection_source: "test".to_string(),
        };
        let credential_sentinel = "CODEDB_ENVCTL_DSN_CREDENTIAL_SENTINEL";
        let query_sentinel = "CODEDB_ENVCTL_DSN_QUERY_SENTINEL";
        let postgresql_selection = RepoSelection {
            repo_id: "test".to_string(),
            repo_path: repo.clone(),
            store_path: "pg".to_string(),
            selection_source: "test".to_string(),
        };
        let postgresql_args = vec![
            "--pg-table".to_string(),
            "codedb_envctl_contract".to_string(),
        ];

        let redb_rows = envctl_export_rows(&redb_selection, &[]).expect("redb envctl rows");
        let _pg_conn_guard = TestEnvGuard::set(
            "CODEDB_PG_CONN",
            &format!(
                "postgresql://codedb:{credential_sentinel}@db.example.test/codedb?sslpassword={query_sentinel}"
            ),
        );
        let postgresql_rows =
            envctl_export_rows(&postgresql_selection, &postgresql_args).expect("PostgreSQL rows");

        for row in &redb_rows {
            assert!(!row.contains_key("redb_access"));
            let table = row.get("table").map(String::as_str);
            if matches!(
                table,
                Some(
                    "meta_repo_selection"
                        | "codedb_database_endpoints"
                        | "codedb_materialization_targets"
                )
            ) {
                assert_eq!(row.get("store_backend").map(String::as_str), Some("redb"));
                assert!(
                    row.get("store_identity")
                        .is_some_and(|identity| identity.starts_with("redb:"))
                );
                assert_eq!(
                    row.get("backend_internal_access").map(String::as_str),
                    Some("forbidden")
                );
            }
            if table == Some("codedb_runtime_integration") {
                assert_eq!(
                    row.get("backend_internal_access").map(String::as_str),
                    Some("forbidden")
                );
            }
        }
        for row in &postgresql_rows {
            assert!(!row.contains_key("redb_access"));
            let table = row.get("table").map(String::as_str);
            if matches!(
                table,
                Some(
                    "meta_repo_selection"
                        | "codedb_database_endpoints"
                        | "codedb_materialization_targets"
                )
            ) {
                assert_eq!(
                    row.get("store_backend").map(String::as_str),
                    Some("postgresql")
                );
                assert_eq!(
                    row.get("store_identity").map(String::as_str),
                    Some("postgresql:codedb_envctl_contract")
                );
                assert_eq!(
                    row.get("backend_internal_access").map(String::as_str),
                    Some("forbidden")
                );
            }
            if table == Some("codedb_runtime_integration") {
                assert_eq!(
                    row.get("backend_internal_access").map(String::as_str),
                    Some("forbidden")
                );
            }
            for (key, value) in row {
                assert!(!key.contains(credential_sentinel));
                assert!(!key.contains(query_sentinel));
                assert!(!value.contains(credential_sentinel));
                assert!(!value.contains(query_sentinel));
                assert!(!value.contains("postgresql://"));
            }
        }

        let normalize = |rows: &[Row]| {
            rows.iter()
                .cloned()
                .map(|mut row| {
                    row.remove("store_backend");
                    row.remove("store_identity");
                    row.remove("store_path");
                    row.remove("generation_timestamp");
                    row
                })
                .collect::<Vec<_>>()
        };
        let normalized_redb = normalize(&redb_rows);
        let normalized_postgresql = normalize(&postgresql_rows);
        assert_eq!(normalized_redb, normalized_postgresql);

        let normalized_text = format!("{normalized_redb:?}").to_ascii_lowercase();
        assert!(!normalized_text.contains("redb"));
        assert!(!normalized_text.contains("postgres"));

        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default.
    // Defends: backend identity metadata does not alter the checksummed row
    // stream after the export manifest binds to it.
    #[test]
    fn envctl_export_manifest_checksum_matches_observable_checksum_rows() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").expect("source");
        let selection = RepoSelection {
            repo_id: "test".to_string(),
            repo_path: repo.clone(),
            store_path: repo.join(".codedb/contract.redb").display().to_string(),
            selection_source: "test".to_string(),
        };

        let rows = envctl_export_rows(&selection, &[]).expect("envctl rows");
        let observable_checksum_rows = rows
            .iter()
            .filter(|row| {
                row.get("table")
                    .is_some_and(|table| table == "codedb_table_checksums")
            })
            .cloned()
            .collect::<Vec<_>>();
        let observable_checksum =
            rows_checksum("codedb_table_checksums", &observable_checksum_rows);
        let manifest_checksum = rows
            .iter()
            .find(|row| {
                row.get("table")
                    .is_some_and(|table| table == "codedb_export_manifests")
            })
            .and_then(|row| row.get("source_table_checksum"))
            .expect("envctl export manifest checksum");

        assert_eq!(manifest_checksum, &observable_checksum);

        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default
    // Defends: CDB071 CLI defaults expose no bidirectional mutation/apply commands.
    #[test]
    fn mutating_bidirectional_commands_are_not_cli_defaults() {
        for command in [
            "apply",
            "patch",
            "patch-apply",
            "source-overwrite",
            "git-mutation",
            "sync-bidirectional",
        ] {
            let error = run(vec![command.to_string()]).expect_err("command must be rejected");
            let message = error.to_string();
            assert!(message.contains("unsupported command"));
            assert!(!message.contains("source overwrite enabled"));
        }
    }

    #[test]
    fn capture_build_cli_refuses_without_unsafe_flag_without_writing_artifacts() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create fixture source");
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"capture-build-refusal\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("write manifest");
        fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 7 }\n").expect("write source");
        let raw_log = repo.with_extension("evidence").join("capture.log");
        let store = repo.with_extension("build.redb");
        let rows = build_capture_rows(&[
            "capture".to_string(),
            "build".to_string(),
            repo.display().to_string(),
            "--raw-log".to_string(),
            raw_log.display().to_string(),
            "--store".to_string(),
            store.display().to_string(),
        ])
        .expect("refusal rows");

        assert!(rows.iter().any(|row| {
            row.get("table").map(String::as_str) == Some("validation_errors")
                && row.get("code").map(String::as_str) == Some("unsafe_execution_refused")
        }));
        assert!(!raw_log.exists());
        assert!(!store.exists());

        let _ = fs::remove_dir_all(repo);
    }

    #[test]
    fn approved_capture_build_cli_requires_complete_named_provenance() {
        let repo = temp_repo();
        fs::create_dir_all(&repo).expect("create fixture repository");
        let error = build_capture_rows(&[
            "capture".to_string(),
            "build".to_string(),
            repo.display().to_string(),
            "--unsafe-execute-build".to_string(),
            "--raw-log".to_string(),
            repo.with_extension("evidence")
                .join("capture.log")
                .display()
                .to_string(),
        ])
        .expect_err("approved execution requires complete provenance");

        assert!(error.to_string().contains("--approver"));
        assert!(error.to_string().contains("--task-id"));
        assert!(error.to_string().contains("--before-state"));
        assert!(error.to_string().contains("--cleanup-plan"));
        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default
    // Defends: CDB090 reports the completed task graph while release provenance remains mandatory.
    #[test]
    fn runner_proof_manifest_satisfies_bidirectional_gate_after_all_tasks_complete() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n").expect("source");
        let selection = RepoSelection {
            repo_id: "test".to_string(),
            repo_path: repo.clone(),
            store_path: repo.join(".codedb/store.redb").display().to_string(),
            selection_source: "test".to_string(),
        };

        let rows = runner_proof_manifest_rows(&selection).expect("runner proof rows");

        assert!(rows.iter().any(|row| {
            row.get("gate_id")
                .is_some_and(|gate_id| gate_id == "bidirectional_issue_212")
                && row
                    .get("status")
                    .is_some_and(|status| status == "satisfied")
                && row
                    .get("release_without_provenance")
                    .is_some_and(|value| value == "forbidden")
                && row.get("task_count").is_some_and(|value| value == "21")
                && row
                    .get("read_only_defaults")
                    .is_some_and(|value| value == "proven")
                && row
                    .get("active_task_count")
                    .is_some_and(|value| value == "0")
        }));

        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default
    // Defends: agent harness export captures fake HOME and repo surfaces with redaction, validation, and a non-mutating materialization plan.
    #[test]
    fn agent_harness_export_captures_full_fixture_surface() {
        let root = temp_repo();
        let home = root.join("fake-home");
        let codex_dir = home.join(".codex");
        let prompts_dir = codex_dir.join("prompts");
        let skills_dir = codex_dir.join("skills").join("reviewer");
        let plugins_dir = codex_dir.join("plugins").join("demo-plugin");
        let stale_plugin_dir = codex_dir.join("plugins").join("stale-plugin").join("0.9.0");
        let conflicting_plugin_dir = codex_dir.join("plugins").join("demo-plugin-shadow");
        let repo = root.join("repo");
        let kb_dir = repo.join(".kb");
        let kb_skills_dir = kb_dir.join("skills").join("repo-reviewer");
        let hooks_dir = repo.join(".codex").join("hooks");
        let generated_nushell_dir = home.join(".local/share/yazelix/initializers/nushell");
        let _env_guard = TestEnvGuard::set("CODEX_AUTH_TOKEN", "github_pat_fixture_secret");
        fs::create_dir_all(&prompts_dir).expect("create prompts");
        fs::create_dir_all(&skills_dir).expect("create skills");
        fs::create_dir_all(&plugins_dir).expect("create plugins");
        fs::create_dir_all(&stale_plugin_dir).expect("create stale plugin dir");
        fs::create_dir_all(&conflicting_plugin_dir).expect("create conflicting plugin dir");
        fs::create_dir_all(&kb_dir).expect("create kb");
        fs::create_dir_all(&kb_skills_dir).expect("create kb skills");
        fs::create_dir_all(&hooks_dir).expect("create hooks");
        fs::create_dir_all(&generated_nushell_dir).expect("create generated nushell dir");

        fs::write(
            codex_dir.join("config.toml"),
            r#"
default_model = "gpt-5-codex"
approval_policy = "never"
OPENAI_API_KEY = "sk-test-secret"

[mcp_servers.primary]
command = "/usr/bin/codedb-mcp"
args = ["serve", "--default-limit", "50"]

[mcp_servers.duplicate]
command = "/usr/bin/codedb-mcp"
args = ["serve", "--default-limit", "50"]

[hooks.post_apply]
command = "/workspace/repo/.codex/hooks/post-apply.sh"

[hooks.pre_tool]
command = "/workspace/repo/.codex/hooks/pre-tool.sh"
enabled = false
"#,
        )
        .expect("write codex config");
        fs::write(
            prompts_dir.join("triage.md"),
            "# triage\nUse bounded scans.\n",
        )
        .expect("write prompt");
        fs::write(
            skills_dir.join("SKILL.md"),
            "# Reviewer\nUse reproducible evidence.\n",
        )
        .expect("write skill");
        fs::write(
            plugins_dir.join("plugin.json"),
            r#"{"name":"demo-plugin","version":"1.2.3","owner":"meta-plugins-codex"}"#,
        )
        .expect("write plugin metadata");
        fs::write(
            conflicting_plugin_dir.join("plugin.json"),
            r#"{"name":"demo-plugin","version":"1.2.3","owner":"other-owner"}"#,
        )
        .expect("write conflicting plugin metadata");
        fs::write(
            stale_plugin_dir.join("plugin.json"),
            r#"{"name":"stale-plugin","version":"1.2.3","owner":"meta-plugins-codex"}"#,
        )
        .expect("write stale plugin metadata");
        fs::write(
            codex_dir.join("auth.json"),
            r#"{"access_token":"github_pat_fixture_secret","account_id":"acct-1"}"#,
        )
        .expect("write auth file");
        fs::write(kb_dir.join("config.toml"), "workspace = \"main\"\n").expect("write kb config");
        fs::write(kb_dir.join("AGENTS.md"), "# KB Agents\nUse git-kb first.\n")
            .expect("write kb agents");
        fs::write(
            kb_skills_dir.join("SKILL.md"),
            "# Repo Reviewer\nUse repo-local harness rules.\n",
        )
        .expect("write kb skill");
        fs::write(
            repo.join("AGENTS.md"),
            "# Repo Agents\nStay read only by default.\n",
        )
        .expect("write repo agents");
        fs::write(
            hooks_dir.join("post-apply.sh"),
            "#!/usr/bin/env bash\necho post-apply\n",
        )
        .expect("write hook");
        fs::write(
            generated_nushell_dir.join("codedb_init.nu"),
            "export-env { $env.CODEDB_YAZELIX_BRIDGE_MODE = \"legacy\" }\n",
        )
        .expect("write stale generated init");

        let selection = RepoSelection {
            repo_id: "fixture".to_string(),
            repo_path: repo.clone(),
            store_path: repo.join(".codedb/store.redb").display().to_string(),
            selection_source: "test".to_string(),
        };

        let rows = agent_harness_export_rows(&selection, &home).expect("harness rows");

        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_manifests")
                && row
                    .get("component_count")
                    .is_some_and(|value| value.parse::<usize>().unwrap_or_default() >= 8)
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_codex_settings")
                && row
                    .get("key")
                    .is_some_and(|value| value == "OPENAI_API_KEY")
                && row
                    .get("value_redacted")
                    .is_some_and(|value| value == "true")
                && row
                    .get("secret_ref")
                    .is_some_and(|value| value.starts_with("sha256:"))
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_prompts")
                && row
                    .get("prompt_name")
                    .is_some_and(|value| value == "triage")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_plugin_skills")
                && row
                    .get("skill_name")
                    .is_some_and(|value| value == "reviewer")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_plugin_skills")
                && row
                    .get("plugin_name")
                    .is_some_and(|value| value == "repo_kb_skills")
                && row
                    .get("skill_name")
                    .is_some_and(|value| value == "repo-reviewer")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_plugins")
                && row
                    .get("plugin_name")
                    .is_some_and(|value| value == "demo-plugin")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_files")
                && row
                    .get("source_class")
                    .is_some_and(|value| value == "codex_auth_file")
                && row
                    .get("owner_boundary")
                    .is_some_and(|value| value == "user_local")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_files")
                && row
                    .get("source_class")
                    .is_some_and(|value| value == "repo_kb_skill")
                && row
                    .get("owner_boundary")
                    .is_some_and(|value| value == "repo_local")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_env")
                && row
                    .get("env_key")
                    .is_some_and(|value| value == "CODEX_AUTH_TOKEN")
                && row
                    .get("env_value")
                    .is_some_and(|value| value == "[redacted]")
                && row
                    .get("value_redacted")
                    .is_some_and(|value| value == "true")
                && row
                    .get("target_class")
                    .is_some_and(|value| value == "private_env")
                && row
                    .get("secret_ref")
                    .is_some_and(|value| value.starts_with("sha256:"))
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_mcp_servers")
                && row.get("server_id").is_some_and(|value| value == "primary")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_hooks")
                && row
                    .get("hook_id")
                    .is_some_and(|value| value == "post_apply")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "duplicate_mcp_command")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "disabled_hook")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "duplicate_plugin_ownership")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "stale_plugin_metadata")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "generated_state_missing")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_validation_errors")
                && row
                    .get("code")
                    .is_some_and(|value| value == "generated_state_stale")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_materialization_plan")
                && row
                    .get("mutation_allowed")
                    .is_some_and(|value| value == "false")
                && row
                    .get("target_class")
                    .is_some_and(|value| value == "user_local")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_export_manifests")
                && row
                    .get("plan_table")
                    .is_some_and(|value| value == "agent_harness_materialization_plan")
        }));
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|value| value == "agent_harness_env")
                && row
                    .get("target_class")
                    .is_some_and(|value| value == "generated_state")
        }));

        let _ = fs::remove_dir_all(root);
    }

    struct TestEnvGuard {
        key: String,
        previous: Option<String>,
    }

    impl TestEnvGuard {
        fn set(key: &str, value: &str) -> Self {
            let previous = env::var(key).ok();
            // SAFETY: this test mutates process env in a scoped guard while focused test runs single-threaded.
            unsafe { env::set_var(key, value) };
            Self {
                key: key.to_string(),
                previous,
            }
        }

        fn remove(key: &str) -> Self {
            let previous = env::var(key).ok();
            // SAFETY: this test mutates process env in a scoped guard while focused test runs single-threaded.
            unsafe { env::remove_var(key) };
            Self {
                key: key.to_string(),
                previous,
            }
        }
    }

    impl Drop for TestEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                // SAFETY: restore the original process env value before leaving the test scope.
                unsafe { env::set_var(&self.key, previous) };
            } else {
                // SAFETY: remove the scoped test env var before leaving the test scope.
                unsafe { env::remove_var(&self.key) };
            }
        }
    }
}

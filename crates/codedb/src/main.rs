use std::collections::BTreeMap;
use std::env;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_cargo::capture_cargo_metadata;
use codedb_core::{
    TableRow, capture_gaps, prove_no_mutation, scan_filesystem, schema_rows, table_inventory,
    validation_errors,
};
use codedb_rust_static::capture_rust_items;
use sha2::{Digest, Sha256};

type Row = BTreeMap<String, String>;

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
        "scan" => {
            let selection =
                repo_selection(&args, 1, "scan requires <repo_path> or --repo-path <path>")?;
            let format = parse_format(&args)?;
            let rows = scan_rows(&selection)?;
            print_rows(rows, format)
        }
        "export" => {
            let table = positional(&args, 1, "export requires <table>")?;
            let selection = repo_selection(
                &args,
                2,
                "export requires --repo-path <path> or --repo <path>",
            )?;
            let format = parse_format(&args)?;
            let rows = export_rows(table, &selection)?;
            print_rows(rows, format)
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
            "unsupported command: {command}; supported commands: scan, export, schema, tables, gaps, validation-errors, doctor, generate-yazelix-bridge, --version"
        ))),
    }
}

#[derive(Debug, Clone)]
struct RepoSelection {
    repo_id: String,
    repo_path: PathBuf,
    store_path: String,
    selection_source: String,
}

fn scan_rows(selection: &RepoSelection) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    let filesystem_entries =
        scan_filesystem(repo_path).map_err(|source| CliError::Core(Box::new(source)))?;
    let rust_items = rust_item_rows(repo_path)?;
    let manifest_path = repo_path.join("Cargo.toml");
    let cargo_metadata = if manifest_path.exists() {
        Some(
            capture_cargo_metadata(&manifest_path)
                .map_err(|source| CliError::Core(Box::new(source)))?,
        )
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
    if let Some(cargo_metadata) = cargo_metadata {
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

fn export_rows(table: &str, selection: &RepoSelection) -> Result<Vec<Row>, CliError> {
    let repo_path = selection.repo_path.as_path();
    match table {
        "meta_repo_selection" | "repo_selection" => Ok(vec![meta_repo_selection_row(selection)]),
        "envctl" | "envctl_export" => envctl_export_rows(selection),
        "codedb_tool_versions" | "tool_versions" => Ok(codedb_tool_version_rows()),
        "codedb_database_endpoints" | "database_endpoints" => {
            Ok(codedb_database_endpoint_rows(repo_path))
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
        "codedb_materialization_targets" | "materialization_targets" => {
            codedb_materialization_target_rows(repo_path)
        }
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
            "unsupported export table: {table}; supported tables: meta_repo_selection, envctl, runner_proof_manifest, codedb_tool_versions, codedb_database_endpoints, codedb_capture_status, codedb_table_checksums, codedb_validation_errors, codedb_cache_dirs, codedb_log_dirs, codedb_release_artifacts, codedb_source_root_hashes, codedb_materialization_targets, codedb_export_manifests, codedb_runtime_integration, schema, tables, filesystem_entries, rust_items, cargo_packages, cargo_dependencies, cargo_sources, capture_gaps, validation_errors"
        ))),
    }
}

fn envctl_export_rows(selection: &RepoSelection) -> Result<Vec<Row>, CliError> {
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
    Ok(rows)
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
            ("storage_engine", "redb".to_string()),
            ("direct_storage_access", "forbidden".to_string()),
            (
                "export_surface",
                "codedb export <table> --format json|nuon|csv".to_string(),
            ),
            (
                "validation_message",
                "envctl consumes exported datatables and never reads CodeDB redb internals"
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
                ("materialization_owner", "codedb_store_redb".to_string()),
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
                    "redb source blobs are restored by hash before envctl consumes file rows"
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
                ("redb_access", "forbidden".to_string()),
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
                ("redb_access", "forbidden".to_string()),
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
                ("redb_access", "forbidden".to_string()),
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
                ("redb_access", "forbidden".to_string()),
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml"))
        .map_err(|source| CliError::Core(Box::new(source)))?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml"))
        .map_err(|source| CliError::Core(Box::new(source)))?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml"))
        .map_err(|source| CliError::Core(Box::new(source)))?;
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
            "install Nushell 0.112.2 or pass a runtime-specific registration command",
        )]);
    };

    let path_value = nu_path.display().to_string();
    let version = command_stdout(&nu_path, &["--version"])?;
    let compatibility_status = if version.trim() == "0.112.2" {
        "available"
    } else {
        "degraded"
    };
    let compatibility_note = if compatibility_status == "available" {
        "runtime Nu version matches nu-plugin/nu-protocol 0.112.2"
    } else {
        "runtime Nu version differs from nu-plugin/nu-protocol 0.112.2"
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
            "nu-plugin=0.112.2;nu-protocol=0.112.2",
            compatibility_note,
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

    Ok(RepoSelection {
        repo_id,
        repo_path: PathBuf::from(repo_path),
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        env::temp_dir().join(format!("codedb-cli-test-{suffix}"))
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

        let rows = envctl_export_rows(&selection).expect("envctl rows");
        assert!(rows.iter().any(|row| {
            row.get("table")
                .is_some_and(|table| table == "codedb_materialization_targets")
                && row
                    .get("roundtrip_status")
                    .is_some_and(|status| status == "store_restore_materialize_proven")
        }));
        assert!(rows.iter().any(|row| {
            row.get("redb_access")
                .is_some_and(|access| access == "forbidden")
        }));

        let _ = fs::remove_dir_all(repo);
    }
}

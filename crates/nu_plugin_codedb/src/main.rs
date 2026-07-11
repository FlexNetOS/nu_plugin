// Test lane: default

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_cargo::{CargoMetadataCapture, capture_cargo_metadata_json};
use codedb_context::{
    CapturedCargoContext, CargoContextRequest, capture_context, detect_host_triple,
};
use codedb_core::{
    FileClassification, FilesystemEntry, TableRow, capture_gaps, scan_filesystem, schema_rows,
    table_inventory, validation_errors,
};
use codedb_rust_static::{
    BuildScriptInventory, MacroInventory, RustItemRow, capture_build_script_static,
    capture_rust_items, capture_rust_macros,
};
use nu_plugin::{
    EngineInterface, EvaluatedCall, MsgPackSerializer, Plugin, PluginCommand, SimplePluginCommand,
    serve_plugin,
};
use nu_protocol::{LabeledError, Signature, Span, SyntaxShape, Type, Value, record};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use toml::Value as TomlValue;

struct CodeDbPlugin;

type Row = Vec<(&'static str, Value)>;

#[derive(Clone)]
struct PluginRecord {
    name: String,
    version: String,
    owner: String,
    source_path: PathBuf,
}

impl Plugin for CodeDbPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![
            Box::new(Scan),
            Box::new(FsEntries),
            Box::new(SourceFiles),
            Box::new(CargoPackages),
            Box::new(CargoDeps),
            Box::new(CargoSources),
            Box::new(RustItems),
            Box::new(RustMacros),
            Box::new(RustCfg),
            Box::new(BuildScripts),
            Box::new(Export),
            Box::new(AgentHarnessImport),
            Box::new(EnvctlInventoryImport),
            Box::new(NixFlakeImport),
            Box::new(Tables),
            Box::new(Gaps),
            Box::new(ValidationErrors),
            Box::new(Schema),
            Box::new(Doctor),
            Box::new(EnvctlDbRoots),
            Box::new(EnvctlDbQuery),
            Box::new(EnvctlDbRefactor),
        ]
    }
}

struct Scan;
struct FsEntries;
struct SourceFiles;
struct CargoPackages;
struct CargoDeps;
struct CargoSources;
struct RustItems;
struct RustMacros;
struct RustCfg;
struct BuildScripts;
struct Export;
struct AgentHarnessImport;
struct EnvctlInventoryImport;
struct NixFlakeImport;
struct Tables;
struct Gaps;
struct ValidationErrors;
struct Schema;
struct Doctor;

fn table_row_to_value(row: TableRow, span: Span) -> Value {
    Value::record(
        record! {
            "table" => Value::string(row.table, span),
            "status" => Value::string(row.status, span),
            "rows" => Value::int(row.rows as i64, span),
            "note" => Value::string(row.note, span),
        },
        span,
    )
}

fn table_rows_to_value(rows: Vec<TableRow>, span: Span) -> Value {
    Value::list(
        rows.into_iter()
            .map(|row| table_row_to_value(row, span))
            .collect(),
        span,
    )
}

fn row_to_value(row: Row, span: Span) -> Value {
    let mut record = record! {};
    for (key, value) in row {
        record.push(key, value);
    }
    Value::record(record, span)
}

fn rows_to_value(rows: Vec<Row>, span: Span) -> Value {
    Value::list(
        rows.into_iter()
            .map(|row| row_to_value(row, span))
            .collect(),
        span,
    )
}

fn string(value: impl Into<String>, span: Span) -> Value {
    Value::string(value.into(), span)
}

fn int(value: impl TryInto<i64>, span: Span) -> Result<Value, LabeledError> {
    let value = value.try_into().map_err(|_| {
        LabeledError::new("integer conversion failed")
            .with_label("value is too large for Nushell int", span)
    })?;
    Ok(Value::int(value, span))
}

fn bool_value(value: bool, span: Span) -> Value {
    Value::bool(value, span)
}

fn list_value(values: Vec<Value>, span: Span) -> Value {
    Value::list(values, span)
}

fn command_signature(name: &str) -> Signature {
    Signature::build(name).input_output_type(Type::Nothing, Type::Table(Vec::new().into()))
}

fn paged_signature(name: &str) -> Signature {
    command_signature(name)
        .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
        .named(
            "repo",
            SyntaxShape::Filepath,
            "Repository path to scan",
            None,
        )
        .named("limit", SyntaxShape::Int, "Maximum rows to return", None)
        .named("cursor", SyntaxShape::Int, "Zero-based row cursor", None)
}

fn repo_from_positional(call: &EvaluatedCall, index: usize) -> Result<PathBuf, LabeledError> {
    let repo: String = call.req(index)?;
    Ok(PathBuf::from(repo))
}

fn repo_from_flag_or_cwd(call: &EvaluatedCall) -> Result<PathBuf, LabeledError> {
    if let Some(repo) = call.get_flag::<String>("repo")? {
        return Ok(PathBuf::from(repo));
    }
    env::current_dir().map_err(|source| {
        LabeledError::new("failed to determine current repository")
            .with_label(source.to_string(), call.head)
    })
}

fn capture_repo_cargo(
    repo_path: &Path,
    span: Span,
) -> Result<(CapturedCargoContext, CargoMetadataCapture), LabeledError> {
    let target_triple = detect_host_triple().map_err(|source| {
        LabeledError::new("rustc host detection failed").with_label(source.to_string(), span)
    })?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(|source| {
        LabeledError::new("locked Cargo context capture failed")
            .with_label(source.to_string(), span)
    })?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json).map_err(|source| {
        LabeledError::new("captured Cargo metadata projection failed")
            .with_label(source.to_string(), span)
    })?;
    Ok((context, metadata))
}

fn page_rows(rows: Vec<Row>, call: &EvaluatedCall) -> Result<Vec<Row>, LabeledError> {
    let cursor = call.get_flag::<i64>("cursor")?.unwrap_or(0);
    let limit = call.get_flag::<i64>("limit")?.unwrap_or(rows.len() as i64);
    if cursor < 0 || limit < 0 {
        return Err(LabeledError::new("invalid pagination")
            .with_label("cursor and limit must be non-negative", call.head));
    }
    Ok(rows
        .into_iter()
        .skip(cursor as usize)
        .take(limit as usize)
        .collect())
}

fn scan_summary_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let filesystem_entries = scan_filesystem(repo_path).map_err(scan_error)?;
    let rust_items = rust_item_rows(repo_path, span)?;
    let manifest_path = repo_path.join("Cargo.toml");

    let mut rows = vec![
        summary_row(
            "filesystem_entries",
            "available",
            filesystem_entries.len(),
            "read-only filesystem scan completed",
            span,
        )?,
        summary_row(
            "rust_items",
            "available",
            rust_items.len(),
            "static syntax item scan completed",
            span,
        )?,
    ];

    if manifest_path.exists() {
        let (_context, cargo_metadata) = capture_repo_cargo(repo_path, span)?;
        rows.push(summary_row(
            "cargo_packages",
            "available",
            cargo_metadata.packages.len(),
            "cargo metadata package rows captured",
            span,
        )?);
        rows.push(summary_row(
            "cargo_dependencies",
            "available",
            cargo_metadata.dependencies.len(),
            "cargo metadata dependency rows captured",
            span,
        )?);
        rows.push(summary_row(
            "cargo_sources",
            "available",
            cargo_metadata.sources.len(),
            "cargo source provenance rows captured",
            span,
        )?);
    } else {
        rows.push(summary_row(
            "cargo_packages",
            "degraded",
            0usize,
            "Cargo.toml not found",
            span,
        )?);
    }

    Ok(rows)
}

fn summary_row(
    table: &'static str,
    status: &'static str,
    rows: usize,
    note: &'static str,
    span: Span,
) -> Result<Row, LabeledError> {
    Ok(vec![
        ("table", string(table, span)),
        ("status", string(status, span)),
        ("rows", int(rows, span)?),
        ("note", string(note, span)),
    ])
}

fn filesystem_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    scan_filesystem(repo_path)
        .map_err(scan_error)?
        .into_iter()
        .map(|entry| filesystem_row(entry, span))
        .collect()
}

fn source_file_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    scan_filesystem(repo_path)
        .map_err(scan_error)?
        .into_iter()
        .filter(|entry| entry.classification == FileClassification::RustSource)
        .map(|entry| filesystem_row(entry, span))
        .collect()
}

fn filesystem_row(entry: FilesystemEntry, span: Span) -> Result<Row, LabeledError> {
    Ok(vec![
        ("table", string("filesystem_entries", span)),
        ("relative_path", string(entry.relative_path, span)),
        ("kind", string(entry.kind.as_str(), span)),
        (
            "classification",
            string(entry.classification.as_str(), span),
        ),
        ("size_bytes", int(entry.size_bytes, span)?),
        ("readonly", bool_value(entry.readonly, span)),
        ("is_symlink", bool_value(entry.is_symlink, span)),
        (
            "symlink_target",
            string(entry.symlink_target.unwrap_or_default(), span),
        ),
    ])
}

fn cargo_package_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let (_context, metadata) = capture_repo_cargo(repo_path, span)?;
    Ok(metadata
        .packages
        .into_iter()
        .map(|package| {
            vec![
                ("table", string("cargo_packages", span)),
                ("package_id", string(package.package_id, span)),
                ("name", string(package.name, span)),
                ("version", string(package.version, span)),
                ("edition", string(package.edition, span)),
                ("manifest_path", string(package.manifest_path, span)),
                ("source", string(package.source.unwrap_or_default(), span)),
            ]
        })
        .collect())
}

fn cargo_dependency_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let (_context, metadata) = capture_repo_cargo(repo_path, span)?;
    Ok(metadata
        .dependencies
        .into_iter()
        .map(|dependency| {
            vec![
                ("table", string("cargo_dependencies", span)),
                ("package_id", string(dependency.package_id, span)),
                ("name", string(dependency.name, span)),
                ("req", string(dependency.req, span)),
                ("kind", string(dependency.kind.unwrap_or_default(), span)),
                (
                    "target",
                    string(dependency.target.unwrap_or_default(), span),
                ),
                ("optional", bool_value(dependency.optional, span)),
                (
                    "uses_default_features",
                    bool_value(dependency.uses_default_features, span),
                ),
                ("features", string(dependency.features.join(";"), span)),
                (
                    "source",
                    string(dependency.source.unwrap_or_default(), span),
                ),
            ]
        })
        .collect())
}

fn cargo_source_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let (_context, metadata) = capture_repo_cargo(repo_path, span)?;
    Ok(metadata
        .sources
        .into_iter()
        .map(|source| {
            vec![
                ("table", string("cargo_sources", span)),
                ("owner_package_id", string(source.owner_package_id, span)),
                ("source_name", string(source.source_name, span)),
                ("kind", string(source.kind.as_str(), span)),
                ("source", string(source.source.unwrap_or_default(), span)),
                (
                    "observed_from",
                    string(format!("{:?}", source.observed_from), span),
                ),
            ]
        })
        .collect())
}

fn rust_item_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let mut rows = Vec::new();
    for path in rust_source_paths(repo_path)? {
        let items = capture_rust_items(repo_path, &path, "nu-plugin-static").map_err(rust_error)?;
        rows.extend(items.into_iter().map(|item| rust_item_row(item, span)));
    }
    Ok(rows)
}

fn rust_item_row(item: RustItemRow, span: Span) -> Row {
    vec![
        ("table", string("rust_items", span)),
        ("stable_id", string(item.stable_id, span)),
        ("context_id", string(item.context_id, span)),
        ("relative_path", string(item.relative_path, span)),
        ("module_path", string(item.module_path, span)),
        ("item_kind", string(item.item_kind.as_str(), span)),
        ("name", string(item.name, span)),
        ("visibility", string(item.visibility.as_str(), span)),
        ("confidence", string(item.confidence.as_str(), span)),
    ]
}

fn rust_macro_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let mut rows = Vec::new();
    for path in rust_source_paths(repo_path)? {
        let inventory =
            capture_rust_macros(repo_path, &path, "nu-plugin-static").map_err(rust_error)?;
        rows.extend(macro_inventory_rows(inventory, span));
    }
    Ok(rows)
}

fn macro_inventory_rows(inventory: MacroInventory, span: Span) -> Vec<Row> {
    let mut rows = Vec::new();
    rows.extend(inventory.definitions.into_iter().map(|definition| {
        vec![
            ("table", string("rust_macros", span)),
            ("row_kind", string("definition", span)),
            ("stable_id", string(definition.stable_id, span)),
            ("context_id", string(definition.context_id, span)),
            ("relative_path", string(definition.relative_path, span)),
            ("module_path", string(definition.module_path, span)),
            ("name", string(definition.name, span)),
            ("matcher_summary", string(definition.matcher_summary, span)),
            (
                "transcriber_summary",
                string(definition.transcriber_summary, span),
            ),
            ("confidence", string(definition.confidence.as_str(), span)),
        ]
    }));
    rows.extend(inventory.invocations.into_iter().map(|invocation| {
        vec![
            ("table", string("rust_macros", span)),
            ("row_kind", string("invocation", span)),
            ("stable_id", string(invocation.stable_id, span)),
            ("context_id", string(invocation.context_id, span)),
            ("relative_path", string(invocation.relative_path, span)),
            ("module_path", string(invocation.module_path, span)),
            ("macro_path", string(invocation.macro_path, span)),
            (
                "invocation_kind",
                string(invocation.invocation_kind.as_str(), span),
            ),
            ("token_summary", string(invocation.token_summary, span)),
            ("confidence", string(invocation.confidence.as_str(), span)),
        ]
    }));
    rows.extend(inventory.gaps.into_iter().map(|gap| {
        vec![
            ("table", string("rust_macros", span)),
            ("row_kind", string("gap", span)),
            ("context_id", string(gap.context_id, span)),
            ("relative_path", string(gap.relative_path, span)),
            ("module_path", string(gap.module_path, span)),
            ("macro_name", string(gap.macro_name, span)),
            ("missing_truth", string(gap.missing_truth.as_str(), span)),
            ("reason", string(gap.reason, span)),
        ]
    }));
    rows
}

fn rust_cfg_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let (context, metadata) = capture_repo_cargo(repo_path, span)?;
    let edition = metadata
        .packages
        .first()
        .map(|package| package.edition.clone())
        .unwrap_or_default();
    let mut declared_features = Vec::new();
    for feature in &metadata.features {
        declared_features.push(format!("{}={}", feature.package_id, feature.feature));
    }
    declared_features.sort();
    declared_features.dedup();
    let mut resolved_features = Vec::new();
    for (package_id, features) in &context.resolved_features {
        for feature in features {
            resolved_features.push(format!("{package_id}={feature}"));
        }
    }
    Ok(vec![
        vec![
            ("table", string("codedb_contexts", span)),
            ("context_id", string(context.context_id.clone(), span)),
            ("cargo_version", string(context.cargo_version, span)),
            ("rustc_version", string(context.rustc_version, span)),
            ("host_triple", string(context.host_triple, span)),
            ("target_triple", string(context.target_triple, span)),
            ("target_cfgs", string(context.target_cfgs.join(";"), span)),
            (
                "requested_features",
                string(context.requested_features.join(";"), span),
            ),
            ("all_features", bool_value(context.all_features, span)),
            (
                "no_default_features",
                bool_value(context.no_default_features, span),
            ),
            ("profile", string(context.profile, span)),
            ("edition", string(edition, span)),
            ("cargo_lock_sha256", string(context.cargo_lock_sha256, span)),
        ],
        vec![
            ("table", string("feature_sets", span)),
            ("context_id", string(context.context_id, span)),
            (
                "requested_features",
                string(context.requested_features.join(";"), span),
            ),
            (
                "declared_features",
                string(declared_features.join(";"), span),
            ),
            (
                "resolved_features",
                string(resolved_features.join(";"), span),
            ),
            (
                "resolved_package_count",
                int(context.resolved_features.len(), span)?,
            ),
        ],
    ])
}

fn build_script_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let mut rows = Vec::new();
    for path in rust_source_paths(repo_path)? {
        if path.file_name().and_then(|name| name.to_str()) != Some("build.rs") {
            continue;
        }
        let inventory = capture_build_script_static(repo_path, &path, "nu-plugin-static")
            .map_err(rust_error)?;
        rows.extend(build_inventory_rows(inventory, span));
    }
    Ok(rows)
}

fn build_inventory_rows(inventory: BuildScriptInventory, span: Span) -> Vec<Row> {
    let mut rows = Vec::new();
    rows.extend(inventory.scripts.into_iter().map(|script| {
        vec![
            ("table", string("build_scripts", span)),
            ("row_kind", string("script", span)),
            ("stable_id", string(script.stable_id, span)),
            ("context_id", string(script.context_id, span)),
            ("relative_path", string(script.relative_path, span)),
            (
                "is_canonical_build_rs",
                bool_value(script.is_canonical_build_rs, span),
            ),
            ("confidence", string(script.confidence.as_str(), span)),
        ]
    }));
    rows.extend(inventory.instructions.into_iter().map(|instruction| {
        vec![
            ("table", string("build_scripts", span)),
            ("row_kind", string("instruction", span)),
            ("stable_id", string(instruction.stable_id, span)),
            ("context_id", string(instruction.context_id, span)),
            ("relative_path", string(instruction.relative_path, span)),
            ("function_name", string(instruction.function_name, span)),
            ("macro_path", string(instruction.macro_path, span)),
            ("directive", string(instruction.directive, span)),
            ("value", string(instruction.value, span)),
            ("raw_instruction", string(instruction.raw_instruction, span)),
            ("confidence", string(instruction.confidence.as_str(), span)),
        ]
    }));
    rows.extend(inventory.gaps.into_iter().map(|gap| {
        vec![
            ("table", string("build_scripts", span)),
            ("row_kind", string("gap", span)),
            ("context_id", string(gap.context_id, span)),
            ("relative_path", string(gap.relative_path, span)),
            ("missing_truth", string(gap.missing_truth.as_str(), span)),
            ("reason", string(gap.reason, span)),
        ]
    }));
    rows
}

fn envctl_inventory_import_rows(
    inventory_path: &Path,
    span: Span,
) -> Result<Vec<Row>, LabeledError> {
    let raw = fs::read_to_string(inventory_path).map_err(|source| {
        LabeledError::new("failed to read inventory artifact")
            .with_label(format!("{}: {source}", inventory_path.display()), span)
    })?;
    let rows: Vec<JsonValue> = serde_json::from_str(&raw).map_err(|source| {
        LabeledError::new("failed to parse inventory artifact")
            .with_label(format!("{}: {source}", inventory_path.display()), span)
    })?;

    rows.iter()
        .enumerate()
        .map(|(index, row)| envctl_inventory_import_row(index, row, span))
        .collect()
}

fn envctl_inventory_import_row(
    index: usize,
    row: &JsonValue,
    span: Span,
) -> Result<Row, LabeledError> {
    let target_id = json_string(row, "target_id");
    let absolute_path = json_string(row, "absolute_path");
    let import_mode = json_string(row, "import_mode");
    let safety_policy = json_string(row, "safety_policy");
    let path = PathBuf::from(&absolute_path);
    let metadata = fs::symlink_metadata(&path).ok();
    let byte_length = metadata
        .as_ref()
        .filter(|metadata| metadata.is_file())
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let observed = format!("unix:{}", unix_timestamp_seconds());
    let (content_hash, blob_ref, import_status, skip_reason, bytes) = if import_mode
        == "content_blob"
        && metadata.as_ref().is_some_and(|metadata| metadata.is_file())
    {
        let bytes = fs::read(&path).map_err(|source| {
            LabeledError::new("failed to hash content_blob inventory target")
                .with_label(format!("{absolute_path}: {source}"), span)
        })?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        (
            hash.clone(),
            format!("sha256:{hash}"),
            "blob_metadata_ready".to_string(),
            String::new(),
            Some(bytes),
        )
    } else if import_mode == "content_blob" {
        (
            String::new(),
            String::new(),
            "metadata_only".to_string(),
            "content_blob target is not a regular file".to_string(),
            None,
        )
    } else {
        (
            String::new(),
            String::new(),
            "metadata_only".to_string(),
            safety_policy.clone(),
            None,
        )
    };
    let structured = bytes
        .as_deref()
        .and_then(|bytes| structured_file_rows(&json_string(row, "parser_hint"), bytes, span));
    let structured_status = if structured.is_some() {
        "structured_rows_ready"
    } else if import_mode == "metadata_only" {
        "metadata_only"
    } else {
        "unstructured_blob"
    };

    Ok(vec![
        ("table", string("envctl_yazelix_file_import", span)),
        (
            "row_id",
            string(
                format!("envctl_yazelix_file_import:{index}:{target_id}"),
                span,
            ),
        ),
        ("target_id", string(target_id, span)),
        ("logical_owner", string(json_string(row, "owner"), span)),
        ("absolute_path", string(absolute_path, span)),
        (
            "normalized_path",
            string(json_string(row, "normalized_logical_path"), span),
        ),
        (
            "source_of_truth_class",
            string(json_string(row, "source_of_truth_class"), span),
        ),
        ("file_kind", string(json_string(row, "file_kind"), span)),
        ("parser_hint", string(json_string(row, "parser_hint"), span)),
        ("content_hash", string(content_hash, span)),
        ("byte_length", int(byte_length, span)?),
        ("blob_ref", string(blob_ref, span)),
        ("import_safety_policy", string(safety_policy, span)),
        (
            "reproduction_policy",
            string(json_string(row, "reproduction_policy"), span),
        ),
        ("import_mode", string(import_mode, span)),
        ("import_status", string(import_status, span)),
        ("skip_reason", string(skip_reason, span)),
        (
            "structured_table",
            string("envctl_yazelix_file_structured_rows", span),
        ),
        ("structured_status", string(structured_status, span)),
        (
            "structured_row_count",
            int(structured.as_ref().map_or(0usize, Vec::len), span)?,
        ),
        (
            "structured_rows",
            list_value(structured.unwrap_or_default(), span),
        ),
        ("last_observed", string(observed, span)),
        ("provenance", string("yazelix_file_target_inventory", span)),
    ])
}

fn unix_timestamp_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn nix_flake_import_rows(
    metadata_path: &Path,
    outputs_path: Option<&Path>,
    span: Span,
) -> Result<Vec<Row>, LabeledError> {
    let metadata_bytes = fs::read(metadata_path).map_err(|source| {
        LabeledError::new("failed to read nix flake metadata artifact")
            .with_label(format!("{}: {source}", metadata_path.display()), span)
    })?;
    let metadata_hash = format!("{:x}", Sha256::digest(&metadata_bytes));
    let metadata: JsonValue = serde_json::from_slice(&metadata_bytes).map_err(|source| {
        LabeledError::new("failed to parse nix flake metadata artifact")
            .with_label(format!("{}: {source}", metadata_path.display()), span)
    })?;
    let observed = format!("unix:{}", unix_timestamp_seconds());
    let mut rows = Vec::new();

    rows.push(nix_flake_summary_row(
        &metadata,
        metadata_path,
        &metadata_hash,
        &observed,
        span,
    )?);
    rows.extend(nix_flake_reference_rows(
        &metadata,
        metadata_path,
        &metadata_hash,
        span,
    ));
    rows.extend(nix_flake_lock_rows(
        &metadata,
        metadata_path,
        &metadata_hash,
        span,
    )?);

    if let Some(outputs_path) = outputs_path {
        let output_bytes = fs::read(outputs_path).map_err(|source| {
            LabeledError::new("failed to read nix flake outputs artifact")
                .with_label(format!("{}: {source}", outputs_path.display()), span)
        })?;
        let output_hash = format!("{:x}", Sha256::digest(&output_bytes));
        let outputs: JsonValue = serde_json::from_slice(&output_bytes).map_err(|source| {
            LabeledError::new("failed to parse nix flake outputs artifact")
                .with_label(format!("{}: {source}", outputs_path.display()), span)
        })?;
        rows.extend(nix_flake_output_rows(
            &outputs,
            outputs_path,
            &output_hash,
            span,
        ));
    }

    Ok(rows)
}

fn nix_flake_summary_row(
    metadata: &JsonValue,
    metadata_path: &Path,
    source_hash: &str,
    observed: &str,
    span: Span,
) -> Result<Row, LabeledError> {
    Ok(vec![
        ("table", string("nix_flake_summary", span)),
        (
            "row_id",
            string(
                format!("nix_flake_summary:{}", json_string(metadata, "url")),
                span,
            ),
        ),
        ("schema_version", string("codedb.nix_flake_import.v1", span)),
        (
            "description",
            string(json_string(metadata, "description"), span),
        ),
        (
            "original_url",
            string(json_string(metadata, "originalUrl"), span),
        ),
        (
            "resolved_url",
            string(json_string(metadata, "resolvedUrl"), span),
        ),
        (
            "locked_url",
            string(json_string(metadata, "lockedUrl"), span),
        ),
        ("url", string(json_string(metadata, "url"), span)),
        ("store_path", string(json_string(metadata, "path"), span)),
        ("revision", string(json_string(metadata, "revision"), span)),
        (
            "rev_count",
            int(json_i64(metadata, "revCount").unwrap_or_default(), span)?,
        ),
        (
            "last_modified",
            int(json_i64(metadata, "lastModified").unwrap_or_default(), span)?,
        ),
        (
            "metadata_artifact",
            string(metadata_path.display().to_string(), span),
        ),
        ("metadata_hash", string(source_hash, span)),
        ("import_status", string("flake_metadata_ready", span)),
        ("last_observed", string(observed, span)),
        ("provenance", string("nix flake metadata --json", span)),
    ])
}

fn nix_flake_reference_rows(
    metadata: &JsonValue,
    metadata_path: &Path,
    source_hash: &str,
    span: Span,
) -> Vec<Row> {
    ["original", "resolved", "locked"]
        .into_iter()
        .filter_map(|kind| {
            let value = metadata.get(kind)?;
            Some(vec![
                ("table", string("nix_flake_refs", span)),
                (
                    "row_id",
                    string(
                        format!(
                            "nix_flake_refs:{}:{}",
                            kind,
                            json_string(metadata, &format!("{kind}Url"))
                        ),
                        span,
                    ),
                ),
                ("schema_version", string("codedb.nix_flake_import.v1", span)),
                ("ref_kind", string(kind, span)),
                (
                    "url",
                    string(json_string(metadata, &format!("{kind}Url")), span),
                ),
                ("type", string(json_object_string(value, "type"), span)),
                ("owner", string(json_object_string(value, "owner"), span)),
                ("repo", string(json_object_string(value, "repo"), span)),
                ("rev", string(json_object_string(value, "rev"), span)),
                ("ref", string(json_object_string(value, "ref"), span)),
                (
                    "nar_hash",
                    string(json_object_string(value, "narHash"), span),
                ),
                ("dir", string(json_object_string(value, "dir"), span)),
                (
                    "artifact_path",
                    string(metadata_path.display().to_string(), span),
                ),
                ("artifact_hash", string(source_hash, span)),
                ("provenance", string("nix flake metadata --json", span)),
            ])
        })
        .collect()
}

fn nix_flake_lock_rows(
    metadata: &JsonValue,
    metadata_path: &Path,
    source_hash: &str,
    span: Span,
) -> Result<Vec<Row>, LabeledError> {
    let Some(nodes) = metadata
        .get("locks")
        .and_then(|locks| locks.get("nodes"))
        .and_then(JsonValue::as_object)
    else {
        return Ok(vec![nix_flake_validation_row(
            "nix_flake_lock_nodes",
            "missing_locks_nodes",
            "metadata JSON did not contain locks.nodes",
            metadata_path,
            source_hash,
            span,
        )]);
    };

    let mut rows = Vec::new();
    for (node_name, node) in nodes {
        let locked = node.get("locked").unwrap_or(&JsonValue::Null);
        let original = node.get("original").unwrap_or(&JsonValue::Null);
        rows.push(vec![
            ("table", string("nix_flake_lock_nodes", span)),
            (
                "row_id",
                string(format!("nix_flake_lock_nodes:{node_name}"), span),
            ),
            ("schema_version", string("codedb.nix_flake_import.v1", span)),
            ("node_name", string(node_name, span)),
            (
                "locked_type",
                string(json_object_string(locked, "type"), span),
            ),
            (
                "locked_owner",
                string(json_object_string(locked, "owner"), span),
            ),
            (
                "locked_repo",
                string(json_object_string(locked, "repo"), span),
            ),
            (
                "locked_rev",
                string(json_object_string(locked, "rev"), span),
            ),
            (
                "locked_ref",
                string(json_object_string(locked, "ref"), span),
            ),
            (
                "locked_nar_hash",
                string(json_object_string(locked, "narHash"), span),
            ),
            (
                "locked_last_modified",
                int(
                    json_object_i64(locked, "lastModified").unwrap_or_default(),
                    span,
                )?,
            ),
            (
                "original_type",
                string(json_object_string(original, "type"), span),
            ),
            (
                "original_owner",
                string(json_object_string(original, "owner"), span),
            ),
            (
                "original_repo",
                string(json_object_string(original, "repo"), span),
            ),
            (
                "artifact_path",
                string(metadata_path.display().to_string(), span),
            ),
            ("artifact_hash", string(source_hash, span)),
            ("provenance", string("flake.lock via metadata.locks", span)),
        ]);

        if let Some(inputs) = node.get("inputs").and_then(JsonValue::as_object) {
            for (input_name, target) in inputs {
                let target_path = nix_lock_input_target(target);
                rows.push(vec![
                    ("table", string("nix_flake_lock_edges", span)),
                    (
                        "row_id",
                        string(
                            format!("nix_flake_lock_edges:{node_name}:{input_name}:{target_path}"),
                            span,
                        ),
                    ),
                    ("schema_version", string("codedb.nix_flake_import.v1", span)),
                    ("source_node", string(node_name, span)),
                    ("input_name", string(input_name, span)),
                    ("target_path", string(target_path, span)),
                    (
                        "edge_kind",
                        string(
                            if target.is_array() {
                                "follows_path"
                            } else {
                                "node_ref"
                            },
                            span,
                        ),
                    ),
                    (
                        "artifact_path",
                        string(metadata_path.display().to_string(), span),
                    ),
                    ("artifact_hash", string(source_hash, span)),
                    ("provenance", string("flake.lock inputs", span)),
                ]);
            }
        }
    }
    Ok(rows)
}

fn nix_flake_output_rows(
    outputs: &JsonValue,
    outputs_path: &Path,
    source_hash: &str,
    span: Span,
) -> Vec<Row> {
    let mut rows = Vec::new();
    collect_nix_output_rows(
        Vec::new(),
        outputs,
        outputs_path,
        source_hash,
        span,
        &mut rows,
    );
    if rows.is_empty() {
        rows.push(nix_flake_validation_row(
            "nix_flake_outputs",
            "empty_outputs",
            "outputs artifact did not contain any traversable output values",
            outputs_path,
            source_hash,
            span,
        ));
    }
    rows
}

fn collect_nix_output_rows(
    path: Vec<String>,
    value: &JsonValue,
    outputs_path: &Path,
    source_hash: &str,
    span: Span,
    rows: &mut Vec<Row>,
) {
    if path.len() >= 2 && value.get("type").and_then(JsonValue::as_str).is_some() {
        rows.push(nix_flake_output_row(
            &path,
            value,
            outputs_path,
            source_hash,
            span,
        ));
        return;
    }
    match value {
        JsonValue::Object(map) => {
            for (key, child) in map {
                let mut next = path.clone();
                next.push(key.clone());
                collect_nix_output_rows(next, child, outputs_path, source_hash, span, rows);
            }
        }
        JsonValue::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                let mut next = path.clone();
                next.push(idx.to_string());
                collect_nix_output_rows(next, child, outputs_path, source_hash, span, rows);
            }
        }
        _ if !path.is_empty() => rows.push(nix_flake_output_row(
            &path,
            value,
            outputs_path,
            source_hash,
            span,
        )),
        _ => {}
    }
}

fn nix_flake_output_row(
    path: &[String],
    value: &JsonValue,
    outputs_path: &Path,
    source_hash: &str,
    span: Span,
) -> Row {
    let category = path.first().cloned().unwrap_or_default();
    let system = if matches!(
        category.as_str(),
        "apps" | "checks" | "devShells" | "formatter" | "legacyPackages" | "packages"
    ) {
        path.get(1).cloned().unwrap_or_default()
    } else {
        String::new()
    };
    let output_kind = value
        .get("type")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| json_value_kind(value).to_string());
    vec![
        ("table", string("nix_flake_outputs", span)),
        (
            "row_id",
            string(format!("nix_flake_outputs:{}", path.join(".")), span),
        ),
        ("schema_version", string("codedb.nix_flake_import.v1", span)),
        ("attr_path", string(path.join("."), span)),
        ("category", string(category, span)),
        ("system", string(system, span)),
        (
            "name",
            string(path.last().cloned().unwrap_or_default(), span),
        ),
        ("output_kind", string(output_kind, span)),
        (
            "description",
            string(json_object_string(value, "description"), span),
        ),
        ("value", string(json_value_string(value), span)),
        (
            "artifact_path",
            string(outputs_path.display().to_string(), span),
        ),
        ("artifact_hash", string(source_hash, span)),
        (
            "provenance",
            string("nix flake show --json --all-systems", span),
        ),
    ]
}

fn nix_flake_validation_row(
    table: &'static str,
    code: &'static str,
    message: &'static str,
    path: &Path,
    source_hash: &str,
    span: Span,
) -> Row {
    vec![
        ("table", string("codedb_validation_errors", span)),
        (
            "row_id",
            string(format!("codedb_validation_errors:{table}:{code}"), span),
        ),
        ("source_table", string(table, span)),
        ("code", string(code, span)),
        ("message", string(message, span)),
        ("artifact_path", string(path.display().to_string(), span)),
        ("artifact_hash", string(source_hash, span)),
        ("provenance", string("nix flake import validation", span)),
    ]
}

fn structured_file_rows(parser_hint: &str, bytes: &[u8], span: Span) -> Option<Vec<Value>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match parser_hint {
        "json" | "jsonc" => {
            json_table_rows(text, span).or_else(|| text_table_rows(parser_hint, text, span))
        }
        "toml" => toml_table_rows(text, span).or_else(|| text_table_rows(parser_hint, text, span)),
        "nix" | "kdl" | "nu" | "lua" | "yaml" | "yml" | "markdown" | "desktop" | "service"
        | "shell" | "conf" | "terminal_conf" | "plain_config" => {
            text_table_rows(parser_hint, text, span)
        }
        _ => None,
    }
}

fn json_table_rows(text: &str, span: Span) -> Option<Vec<Value>> {
    let value: JsonValue = serde_json::from_str(&jsonc_to_json(text)).ok()?;
    let mut flattened = Vec::new();
    flatten_json(None, &value, &mut flattened);
    Some(
        flattened
            .into_iter()
            .enumerate()
            .map(|(idx, (key, value))| {
                Value::record(
                    record! {
                        "row_index" => Value::int(idx as i64, span),
                        "row_kind" => string("json_value", span),
                        "format" => string("json", span),
                        "key" => string(key, span),
                        "value" => string(value, span),
                    },
                    span,
                )
            })
            .collect(),
    )
}

fn toml_table_rows(text: &str, span: Span) -> Option<Vec<Value>> {
    let value: TomlValue = toml::from_str(text).ok()?;
    let mut flattened = Vec::new();
    flatten_toml(None, &value, &mut flattened);
    Some(
        flattened
            .into_iter()
            .enumerate()
            .map(|(idx, (key, value))| {
                Value::record(
                    record! {
                        "row_index" => Value::int(idx as i64, span),
                        "row_kind" => string("toml_value", span),
                        "format" => string("toml", span),
                        "key" => string(key, span),
                        "value" => string(value, span),
                    },
                    span,
                )
            })
            .collect(),
    )
}

fn text_table_rows(parser_hint: &str, text: &str, span: Span) -> Option<Vec<Value>> {
    let rows = text
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (row_kind, key, value) = text_line_parts(trimmed);
            Some(Value::record(
                record! {
                    "row_index" => Value::int(idx as i64, span),
                    "row_kind" => string(row_kind, span),
                    "format" => string(parser_hint, span),
                    "key" => string(key, span),
                    "value" => string(value, span),
                },
                span,
            ))
        })
        .collect::<Vec<_>>();
    (!rows.is_empty()).then_some(rows)
}

fn text_line_parts(line: &str) -> (&'static str, String, String) {
    if line.starts_with('#') || line.starts_with("//") || line.starts_with("--") {
        return ("comment", String::new(), line.to_string());
    }
    for delimiter in ["=", ":", " "] {
        if let Some((key, value)) = line.split_once(delimiter) {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return ("entry", key, value.trim().to_string());
            }
        }
    }
    ("line", String::new(), line.to_string())
}

fn flatten_toml(prefix: Option<String>, value: &TomlValue, out: &mut Vec<(String, String)>) {
    match value {
        TomlValue::Table(map) => {
            for (key, child) in map {
                let next = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}.{key}"))
                    .unwrap_or_else(|| key.clone());
                flatten_toml(Some(next), child, out);
            }
        }
        TomlValue::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                let next = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}[{idx}]"))
                    .unwrap_or_else(|| format!("[{idx}]"));
                flatten_toml(Some(next), child, out);
            }
        }
        _ => out.push((
            prefix.unwrap_or_else(|| "value".to_string()),
            toml_value_string(value),
        )),
    }
}

fn toml_value_string(value: &TomlValue) -> String {
    match value {
        TomlValue::String(value) => value.clone(),
        TomlValue::Integer(value) => value.to_string(),
        TomlValue::Float(value) => value.to_string(),
        TomlValue::Boolean(value) => value.to_string(),
        TomlValue::Datetime(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn flatten_json(prefix: Option<String>, value: &JsonValue, out: &mut Vec<(String, String)>) {
    match value {
        JsonValue::Object(map) => {
            for (key, child) in map {
                let next = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}.{key}"))
                    .unwrap_or_else(|| key.clone());
                flatten_json(Some(next), child, out);
            }
        }
        JsonValue::Array(items) => {
            for (idx, child) in items.iter().enumerate() {
                let next = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}[{idx}]"))
                    .unwrap_or_else(|| format!("[{idx}]"));
                flatten_json(Some(next), child, out);
            }
        }
        _ => out.push((
            prefix.unwrap_or_else(|| "value".to_string()),
            json_value_string(value),
        )),
    }
}

fn json_value_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::String(value) => value.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn jsonc_to_json(input: &str) -> String {
    let mut without_comments = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            without_comments.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            without_comments.push(ch);
            continue;
        }
        if ch == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            without_comments.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut previous = '\0';
                    for next in chars.by_ref() {
                        if previous == '*' && next == '/' {
                            break;
                        }
                        previous = next;
                    }
                    continue;
                }
                _ => {}
            }
        }
        without_comments.push(ch);
    }

    remove_json_trailing_commas(&without_comments)
}

fn remove_json_trailing_commas(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            output.push(ch);
            continue;
        }
        if ch == ',' {
            let mut lookahead = chars.clone();
            while matches!(lookahead.peek(), Some(next) if next.is_whitespace()) {
                lookahead.next();
            }
            if matches!(lookahead.peek(), Some('}' | ']')) {
                continue;
            }
        }
        output.push(ch);
    }

    output
}

fn json_string(row: &JsonValue, key: &str) -> String {
    row.get(key)
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn json_i64(row: &JsonValue, key: &str) -> Option<i64> {
    row.get(key).and_then(JsonValue::as_i64)
}

fn json_object_string(row: &JsonValue, key: &str) -> String {
    row.get(key)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            row.get(key)
                .filter(|value| !value.is_null())
                .map(json_value_string)
                .unwrap_or_default()
        })
}

fn json_object_i64(row: &JsonValue, key: &str) -> Option<i64> {
    row.get(key).and_then(JsonValue::as_i64)
}

fn json_value_kind(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn nix_lock_input_target(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Array(parts) => parts
            .iter()
            .map(json_value_string)
            .collect::<Vec<_>>()
            .join("/"),
        _ => json_value_string(value),
    }
}

fn agent_harness_import_rows(
    repo_path: &Path,
    home_path: &Path,
    span: Span,
) -> Result<Vec<Row>, LabeledError> {
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
    let repo_kb_config = repo_path.join(".kb/config.toml");
    let repo_kb_agents = repo_path.join(".kb/AGENTS.md");
    let repo_kb_skills_dir = repo_path.join(".kb/skills");
    let repo_agents = repo_path.join("AGENTS.md");
    let manifest_id = format!(
        "agent_harness:{}",
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
                span,
            )?);
            rows.push(agent_harness_file_row(
                &manifest_id,
                source_id,
                path,
                source_class,
                owner_boundary,
                span,
            )?);
            source_ids.push(source_id.to_string());
        }
    }

    if config_path.exists() {
        let raw = fs::read_to_string(&config_path).map_err(|source| {
            LabeledError::new("failed to read Codex config")
                .with_label(format!("{}: {source}", config_path.display()), span)
        })?;
        let parsed: TomlValue = raw.parse().map_err(|source| {
            LabeledError::new("failed to parse Codex config")
                .with_label(format!("{}: {source}", config_path.display()), span)
        })?;
        let Some(config_table) = parsed.as_table() else {
            return Err(LabeledError::new("Codex config root must be a TOML table")
                .with_label(config_path.display().to_string(), span));
        };
        let config_hash = sha256_hex(raw.as_bytes());

        for (key, value) in config_table {
            if key == "mcp_servers" || key == "hooks" {
                continue;
            }
            if let Some(value_string) = toml_scalar_string(value) {
                let (rendered_value, value_redacted, secret_ref) =
                    redacted_value(key, &value_string);
                rows.push(vec![
                    ("table", string("agent_harness_codex_settings", span)),
                    ("manifest_id", string(manifest_id.clone(), span)),
                    ("key", string(key.clone(), span)),
                    ("value", string(rendered_value, span)),
                    ("value_redacted", string(value_redacted.to_string(), span)),
                    ("secret_ref", string(secret_ref, span)),
                    (
                        "source_path",
                        string(config_path.display().to_string(), span),
                    ),
                    ("owner_boundary", string("user_local", span)),
                    ("source_hash", string(config_hash.clone(), span)),
                ]);
            }
        }

        if let Some(mcp_servers) = config_table
            .get("mcp_servers")
            .and_then(TomlValue::as_table)
        {
            let mut signatures: Vec<(String, String)> = Vec::new();
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
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(toml_scalar_string)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let signature = format!("{command}::{args}");
                signatures.push((server_id.clone(), signature.clone()));
                rows.push(vec![
                    ("table", string("agent_harness_mcp_servers", span)),
                    ("manifest_id", string(manifest_id.clone(), span)),
                    ("server_id", string(server_id.clone(), span)),
                    ("command", string(command, span)),
                    ("args", string(args, span)),
                    ("signature", string(signature, span)),
                    (
                        "source_path",
                        string(config_path.display().to_string(), span),
                    ),
                    ("owner_boundary", string("user_local", span)),
                    ("source_hash", string(config_hash.clone(), span)),
                ]);
            }
            for (server_id, signature) in &signatures {
                let duplicates = signatures
                    .iter()
                    .filter(|(_, candidate)| candidate == signature)
                    .map(|(candidate_id, _)| candidate_id.clone())
                    .collect::<Vec<_>>();
                if duplicates.len() > 1 {
                    validations.push(agent_harness_validation_row(
                        "duplicate_mcp_command",
                        &format!(
                            "duplicate MCP command signature shared by {}",
                            duplicates.join(",")
                        ),
                        &config_path,
                        "user_local",
                        &config_hash,
                        Some(server_id.as_str()),
                        span,
                    ));
                }
            }
        }

        if let Some(hooks) = config_table.get("hooks").and_then(TomlValue::as_table) {
            for (hook_id, hook_value) in hooks {
                let Some(hook_table) = hook_value.as_table() else {
                    continue;
                };
                let enabled = hook_table
                    .get("enabled")
                    .and_then(TomlValue::as_bool)
                    .unwrap_or(true);
                let command = hook_table
                    .get("command")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default()
                    .to_string();
                let hook_path = repo_path
                    .join(".codex/hooks")
                    .join(command.rsplit('/').next().unwrap_or_default());
                rows.push(vec![
                    ("table", string("agent_harness_hooks", span)),
                    ("manifest_id", string(manifest_id.clone(), span)),
                    ("hook_id", string(hook_id.clone(), span)),
                    ("command", string(command.clone(), span)),
                    ("enabled", string(enabled.to_string(), span)),
                    ("hook_path", string(hook_path.display().to_string(), span)),
                    ("exists", string(hook_path.exists().to_string(), span)),
                    (
                        "source_path",
                        string(config_path.display().to_string(), span),
                    ),
                    ("owner_boundary", string("user_local", span)),
                    ("source_hash", string(config_hash.clone(), span)),
                ]);
                if hook_path.exists() {
                    rows.push(agent_harness_source_row(
                        &format!("hook:{hook_id}"),
                        &hook_path,
                        "repo_hook",
                        "repo_local",
                        span,
                    )?);
                    rows.push(agent_harness_file_row(
                        &manifest_id,
                        &format!("hook:{hook_id}"),
                        &hook_path,
                        "repo_hook",
                        "repo_local",
                        span,
                    )?);
                    source_ids.push(format!("hook:{hook_id}"));
                } else {
                    validations.push(agent_harness_validation_row(
                        "missing_hook",
                        &format!("hook {hook_id} target is missing"),
                        &hook_path,
                        "repo_local",
                        "",
                        Some(hook_id.as_str()),
                        span,
                    ));
                }
                if !enabled {
                    validations.push(agent_harness_validation_row(
                        "disabled_hook",
                        &format!("hook {hook_id} is disabled and will not run"),
                        &config_path,
                        "user_local",
                        &config_hash,
                        Some(hook_id.as_str()),
                        span,
                    ));
                }
            }
        }
    }

    for prompt_path in collect_files_recursive(&prompts_dir)? {
        let bytes = fs::read(&prompt_path).map_err(|source| {
            LabeledError::new("failed to read Codex prompt")
                .with_label(format!("{}: {source}", prompt_path.display()), span)
        })?;
        rows.push(agent_harness_source_row(
            &format!("prompt:{}", prompt_path.display()),
            &prompt_path,
            "codex_prompt",
            "user_local",
            span,
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("prompt:{}", prompt_path.display()),
            &prompt_path,
            "codex_prompt",
            "user_local",
            span,
        )?);
        rows.push(vec![
            ("table", string("agent_harness_prompts", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            (
                "prompt_name",
                string(
                    prompt_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    span,
                ),
            ),
            (
                "source_path",
                string(prompt_path.display().to_string(), span),
            ),
            ("owner_boundary", string("user_local", span)),
            ("source_hash", string(sha256_hex(&bytes), span)),
        ]);
        source_ids.push(format!("prompt:{}", prompt_path.display()));
    }

    for skill_path in collect_files_recursive(&skills_dir)? {
        let bytes = fs::read(&skill_path).map_err(|source| {
            LabeledError::new("failed to read Codex skill")
                .with_label(format!("{}: {source}", skill_path.display()), span)
        })?;
        rows.push(agent_harness_source_row(
            &format!("skill:{}", skill_path.display()),
            &skill_path,
            "codex_skill",
            "user_local",
            span,
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("skill:{}", skill_path.display()),
            &skill_path,
            "codex_skill",
            "user_local",
            span,
        )?);
        rows.push(vec![
            ("table", string("agent_harness_plugin_skills", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            (
                "skill_name",
                string(
                    skill_path
                        .parent()
                        .and_then(Path::file_name)
                        .unwrap_or_default()
                        .to_string_lossy(),
                    span,
                ),
            ),
            (
                "source_path",
                string(skill_path.display().to_string(), span),
            ),
            ("owner_boundary", string("user_local", span)),
            ("source_hash", string(sha256_hex(&bytes), span)),
        ]);
        source_ids.push(format!("skill:{}", skill_path.display()));
    }

    for skill_path in collect_files_recursive(&repo_kb_skills_dir)? {
        if skill_path.file_name().and_then(|name| name.to_str()) != Some("SKILL.md") {
            continue;
        }
        let bytes = fs::read(&skill_path).map_err(|source| {
            LabeledError::new("failed to read repo KB skill")
                .with_label(format!("{}: {source}", skill_path.display()), span)
        })?;
        rows.push(agent_harness_source_row(
            &format!("repo_skill:{}", skill_path.display()),
            &skill_path,
            "repo_kb_skill",
            "repo_local",
            span,
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("repo_skill:{}", skill_path.display()),
            &skill_path,
            "repo_kb_skill",
            "repo_local",
            span,
        )?);
        rows.push(vec![
            ("table", string("agent_harness_plugin_skills", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            ("plugin_name", string("repo_kb_skills", span)),
            (
                "skill_name",
                string(
                    skill_path
                        .parent()
                        .and_then(Path::file_name)
                        .unwrap_or_default()
                        .to_string_lossy(),
                    span,
                ),
            ),
            (
                "source_path",
                string(skill_path.display().to_string(), span),
            ),
            ("owner_boundary", string("repo_local", span)),
            ("source_hash", string(sha256_hex(&bytes), span)),
        ]);
        source_ids.push(format!("repo_skill:{}", skill_path.display()));
    }

    let mut plugin_groups: BTreeMap<String, Vec<PluginRecord>> = BTreeMap::new();
    for plugin_path in collect_files_recursive(&plugins_dir)? {
        if plugin_path.file_name().and_then(|name| name.to_str()) != Some("plugin.json") {
            continue;
        }
        let bytes = fs::read(&plugin_path).map_err(|source| {
            LabeledError::new("failed to read plugin metadata")
                .with_label(format!("{}: {source}", plugin_path.display()), span)
        })?;
        let plugin_json: JsonValue = serde_json::from_slice(&bytes).map_err(|source| {
            LabeledError::new("failed to parse plugin metadata")
                .with_label(format!("{}: {source}", plugin_path.display()), span)
        })?;
        rows.push(agent_harness_source_row(
            &format!("plugin:{}", plugin_path.display()),
            &plugin_path,
            "codex_plugin_metadata",
            "user_local",
            span,
        )?);
        rows.push(agent_harness_file_row(
            &manifest_id,
            &format!("plugin:{}", plugin_path.display()),
            &plugin_path,
            "codex_plugin_metadata",
            "user_local",
            span,
        )?);
        rows.push(vec![
            ("table", string("agent_harness_plugins", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            (
                "plugin_name",
                string(json_object_string(&plugin_json, "name"), span),
            ),
            (
                "version",
                string(json_object_string(&plugin_json, "version"), span),
            ),
            (
                "owner",
                string(json_object_string(&plugin_json, "owner"), span),
            ),
            (
                "source_path",
                string(plugin_path.display().to_string(), span),
            ),
            ("owner_boundary", string("user_local", span)),
            ("source_hash", string(sha256_hex(&bytes), span)),
        ]);
        let plugin_name = json_object_string(&plugin_json, "name");
        plugin_groups
            .entry(plugin_name.clone())
            .or_default()
            .push(PluginRecord {
                name: plugin_name,
                version: json_object_string(&plugin_json, "version"),
                owner: json_object_string(&plugin_json, "owner"),
                source_path: plugin_path.clone(),
            });
        source_ids.push(format!("plugin:{}", plugin_path.display()));
    }

    for records in plugin_groups.values() {
        let owner_set = records
            .iter()
            .map(|record| record.owner.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if records.len() > 1 && owner_set.len() > 1 {
            validations.push(agent_harness_validation_row(
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
                "user_local",
                "",
                Some(records[0].name.as_str()),
                span,
            ));
        }
        for record in records {
            if let Some(parent_version) = version_dir_name(&record.source_path) {
                if !record.version.is_empty() && parent_version != record.version {
                    validations.push(agent_harness_validation_row(
                        "stale_plugin_metadata",
                        &format!(
                            "plugin {} metadata version {} does not match cache path {}",
                            record.name, record.version, parent_version
                        ),
                        &record.source_path,
                        "user_local",
                        "",
                        Some(record.name.as_str()),
                        span,
                    ));
                }
            }
        }
    }

    rows.push(vec![
        ("table", string("agent_harness_env", span)),
        ("manifest_id", string(manifest_id.clone(), span)),
        ("key", string("CODEX_HOME", span)),
        ("value", string(codex_dir.display().to_string(), span)),
        ("owner_boundary", string("user_local", span)),
    ]);
    rows.push(vec![
        ("table", string("agent_harness_env", span)),
        ("manifest_id", string(manifest_id.clone(), span)),
        ("key", string("REPO_ROOT", span)),
        ("value", string(repo_path.display().to_string(), span)),
        ("owner_boundary", string("repo_local", span)),
    ]);
    for (env_key, env_value) in secret_env_entries() {
        let (rendered_value, value_redacted, secret_ref) = redacted_value(&env_key, &env_value);
        rows.push(vec![
            ("table", string("agent_harness_env", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            ("key", string(env_key, span)),
            ("value", string(rendered_value, span)),
            ("value_redacted", string(value_redacted.to_string(), span)),
            ("secret_ref", string(secret_ref, span)),
            ("owner_boundary", string("private_env", span)),
        ]);
    }
    if yazelix_nushell_dir.exists() {
        rows.push(vec![
            ("table", string("agent_harness_env", span)),
            ("manifest_id", string(manifest_id.clone(), span)),
            ("key", string("YAZELIX_NUSHELL_INIT_DIR", span)),
            (
                "value",
                string(yazelix_nushell_dir.display().to_string(), span),
            ),
            ("owner_boundary", string("generated_state", span)),
        ]);
        for file_name in ["codedb_init.nu", "codedb_extern.nu"] {
            let bridge_path = yazelix_nushell_dir.join(file_name);
            if bridge_path.is_file() {
                rows.push(agent_harness_source_row(
                    &format!("generated:{file_name}"),
                    &bridge_path,
                    "yazelix_generated_bridge",
                    "generated_state",
                    span,
                )?);
                rows.push(agent_harness_file_row(
                    &manifest_id,
                    &format!("generated:{file_name}"),
                    &bridge_path,
                    "yazelix_generated_bridge",
                    "generated_state",
                    span,
                )?);
                source_ids.push(format!("generated:{file_name}"));
            } else {
                validations.push(agent_harness_validation_row(
                    "generated_state_missing",
                    &format!("expected Yazelix generated bridge file {file_name} is missing"),
                    &bridge_path,
                    "generated_state",
                    "",
                    Some(file_name),
                    span,
                ));
            }
        }
        let init_path = yazelix_nushell_dir.join("codedb_init.nu");
        if init_path.is_file() {
            let raw = fs::read_to_string(&init_path).map_err(|source| {
                LabeledError::new("failed to read generated Yazelix bridge")
                    .with_label(format!("{}: {source}", init_path.display()), span)
            })?;
            if !raw.contains("CODEDB_YAZELIX_BRIDGE_MODE = \"generated-state\"") {
                validations.push(agent_harness_validation_row(
                    "generated_state_stale",
                    "Yazelix generated bridge is missing the generated-state mode marker",
                    &init_path,
                    "generated_state",
                    "",
                    Some("codedb_init.nu"),
                    span,
                ));
            }
        }
    }
    rows.push(vec![
        ("table", string("agent_harness_policy_rows", span)),
        ("manifest_id", string(manifest_id.clone(), span)),
        ("policy_key", string("secret_handling", span)),
        ("policy_value", string("hash_secret_like_values", span)),
    ]);
    rows.push(vec![
        ("table", string("agent_harness_policy_rows", span)),
        ("manifest_id", string(manifest_id.clone(), span)),
        ("policy_key", string("mutation_policy", span)),
        (
            "policy_value",
            string("read_only_import_and_planned_materialization_only", span),
        ),
    ]);

    let mut materialization_rows = Vec::new();
    for harness_row in &rows {
        let Some(table) = harness_row
            .iter()
            .find_map(|(key, value)| (*key == "table").then_some(value))
        else {
            continue;
        };
        let Value::String { val: table, .. } = table else {
            continue;
        };
        if matches!(
            table.as_str(),
            "agent_harness_sources"
                | "agent_harness_prompts"
                | "agent_harness_plugin_skills"
                | "agent_harness_plugins"
                | "agent_harness_hooks"
        ) {
            let target_class = if harness_row.iter().any(|(key, value)| {
                *key == "owner_boundary"
                    && matches!(value, Value::String { val, .. } if val == "user_local")
            }) {
                "user_local"
            } else {
                "repo_local"
            };
            materialization_rows.push(vec![
                ("table", string("agent_harness_materialization_plan", span)),
                ("manifest_id", string(manifest_id.clone(), span)),
                ("source_table", string(table.clone(), span)),
                ("target_class", string(target_class, span)),
                ("mutation_allowed", string("false", span)),
                ("approval_required", string("true", span)),
            ]);
        }
    }
    rows.extend(materialization_rows);
    rows.extend(validations.clone());

    rows.push(vec![
        ("table", string("agent_harness_manifests", span)),
        ("manifest_id", string(manifest_id.clone(), span)),
        ("schema_version", string("codedb.agent_harness.v1", span)),
        ("repo_root", string(repo_path.display().to_string(), span)),
        ("home_root", string(home_path.display().to_string(), span)),
        ("component_count", int(source_ids.len(), span)?),
        ("validation_count", int(validations.len(), span)?),
    ]);
    rows.push(vec![
        ("table", string("agent_harness_export_manifests", span)),
        ("manifest_id", string(manifest_id, span)),
        ("format", string("nushell_rows", span)),
        (
            "plan_table",
            string("agent_harness_materialization_plan", span),
        ),
        (
            "validation_table",
            string("agent_harness_validation_errors", span),
        ),
        (
            "row_checksum",
            string(sha256_hex(rows_debug_bytes(&rows).as_bytes()), span),
        ),
    ]);

    Ok(rows)
}

fn agent_harness_source_row(
    source_id: &str,
    path: &Path,
    source_class: &str,
    owner_boundary: &str,
    span: Span,
) -> Result<Row, LabeledError> {
    let bytes = fs::read(path).map_err(|source| {
        LabeledError::new("failed to read harness source")
            .with_label(format!("{}: {source}", path.display()), span)
    })?;
    Ok(vec![
        ("table", string("agent_harness_sources", span)),
        ("source_id", string(source_id, span)),
        ("source_path", string(path.display().to_string(), span)),
        ("source_hash", string(sha256_hex(&bytes), span)),
        ("source_class", string(source_class, span)),
        ("owner_boundary", string(owner_boundary, span)),
    ])
}

fn agent_harness_file_row(
    manifest_id: &str,
    source_id: &str,
    path: &Path,
    source_class: &str,
    owner_boundary: &str,
    span: Span,
) -> Result<Row, LabeledError> {
    let bytes = fs::read(path).map_err(|source| {
        LabeledError::new("failed to read harness file")
            .with_label(format!("{}: {source}", path.display()), span)
    })?;
    Ok(vec![
        ("table", string("agent_harness_files", span)),
        ("manifest_id", string(manifest_id, span)),
        ("source_id", string(source_id, span)),
        ("source_path", string(path.display().to_string(), span)),
        (
            "file_name",
            string(path.file_name().unwrap_or_default().to_string_lossy(), span),
        ),
        ("source_class", string(source_class, span)),
        ("owner_boundary", string(owner_boundary, span)),
        ("byte_len", int(bytes.len(), span)?),
        ("source_hash", string(sha256_hex(&bytes), span)),
    ])
}

fn agent_harness_validation_row(
    code: &str,
    message: &str,
    path: &Path,
    owner_boundary: &str,
    source_hash: &str,
    component_id: Option<&str>,
    span: Span,
) -> Row {
    vec![
        ("table", string("agent_harness_validation_errors", span)),
        ("code", string(code, span)),
        ("message", string(message, span)),
        ("source_path", string(path.display().to_string(), span)),
        ("owner_boundary", string(owner_boundary, span)),
        ("source_hash", string(source_hash, span)),
        (
            "component_id",
            string(component_id.unwrap_or_default(), span),
        ),
    ]
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

fn collect_files_recursive(root: &Path) -> Result<Vec<PathBuf>, LabeledError> {
    let mut paths = Vec::new();
    if !root.exists() {
        return Ok(paths);
    }
    for entry in fs::read_dir(root).map_err(|source| {
        LabeledError::new("failed to walk harness directory")
            .with_label(format!("{}: {source}", root.display()), Span::unknown())
    })? {
        let entry = entry.map_err(|source| {
            LabeledError::new("failed to read harness directory entry")
                .with_label(format!("{}: {source}", root.display()), Span::unknown())
        })?;
        let path = entry.path();
        if path.is_dir() {
            paths.extend(collect_files_recursive(&path)?);
        } else if path.is_file() {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
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
                .join(" "),
        ),
        _ => None,
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
    let lowered = key.to_ascii_lowercase();
    ["token", "secret", "password", "api_key", "apikey", "auth"]
        .iter()
        .any(|needle| lowered.contains(needle))
}

fn secret_like_value(value: &str) -> bool {
    value.starts_with("sk-") || value.starts_with("ghp_") || value.starts_with("github_pat_")
}

fn secret_env_entries() -> Vec<(String, String)> {
    let mut entries = env::vars()
        .filter(|(key, _)| secret_like_key(key))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn rows_debug_bytes(rows: &[Row]) -> String {
    format!("{rows:?}")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn export_rows(table: &str, repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    match table {
        "filesystem_entries" | "fs_entries" => filesystem_rows(repo_path, span),
        "source_files" | "source" => source_file_rows(repo_path, span),
        "cargo_packages" | "packages" => cargo_package_rows(repo_path, span),
        "cargo_dependencies" | "cargo_deps" | "deps" => cargo_dependency_rows(repo_path, span),
        "cargo_sources" => cargo_source_rows(repo_path, span),
        "rust_items" | "items" => rust_item_rows(repo_path, span),
        "rust_macros" | "macros" => rust_macro_rows(repo_path, span),
        "rust_cfg" | "cfg" => rust_cfg_rows(repo_path, span),
        "build_scripts" => build_script_rows(repo_path, span),
        other => Err(LabeledError::new("unsupported export table").with_label(
            format!(
                "{other}; expected filesystem_entries, source_files, cargo_packages, cargo_dependencies, cargo_sources, rust_items, rust_macros, rust_cfg, or build_scripts"
            ),
            span,
        )),
    }
}

fn rust_source_paths(repo_path: &Path) -> Result<Vec<PathBuf>, LabeledError> {
    Ok(scan_filesystem(repo_path)
        .map_err(scan_error)?
        .into_iter()
        .filter(|entry| entry.classification == FileClassification::RustSource)
        .map(|entry| repo_path.join(entry.relative_path))
        .collect())
}

fn scan_error(error: codedb_core::ScanError) -> LabeledError {
    LabeledError::new("filesystem scan failed").with_label(error.to_string(), Span::unknown())
}

fn cargo_error(error: codedb_cargo::CargoMetadataError) -> LabeledError {
    LabeledError::new("cargo metadata capture failed")
        .with_label(error.to_string(), Span::unknown())
}

fn rust_error(error: codedb_rust_static::RustStaticError) -> LabeledError {
    LabeledError::new("static Rust capture failed").with_label(error.to_string(), Span::unknown())
}

macro_rules! static_table_command {
    ($ty:ty, $name:literal, $description:literal, $rows:path) => {
        impl SimplePluginCommand for $ty {
            type Plugin = CodeDbPlugin;

            fn name(&self) -> &str {
                $name
            }

            fn description(&self) -> &str {
                $description
            }

            fn signature(&self) -> Signature {
                command_signature(PluginCommand::name(self)).named(
                    "store",
                    SyntaxShape::Filepath,
                    "CodeDB store path",
                    None,
                )
            }

            fn run(
                &self,
                _plugin: &CodeDbPlugin,
                _engine: &EngineInterface,
                call: &EvaluatedCall,
                _input: &Value,
            ) -> Result<Value, LabeledError> {
                Ok(table_rows_to_value($rows(), call.head))
            }
        }
    };
}

macro_rules! repo_table_command {
    ($ty:ty, $name:literal, $description:literal, $rows:ident, $paged:expr) => {
        impl SimplePluginCommand for $ty {
            type Plugin = CodeDbPlugin;

            fn name(&self) -> &str {
                $name
            }

            fn description(&self) -> &str {
                $description
            }

            fn signature(&self) -> Signature {
                if $paged {
                    paged_signature(PluginCommand::name(self))
                } else {
                    command_signature(PluginCommand::name(self))
                        .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
                        .named(
                            "repo",
                            SyntaxShape::Filepath,
                            "Repository path to scan",
                            None,
                        )
                }
            }

            fn run(
                &self,
                _plugin: &CodeDbPlugin,
                _engine: &EngineInterface,
                call: &EvaluatedCall,
                _input: &Value,
            ) -> Result<Value, LabeledError> {
                let repo_path = repo_from_flag_or_cwd(call)?;
                let rows = $rows(&repo_path, call.head)?;
                let rows = if $paged { page_rows(rows, call)? } else { rows };
                Ok(rows_to_value(rows, call.head))
            }
        }
    };
}

impl SimplePluginCommand for Scan {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb scan"
    }

    fn description(&self) -> &str {
        "Scan a repository and return read-only CodeDB summary rows."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required(
                "repo_path",
                SyntaxShape::Filepath,
                "Repository path to scan",
            )
            .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
            .named("profile", SyntaxShape::String, "Capture profile", None)
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let repo_path = repo_from_positional(call, 0)?;
        Ok(rows_to_value(
            scan_summary_rows(&repo_path, call.head)?,
            call.head,
        ))
    }
}

impl SimplePluginCommand for Export {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb export"
    }

    fn description(&self) -> &str {
        "Export a CodeDB table as native Nushell rows."
    }

    fn signature(&self) -> Signature {
        paged_signature(PluginCommand::name(self))
            .required("table", SyntaxShape::String, "Table name to export")
            .named(
                "format",
                SyntaxShape::String,
                "Accepted for CLI parity; plugin output remains native Nushell values",
                None,
            )
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let table: String = call.req(0)?;
        let repo_path = repo_from_flag_or_cwd(call)?;
        Ok(rows_to_value(
            page_rows(export_rows(&table, &repo_path, call.head)?, call)?,
            call.head,
        ))
    }
}

impl SimplePluginCommand for AgentHarnessImport {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb agent harness import"
    }

    fn description(&self) -> &str {
        "Import a fake-HOME or live agent harness into secret-safe CodeDB rows."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required(
                "home_path",
                SyntaxShape::Filepath,
                "Harness HOME root to import",
            )
            .named(
                "repo",
                SyntaxShape::Filepath,
                "Repository path to scan",
                None,
            )
            .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
            .named("limit", SyntaxShape::Int, "Maximum rows to return", None)
            .named("cursor", SyntaxShape::Int, "Zero-based row cursor", None)
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let home_path: String = call.req(0)?;
        let repo_path = repo_from_flag_or_cwd(call)?;
        let rows = agent_harness_import_rows(&repo_path, Path::new(&home_path), call.head)?;
        Ok(rows_to_value(page_rows(rows, call)?, call.head))
    }
}

impl SimplePluginCommand for EnvctlInventoryImport {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb envctl import inventory"
    }

    fn description(&self) -> &str {
        "Convert a Yazelix file target inventory artifact into envctl-visible import rows."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required(
                "inventory_path",
                SyntaxShape::Filepath,
                "Yazelix file target inventory JSON artifact",
            )
            .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
            .named("limit", SyntaxShape::Int, "Maximum rows to return", None)
            .named("cursor", SyntaxShape::Int, "Zero-based row cursor", None)
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let inventory_path: String = call.req(0)?;
        let rows = envctl_inventory_import_rows(Path::new(&inventory_path), call.head)?;
        Ok(rows_to_value(page_rows(rows, call)?, call.head))
    }
}

impl SimplePluginCommand for NixFlakeImport {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb nix flake import"
    }

    fn description(&self) -> &str {
        "Convert Nix flake metadata/show JSON artifacts into CodeDB-style import rows."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required(
                "metadata_path",
                SyntaxShape::Filepath,
                "JSON produced by nix flake metadata --json",
            )
            .named(
                "outputs",
                SyntaxShape::Filepath,
                "Optional JSON produced by nix flake show --json --all-systems",
                None,
            )
            .named("store", SyntaxShape::Filepath, "CodeDB store path", None)
            .named("limit", SyntaxShape::Int, "Maximum rows to return", None)
            .named("cursor", SyntaxShape::Int, "Zero-based row cursor", None)
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let metadata_path: String = call.req(0)?;
        let outputs_path = call.get_flag::<String>("outputs")?;
        let rows = nix_flake_import_rows(
            Path::new(&metadata_path),
            outputs_path.as_deref().map(Path::new),
            call.head,
        )?;
        Ok(rows_to_value(page_rows(rows, call)?, call.head))
    }
}

repo_table_command!(
    FsEntries,
    "codedb fs entries",
    "Return read-only filesystem entry rows.",
    filesystem_rows,
    true
);
repo_table_command!(
    SourceFiles,
    "codedb source files",
    "Return Rust source file metadata rows without raw source bytes.",
    source_file_rows,
    true
);
repo_table_command!(
    CargoPackages,
    "codedb cargo packages",
    "Return Cargo package rows.",
    cargo_package_rows,
    false
);
repo_table_command!(
    CargoDeps,
    "codedb cargo deps",
    "Return Cargo dependency rows.",
    cargo_dependency_rows,
    false
);
repo_table_command!(
    CargoSources,
    "codedb cargo sources",
    "Return Cargo source provenance rows.",
    cargo_source_rows,
    false
);
repo_table_command!(
    RustItems,
    "codedb rust items",
    "Return static Rust item rows.",
    rust_item_rows,
    true
);
repo_table_command!(
    RustMacros,
    "codedb rust macros",
    "Return static macro definition, invocation, and gap rows.",
    rust_macro_rows,
    true
);
repo_table_command!(
    RustCfg,
    "codedb rust cfg",
    "Return deterministic static cfg and feature context rows.",
    rust_cfg_rows,
    false
);
repo_table_command!(
    BuildScripts,
    "codedb build scripts",
    "Return static build.rs rows and safe capture gaps.",
    build_script_rows,
    false
);
static_table_command!(
    Tables,
    "codedb tables",
    "Return the current CodeDB table inventory as a Nushell table.",
    table_inventory
);
static_table_command!(
    Gaps,
    "codedb gaps",
    "Return capture gaps for compiler-observable facts not yet captured.",
    capture_gaps
);
static_table_command!(
    ValidationErrors,
    "codedb validation errors",
    "Return CodeDB validation errors.",
    validation_errors
);
static_table_command!(
    Schema,
    "codedb schema",
    "Return CodeDB schema/version rows.",
    schema_rows
);
static_table_command!(
    Doctor,
    "codedb doctor",
    "Return package/runtime status rows without mutating user configuration.",
    codedb_core::doctor_rows
);

// ---------------------------------------------------------------------------
// REQ-061 (GH FlexNetOS/envctl#414): envctl code-graph db surface.
//
// The plugin is a CONTROL / VISUAL surface only. It shells out to the `envctl db`
// CLI — the source of truth — and renders its `--json` output as Nushell-native
// tables. It owns NO db state and NEVER applies a refactor/deploy: apply is
// routed through the envctl boundary (confirm + approval), which this plugin
// surfaces (the fail-closed plan) but does not perform. The envctl binary is
// resolved from `ENVCTL_BIN` (default `envctl` on PATH).
// ---------------------------------------------------------------------------

fn envctl_bin() -> String {
    env::var("ENVCTL_BIN").unwrap_or_else(|_| "envctl".to_string())
}

/// Build the envctl argv for a db subcommand. Pure, so routing is unit-testable.
/// Always requests `--json`; NEVER includes apply/confirm — the plugin is
/// read/plan-only and cannot mutate through the boundary.
fn envctl_db_argv(repo: Option<&str>, sub: &[&str]) -> Vec<String> {
    let mut argv = vec!["--json".to_string(), "db".to_string()];
    if let Some(r) = repo {
        argv.push("--repo-root".to_string());
        argv.push(r.to_string());
    }
    argv.extend(sub.iter().map(|s| (*s).to_string()));
    argv
}

/// Run envctl with `argv` and parse stdout as JSON. Errors (spawn failure,
/// nonzero exit, invalid JSON) surface as a `LabeledError` — no plugin state.
fn run_envctl_json(argv: &[String], span: Span) -> Result<JsonValue, LabeledError> {
    let out = std::process::Command::new(envctl_bin())
        .args(argv)
        .output()
        .map_err(|e| {
            LabeledError::new("failed to launch envctl")
                .with_label(format!("{}: {e}", envctl_bin()), span)
        })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(LabeledError::new("envctl db command failed")
            .with_label(stderr.trim().to_string(), span));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| {
        LabeledError::new("envctl returned invalid JSON").with_label(e.to_string(), span)
    })
}

/// Render a JSON array of flat objects as a Nushell table Value. Scalars become
/// typed cells; nested arrays/objects render as compact JSON strings.
fn json_array_to_table(json: &JsonValue, span: Span) -> Value {
    let rows = match json.as_array() {
        Some(a) => a,
        None => return Value::list(Vec::new(), span),
    };
    Value::list(
        rows.iter()
            .map(|r| json_object_to_record(r, span))
            .collect(),
        span,
    )
}

fn json_object_to_record(obj: &JsonValue, span: Span) -> Value {
    let mut rec = nu_protocol::Record::new();
    match obj.as_object() {
        Some(map) => {
            for (k, v) in map {
                rec.push(k.clone(), json_scalar_to_value(v, span));
            }
        }
        None => rec.push("value", json_scalar_to_value(obj, span)),
    }
    Value::record(rec, span)
}

fn json_scalar_to_value(v: &JsonValue, span: Span) -> Value {
    match v {
        JsonValue::Null => Value::nothing(span),
        JsonValue::Bool(b) => Value::bool(*b, span),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i, span)
            } else if let Some(f) = n.as_f64() {
                Value::float(f, span)
            } else {
                Value::string(n.to_string(), span)
            }
        }
        JsonValue::String(s) => Value::string(s.clone(), span),
        JsonValue::Array(_) | JsonValue::Object(_) => Value::string(v.to_string(), span),
    }
}

fn repo_flag_or_cwd_string(call: &EvaluatedCall) -> Result<String, LabeledError> {
    if let Some(repo) = call.get_flag::<String>("repo")? {
        return Ok(repo);
    }
    env::current_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| {
            LabeledError::new("failed to determine repository").with_label(e.to_string(), call.head)
        })
}

struct EnvctlDbRoots;

impl SimplePluginCommand for EnvctlDbRoots {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb envctl-db roots"
    }

    fn description(&self) -> &str {
        "Render the envctl multi-root model (observed META_ROOT + release-target LIFE_OS_ROOT) as a table via the envctl boundary. Read-only; no plugin-owned state."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .named(
                "observed",
                SyntaxShape::String,
                "Observed current root path",
                None,
            )
            .named(
                "release",
                SyntaxShape::String,
                "Release-target root path",
                None,
            )
            .named(
                "profile",
                SyntaxShape::String,
                "Release profile label",
                None,
            )
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let mut argv = envctl_db_argv(None, &["roots"]);
        if let Some(o) = call.get_flag::<String>("observed")? {
            argv.push("--observed".into());
            argv.push(o);
        }
        if let Some(r) = call.get_flag::<String>("release")? {
            argv.push("--release".into());
            argv.push(r);
        }
        if let Some(p) = call.get_flag::<String>("profile")? {
            argv.push("--profile".into());
            argv.push(p);
        }
        let json = run_envctl_json(&argv, call.head)?;
        Ok(json_array_to_table(&json, call.head))
    }
}

struct EnvctlDbQuery;

impl SimplePluginCommand for EnvctlDbQuery {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb envctl-db query"
    }

    fn description(&self) -> &str {
        "Run an envctl agent preset query (root-meta, mutable-unsafe, symbols-rust-cli, …) and render the result rows as a table. Read-only via the envctl boundary."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required("preset", SyntaxShape::String, "Agent preset name")
            .named(
                "repo",
                SyntaxShape::Filepath,
                "Repository root to index",
                None,
            )
            .switch("explain", "Include the resolved table/filters trace", None)
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let preset: String = call.req(0)?;
        let repo = repo_flag_or_cwd_string(call)?;
        let mut argv = envctl_db_argv(Some(&repo), &["query", "--preset", &preset]);
        if call.has_flag("explain")? {
            argv.push("--explain".into());
        }
        let json = run_envctl_json(&argv, call.head)?;
        // QueryResult is `{ rows, row_count, explain }`; render the rows table.
        let rows = json
            .get("rows")
            .cloned()
            .unwrap_or(JsonValue::Array(vec![]));
        Ok(json_array_to_table(&rows, call.head))
    }
}

struct EnvctlDbRefactor;

impl SimplePluginCommand for EnvctlDbRefactor {
    type Plugin = CodeDbPlugin;

    fn name(&self) -> &str {
        "codedb envctl-db refactor"
    }

    fn description(&self) -> &str {
        "Render the fail-closed envctl root-alias refactor PLAN (e.g. META_ROOT -> LIFE_OS_ROOT) as a table of proposed changes. The plugin never applies: apply is routed through the envctl boundary (confirm + approval)."
    }

    fn signature(&self) -> Signature {
        command_signature(PluginCommand::name(self))
            .required(
                "from",
                SyntaxShape::String,
                "Source root var, e.g. META_ROOT",
            )
            .required(
                "to",
                SyntaxShape::String,
                "Target root var, e.g. LIFE_OS_ROOT",
            )
            .named(
                "repo",
                SyntaxShape::Filepath,
                "Repository root to index",
                None,
            )
            .named(
                "render-out",
                SyntaxShape::Filepath,
                "Record a render-out target tree (plan-only here)",
                None,
            )
    }

    fn run(
        &self,
        _plugin: &CodeDbPlugin,
        _engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: &Value,
    ) -> Result<Value, LabeledError> {
        let from: String = call.req(0)?;
        let to: String = call.req(1)?;
        let repo = repo_flag_or_cwd_string(call)?;
        let mut argv = envctl_db_argv(Some(&repo), &["refactor", "--from", &from, "--to", &to]);
        if let Some(out) = call.get_flag::<String>("render-out")? {
            argv.push("--render-out".into());
            argv.push(out);
        }
        let json = run_envctl_json(&argv, call.head)?;
        // RefactorPlan is `{ mode, changes:[...], ... }`; render the changes table.
        let changes = json
            .get("changes")
            .cloned()
            .unwrap_or(JsonValue::Array(vec![]));
        Ok(json_array_to_table(&changes, call.head))
    }
}

fn main() {
    serve_plugin(&CodeDbPlugin, MsgPackSerializer)
}

#[cfg(test)]
mod envctl_db_tests {
    use super::*;

    // Defends REQ-061: the plugin routes through the envctl boundary and is
    // read/plan-only — the argv it builds never carries apply/confirm, so the
    // plugin cannot mutate through the boundary.
    #[test]
    fn envctl_db_argv_routes_read_only_through_boundary() {
        let argv = envctl_db_argv(Some("/repo"), &["query", "--preset", "root-meta"]);
        assert_eq!(
            argv,
            vec![
                "--json",
                "db",
                "--repo-root",
                "/repo",
                "query",
                "--preset",
                "root-meta"
            ]
        );
        // Read-only invariant: no mutating flags reach envctl from the plugin.
        assert!(
            !argv
                .iter()
                .any(|a| a == "apply" || a == "--apply" || a == "--confirm")
        );
        // roots needs no repo.
        assert_eq!(
            envctl_db_argv(None, &["roots"]),
            vec!["--json", "db", "roots"]
        );
    }

    // Defends REQ-061: `envctl db roots --json` renders as a Nushell table with
    // typed cells (the multi-root model surfaced without plugin-owned state).
    #[test]
    fn roots_json_renders_as_nushell_table() {
        let json: JsonValue = serde_json::from_str(
            r#"[
                {"root_id":"root-meta","kind":"meta_root","role":"observed_current",
                 "var_names":["META_ROOT"],"precedence":100,"active":true},
                {"root_id":"root-lifeos","kind":"life_os_root","role":"release_target",
                 "var_names":["LIFE_OS_ROOT"],"precedence":90,"active":true}
            ]"#,
        )
        .unwrap();
        let table = json_array_to_table(&json, Span::unknown());
        let Value::List { vals, .. } = &table else {
            panic!("expected a table/list, got {table:?}");
        };
        assert_eq!(vals.len(), 2);
        let Value::Record { val, .. } = &vals[0] else {
            panic!("expected a record row");
        };
        // Scalar cells are typed; the nested array renders as a compact string.
        assert!(matches!(val.get("kind"), Some(Value::String { val, .. }) if val == "meta_root"));
        assert!(matches!(val.get("precedence"), Some(Value::Int { val, .. }) if *val == 100));
        assert!(matches!(val.get("active"), Some(Value::Bool { val, .. }) if *val));
        assert!(
            matches!(val.get("var_names"), Some(Value::String { val, .. }) if val.contains("META_ROOT"))
        );
    }

    // Defends REQ-061: the refactor PLAN's changes render as a table; the plugin
    // surfaces the fail-closed plan (safe + refused rows) but performs no apply.
    #[test]
    fn refactor_plan_changes_render_as_table() {
        let plan: JsonValue = serde_json::from_str(
            r#"{
                "mode":"plan","files_touched":1,"occurrences_total":2,"refused":1,"approved":false,
                "changes":[
                    {"absolute_path":"/r/.env","safe":false,"occurrence_count":1,
                     "refused_reason":"policy Never refuses auto-rewrite","unified_diff":""},
                    {"absolute_path":"/r/wrapper.sh","safe":true,"occurrence_count":1,
                     "refused_reason":"","unified_diff":"--- a/wrapper.sh\n+cd $LIFE_OS_ROOT\n"}
                ]
            }"#,
        )
        .unwrap();
        // Plan is fail-closed and un-approved — the plugin renders, never applies.
        assert_eq!(plan["approved"], serde_json::json!(false));
        let changes = plan.get("changes").cloned().unwrap();
        let table = json_array_to_table(&changes, Span::unknown());
        let Value::List { vals, .. } = &table else {
            panic!("expected changes table");
        };
        assert_eq!(vals.len(), 2);
        let refused = vals.iter().any(|row| {
            matches!(row, Value::Record { val, .. }
                if matches!(val.get("safe"), Some(Value::Bool { val, .. }) if !*val))
        });
        assert!(
            refused,
            "the .env change must surface as refused (safe=false)"
        );
    }

    // Defends REQ-061: the plugin registers the three envctl-db commands and none
    // of them are mutating (no `apply` verb in the plugin's command surface).
    #[test]
    fn plugin_registers_envctl_db_commands_read_only() {
        let plugin = CodeDbPlugin;
        let names: Vec<String> = plugin
            .commands()
            .iter()
            .map(|c| PluginCommand::name(c.as_ref()).to_string())
            .collect();
        assert!(names.iter().any(|n| n == "codedb envctl-db roots"));
        assert!(names.iter().any(|n| n == "codedb envctl-db query"));
        assert!(names.iter().any(|n| n == "codedb envctl-db refactor"));
        assert!(
            !names.iter().any(|n| n.contains("apply")),
            "plugin must expose no mutating apply command"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codedb-{name}-{nanos}"))
    }

    #[derive(Debug, PartialEq, Eq)]
    struct FileSnapshot {
        len: u64,
        readonly: bool,
        sha256: String,
    }

    fn file_snapshot(path: &Path) -> FileSnapshot {
        let metadata = fs::metadata(path).unwrap();
        let bytes = fs::read(path).unwrap();
        FileSnapshot {
            len: metadata.len(),
            readonly: metadata.permissions().readonly(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        }
    }

    // Defends: envctl inventory import rows preserve blob hashes for safe content and skip unsafe rows.
    #[test]
    fn envctl_inventory_import_rows_hash_content_and_skip_metadata_only() {
        let root = temp_path("inventory-import");
        fs::create_dir_all(&root).unwrap();
        let content_path = root.join("settings.jsonc");
        fs::write(&content_path, "{ \"ok\": true }\n").unwrap();
        let state_path = root.join("welcome.log");
        fs::write(&state_path, "runtime log\n").unwrap();
        let inventory_path = root.join("inventory.json");
        fs::write(
            &inventory_path,
            format!(
                r#"[{{
                    "target_id":"settings",
                    "absolute_path":"{}",
                    "normalized_logical_path":"real_home_config:settings.jsonc",
                    "owner":"user",
                    "source_of_truth_class":"real_home_user_config",
                    "file_kind":"regular_file",
                    "parser_hint":"jsonc",
                    "safety_policy":"generated_content_import_allowed",
                    "reproduction_policy":"user_config_source_or_import",
                    "import_mode":"content_blob"
                }},{{
                    "target_id":"welcome_log",
                    "absolute_path":"{}",
                    "normalized_logical_path":"real_home_local:logs/welcome.log",
                    "owner":"yazelix",
                    "source_of_truth_class":"real_home_runtime_state",
                    "file_kind":"regular_file",
                    "parser_hint":"log",
                    "safety_policy":"runtime_state_no_content_import",
                    "reproduction_policy":"observed_runtime_state_only",
                    "import_mode":"metadata_only"
                }}]"#,
                content_path.display(),
                state_path.display(),
            ),
        )
        .unwrap();

        let rows = envctl_inventory_import_rows(&inventory_path, Span::unknown()).unwrap();
        assert_eq!(rows.len(), 2);
        let content = &rows[0];
        assert!(content.iter().any(|(key, value)| {
            *key == "content_hash" && matches!(value, Value::String { val, .. } if val.len() == 64)
        }));
        assert!(content.iter().any(|(key, value)| {
            *key == "blob_ref"
                && matches!(value, Value::String { val, .. } if val.starts_with("sha256:"))
        }));
        let metadata = &rows[1];
        assert!(metadata.iter().any(|(key, value)| {
            *key == "import_status"
                && matches!(value, Value::String { val, .. } if val == "metadata_only")
        }));
        assert!(metadata.iter().any(|(key, value)| {
            *key == "skip_reason" && matches!(value, Value::String { val, .. } if val == "runtime_state_no_content_import")
        }));
    }

    // Defends: CDB071 Nu plugin command list remains read-only before apply gates exist.
    #[test]
    fn plugin_command_surface_has_no_mutating_bidirectional_defaults() {
        let plugin = CodeDbPlugin;
        let names = plugin
            .commands()
            .into_iter()
            .map(|command| command.name().to_string())
            .collect::<Vec<_>>();
        for forbidden in [
            "codedb apply",
            "codedb patch apply",
            "codedb source overwrite",
            "codedb git mutation",
            "codedb bidirectional sync",
        ] {
            assert!(
                !names.iter().any(|name| name == forbidden),
                "unexpected mutating command exposed: {forbidden}"
            );
        }
        assert!(names.iter().any(|name| name == "codedb scan"));
        assert!(names.iter().any(|name| name == "codedb export"));
    }

    // Defends: safe structured inventory targets expose native datatable payload rows.
    #[test]
    fn envctl_inventory_import_rows_include_structured_datatable_payload() {
        let root = temp_path("inventory-structured");
        fs::create_dir_all(&root).unwrap();
        let content_path = root.join("settings.jsonc");
        fs::write(
            &content_path,
            r#"{
  // comment tolerated by jsonc cleaner
  "theme": "zed",
  "show_banner": false,
}
"#,
        )
        .unwrap();
        let log_path = root.join("welcome.log");
        fs::write(&log_path, "runtime log\n").unwrap();
        let inventory_path = root.join("inventory.json");
        fs::write(
            &inventory_path,
            format!(
                r#"[{{
                    "target_id":"settings",
                    "absolute_path":"{}",
                    "normalized_logical_path":"repo_source:settings.jsonc",
                    "owner":"yazelix",
                    "source_of_truth_class":"repo_source",
                    "file_kind":"regular_file",
                    "parser_hint":"jsonc",
                    "safety_policy":"source_content_import_allowed",
                    "reproduction_policy":"git_checkout",
                    "import_mode":"content_blob"
                }},{{
                    "target_id":"welcome_log",
                    "absolute_path":"{}",
                    "normalized_logical_path":"real_home_local:logs/welcome.log",
                    "owner":"yazelix",
                    "source_of_truth_class":"real_home_runtime_state",
                    "file_kind":"regular_file",
                    "parser_hint":"log",
                    "safety_policy":"runtime_state_no_content_import",
                    "reproduction_policy":"observed_runtime_state_only",
                    "import_mode":"metadata_only"
                }}]"#,
                content_path.display(),
                log_path.display(),
            ),
        )
        .unwrap();

        let rows = envctl_inventory_import_rows(&inventory_path, Span::unknown()).unwrap();
        let content = &rows[0];
        assert!(content.iter().any(|(key, value)| {
            *key == "last_observed"
                && matches!(value, Value::String { val, .. } if val.starts_with("unix:"))
        }));
        assert!(content.iter().any(|(key, value)| {
            *key == "structured_status"
                && matches!(value, Value::String { val, .. } if val == "structured_rows_ready")
        }));
        assert!(content.iter().any(|(key, value)| {
            *key == "structured_row_count" && matches!(value, Value::Int { val, .. } if *val >= 2)
        }));
        let structured_rows = content
            .iter()
            .find_map(|(key, value)| (*key == "structured_rows").then_some(value))
            .expect("structured_rows field");
        match structured_rows {
            Value::List { vals, .. } => {
                assert!(vals.iter().any(|value| {
                    let Value::Record { val, .. } = value else {
                        return false;
                    };
                    val.get("key").is_some_and(
                        |value| matches!(value, Value::String { val, .. } if val == "theme"),
                    ) && val.get("value").is_some_and(
                        |value| matches!(value, Value::String { val, .. } if val == "zed"),
                    )
                }));
            }
            other => panic!("structured_rows should be a list, got {other:?}"),
        }

        let metadata = &rows[1];
        assert!(metadata.iter().any(|(key, value)| {
            *key == "structured_status"
                && matches!(value, Value::String { val, .. } if val == "metadata_only")
        }));
        assert!(metadata.iter().any(|(key, value)| {
            *key == "structured_row_count" && matches!(value, Value::Int { val, .. } if *val == 0)
        }));
    }

    // Defends: TOML has a native parser bridge and is not overclaimed as line fallback.
    #[test]
    fn envctl_inventory_import_rows_include_native_toml_payload() {
        let root = temp_path("inventory-toml");
        fs::create_dir_all(&root).unwrap();
        let content_path = root.join("settings.toml");
        fs::write(
            &content_path,
            r#"
theme = "zed"

[editor]
tab_width = 2
"#,
        )
        .unwrap();
        let inventory_path = root.join("inventory.json");
        fs::write(
            &inventory_path,
            format!(
                r#"[{{
                    "target_id":"settings_toml",
                    "absolute_path":"{}",
                    "normalized_logical_path":"repo_source:settings.toml",
                    "owner":"yazelix",
                    "source_of_truth_class":"repo_source",
                    "file_kind":"regular_file",
                    "parser_hint":"toml",
                    "safety_policy":"source_content_import_allowed",
                    "reproduction_policy":"git_checkout",
                    "import_mode":"content_blob"
                }}]"#,
                content_path.display(),
            ),
        )
        .unwrap();

        let rows = envctl_inventory_import_rows(&inventory_path, Span::unknown()).unwrap();
        let structured_rows = rows[0]
            .iter()
            .find_map(|(key, value)| (*key == "structured_rows").then_some(value))
            .expect("structured_rows field");
        match structured_rows {
            Value::List { vals, .. } => {
                assert!(vals.iter().any(|value| {
                    let Value::Record { val, .. } = value else {
                        return false;
                    };
                    val.get("row_kind").is_some_and(
                        |value| matches!(value, Value::String { val, .. } if val == "toml_value"),
                    ) && val.get("key").is_some_and(
                        |value| matches!(value, Value::String { val, .. } if val == "editor.tab_width"),
                    ) && val.get("value").is_some_and(
                        |value| matches!(value, Value::String { val, .. } if val == "2"),
                    )
                }));
            }
            other => panic!("structured_rows should be a list, got {other:?}"),
        }
    }

    // Defends: inventory import is read-only for source, real-home-like, and metadata-only targets.
    #[test]
    fn envctl_inventory_import_rows_do_not_mutate_targets() {
        let root = temp_path("inventory-no-mutation");
        let repo_dir = root.join("repo");
        let local_dir = root.join("home/.local/share/yazelix/sessions/session-1");
        fs::create_dir_all(&repo_dir).unwrap();
        fs::create_dir_all(&local_dir).unwrap();
        let source_path = repo_dir.join("settings.jsonc");
        let runtime_path = local_dir.join("status_bar_cache.json");
        fs::write(&source_path, "{ \"theme\": \"zed\" }\n").unwrap();
        fs::write(&runtime_path, "{ \"status\": \"cached\" }\n").unwrap();
        let inventory_path = root.join("inventory.json");
        fs::write(
            &inventory_path,
            format!(
                r#"[{{
                    "target_id":"settings",
                    "absolute_path":"{}",
                    "normalized_logical_path":"repo_source:settings.jsonc",
                    "owner":"yazelix",
                    "source_of_truth_class":"repo_source",
                    "file_kind":"regular_file",
                    "parser_hint":"jsonc",
                    "safety_policy":"source_content_import_allowed",
                    "reproduction_policy":"git_checkout",
                    "import_mode":"content_blob"
                }},{{
                    "target_id":"status_cache",
                    "absolute_path":"{}",
                    "normalized_logical_path":"real_home_runtime_state:.local/share/yazelix/sessions/session-1/status_bar_cache.json",
                    "owner":"yazelix",
                    "source_of_truth_class":"real_home_runtime_state",
                    "file_kind":"regular_file",
                    "parser_hint":"json",
                    "safety_policy":"runtime_state_no_content_import",
                    "reproduction_policy":"observed_runtime_state_only",
                    "import_mode":"metadata_only"
                }},{{
                    "target_id":"nix_store_runtime",
                    "absolute_path":"/nix/store/example-yazelix-runtime",
                    "normalized_logical_path":"nix_store:/nix/store/example-yazelix-runtime",
                    "owner":"nix",
                    "source_of_truth_class":"nix_store_package_output",
                    "file_kind":"package_output",
                    "parser_hint":"nix_store_path",
                    "safety_policy":"nix_store_metadata_only",
                    "reproduction_policy":"nix_realise",
                    "import_mode":"metadata_only"
                }}]"#,
                source_path.display(),
                runtime_path.display(),
            ),
        )
        .unwrap();
        let source_before = file_snapshot(&source_path);
        let runtime_before = file_snapshot(&runtime_path);

        let rows = envctl_inventory_import_rows(&inventory_path, Span::unknown()).unwrap();

        assert_eq!(source_before, file_snapshot(&source_path));
        assert_eq!(runtime_before, file_snapshot(&runtime_path));
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| {
            row.iter().any(|(key, value)| {
                *key == "target_id"
                    && matches!(value, Value::String { val, .. } if val == "nix_store_runtime")
            }) && row.iter().any(|(key, value)| {
                *key == "import_status"
                    && matches!(value, Value::String { val, .. } if val == "metadata_only")
            })
        }));
    }

    // Defends: Nix flake metadata JSON imports into summary, ref, lock-node, lock-edge, and output rows.
    #[test]
    fn nix_flake_import_rows_include_metadata_lock_graph_and_outputs() {
        let root = temp_path("nix-flake-import");
        fs::create_dir_all(&root).unwrap();
        let metadata_path = root.join("metadata.json");
        let outputs_path = root.join("outputs.json");
        fs::write(
            &metadata_path,
            r#"{
  "description": "CodeDB package",
  "lastModified": 1783020000,
  "locked": {
    "lastModified": 1783020000,
    "narHash": "sha256-local",
    "rev": "abcdef",
    "type": "git"
  },
  "lockedUrl": "git+file:///repo?rev=abcdef",
  "locks": {
    "nodes": {
      "root": {
        "inputs": {
          "nixpkgs": "nixpkgs",
          "systems": ["flake-utils", "systems"]
        }
      },
      "nixpkgs": {
        "locked": {
          "lastModified": 1783010000,
          "narHash": "sha256-nixpkgs",
          "owner": "NixOS",
          "repo": "nixpkgs",
          "rev": "123456",
          "type": "github"
        },
        "original": {
          "owner": "NixOS",
          "repo": "nixpkgs",
          "type": "github"
        }
      }
    },
    "root": "root",
    "version": 7
  },
  "original": { "type": "path", "path": "." },
  "originalUrl": "path:.",
  "path": "/nix/store/example-source",
  "resolved": { "type": "git", "url": "file:///repo" },
  "resolvedUrl": "git+file:///repo",
  "revision": "abcdef",
  "url": "git+file:///repo?rev=abcdef"
}"#,
        )
        .unwrap();
        fs::write(
            &outputs_path,
            r#"{
  "packages": {
    "x86_64-linux": {
      "default": {
        "type": "derivation",
        "name": "codedb-runtime-tools",
        "description": "runtime tools"
      }
    }
  },
  "apps": {
    "x86_64-linux": {
      "codedb": {
        "type": "app",
        "program": "/nix/store/example/bin/codedb"
      }
    }
  }
}"#,
        )
        .unwrap();

        let rows =
            nix_flake_import_rows(&metadata_path, Some(&outputs_path), Span::unknown()).unwrap();

        assert!(
            rows.iter()
                .any(|row| row_has_string(row, "table", "nix_flake_summary")
                    && row_has_string(row, "description", "CodeDB package")
                    && row_has_string(row, "revision", "abcdef")
                    && row_has_hash(row, "metadata_hash"))
        );
        assert!(
            rows.iter()
                .any(|row| row_has_string(row, "table", "nix_flake_refs")
                    && row_has_string(row, "ref_kind", "locked")
                    && row_has_string(row, "nar_hash", "sha256-local"))
        );
        assert!(
            rows.iter()
                .any(|row| row_has_string(row, "table", "nix_flake_lock_nodes")
                    && row_has_string(row, "node_name", "nixpkgs")
                    && row_has_string(row, "locked_owner", "NixOS"))
        );
        assert!(
            rows.iter()
                .any(|row| row_has_string(row, "table", "nix_flake_lock_edges")
                    && row_has_string(row, "source_node", "root")
                    && row_has_string(row, "input_name", "systems")
                    && row_has_string(row, "target_path", "flake-utils/systems")
                    && row_has_string(row, "edge_kind", "follows_path"))
        );
        assert!(
            rows.iter()
                .any(|row| row_has_string(row, "table", "nix_flake_outputs")
                    && row_has_string(row, "attr_path", "packages.x86_64-linux.default")
                    && row_has_string(row, "category", "packages")
                    && row_has_string(row, "system", "x86_64-linux")
                    && row_has_string(row, "output_kind", "derivation"))
        );
    }

    // Defends: malformed metadata produces an import error before any partial row claim.
    #[test]
    fn nix_flake_import_rows_reject_bad_metadata_json() {
        let root = temp_path("nix-flake-bad-json");
        fs::create_dir_all(&root).unwrap();
        let metadata_path = root.join("metadata.json");
        fs::write(&metadata_path, "{not json").unwrap();

        let error = nix_flake_import_rows(&metadata_path, None, Span::unknown()).unwrap_err();

        assert!(error.to_string().contains("failed to parse"));
    }

    // Defends: agent harness import captures fake HOME and repo surfaces without mutating them.
    #[test]
    fn agent_harness_import_rows_capture_full_fixture_surface() {
        let root = temp_path("agent-harness-import");
        let home = root.join("home");
        let repo = root.join("repo");
        let codex_dir = home.join(".codex");
        let prompts_dir = codex_dir.join("prompts");
        let skills_dir = codex_dir.join("skills").join("reviewer");
        let plugins_dir = codex_dir.join("plugins").join("demo-plugin");
        let stale_plugin_dir = codex_dir.join("plugins").join("stale-plugin").join("0.9.0");
        let conflicting_plugin_dir = codex_dir.join("plugins").join("demo-plugin-shadow");
        let kb_dir = repo.join(".kb");
        let kb_skills_dir = kb_dir.join("skills").join("repo-reviewer");
        let hooks_dir = repo.join(".codex").join("hooks");
        let generated_nushell_dir = home.join(".local/share/yazelix/initializers/nushell");
        let _env_guard = TestEnvGuard::set("CODEX_AUTH_TOKEN", "github_pat_fixture_secret");
        fs::create_dir_all(&prompts_dir).unwrap();
        fs::create_dir_all(&skills_dir).unwrap();
        fs::create_dir_all(&plugins_dir).unwrap();
        fs::create_dir_all(&stale_plugin_dir).unwrap();
        fs::create_dir_all(&conflicting_plugin_dir).unwrap();
        fs::create_dir_all(&kb_dir).unwrap();
        fs::create_dir_all(&kb_skills_dir).unwrap();
        fs::create_dir_all(&hooks_dir).unwrap();
        fs::create_dir_all(&generated_nushell_dir).unwrap();

        let config_path = codex_dir.join("config.toml");
        fs::write(
            &config_path,
            r#"
model = "gpt-5-codex"
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
        .unwrap();
        fs::write(
            prompts_dir.join("triage.md"),
            "# triage\nUse bounded scans.\n",
        )
        .unwrap();
        fs::write(
            skills_dir.join("SKILL.md"),
            "# Reviewer\nUse reproducible evidence.\n",
        )
        .unwrap();
        fs::write(
            plugins_dir.join("plugin.json"),
            r#"{"name":"demo-plugin","version":"1.2.3","owner":"meta-plugins-codex"}"#,
        )
        .unwrap();
        fs::write(
            conflicting_plugin_dir.join("plugin.json"),
            r#"{"name":"demo-plugin","version":"1.2.3","owner":"other-owner"}"#,
        )
        .unwrap();
        fs::write(
            stale_plugin_dir.join("plugin.json"),
            r#"{"name":"stale-plugin","version":"1.2.3","owner":"meta-plugins-codex"}"#,
        )
        .unwrap();
        fs::write(
            codex_dir.join("auth.json"),
            r#"{"access_token":"github_pat_fixture_secret","account_id":"acct-1"}"#,
        )
        .unwrap();
        fs::write(kb_dir.join("config.toml"), "workspace = \"main\"\n").unwrap();
        fs::write(kb_dir.join("AGENTS.md"), "# KB Agents\nUse git-kb first.\n").unwrap();
        fs::write(
            kb_skills_dir.join("SKILL.md"),
            "# Repo Reviewer\nUse repo-local harness rules.\n",
        )
        .unwrap();
        fs::write(
            repo.join("AGENTS.md"),
            "# Repo Agents\nStay read only by default.\n",
        )
        .unwrap();
        fs::write(
            hooks_dir.join("post-apply.sh"),
            "#!/usr/bin/env bash\necho post-apply\n",
        )
        .unwrap();
        fs::write(
            generated_nushell_dir.join("codedb_init.nu"),
            "export-env { $env.CODEDB_YAZELIX_BRIDGE_MODE = \"legacy\" }\n",
        )
        .unwrap();

        let config_before = file_snapshot(&config_path);
        let hook_before = file_snapshot(&hooks_dir.join("post-apply.sh"));

        let rows = agent_harness_import_rows(&repo, &home, Span::unknown()).unwrap();

        assert_eq!(config_before, file_snapshot(&config_path));
        assert_eq!(hook_before, file_snapshot(&hooks_dir.join("post-apply.sh")));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_manifests")
                && row.iter().any(|(key, value)| {
                    *key == "component_count"
                        && matches!(value, Value::Int { val, .. } if *val >= 8)
                })
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_codex_settings")
                && row_has_string(row, "key", "OPENAI_API_KEY")
                && row_has_string(row, "value", "[redacted]")
                && row_has_string(row, "value_redacted", "true")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_prompts")
                && row_has_string(row, "prompt_name", "triage")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_plugin_skills")
                && row_has_string(row, "skill_name", "reviewer")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_plugin_skills")
                && row_has_string(row, "plugin_name", "repo_kb_skills")
                && row_has_string(row, "skill_name", "repo-reviewer")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_plugins")
                && row_has_string(row, "plugin_name", "demo-plugin")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_files")
                && row_has_string(row, "source_class", "codex_auth_file")
                && row_has_string(row, "owner_boundary", "user_local")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_files")
                && row_has_string(row, "source_class", "repo_kb_skill")
                && row_has_string(row, "owner_boundary", "repo_local")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_env")
                && row_has_string(row, "key", "CODEX_AUTH_TOKEN")
                && row_has_string(row, "value", "[redacted]")
                && row_has_string(row, "value_redacted", "true")
                && row_has_string(row, "owner_boundary", "private_env")
                && row_has_prefix(row, "secret_ref", "sha256:")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_mcp_servers")
                && row_has_string(row, "server_id", "primary")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_hooks")
                && row_has_string(row, "hook_id", "post_apply")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "duplicate_mcp_command")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "disabled_hook")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "duplicate_plugin_ownership")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "stale_plugin_metadata")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "generated_state_missing")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_validation_errors")
                && row_has_string(row, "code", "generated_state_stale")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_materialization_plan")
                && row_has_string(row, "mutation_allowed", "false")
                && row_has_string(row, "target_class", "user_local")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_export_manifests")
                && row_has_string(row, "plan_table", "agent_harness_materialization_plan")
        }));
        assert!(rows.iter().any(|row| {
            row_has_string(row, "table", "agent_harness_env")
                && row_has_string(row, "owner_boundary", "generated_state")
        }));

        let plugin = CodeDbPlugin;
        let names = plugin
            .commands()
            .into_iter()
            .map(|command| command.name().to_string())
            .collect::<Vec<_>>();
        assert!(
            names
                .iter()
                .any(|name| name == "codedb agent harness import")
        );

        let _ = fs::remove_dir_all(root);
    }

    fn row_has_string(row: &Row, key: &str, expected: &str) -> bool {
        row.iter().any(|(candidate, value)| {
            *candidate == key && matches!(value, Value::String { val, .. } if val == expected)
        })
    }

    fn row_has_hash(row: &Row, key: &str) -> bool {
        row.iter().any(|(candidate, value)| {
            *candidate == key && matches!(value, Value::String { val, .. } if val.len() == 64)
        })
    }

    fn row_has_prefix(row: &Row, key: &str, prefix: &str) -> bool {
        row.iter().any(|(candidate, value)| {
            *candidate == key
                && matches!(value, Value::String { val, .. } if val.starts_with(prefix))
        })
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

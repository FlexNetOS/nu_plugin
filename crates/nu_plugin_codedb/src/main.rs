// Test lane: default

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_cargo::{CargoContextInput, build_context_rows, capture_cargo_metadata};
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

struct CodeDbPlugin;

type Row = Vec<(&'static str, Value)>;

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
            Box::new(EnvctlInventoryImport),
            Box::new(Tables),
            Box::new(Gaps),
            Box::new(ValidationErrors),
            Box::new(Schema),
            Box::new(Doctor),
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
struct EnvctlInventoryImport;
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
        let cargo_metadata = capture_cargo_metadata(&manifest_path).map_err(cargo_error)?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(cargo_error)?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(cargo_error)?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(cargo_error)?;
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
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(cargo_error)?;
    let edition = metadata
        .packages
        .first()
        .map(|package| package.edition.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let capture = build_context_rows(CargoContextInput {
        cargo_version: "unknown".to_string(),
        rustc_version: "unknown".to_string(),
        host_triple: "unknown".to_string(),
        target_triple: "unknown".to_string(),
        cfgs: Vec::new(),
        features: metadata
            .features
            .iter()
            .map(|feature| feature.feature.clone())
            .collect(),
        profile: "static".to_string(),
        edition,
        cargo_lock_hash: None,
    });
    Ok(vec![
        vec![
            ("table", string("codedb_contexts", span)),
            ("context_id", string(capture.context.context_id, span)),
            ("toolchain_id", string(capture.context.toolchain_id, span)),
            ("target_triple", string(capture.context.target_triple, span)),
            (
                "feature_set_hash",
                string(capture.context.feature_set_hash, span),
            ),
            ("cfg_hash", string(capture.context.cfg_hash, span)),
            ("profile", string(capture.context.profile, span)),
            ("edition", string(capture.context.edition, span)),
        ],
        vec![
            ("table", string("feature_sets", span)),
            (
                "feature_set_hash",
                string(capture.feature_set.feature_set_hash, span),
            ),
            (
                "features",
                string(capture.feature_set.features.join(";"), span),
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

fn structured_file_rows(parser_hint: &str, bytes: &[u8], span: Span) -> Option<Vec<Value>> {
    let text = std::str::from_utf8(bytes).ok()?;
    match parser_hint {
        "json" | "jsonc" => {
            json_table_rows(text, span).or_else(|| text_table_rows(parser_hint, text, span))
        }
        "toml" | "nix" | "kdl" | "nu" | "lua" | "yaml" | "yml" | "markdown" | "desktop"
        | "service" | "shell" | "conf" | "terminal_conf" | "plain_config" => {
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

fn main() {
    serve_plugin(&CodeDbPlugin, MsgPackSerializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("codedb-{name}-{nanos}"))
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
}

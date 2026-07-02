#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use codedb_cargo::capture_cargo_metadata;
use codedb_core::{
    FileClassification, TableRow, capture_gaps, prove_no_mutation, scan_filesystem, schema_rows,
    table_inventory, validation_errors,
};
use codedb_rust_static::{capture_build_script_static, capture_rust_items, capture_rust_macros};
use serde::{Deserialize, Serialize};

pub const STATUS: &str = "bounded_read_only_mcp_available";
pub const DEFAULT_ROW_LIMIT: usize = 50;
pub const MAX_ROW_LIMIT: usize = 200;
pub const DEFAULT_MAX_BYTES: usize = 65_536;
pub const DEFAULT_TRANSPORT: &str = "stdio";

pub const ALLOWED_TOOLS: &[&str] = &[
    "codedb_schema",
    "codedb_list_tables",
    "codedb_get_table_page",
    "codedb_get_capture_gaps",
    "codedb_get_validation_errors",
    "codedb_get_repo_summary",
    "codedb_get_cargo_summary",
    "codedb_get_rust_item_summary",
    "codedb_get_macro_summary",
    "codedb_get_build_script_summary",
    "codedb_get_no_mutation_proof",
];

pub const BLOCKED_TOOLS: &[&str] = &[
    "raw_source_blob_read",
    "full_file_dump",
    "unsafe_build_capture",
    "source_overwrite",
    "patch_apply",
    "git_mutation",
    "unbounded_table_dump",
];

pub type Row = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpRequest {
    pub tool: String,
    pub repo_path: Option<PathBuf>,
    pub table: Option<String>,
    pub cursor: Option<usize>,
    pub limit: Option<usize>,
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpResponse {
    pub tool: String,
    pub status: String,
    pub cursor: usize,
    pub next_cursor: Option<usize>,
    pub limit: usize,
    pub max_bytes: usize,
    pub truncated: bool,
    pub rows: Vec<Row>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub transport: String,
    pub default_row_limit: usize,
    pub max_row_limit: usize,
    pub default_max_bytes: usize,
    pub raw_source_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpLifecycleReport {
    pub phase: String,
    pub status: String,
    pub transport: String,
    pub bounded_defaults: bool,
    pub raw_source_enabled: bool,
    pub note: String,
}

#[derive(Debug)]
pub enum McpError {
    BlockedTool(String),
    UnknownTool(String),
    MissingRepoPath(&'static str),
    MissingTable,
    LimitTooLarge { requested: usize, max: usize },
    Core(Box<dyn StdError>),
}

impl Display for McpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockedTool(tool) => write!(f, "blocked MCP tool: {tool}"),
            Self::UnknownTool(tool) => write!(f, "unknown MCP tool: {tool}"),
            Self::MissingRepoPath(tool) => write!(f, "{tool} requires repo_path"),
            Self::MissingTable => write!(f, "codedb_get_table_page requires table"),
            Self::LimitTooLarge { requested, max } => {
                write!(f, "row limit {requested} exceeds maximum {max}")
            }
            Self::Core(source) => write!(f, "{source}"),
        }
    }
}

impl StdError for McpError {}

pub fn list_allowed_tools() -> Vec<Row> {
    ALLOWED_TOOLS
        .iter()
        .map(|tool| {
            row([
                ("tool", (*tool).to_string()),
                ("access", "read_only".to_string()),
                ("bounded", "true".to_string()),
            ])
        })
        .collect()
}

pub fn list_blocked_tools() -> Vec<Row> {
    BLOCKED_TOOLS
        .iter()
        .map(|tool| {
            row([
                ("tool", (*tool).to_string()),
                ("access", "blocked".to_string()),
                ("reason", "raw, mutating, unsafe, or unbounded".to_string()),
            ])
        })
        .collect()
}

pub fn default_server_config() -> McpServerConfig {
    McpServerConfig {
        transport: DEFAULT_TRANSPORT.to_string(),
        default_row_limit: DEFAULT_ROW_LIMIT,
        max_row_limit: MAX_ROW_LIMIT,
        default_max_bytes: DEFAULT_MAX_BYTES,
        raw_source_enabled: false,
    }
}

pub fn lifecycle_start(config: &McpServerConfig) -> Result<McpLifecycleReport, McpError> {
    if config.default_row_limit > config.max_row_limit {
        return Err(McpError::LimitTooLarge {
            requested: config.default_row_limit,
            max: config.max_row_limit,
        });
    }
    Ok(McpLifecycleReport {
        phase: "startup".to_string(),
        status: "ready".to_string(),
        transport: config.transport.clone(),
        bounded_defaults: config.max_row_limit <= MAX_ROW_LIMIT
            && config.default_max_bytes <= DEFAULT_MAX_BYTES,
        raw_source_enabled: config.raw_source_enabled,
        note: "external MCP server lifecycle configured with bounded read-only defaults"
            .to_string(),
    })
}

pub fn lifecycle_shutdown(config: &McpServerConfig) -> McpLifecycleReport {
    McpLifecycleReport {
        phase: "shutdown".to_string(),
        status: "stopped".to_string(),
        transport: config.transport.clone(),
        bounded_defaults: true,
        raw_source_enabled: config.raw_source_enabled,
        note: "shutdown completes without mutating repositories or exposing raw source".to_string(),
    }
}

pub fn lifecycle_rows(config: &McpServerConfig) -> Result<Vec<Row>, McpError> {
    let start = lifecycle_start(config)?;
    let shutdown = lifecycle_shutdown(config);
    Ok(vec![
        lifecycle_row(&start),
        lifecycle_row(&shutdown),
        row([
            ("table", "mcp_config".to_string()),
            ("phase", "config".to_string()),
            ("status", "available".to_string()),
            ("transport", config.transport.clone()),
            ("default_row_limit", config.default_row_limit.to_string()),
            ("max_row_limit", config.max_row_limit.to_string()),
            ("default_max_bytes", config.default_max_bytes.to_string()),
            ("raw_source_enabled", config.raw_source_enabled.to_string()),
        ]),
    ])
}

pub fn handle_request(request: McpRequest) -> Result<McpResponse, McpError> {
    ensure_tool_allowed(&request.tool)?;
    let limit = request.limit.unwrap_or(DEFAULT_ROW_LIMIT);
    if limit > MAX_ROW_LIMIT {
        return Err(McpError::LimitTooLarge {
            requested: limit,
            max: MAX_ROW_LIMIT,
        });
    }

    let max_bytes = request.max_bytes.unwrap_or(DEFAULT_MAX_BYTES);
    let cursor = request.cursor.unwrap_or(0);
    let rows = match request.tool.as_str() {
        "codedb_schema" => table_rows(schema_rows()),
        "codedb_list_tables" => table_rows(table_inventory()),
        "codedb_get_capture_gaps" => table_rows(capture_gaps()),
        "codedb_get_validation_errors" => table_rows(validation_errors()),
        "codedb_get_table_page" => {
            let table = request.table.as_deref().ok_or(McpError::MissingTable)?;
            let repo_path = request.repo_path.as_deref();
            table_page_rows(table, repo_path)?
        }
        "codedb_get_repo_summary" => {
            let repo_path = required_repo_path(&request)?;
            repo_summary_rows(repo_path)?
        }
        "codedb_get_cargo_summary" => {
            let repo_path = required_repo_path(&request)?;
            cargo_summary_rows(repo_path)?
        }
        "codedb_get_rust_item_summary" => {
            let repo_path = required_repo_path(&request)?;
            rust_item_summary_rows(repo_path)?
        }
        "codedb_get_macro_summary" => {
            let repo_path = required_repo_path(&request)?;
            macro_summary_rows(repo_path)?
        }
        "codedb_get_build_script_summary" => {
            let repo_path = required_repo_path(&request)?;
            build_script_summary_rows(repo_path)?
        }
        "codedb_get_no_mutation_proof" => {
            let repo_path = required_repo_path(&request)?;
            no_mutation_rows(repo_path)?
        }
        other => return Err(McpError::UnknownTool(other.to_string())),
    };

    Ok(bound_response(request.tool, rows, cursor, limit, max_bytes))
}

pub fn ensure_tool_allowed(tool: &str) -> Result<(), McpError> {
    if BLOCKED_TOOLS.contains(&tool) {
        return Err(McpError::BlockedTool(tool.to_string()));
    }
    if !ALLOWED_TOOLS.contains(&tool) {
        return Err(McpError::UnknownTool(tool.to_string()));
    }
    Ok(())
}

fn bound_response(
    tool: String,
    rows: Vec<Row>,
    cursor: usize,
    limit: usize,
    max_bytes: usize,
) -> McpResponse {
    let mut selected = Vec::new();
    let mut consumed_bytes = 2usize;
    let mut truncated = false;
    for row in rows.iter().skip(cursor).take(limit) {
        let row_bytes = serde_json::to_vec(row)
            .map(|bytes| bytes.len())
            .unwrap_or(0);
        if consumed_bytes.saturating_add(row_bytes) > max_bytes {
            truncated = true;
            break;
        }
        consumed_bytes = consumed_bytes.saturating_add(row_bytes).saturating_add(1);
        selected.push(row.clone());
    }

    let next_cursor = cursor + selected.len();
    let next_cursor = if truncated || next_cursor < rows.len() {
        Some(next_cursor)
    } else {
        None
    };

    McpResponse {
        tool,
        status: "ok".to_string(),
        cursor,
        next_cursor,
        limit,
        max_bytes,
        truncated,
        rows: selected,
        errors: Vec::new(),
    }
}

fn lifecycle_row(report: &McpLifecycleReport) -> Row {
    row([
        ("table", "mcp_lifecycle".to_string()),
        ("phase", report.phase.clone()),
        ("status", report.status.clone()),
        ("transport", report.transport.clone()),
        ("bounded_defaults", report.bounded_defaults.to_string()),
        ("raw_source_enabled", report.raw_source_enabled.to_string()),
        ("note", report.note.clone()),
    ])
}

fn table_page_rows(table: &str, repo_path: Option<&Path>) -> Result<Vec<Row>, McpError> {
    match table {
        "schema" | "schema_versions" => Ok(table_rows(schema_rows())),
        "tables" => Ok(table_rows(table_inventory())),
        "capture_gaps" | "gaps" => Ok(table_rows(capture_gaps())),
        "validation_errors" | "validation-errors" => Ok(table_rows(validation_errors())),
        "repo_summary" | "filesystem_entries" => {
            repo_summary_rows(repo_path.ok_or(McpError::MissingRepoPath("codedb_get_table_page"))?)
        }
        "cargo_summary" | "cargo_packages" => {
            cargo_summary_rows(repo_path.ok_or(McpError::MissingRepoPath("codedb_get_table_page"))?)
        }
        "rust_item_summary" | "rust_items" => rust_item_summary_rows(
            repo_path.ok_or(McpError::MissingRepoPath("codedb_get_table_page"))?,
        ),
        "macro_summary" | "rust_macros" => {
            macro_summary_rows(repo_path.ok_or(McpError::MissingRepoPath("codedb_get_table_page"))?)
        }
        "build_script_summary" | "build_scripts" => build_script_summary_rows(
            repo_path.ok_or(McpError::MissingRepoPath("codedb_get_table_page"))?,
        ),
        "mcp_lifecycle" | "mcp_config" => lifecycle_rows(&default_server_config()),
        other => Ok(vec![row([
            ("table", "validation_errors".to_string()),
            ("code", "unsupported_table".to_string()),
            ("message", format!("unsupported MCP table page: {other}")),
        ])]),
    }
}

fn repo_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let entries = scan_filesystem(repo_path).map_err(core_error)?;
    let rust_sources = entries
        .iter()
        .filter(|entry| entry.classification == FileClassification::RustSource)
        .count();
    Ok(vec![
        summary_row("filesystem_entries", entries.len(), "read-only scan"),
        summary_row(
            "rust_sources",
            rust_sources,
            "source paths only; no raw bytes",
        ),
    ])
}

fn cargo_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(core_error)?;
    Ok(vec![
        summary_row("cargo_packages", metadata.packages.len(), "package rows"),
        summary_row(
            "cargo_dependencies",
            metadata.dependencies.len(),
            "dependency rows",
        ),
        summary_row("cargo_sources", metadata.sources.len(), "source rows"),
    ])
}

fn rust_item_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let mut count = 0usize;
    for source_path in rust_source_paths(repo_path)? {
        count += capture_rust_items(repo_path, &source_path, "mcp-static")
            .map_err(core_error)?
            .len();
    }
    Ok(vec![summary_row(
        "rust_items",
        count,
        "static syntax item rows",
    )])
}

fn macro_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let mut definitions = 0usize;
    let mut invocations = 0usize;
    let mut gaps = 0usize;
    for source_path in rust_source_paths(repo_path)? {
        let inventory =
            capture_rust_macros(repo_path, &source_path, "mcp-static").map_err(core_error)?;
        definitions += inventory.definitions.len();
        invocations += inventory.invocations.len();
        gaps += inventory.gaps.len();
    }
    Ok(vec![
        summary_row(
            "rust_macro_definitions",
            definitions,
            "static macro_rules rows",
        ),
        summary_row(
            "rust_macro_invocations",
            invocations,
            "static macro call rows",
        ),
        summary_row("rust_macro_gaps", gaps, "safe static capture gaps"),
    ])
}

fn build_script_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let mut scripts = 0usize;
    let mut instructions = 0usize;
    let mut gaps = 0usize;
    for source_path in rust_source_paths(repo_path)? {
        if source_path.file_name().and_then(|name| name.to_str()) != Some("build.rs") {
            continue;
        }
        let inventory = capture_build_script_static(repo_path, &source_path, "mcp-static")
            .map_err(core_error)?;
        scripts += inventory.scripts.len();
        instructions += inventory.instructions.len();
        gaps += inventory.gaps.len();
    }
    Ok(vec![
        summary_row("build_scripts", scripts, "static build.rs files"),
        summary_row(
            "build_script_instructions",
            instructions,
            "static cargo directive rows",
        ),
        summary_row(
            "build_script_gaps",
            gaps,
            "dynamic execution facts remain gated",
        ),
    ])
}

fn no_mutation_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let proof = prove_no_mutation(repo_path, "codedb_mcp_read_only_summary", || {
        let _ = scan_filesystem(repo_path);
    })
    .map_err(core_error)?;
    Ok(vec![row([
        ("table", "no_mutation_proofs".to_string()),
        ("operation", proof.operation),
        ("status", proof.status.as_str().to_string()),
        ("pre_existing_dirty", proof.pre_existing_dirty.to_string()),
        ("mutation_detected", proof.mutation_detected.to_string()),
        (
            "degradation_reason",
            proof.degradation_reason.unwrap_or_default(),
        ),
    ])])
}

fn required_repo_path(request: &McpRequest) -> Result<&Path, McpError> {
    request
        .repo_path
        .as_deref()
        .ok_or(McpError::MissingRepoPath("MCP summary tool"))
}

fn rust_source_paths(repo_path: &Path) -> Result<Vec<PathBuf>, McpError> {
    Ok(scan_filesystem(repo_path)
        .map_err(core_error)?
        .into_iter()
        .filter(|entry| entry.classification == FileClassification::RustSource)
        .map(|entry| repo_path.join(entry.relative_path))
        .collect())
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

fn summary_row(table: &str, rows: usize, note: &str) -> Row {
    row([
        ("table", table.to_string()),
        ("status", "available".to_string()),
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

fn core_error(error: impl StdError + 'static) -> McpError {
    McpError::Core(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Test lane: default
    // Defends: CDB032 exposes only the PRD-approved read-only MCP tools.
    #[test]
    fn allowed_and_blocked_tools_are_explicit() {
        assert!(ensure_tool_allowed("codedb_schema").is_ok());
        assert!(matches!(
            ensure_tool_allowed("raw_source_blob_read"),
            Err(McpError::BlockedTool(_))
        ));
        assert!(matches!(
            ensure_tool_allowed("patch_apply"),
            Err(McpError::BlockedTool(_))
        ));
        assert!(matches!(
            ensure_tool_allowed("codedb_dump_everything"),
            Err(McpError::UnknownTool(_))
        ));
    }

    // Test lane: default
    // Defends: CDB032 table page output remains bounded by caller-supplied row limits.
    #[test]
    fn table_page_enforces_row_limits() {
        let response = handle_request(McpRequest {
            tool: "codedb_list_tables".to_string(),
            repo_path: None,
            table: None,
            cursor: Some(0),
            limit: Some(2),
            max_bytes: Some(DEFAULT_MAX_BYTES),
        })
        .expect("table list should be available");

        assert_eq!(response.rows.len(), 2);
        assert_eq!(response.next_cursor, Some(2));
    }

    // Test lane: default
    // Defends: CDB032 rejects unbounded or excessive table reads.
    #[test]
    fn excessive_limit_is_rejected() {
        assert!(matches!(
            handle_request(McpRequest {
                tool: "codedb_list_tables".to_string(),
                repo_path: None,
                table: None,
                cursor: None,
                limit: Some(MAX_ROW_LIMIT + 1),
                max_bytes: None,
            }),
            Err(McpError::LimitTooLarge { .. })
        ));
    }

    // Test lane: default
    // Defends: CDB032 byte limits truncate responses before oversized dumps are emitted.
    #[test]
    fn byte_limit_truncates_rows() {
        let response = handle_request(McpRequest {
            tool: "codedb_list_tables".to_string(),
            repo_path: None,
            table: None,
            cursor: Some(0),
            limit: Some(MAX_ROW_LIMIT),
            max_bytes: Some(80),
        })
        .expect("table list should be available");

        assert!(response.truncated);
        assert!(response.rows.len() < table_inventory().len());
    }

    // Test lane: default
    // Defends: CDB032 repository summaries expose metadata only, not raw source content.
    #[test]
    fn repo_summary_does_not_leak_raw_source() {
        let repo = temp_repo();
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(
            repo.join("src/lib.rs"),
            "pub const SECRET_TOKEN: &str = \"should_not_escape\";\n",
        )
        .expect("write source");

        let response = handle_request(McpRequest {
            tool: "codedb_get_repo_summary".to_string(),
            repo_path: Some(repo.clone()),
            table: None,
            cursor: None,
            limit: Some(DEFAULT_ROW_LIMIT),
            max_bytes: Some(DEFAULT_MAX_BYTES),
        })
        .expect("repo summary should work");
        let output = serde_json::to_string(&response).expect("serialize response");

        assert!(!output.contains("should_not_escape"));
        assert!(!output.contains("SECRET_TOKEN"));

        let _ = fs::remove_dir_all(repo);
    }

    // Test lane: default
    // Defends: packaged MCP lifecycle exposes startup/shutdown/config proof without enabling raw source.
    #[test]
    fn lifecycle_rows_keep_raw_source_disabled() {
        let config = default_server_config();
        let rows = lifecycle_rows(&config).expect("lifecycle rows");
        assert!(rows.iter().any(|row| {
            row.get("phase").is_some_and(|phase| phase == "startup")
                && row.get("status").is_some_and(|status| status == "ready")
        }));
        assert!(rows.iter().all(|row| {
            row.get("raw_source_enabled")
                .is_none_or(|enabled| enabled == "false")
        }));
    }

    // Test lane: default
    // Defends: external MCP config remains bounded before serving requests.
    #[test]
    fn lifecycle_rejects_unbounded_default_limit() {
        let mut config = default_server_config();
        config.default_row_limit = MAX_ROW_LIMIT + 1;
        assert!(matches!(
            lifecycle_start(&config),
            Err(McpError::LimitTooLarge { .. })
        ));
    }

    fn temp_repo() -> PathBuf {
        let mut path = std::env::temp_dir();
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        path.push(format!("codedb_mcp_test_{suffix}"));
        path
    }
}

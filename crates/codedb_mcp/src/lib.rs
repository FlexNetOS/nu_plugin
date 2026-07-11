#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::env;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Component, Path, PathBuf};

use codedb_cargo::capture_cargo_metadata_json;
use codedb_context::{capture_context, detect_host_triple, CargoContextRequest};
use codedb_core::{
    capture_gaps, prove_no_mutation, schema_rows, table_inventory, validation_errors, TableRow,
};
use codedb_rust_static::{capture_build_script_static, capture_rust_items, capture_rust_macros};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const STATUS: &str = "bounded_read_only_mcp_available";
pub const DEFAULT_TRANSPORT: &str = "stdio";
pub const DEFAULT_ROW_LIMIT: usize = 50;
pub const MAX_ROW_LIMIT: usize = 200;
pub const DEFAULT_MAX_BYTES: usize = 65_536;
pub const MIN_RESPONSE_BYTES: usize = 256;
pub const MAX_RESPONSE_BYTES: usize = 65_536;
pub const DEFAULT_MAX_SCAN_ENTRIES: usize = 10_000;
pub const MAX_SCAN_ENTRIES: usize = 10_000;
pub const DEFAULT_MAX_RUST_SOURCES: usize = 1_000;
pub const MAX_RUST_SOURCES: usize = 1_000;
pub const DEFAULT_MAX_TRAVERSAL_DEPTH: usize = 32;
pub const MAX_TRAVERSAL_DEPTH: usize = 32;
pub const DEFAULT_MAX_REQUESTS: usize = 128;
pub const MAX_REQUESTS: usize = 128;
pub const MAX_JSON_RPC_LINE_BYTES: usize = 1_048_576;
pub const MAX_CURSOR: usize = 1_000_000;

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
    "raw_source_read",
    "raw_blob_read",
    "source_blob_read",
    "artifact_blob_read",
    "full_file_dump",
    "unsafe_build_capture",
    "source_overwrite",
    "patch_apply",
    "git_mutation",
    "unbounded_table_dump",
];

const BLOCKED_TABLES: &[&str] = &[
    "source_blobs",
    "artifact_blobs",
    "blob_refs",
    "raw_source",
    "raw_blobs",
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
    pub allowed_root: PathBuf,
    pub transport: String,
    pub default_row_limit: usize,
    pub max_row_limit: usize,
    pub default_max_bytes: usize,
    pub max_response_bytes: usize,
    pub max_scan_entries: usize,
    pub max_rust_sources: usize,
    pub max_traversal_depth: usize,
    pub max_requests: usize,
    pub raw_source_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkLimits {
    pub max_scan_entries: usize,
    pub max_rust_sources: usize,
    pub max_traversal_depth: usize,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerReport {
    pub status: String,
    pub requests: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpError {
    BlockedTool,
    UnknownTool,
    RawSourceDisabled,
    MissingRepoPath,
    MissingTable,
    InvalidConfiguration,
    InvalidRepositoryPath,
    BoundExceeded,
    WorkLimitExceeded,
    ResponseBudgetTooSmall,
    BackendFailure,
    ProtocolViolation,
    IoFailure,
}

impl McpError {
    pub fn code(self) -> &'static str {
        match self {
            Self::BlockedTool | Self::RawSourceDisabled => "policy_denied",
            Self::UnknownTool => "unknown_tool",
            Self::MissingRepoPath | Self::MissingTable | Self::ProtocolViolation => {
                "invalid_request"
            }
            Self::InvalidConfiguration => "invalid_configuration",
            Self::InvalidRepositoryPath => "invalid_repository_path",
            Self::BoundExceeded | Self::ResponseBudgetTooSmall => "request_bound_exceeded",
            Self::WorkLimitExceeded => "work_limit_exceeded",
            Self::BackendFailure => "backend_unavailable",
            Self::IoFailure => "transport_failure",
        }
    }
}

impl Display for McpError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::BlockedTool | Self::RawSourceDisabled => {
                "requested operation is disabled by policy"
            }
            Self::UnknownTool => "requested tool is not available",
            Self::MissingRepoPath | Self::MissingTable | Self::ProtocolViolation => {
                "request is invalid"
            }
            Self::InvalidConfiguration => "server configuration is invalid",
            Self::InvalidRepositoryPath => "repository path is not permitted",
            Self::BoundExceeded | Self::ResponseBudgetTooSmall => {
                "request exceeds a configured safety bound"
            }
            Self::WorkLimitExceeded => "repository work exceeds a configured safety bound",
            Self::BackendFailure => "read-only backend operation failed",
            Self::IoFailure => "stdio transport failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for McpError {}

impl McpServerConfig {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self {
            allowed_root,
            transport: DEFAULT_TRANSPORT.to_string(),
            default_row_limit: DEFAULT_ROW_LIMIT,
            max_row_limit: MAX_ROW_LIMIT,
            default_max_bytes: DEFAULT_MAX_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
            max_scan_entries: DEFAULT_MAX_SCAN_ENTRIES,
            max_rust_sources: DEFAULT_MAX_RUST_SOURCES,
            max_traversal_depth: DEFAULT_MAX_TRAVERSAL_DEPTH,
            max_requests: DEFAULT_MAX_REQUESTS,
            raw_source_enabled: false,
        }
    }

    pub fn validate(&self) -> Result<(), McpError> {
        self.policy().map(|_| ())
    }

    fn policy(&self) -> Result<ValidatedPolicy, McpError> {
        if self.transport != DEFAULT_TRANSPORT {
            return Err(McpError::InvalidConfiguration);
        }
        if self.raw_source_enabled {
            return Err(McpError::RawSourceDisabled);
        }
        validate_range(self.default_row_limit, self.max_row_limit, MAX_ROW_LIMIT, 1)?;
        validate_range(
            self.default_max_bytes,
            self.max_response_bytes,
            MAX_RESPONSE_BYTES,
            MIN_RESPONSE_BYTES,
        )?;
        validate_positive_bounded(self.max_scan_entries, MAX_SCAN_ENTRIES)?;
        validate_positive_bounded(self.max_rust_sources, MAX_RUST_SOURCES)?;
        validate_positive_bounded(self.max_traversal_depth, MAX_TRAVERSAL_DEPTH)?;
        validate_positive_bounded(self.max_requests, MAX_REQUESTS)?;

        Ok(ValidatedPolicy {
            canonical_allowed_root: canonical_allowed_root(&self.allowed_root)?,
            work_limits: WorkLimits {
                max_scan_entries: self.max_scan_entries,
                max_rust_sources: self.max_rust_sources,
                max_traversal_depth: self.max_traversal_depth,
            },
        })
    }
}

pub fn server_config_from_environment() -> Result<McpServerConfig, McpError> {
    let allowed_root = env::var_os("CODEDB_MCP_ALLOWED_ROOT")
        .map(PathBuf::from)
        .ok_or(McpError::InvalidConfiguration)?;
    let mut config = McpServerConfig::new(allowed_root);

    if let Some(value) = env::var_os("CODEDB_MCP_TRANSPORT") {
        config.transport = value
            .into_string()
            .map_err(|_| McpError::InvalidConfiguration)?;
    }
    set_environment_usize(
        "CODEDB_MCP_DEFAULT_ROW_LIMIT",
        &mut config.default_row_limit,
    )?;
    set_environment_usize("CODEDB_MCP_MAX_ROW_LIMIT", &mut config.max_row_limit)?;
    set_environment_usize(
        "CODEDB_MCP_DEFAULT_MAX_BYTES",
        &mut config.default_max_bytes,
    )?;
    set_environment_usize(
        "CODEDB_MCP_MAX_RESPONSE_BYTES",
        &mut config.max_response_bytes,
    )?;
    set_environment_usize("CODEDB_MCP_MAX_SCAN_ENTRIES", &mut config.max_scan_entries)?;
    set_environment_usize("CODEDB_MCP_MAX_RUST_SOURCES", &mut config.max_rust_sources)?;
    set_environment_usize(
        "CODEDB_MCP_MAX_TRAVERSAL_DEPTH",
        &mut config.max_traversal_depth,
    )?;
    set_environment_usize("CODEDB_MCP_MAX_REQUESTS", &mut config.max_requests)?;

    if let Some(value) = env::var_os("CODEDB_MCP_RAW_SOURCE_ENABLED") {
        config.raw_source_enabled = match value
            .into_string()
            .map_err(|_| McpError::InvalidConfiguration)?
            .as_str()
        {
            "0" | "false" | "FALSE" => false,
            "1" | "true" | "TRUE" => true,
            _ => return Err(McpError::InvalidConfiguration),
        };
    }
    config.validate()?;
    Ok(config)
}

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
                ("reason", "policy denied".to_string()),
            ])
        })
        .collect()
}

pub fn lifecycle_start(config: &McpServerConfig) -> Result<McpLifecycleReport, McpError> {
    config.validate()?;
    Ok(McpLifecycleReport {
        phase: "startup".to_string(),
        status: "ready".to_string(),
        transport: DEFAULT_TRANSPORT.to_string(),
        bounded_defaults: true,
        raw_source_enabled: false,
        note: "stdio read-only server is configured with compiled request and work bounds"
            .to_string(),
    })
}

pub fn lifecycle_shutdown(_config: &McpServerConfig) -> McpLifecycleReport {
    McpLifecycleReport {
        phase: "shutdown".to_string(),
        status: "stopped".to_string(),
        transport: DEFAULT_TRANSPORT.to_string(),
        bounded_defaults: true,
        raw_source_enabled: false,
        note: "EOF shuts down the stdio server without repository mutation".to_string(),
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
            ("transport", DEFAULT_TRANSPORT.to_string()),
            ("default_row_limit", config.default_row_limit.to_string()),
            ("max_row_limit", config.max_row_limit.to_string()),
            ("default_max_bytes", config.default_max_bytes.to_string()),
            ("max_response_bytes", config.max_response_bytes.to_string()),
            ("raw_source_enabled", "false".to_string()),
        ]),
    ])
}

pub trait ReadOnlyBackend {
    fn read(
        &self,
        tool: &str,
        table: Option<&str>,
        repo_path: Option<&Path>,
        limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StaticAnalysisBackend;

impl ReadOnlyBackend for StaticAnalysisBackend {
    fn read(
        &self,
        tool: &str,
        table: Option<&str>,
        repo_path: Option<&Path>,
        limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError> {
        match tool {
            "codedb_schema" => Ok(table_rows(schema_rows())),
            "codedb_list_tables" => Ok(table_rows(table_inventory())),
            "codedb_get_capture_gaps" => Ok(table_rows(capture_gaps())),
            "codedb_get_validation_errors" => Ok(table_rows(validation_errors())),
            "codedb_get_table_page" => static_table_page_rows(table, repo_path, limits),
            "codedb_get_repo_summary" => repo_summary_rows(required_repo_path(repo_path)?, limits),
            "codedb_get_cargo_summary" => {
                cargo_summary_rows(required_repo_path(repo_path)?, limits)
            }
            "codedb_get_rust_item_summary" => {
                rust_item_summary_rows(required_repo_path(repo_path)?, limits)
            }
            "codedb_get_macro_summary" => {
                macro_summary_rows(required_repo_path(repo_path)?, limits)
            }
            "codedb_get_build_script_summary" => {
                build_script_summary_rows(required_repo_path(repo_path)?, limits)
            }
            "codedb_get_no_mutation_proof" => {
                no_mutation_rows(required_repo_path(repo_path)?, limits)
            }
            _ => Err(McpError::UnknownTool),
        }
    }
}

pub fn handle_request(
    config: &McpServerConfig,
    request: McpRequest,
) -> Result<McpResponse, McpError> {
    handle_request_with_backend(config, &StaticAnalysisBackend, request)
}

pub fn handle_request_with_backend<B: ReadOnlyBackend + ?Sized>(
    config: &McpServerConfig,
    backend: &B,
    request: McpRequest,
) -> Result<McpResponse, McpError> {
    let policy = config.policy()?;
    ensure_tool_allowed(&request.tool)?;
    if request.table.as_deref().is_some_and(is_blocked_table) {
        return Err(McpError::RawSourceDisabled);
    }

    let limit = request.limit.unwrap_or(config.default_row_limit);
    if limit == 0 || limit > config.max_row_limit || limit > MAX_ROW_LIMIT {
        return Err(McpError::BoundExceeded);
    }
    let max_bytes = request.max_bytes.unwrap_or(config.default_max_bytes);
    if !(MIN_RESPONSE_BYTES..=config.max_response_bytes).contains(&max_bytes)
        || max_bytes > MAX_RESPONSE_BYTES
    {
        return Err(McpError::BoundExceeded);
    }
    let cursor = request.cursor.unwrap_or(0);
    if cursor > MAX_CURSOR {
        return Err(McpError::BoundExceeded);
    }

    let repo_path = request
        .repo_path
        .as_deref()
        .map(|path| canonical_request_path(path, &policy.canonical_allowed_root))
        .transpose()?;
    if tool_requires_repo_path(&request.tool) && repo_path.is_none() {
        return Err(McpError::MissingRepoPath);
    }

    let rows = if request.tool == "codedb_get_table_page"
        && request
            .table
            .as_deref()
            .is_some_and(|table| matches!(table, "mcp_lifecycle" | "mcp_config"))
    {
        lifecycle_rows(config)?
    } else {
        backend.read(
            &request.tool,
            request.table.as_deref(),
            repo_path.as_deref(),
            policy.work_limits,
        )?
    };

    bound_response(request.tool, rows, cursor, limit, max_bytes)
}

pub fn ensure_tool_allowed(tool: &str) -> Result<(), McpError> {
    if BLOCKED_TOOLS.contains(&tool) {
        return Err(McpError::BlockedTool);
    }
    if !ALLOWED_TOOLS.contains(&tool) {
        return Err(McpError::UnknownTool);
    }
    Ok(())
}

pub fn serve_json_rpc<R: BufRead, W: Write>(
    config: &McpServerConfig,
    reader: &mut R,
    writer: &mut W,
) -> Result<ServerReport, McpError> {
    config.validate()?;

    let mut state = ServerState::AwaitingInitialize;
    let mut requests = 0usize;
    let mut line = Vec::new();
    loop {
        match read_bounded_line(reader, &mut line).map_err(|_| McpError::IoFailure)? {
            ReadLine::Eof => {
                writer.flush().map_err(|_| McpError::IoFailure)?;
                return Ok(ServerReport {
                    status: "eof_shutdown".to_string(),
                    requests,
                });
            }
            ReadLine::TooLong => {
                requests = requests.saturating_add(1);
                if requests > config.max_requests {
                    write_json_rpc_error(writer, None, -32000, McpError::WorkLimitExceeded)?;
                    writer.flush().map_err(|_| McpError::IoFailure)?;
                    return Ok(ServerReport {
                        status: "request_limit_shutdown".to_string(),
                        requests,
                    });
                }
                write_json_rpc_error(writer, None, -32600, McpError::BoundExceeded)?;
                continue;
            }
            ReadLine::Line => {}
        }

        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        requests = requests.saturating_add(1);
        if requests > config.max_requests {
            write_json_rpc_error(writer, None, -32000, McpError::WorkLimitExceeded)?;
            writer.flush().map_err(|_| McpError::IoFailure)?;
            return Ok(ServerReport {
                status: "request_limit_shutdown".to_string(),
                requests,
            });
        }

        let request = match serde_json::from_slice::<JsonRpcRequest>(&line) {
            Ok(request) if request.jsonrpc == "2.0" => request,
            _ => {
                write_json_rpc_error(writer, None, -32600, McpError::ProtocolViolation)?;
                continue;
            }
        };
        if request
            .id
            .as_ref()
            .is_some_and(|id| !matches!(id, Value::Null | Value::String(_) | Value::Number(_)))
        {
            write_json_rpc_error(writer, None, -32600, McpError::ProtocolViolation)?;
            continue;
        }
        let id = request.id.clone();
        let outcome = process_json_rpc_request(config, &mut state, request);
        if let Some(id) = id {
            match outcome {
                Ok(result) => write_json_rpc_result(writer, id, result)?,
                Err(error) => write_json_rpc_error(writer, Some(id), rpc_error_code(error), error)?,
            }
        }
    }
}

pub fn run_stdio(config: &McpServerConfig) -> Result<ServerReport, McpError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    serve_json_rpc(config, &mut reader, &mut writer)
}

fn process_json_rpc_request(
    config: &McpServerConfig,
    state: &mut ServerState,
    request: JsonRpcRequest,
) -> Result<Value, McpError> {
    match request.method.as_str() {
        "initialize" => {
            if *state != ServerState::AwaitingInitialize {
                return Err(McpError::ProtocolViolation);
            }
            *state = ServerState::Ready;
            Ok(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "codedb-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false,
                    },
                },
            }))
        }
        "tools/list" => {
            require_ready(*state)?;
            Ok(json!({ "tools": json_rpc_tools() }))
        }
        "tools/call" => {
            require_ready(*state)?;
            let request = tool_request(request.params)?;
            let response = handle_request(config, request)?;
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": "bounded read-only result",
                }],
                "structuredContent": response,
            }))
        }
        _ => Err(McpError::UnknownTool),
    }
}

fn json_rpc_tools() -> Vec<Value> {
    ALLOWED_TOOLS
        .iter()
        .map(|name| {
            json!({
                "name": name,
                "description": "Bounded read-only CodeDB summary.",
                "inputSchema": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "repo_path": { "type": "string" },
                        "table": { "type": "string" },
                        "cursor": { "type": "integer", "minimum": 0 },
                        "limit": { "type": "integer", "minimum": 1, "maximum": MAX_ROW_LIMIT },
                        "max_bytes": {
                            "type": "integer",
                            "minimum": MIN_RESPONSE_BYTES,
                            "maximum": MAX_RESPONSE_BYTES,
                        },
                    },
                },
            })
        })
        .collect()
}

fn tool_request(params: Option<Value>) -> Result<McpRequest, McpError> {
    let params = params.ok_or(McpError::ProtocolViolation)?;
    let call: JsonRpcToolCall =
        serde_json::from_value(params).map_err(|_| McpError::ProtocolViolation)?;
    let arguments = call.arguments.unwrap_or_default();
    Ok(McpRequest {
        tool: call.name,
        repo_path: arguments.repo_path.map(PathBuf::from),
        table: arguments.table,
        cursor: arguments.cursor,
        limit: arguments.limit,
        max_bytes: arguments.max_bytes,
    })
}

fn write_json_rpc_result<W: Write>(
    writer: &mut W,
    id: Value,
    result: Value,
) -> Result<(), McpError> {
    write_json_line(
        writer,
        json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
}

fn write_json_rpc_error<W: Write>(
    writer: &mut W,
    id: Option<Value>,
    code: i64,
    error: McpError,
) -> Result<(), McpError> {
    write_json_line(
        writer,
        json!({
            "jsonrpc": "2.0",
            "id": id.unwrap_or(Value::Null),
            "error": {
                "code": code,
                "message": error.to_string(),
                "data": { "reason": error.code() },
            },
        }),
    )
}

fn write_json_line<W: Write>(writer: &mut W, value: Value) -> Result<(), McpError> {
    serde_json::to_writer(&mut *writer, &value).map_err(|_| McpError::IoFailure)?;
    writer.write_all(b"\n").map_err(|_| McpError::IoFailure)
}

fn rpc_error_code(error: McpError) -> i64 {
    match error {
        McpError::UnknownTool => -32601,
        McpError::ProtocolViolation | McpError::MissingRepoPath | McpError::MissingTable => -32602,
        _ => -32000,
    }
}

fn require_ready(state: ServerState) -> Result<(), McpError> {
    if state == ServerState::Ready {
        Ok(())
    } else {
        Err(McpError::ProtocolViolation)
    }
}

fn bound_response(
    tool: String,
    rows: Vec<Row>,
    cursor: usize,
    limit: usize,
    max_bytes: usize,
) -> Result<McpResponse, McpError> {
    let available = rows.len().saturating_sub(cursor);
    let target_count = available.min(limit);
    let mut selected = Vec::new();

    for row in rows.iter().skip(cursor).take(target_count) {
        let mut candidate = selected.clone();
        candidate.push(row.clone());
        let has_more = candidate.len() < available;
        let candidate_response =
            response(tool.clone(), candidate, cursor, limit, max_bytes, has_more);
        let encoded =
            serde_json::to_vec(&candidate_response).map_err(|_| McpError::BackendFailure)?;
        if encoded.len() > max_bytes {
            if selected.is_empty() {
                return Err(McpError::ResponseBudgetTooSmall);
            }
            break;
        }
        selected.push(row.clone());
    }

    let truncated = selected.len() < available;
    let response = response(tool, selected, cursor, limit, max_bytes, truncated);
    let encoded = serde_json::to_vec(&response).map_err(|_| McpError::BackendFailure)?;
    if encoded.len() > max_bytes {
        return Err(McpError::ResponseBudgetTooSmall);
    }
    Ok(response)
}

fn response(
    tool: String,
    rows: Vec<Row>,
    cursor: usize,
    limit: usize,
    max_bytes: usize,
    truncated: bool,
) -> McpResponse {
    let next = cursor.saturating_add(rows.len());
    McpResponse {
        tool,
        status: "ok".to_string(),
        cursor,
        next_cursor: truncated.then_some(next),
        limit,
        max_bytes,
        truncated,
        rows,
        errors: Vec::new(),
    }
}

fn static_table_page_rows(
    table: Option<&str>,
    repo_path: Option<&Path>,
    limits: WorkLimits,
) -> Result<Vec<Row>, McpError> {
    match table.ok_or(McpError::MissingTable)? {
        "schema" | "schema_versions" => Ok(table_rows(schema_rows())),
        "tables" => Ok(table_rows(table_inventory())),
        "capture_gaps" | "gaps" => Ok(table_rows(capture_gaps())),
        "validation_errors" | "validation-errors" => Ok(table_rows(validation_errors())),
        "repo_summary" | "filesystem_entries" => {
            repo_summary_rows(required_repo_path(repo_path)?, limits)
        }
        "cargo_summary" | "cargo_packages" => {
            cargo_summary_rows(required_repo_path(repo_path)?, limits)
        }
        "rust_item_summary" | "rust_items" => {
            rust_item_summary_rows(required_repo_path(repo_path)?, limits)
        }
        "macro_summary" | "rust_macros" => {
            macro_summary_rows(required_repo_path(repo_path)?, limits)
        }
        "build_script_summary" | "build_scripts" => {
            build_script_summary_rows(required_repo_path(repo_path)?, limits)
        }
        "mcp_lifecycle" | "mcp_config" => Err(McpError::BackendFailure),
        _ => Ok(vec![row([
            ("table", "validation_errors".to_string()),
            ("code", "unsupported_table".to_string()),
            (
                "message",
                "requested table is not available through MCP".to_string(),
            ),
        ])]),
    }
}

fn repo_summary_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let inventory = repository_inventory(repo_path, limits)?;
    Ok(vec![
        summary_row(
            "filesystem_entries",
            inventory.entries,
            "bounded metadata scan",
        ),
        summary_row(
            "rust_sources",
            inventory.rust_sources.len(),
            "source paths only; no raw bytes",
        ),
    ])
}

fn cargo_summary_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let _ = repository_inventory(repo_path, limits)?;
    let target_triple = detect_host_triple().map_err(|_| McpError::BackendFailure)?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(|_| McpError::BackendFailure)?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json)
        .map_err(|_| McpError::BackendFailure)?;
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

fn rust_item_summary_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let inventory = repository_inventory(repo_path, limits)?;
    let mut count = 0usize;
    for source_path in inventory.rust_sources {
        count = count.saturating_add(
            capture_rust_items(repo_path, &source_path, "mcp-static")
                .map_err(|_| McpError::BackendFailure)?
                .len(),
        );
    }
    Ok(vec![summary_row(
        "rust_items",
        count,
        "static syntax item rows",
    )])
}

fn macro_summary_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let inventory = repository_inventory(repo_path, limits)?;
    let mut definitions = 0usize;
    let mut invocations = 0usize;
    let mut gaps = 0usize;
    for source_path in inventory.rust_sources {
        let captured = capture_rust_macros(repo_path, &source_path, "mcp-static")
            .map_err(|_| McpError::BackendFailure)?;
        definitions = definitions.saturating_add(captured.definitions.len());
        invocations = invocations.saturating_add(captured.invocations.len());
        gaps = gaps.saturating_add(captured.gaps.len());
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

fn build_script_summary_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let inventory = repository_inventory(repo_path, limits)?;
    let mut scripts = 0usize;
    let mut instructions = 0usize;
    let mut gaps = 0usize;
    for source_path in inventory.rust_sources {
        if source_path.file_name().and_then(|name| name.to_str()) != Some("build.rs") {
            continue;
        }
        let captured = capture_build_script_static(repo_path, &source_path, "mcp-static")
            .map_err(|_| McpError::BackendFailure)?;
        scripts = scripts.saturating_add(captured.scripts.len());
        instructions = instructions.saturating_add(captured.instructions.len());
        gaps = gaps.saturating_add(captured.gaps.len());
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

fn no_mutation_rows(repo_path: &Path, limits: WorkLimits) -> Result<Vec<Row>, McpError> {
    let proof = prove_no_mutation(repo_path, "codedb_mcp_read_only_summary", || {
        let _ = repository_inventory(repo_path, limits);
    })
    .map_err(|_| McpError::BackendFailure)?;
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

fn repository_inventory(root: &Path, limits: WorkLimits) -> Result<RepositoryInventory, McpError> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    let mut rust_sources = Vec::new();

    while let Some((directory, depth)) = stack.pop() {
        if depth > limits.max_traversal_depth {
            return Err(McpError::WorkLimitExceeded);
        }
        let children = fs::read_dir(&directory).map_err(|_| McpError::BackendFailure)?;
        for child in children {
            let child = child.map_err(|_| McpError::BackendFailure)?;
            entries = entries.saturating_add(1);
            if entries > limits.max_scan_entries {
                return Err(McpError::WorkLimitExceeded);
            }
            let path = child.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| McpError::BackendFailure)?;
            let kind = metadata.file_type();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if depth.saturating_add(1) > limits.max_traversal_depth {
                    return Err(McpError::WorkLimitExceeded);
                }
                stack.push((path, depth + 1));
                continue;
            }
            if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
                rust_sources.push(path);
                if rust_sources.len() > limits.max_rust_sources {
                    return Err(McpError::WorkLimitExceeded);
                }
            }
        }
    }

    rust_sources.sort();
    Ok(RepositoryInventory {
        entries,
        rust_sources,
    })
}

fn canonical_allowed_root(path: &Path) -> Result<PathBuf, McpError> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err(McpError::InvalidConfiguration);
    }
    reject_symlink_components(path, McpError::InvalidConfiguration)?;
    let canonical = fs::canonicalize(path).map_err(|_| McpError::InvalidConfiguration)?;
    if canonical == Path::new("/") || !canonical.is_dir() || canonical != path {
        return Err(McpError::InvalidConfiguration);
    }
    Ok(canonical)
}

fn canonical_request_path(path: &Path, allowed_root: &Path) -> Result<PathBuf, McpError> {
    if !path.is_absolute() || path == Path::new("/") {
        return Err(McpError::InvalidRepositoryPath);
    }
    reject_symlink_components(path, McpError::InvalidRepositoryPath)?;
    let canonical = fs::canonicalize(path).map_err(|_| McpError::InvalidRepositoryPath)?;
    if canonical == Path::new("/")
        || !canonical.is_dir()
        || canonical != path
        || !canonical.starts_with(allowed_root)
    {
        return Err(McpError::InvalidRepositoryPath);
    }
    Ok(canonical)
}

fn reject_symlink_components(path: &Path, error: McpError) -> Result<(), McpError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => current.push(".."),
            Component::Normal(segment) => current.push(segment),
        }
        if current == Path::new("/") {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|_| error)?;
        if metadata.file_type().is_symlink() {
            return Err(error);
        }
    }
    Ok(())
}

fn validate_range(
    default: usize,
    configured_max: usize,
    compiled_max: usize,
    minimum: usize,
) -> Result<(), McpError> {
    if default < minimum
        || configured_max < minimum
        || default > configured_max
        || configured_max > compiled_max
    {
        return Err(McpError::InvalidConfiguration);
    }
    Ok(())
}

fn validate_positive_bounded(value: usize, compiled_max: usize) -> Result<(), McpError> {
    if value == 0 || value > compiled_max {
        return Err(McpError::InvalidConfiguration);
    }
    Ok(())
}

fn set_environment_usize(key: &str, destination: &mut usize) -> Result<(), McpError> {
    let Some(value) = env::var_os(key) else {
        return Ok(());
    };
    *destination = value
        .into_string()
        .map_err(|_| McpError::InvalidConfiguration)?
        .parse()
        .map_err(|_| McpError::InvalidConfiguration)?;
    Ok(())
}

fn is_blocked_table(table: &str) -> bool {
    BLOCKED_TABLES.contains(&table)
}

fn tool_requires_repo_path(tool: &str) -> bool {
    matches!(
        tool,
        "codedb_get_repo_summary"
            | "codedb_get_cargo_summary"
            | "codedb_get_rust_item_summary"
            | "codedb_get_macro_summary"
            | "codedb_get_build_script_summary"
            | "codedb_get_no_mutation_proof"
    )
}

fn required_repo_path(repo_path: Option<&Path>) -> Result<&Path, McpError> {
    repo_path.ok_or(McpError::MissingRepoPath)
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

fn row<const N: usize>(pairs: [(&str, String); N]) -> Row {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

#[derive(Debug)]
struct RepositoryInventory {
    entries: usize,
    rust_sources: Vec<PathBuf>,
}

#[derive(Debug)]
struct ValidatedPolicy {
    canonical_allowed_root: PathBuf,
    work_limits: WorkLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerState {
    AwaitingInitialize,
    Ready,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonRpcToolCall {
    name: String,
    #[serde(default)]
    arguments: Option<JsonRpcToolArguments>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonRpcToolArguments {
    repo_path: Option<String>,
    table: Option<String>,
    cursor: Option<usize>,
    limit: Option<usize>,
    max_bytes: Option<usize>,
}

enum ReadLine {
    Eof,
    Line,
    TooLong,
}

fn read_bounded_line<R: BufRead>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<ReadLine> {
    line.clear();
    let mut too_long = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return if line.is_empty() && !too_long {
                Ok(ReadLine::Eof)
            } else if too_long {
                Ok(ReadLine::TooLong)
            } else {
                Ok(ReadLine::Line)
            };
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(buffer.len(), |index| index + 1);
        let chunk = &buffer[..take];
        let data = if newline.is_some() {
            &chunk[..chunk.len().saturating_sub(1)]
        } else {
            chunk
        };
        if !too_long {
            if line.len().saturating_add(data.len()) > MAX_JSON_RPC_LINE_BYTES {
                too_long = true;
            } else {
                line.extend_from_slice(data);
            }
        }
        reader.consume(take);
        if newline.is_some() {
            return if too_long {
                Ok(ReadLine::TooLong)
            } else {
                Ok(ReadLine::Line)
            };
        }
    }
}

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_mcp::{
    ALLOWED_TOOLS, BLOCKED_TOOLS, DEFAULT_MAX_BYTES, MAX_RESPONSE_BYTES, MAX_ROW_LIMIT, McpError,
    McpRequest, McpServerConfig, ReadOnlyBackend, Row, WorkLimits, ensure_tool_allowed,
    handle_request, handle_request_with_backend, serve_json_rpc,
};

#[test]
fn configuration_is_fail_closed_and_cannot_expand_compiled_maxima() {
    let root = temp_dir("configuration");
    let mut config = McpServerConfig::new(root.clone());

    assert!(config.validate().is_ok());

    config.transport = "tcp".to_string();
    assert!(matches!(
        config.validate(),
        Err(McpError::InvalidConfiguration)
    ));
    config.transport = "stdio".to_string();

    config.raw_source_enabled = true;
    assert!(matches!(
        config.validate(),
        Err(McpError::RawSourceDisabled)
    ));
    config.raw_source_enabled = false;

    config.max_row_limit = MAX_ROW_LIMIT + 1;
    assert!(matches!(
        config.validate(),
        Err(McpError::InvalidConfiguration)
    ));
    config.max_row_limit = MAX_ROW_LIMIT;

    config.max_response_bytes = MAX_RESPONSE_BYTES + 1;
    assert!(matches!(
        config.validate(),
        Err(McpError::InvalidConfiguration)
    ));

    assert!(matches!(
        McpServerConfig::new(PathBuf::from("relative-root")).validate(),
        Err(McpError::InvalidConfiguration)
    ));
    assert!(matches!(
        McpServerConfig::new(PathBuf::from("/")).validate(),
        Err(McpError::InvalidConfiguration)
    ));

    #[cfg(unix)]
    {
        let root_link = temp_dir("allowed-root-link");
        std::os::unix::fs::symlink(&root, root_link.join("link")).expect("create root link");
        assert!(matches!(
            McpServerConfig::new(root_link.join("link")).validate(),
            Err(McpError::InvalidConfiguration)
        ));
        remove_dir(root_link);
    }

    remove_dir(root);
}

#[test]
fn repository_paths_are_canonical_absolute_contained_and_nonsymlinked() {
    let root = temp_dir("allowed-root");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repository");
    let outside = temp_dir("outside-root");
    let config = McpServerConfig::new(root.clone());

    for path in [PathBuf::from("repo"), PathBuf::from("/"), outside.clone()] {
        let error = handle_request(
            &config,
            request("codedb_get_repo_summary", Some(path), None),
        )
        .expect_err("unsafe path must fail closed");
        assert!(matches!(error, McpError::InvalidRepositoryPath));
        assert!(!error.to_string().contains("outside-root"));
    }

    #[cfg(unix)]
    {
        let link = root.join("repo-link");
        std::os::unix::fs::symlink(&repo, &link).expect("create symlink");
        assert!(matches!(
            handle_request(
                &config,
                request("codedb_get_repo_summary", Some(link), None)
            ),
            Err(McpError::InvalidRepositoryPath)
        ));
    }

    remove_dir(outside);
    remove_dir(root);
}

#[test]
fn requests_cannot_exceed_configured_or_compiled_row_and_byte_budgets() {
    let root = temp_dir("request-bounds");
    let mut config = McpServerConfig::new(root.clone());
    config.max_row_limit = 2;
    config.default_row_limit = 1;
    config.max_response_bytes = 1_024;
    config.default_max_bytes = 1_024;

    for request in [
        McpRequest {
            limit: Some(3),
            ..request("codedb_list_tables", None, None)
        },
        McpRequest {
            max_bytes: Some(1_025),
            ..request("codedb_list_tables", None, None)
        },
    ] {
        assert!(matches!(
            handle_request(&config, request),
            Err(McpError::BoundExceeded)
        ));
    }

    remove_dir(root);
}

#[test]
fn bounded_traversal_stops_before_unbounded_repository_work() {
    let root = temp_dir("work-bounds");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repo");
    for index in 0..3 {
        fs::write(repo.join(format!("file-{index}.txt")), "metadata only").expect("write file");
    }
    let mut config = McpServerConfig::new(root.clone());
    config.max_scan_entries = 2;

    assert!(matches!(
        handle_request(
            &config,
            request("codedb_get_repo_summary", Some(repo), None)
        ),
        Err(McpError::WorkLimitExceeded)
    ));

    remove_dir(root);
}

#[test]
// Defends CDB083: raw source/blob aliases return only bounded policy evidence.
fn raw_source_and_blob_tools_and_tables_are_denied_without_backend_access() {
    let root = temp_dir("raw-source");
    let config = McpServerConfig::new(root.clone());
    let backend = FixtureBackend::default();

    for tool in BLOCKED_TOOLS {
        let error = handle_request_with_backend(
            &config,
            &backend,
            request(tool, Some(root.join("not-a-secret")), None),
        )
        .expect_err("raw, mutating, dynamic, and unbounded tools must remain disabled");
        assert!(matches!(error, McpError::BlockedTool));
        assert_eq!(
            error.to_string(),
            "requested operation is disabled by policy"
        );
    }

    for table in [
        "source_blobs",
        "artifact_blobs",
        "blob_refs",
        "raw_source",
        "raw_blobs",
    ] {
        let response = handle_request_with_backend(
            &config,
            &backend,
            request("codedb_get_table_page", None, Some(table.to_string())),
        )
        .expect("blocked tables return a bounded denial row");
        assert_eq!(response.rows.len(), 1);
        assert_eq!(
            response.rows[0].get("table").map(String::as_str),
            Some("validation_errors")
        );
        assert_eq!(
            response.rows[0].get("code").map(String::as_str),
            Some("raw_blob_table_blocked")
        );
        assert!(
            !serde_json::to_string(&response)
                .expect("serialize denial")
                .contains("not-a-secret")
        );
    }

    assert!(backend.calls.lock().expect("calls").is_empty());
    remove_dir(root);
}

#[test]
fn backend_boundary_is_read_only_and_backend_neutral() {
    let root = temp_dir("backend-neutral");
    let config = McpServerConfig::new(root.clone());
    let backend = FixtureBackend::default();

    let response =
        handle_request_with_backend(&config, &backend, request("codedb_list_tables", None, None))
            .expect("fixture backend response");

    assert_eq!(response.rows.len(), 1);
    assert_eq!(
        response.rows[0].get("backend"),
        Some(&"fixture".to_string())
    );
    assert_eq!(
        backend.calls.lock().expect("calls").as_slice(),
        ["codedb_list_tables"]
    );
    remove_dir(root);
}

#[test]
// Defends CDB090 and REQ-061: pagination is finite, lossless, and byte bounded.
fn pagination_is_contiguous_bounded_non_overlapping_and_terminal() {
    let root = temp_dir("pagination");
    let config = McpServerConfig::new(root.clone());
    let backend = PagedBackend;
    let mut cursor = 0usize;
    let mut observed = Vec::new();

    loop {
        let response = handle_request_with_backend(
            &config,
            &backend,
            McpRequest {
                cursor: Some(cursor),
                limit: Some(2),
                max_bytes: Some(2_048),
                ..request("codedb_list_tables", None, None)
            },
        )
        .expect("bounded page");

        assert!(response.rows.len() <= 2);
        assert!(
            serde_json::to_vec(&response)
                .expect("serialize response")
                .len()
                <= 2_048
        );
        observed.extend(
            response
                .rows
                .iter()
                .map(|row| row.get("index").expect("index").clone()),
        );

        match response.next_cursor {
            Some(next_cursor) => {
                assert!(response.truncated);
                assert_eq!(next_cursor, cursor + response.rows.len());
                assert!(next_cursor > cursor);
                cursor = next_cursor;
            }
            None => {
                assert!(!response.truncated);
                break;
            }
        }
    }

    assert_eq!(observed, ["0", "1", "2", "3", "4"]);
    remove_dir(root);
}

#[test]
// Defends CDB090 and REQ-061: MCP cannot apply, approve, deploy, or execute.
fn mcp_tool_surface_has_no_mutation_or_dynamic_execution_entrypoint() {
    let forbidden_fragments = [
        "apply",
        "approve",
        "deploy",
        "execute",
        "overwrite",
        "patch",
        "raw",
        "restore",
        "run",
        "sync",
        "unsafe",
        "write",
    ];

    for tool in ALLOWED_TOOLS {
        assert!(ensure_tool_allowed(tool).is_ok());
        for fragment in forbidden_fragments {
            assert!(
                !tool.contains(fragment),
                "allowed MCP tool {tool} contains forbidden operation fragment {fragment}"
            );
        }
    }
    for tool in BLOCKED_TOOLS {
        assert!(matches!(
            ensure_tool_allowed(tool),
            Err(McpError::BlockedTool)
        ));
    }

    for required_denial in [
        "raw_source_blob_read",
        "codedb_get_raw_source",
        "codedb_get_raw_blob",
        "unsafe_build_capture",
        "codedb_execute_build_script",
        "codedb_execute_proc_macro",
        "source_overwrite",
        "patch_apply",
        "git_mutation",
        "codedb_apply",
        "codedb_approve",
        "codedb_deploy",
        "codedb_execute",
        "codedb_refactor_apply",
        "codedb_restore",
        "codedb_sync_bidirectional",
        "codedb_write",
        "unbounded_table_dump",
    ] {
        assert!(
            BLOCKED_TOOLS.contains(&required_denial),
            "mandatory denial alias is missing: {required_denial}"
        );
    }
}

#[test]
fn in_process_stdio_lifecycle_emits_only_json_rpc_messages_and_shuts_down_on_eof() {
    let root = temp_dir("in-process-stdio");
    let config = McpServerConfig::new(root.clone());
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"codedb_list_tables\",\"arguments\":{\"limit\":1,\"max_bytes\":2048}}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"raw_source_read\",\"arguments\":{}}}\n"
    );
    let mut output = Vec::new();

    let report = serve_json_rpc(&config, &mut Cursor::new(input.as_bytes()), &mut output)
        .expect("serve protocol");

    assert_eq!(report.status, "eof_shutdown");
    assert_eq!(report.requests, 4);
    let stdout = String::from_utf8(output).expect("utf-8 json-rpc output");
    let messages = stdout.lines().collect::<Vec<_>>();
    assert_eq!(messages.len(), 4);
    for message in &messages {
        let value: serde_json::Value = serde_json::from_str(message).expect("json-rpc message");
        assert_eq!(value["jsonrpc"], "2.0");
        assert!(!message.contains("codedb-mcp:"));
    }
    assert!(messages[3].contains("requested operation is disabled by policy"));

    remove_dir(root);
}

#[test]
// Defends CDB090: stdio replies are observable before the client closes stdin.
fn stdio_flushes_each_response_and_accepts_initialized_notification() {
    let root = temp_dir("stdio-flush");
    let config = McpServerConfig::new(root.clone());
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n"
    );
    let mut output = FlushCountingWriter::default();

    let report = serve_json_rpc(&config, &mut Cursor::new(input.as_bytes()), &mut output)
        .expect("serve protocol");

    assert_eq!(report.status, "eof_shutdown");
    assert_eq!(report.requests, 3);
    assert_eq!(
        String::from_utf8(output.bytes)
            .expect("utf-8")
            .lines()
            .count(),
        2
    );
    assert!(
        output.flushes >= 3,
        "each response and EOF shutdown must flush stdio"
    );
    remove_dir(root);
}

#[test]
fn stdio_request_budget_shuts_down_fail_closed() {
    let root = temp_dir("stdio-request-budget");
    let mut config = McpServerConfig::new(root.clone());
    config.max_requests = 2;
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/list\",\"params\":{}}\n"
    );
    let mut output = Vec::new();

    let report = serve_json_rpc(&config, &mut Cursor::new(input.as_bytes()), &mut output)
        .expect("bounded shutdown");

    assert_eq!(report.status, "request_limit_shutdown");
    assert_eq!(report.requests, 3);
    let messages = String::from_utf8(output).expect("utf-8");
    assert_eq!(messages.lines().count(), 3);
    assert!(messages.contains("repository work exceeds a configured safety bound"));
    assert!(!messages.contains("\"id\":4"));
    remove_dir(root);
}

#[test]
// Defends CDB083/CDB090: untrusted request values never appear in error text.
fn stdio_errors_do_not_echo_secret_bearing_tool_paths_or_arguments() {
    let root = temp_dir("secret-safe-errors");
    let config = McpServerConfig::new(root.clone());
    let sentinel = "CODEDB_MCP_SECRET_SENTINEL_41f9";
    let input = format!(
        concat!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}\n",
            "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":",
            "{{\"name\":\"unknown_{sentinel}\",\"arguments\":{{}}}}}}\n",
            "{{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":",
            "{{\"name\":\"codedb_get_repo_summary\",\"arguments\":",
            "{{\"repo_path\":\"/outside/{sentinel}\"}}}}}}\n",
            "{{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":",
            "{{\"name\":\"codedb_list_tables\",\"arguments\":",
            "{{\"password\":\"{sentinel}\"}}}}}}\n"
        ),
        sentinel = sentinel
    );
    let mut output = Vec::new();

    serve_json_rpc(&config, &mut Cursor::new(input.as_bytes()), &mut output)
        .expect("serve protocol");

    let stdout = String::from_utf8(output).expect("utf-8");
    assert!(!stdout.contains(sentinel));
    assert!(stdout.contains("requested tool is not available"));
    assert!(stdout.contains("repository path is not permitted"));
    assert!(stdout.contains("request is invalid"));
    remove_dir(root);
}

#[test]
fn packaged_stdio_binary_handles_initialize_tools_and_eof_without_stdout_diagnostics() {
    let root = temp_dir("spawned-stdio");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"codedb_list_tables\",\"arguments\":{\"limit\":1,\"max_bytes\":2048}}}\n"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_codedb-mcp"))
        .env("CODEDB_MCP_ALLOWED_ROOT", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn packaged server");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write request stream");

    let output = child.wait_with_output().expect("wait for EOF shutdown");
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "stderr: {:?}", output.stderr);

    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let messages = stdout.lines().collect::<Vec<_>>();
    assert_eq!(messages.len(), 3);
    for message in messages {
        let value: serde_json::Value = serde_json::from_str(message).expect("json response");
        assert_eq!(value["jsonrpc"], "2.0");
        assert!(!message.contains("codedb-mcp:"));
    }

    remove_dir(root);
}

#[test]
fn packaged_binary_rejects_non_stdio_transport_without_writing_stdout() {
    let root = temp_dir("invalid-transport");
    let output = Command::new(env!("CARGO_BIN_EXE_codedb-mcp"))
        .env("CODEDB_MCP_ALLOWED_ROOT", &root)
        .env("CODEDB_MCP_TRANSPORT", "tcp")
        .output()
        .expect("run packaged server");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("utf-8 stderr");
    assert!(stderr.contains("server configuration is invalid"));
    assert!(!stderr.contains(&root.display().to_string()));
    remove_dir(root);
}

#[derive(Default)]
struct FixtureBackend {
    calls: Mutex<Vec<String>>,
}

impl ReadOnlyBackend for FixtureBackend {
    fn read(
        &self,
        tool: &str,
        _table: Option<&str>,
        _repo_path: Option<&Path>,
        _limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError> {
        self.calls.lock().expect("calls").push(tool.to_string());
        Ok(vec![BTreeMap::from([
            ("backend".to_string(), "fixture".to_string()),
            ("status".to_string(), "available".to_string()),
        ])])
    }
}

struct PagedBackend;

impl ReadOnlyBackend for PagedBackend {
    fn read(
        &self,
        _tool: &str,
        _table: Option<&str>,
        _repo_path: Option<&Path>,
        _limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError> {
        Ok((0..5)
            .map(|index| {
                BTreeMap::from([
                    ("index".to_string(), index.to_string()),
                    ("status".to_string(), "available".to_string()),
                ])
            })
            .collect())
    }
}

#[derive(Default)]
struct FlushCountingWriter {
    bytes: Vec<u8>,
    flushes: usize,
}

impl Write for FlushCountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

fn request(tool: &str, repo_path: Option<PathBuf>, table: Option<String>) -> McpRequest {
    McpRequest {
        tool: tool.to_string(),
        repo_path,
        table,
        cursor: Some(0),
        limit: Some(1),
        max_bytes: Some(DEFAULT_MAX_BYTES),
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("codedb_mcp_{label}_{suffix}"));
    fs::create_dir_all(&path).expect("create temp directory");
    path
}

fn remove_dir(path: PathBuf) {
    fs::remove_dir_all(path).expect("remove temp directory");
}

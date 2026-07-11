use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_mcp::{
    handle_request, handle_request_with_backend, serve_json_rpc, McpError, McpRequest,
    McpServerConfig, ReadOnlyBackend, Row, WorkLimits, DEFAULT_MAX_BYTES, MAX_RESPONSE_BYTES,
    MAX_ROW_LIMIT,
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
fn raw_source_is_blocked_with_a_sanitized_error() {
    let root = temp_dir("raw-source");
    let config = McpServerConfig::new(root.clone());

    let error = handle_request(
        &config,
        request(
            "codedb_get_table_page",
            None,
            Some("source_blobs".to_string()),
        ),
    )
    .expect_err("raw source must remain permanently disabled");

    assert!(matches!(error, McpError::RawSourceDisabled));
    assert_eq!(
        error.to_string(),
        "requested operation is disabled by policy"
    );
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

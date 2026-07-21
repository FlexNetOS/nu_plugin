use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use codedb_mcp::{
    ALLOWED_TOOLS, BLOCKED_TOOLS, DEFAULT_MAX_BYTES, MAX_REDB_STORE_BYTES, MAX_RESPONSE_BYTES,
    MAX_ROW_LIMIT, McpError, McpRequest, McpServerConfig, PersistedStoreConfig, ReadOnlyBackend,
    Row, StaticAnalysisBackend, WorkLimits, ensure_tool_allowed, handle_request,
    handle_request_with_backend, serve_json_rpc, serve_json_rpc_with_backend,
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

#[cfg(target_os = "linux")]
#[test]
fn repository_traversal_stays_bound_when_the_repository_root_is_swapped() {
    let root = temp_dir("repo-root-swap");
    let allowed = root.join("allowed");
    let repo = allowed.join("repo");
    let held = root.join("held-repo");
    let outside = root.join("outside");
    fs::create_dir_all(&repo).expect("create selected repository");
    fs::create_dir_all(&outside).expect("create outside replacement");
    fs::write(repo.join("README.md"), "inside metadata only\n").expect("write inside file");
    fs::write(
        outside.join("secret.rs"),
        "pub const OUTSIDE_SECRET: &str = \"must-not-be-traversed\";\n",
    )
    .expect("write outside source");

    let config = McpServerConfig::new(allowed);
    let backend = SwapThenStaticBackend {
        swap_path: repo.clone(),
        held_path: held,
        replacement_target: outside,
    };
    let response = handle_request_with_backend(
        &config,
        &backend,
        McpRequest {
            limit: Some(2),
            ..request("codedb_get_repo_summary", Some(repo), None)
        },
    )
    .expect("descriptor-bound repository summary");

    assert_eq!(summary_count(&response.rows, "rust_sources"), 0);
    remove_dir(root);
}

#[cfg(target_os = "linux")]
#[test]
fn repository_traversal_stays_bound_when_an_allowed_root_ancestor_is_swapped() {
    let root = temp_dir("repo-ancestor-swap");
    let allowed = root.join("allowed");
    let repo = allowed.join("nested/repo");
    let held = root.join("held-allowed");
    let outside = root.join("outside-allowed");
    fs::create_dir_all(&repo).expect("create selected repository");
    fs::create_dir_all(outside.join("nested/repo")).expect("create outside replacement");
    fs::write(repo.join("README.md"), "inside metadata only\n").expect("write inside file");
    fs::write(
        outside.join("nested/repo/secret.rs"),
        "pub const OUTSIDE_SECRET: &str = \"must-not-be-traversed\";\n",
    )
    .expect("write outside source");

    let config = McpServerConfig::new(allowed.clone());
    let backend = SwapThenStaticBackend {
        swap_path: allowed,
        held_path: held,
        replacement_target: outside,
    };
    let response = handle_request_with_backend(
        &config,
        &backend,
        McpRequest {
            limit: Some(2),
            ..request("codedb_get_repo_summary", Some(repo), None)
        },
    )
    .expect("descriptor-bound repository summary");

    assert_eq!(summary_count(&response.rows, "rust_sources"), 0);
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
fn oversized_redb_store_is_refused_before_the_in_process_adapter_opens_it() {
    let root = temp_dir("oversized-redb");
    let store = root.join("oversized.redb");
    let file = fs::File::create(&store).expect("create sparse redb fixture");
    file.set_len(MAX_REDB_STORE_BYTES + 1)
        .expect("size sparse redb fixture");
    drop(file);

    let mut config = McpServerConfig::new(root.clone());
    config.persisted_store = Some(PersistedStoreConfig {
        selector: store.display().to_string(),
        pg_table: "unused_for_redb".to_string(),
    });
    assert!(matches!(
        handle_request(&config, request("codedb_get_store_summary", None, None)),
        Err(McpError::WorkLimitExceeded)
    ));

    remove_dir(root);
}

#[test]
fn redb_store_selector_rejects_non_normal_parent_components() {
    let root = temp_dir("redb-parent-components");
    let nested = root.join("nested");
    fs::create_dir_all(&nested).expect("create nested directory");
    let store = root.join("store.redb");
    fs::write(&store, b"not opened because the selector is invalid")
        .expect("write rejected store fixture");

    let mut config = McpServerConfig::new(root.clone());
    config.persisted_store = Some(PersistedStoreConfig {
        selector: nested.join("../store.redb").display().to_string(),
        pg_table: "unused_for_redb".to_string(),
    });
    assert!(matches!(
        handle_request(&config, request("codedb_get_store_summary", None, None)),
        Err(McpError::InvalidConfiguration)
    ));

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

#[cfg(target_os = "linux")]
#[test]
fn cargo_summary_can_use_the_descriptor_bound_repository_in_its_child_process() {
    let root = temp_dir("descriptor-cargo-summary");
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create Cargo fixture");
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"descriptor-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo manifest");
    fs::write(
        repo.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"descriptor-fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo lockfile");
    fs::write(repo.join("src/lib.rs"), "pub fn bounded() {}\n").expect("write Rust source");

    let response = handle_request(
        &McpServerConfig::new(root.clone()),
        McpRequest {
            limit: Some(3),
            max_bytes: Some(4_096),
            ..request("codedb_get_cargo_summary", Some(repo), None)
        },
    )
    .expect("descriptor-bound Cargo summary");

    assert_eq!(summary_count(&response.rows, "cargo_packages"), 1);
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
fn redb_snapshot_is_queried_after_live_source_is_removed_without_store_mutation() {
    let root = temp_dir("persisted-redb");
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create repository");
    fs::write(
        repo.join("src/lib.rs"),
        "pub fn persisted_only() -> &'static str { \"redb\" }\n",
    )
    .expect("write source");
    fs::write(repo.join("opaque.bin"), [0xff, 0xfe, b'a'])
        .expect("write invalid UTF-8 uncertainty fixture");
    fs::write(
        repo.join("config.txt"),
        "DATABASE_URL=postgresql://codedb:swordfish@db.example/codedb\n",
    )
    .expect("write detected credential fixture");
    let store = repo.join(".codedb/store.redb");
    let capture_stdout = capture_store(&repo, store.to_str().expect("utf-8 store"), None);
    let capture_rows: Vec<Row> =
        serde_json::from_str(&capture_stdout).expect("parse capture policy rows");
    assert!(capture_rows.iter().any(|row| {
        row.get("relative_path").map(String::as_str) == Some("opaque.bin")
            && row.get("classification_status").map(String::as_str) == Some("uncertain")
            && row.get("classification_evidence").map(String::as_str) == Some("non_text_content")
            && row.get("raw_blob_persisted").map(String::as_str) == Some("false")
    }));
    assert!(capture_rows.iter().any(|row| {
        row.get("relative_path").map(String::as_str) == Some("config.txt")
            && row.get("classification_status").map(String::as_str) == Some("secret_detected")
            && row
                .get("classification_evidence")
                .is_some_and(|value| value.contains("database_uri_credentials"))
            && row.get("raw_blob_persisted").map(String::as_str) == Some("false")
    }));
    let before = fs::read(&store).expect("read redb snapshot before MCP");
    fs::remove_file(repo.join("src/lib.rs")).expect("remove live source after persistence");

    let mut config = McpServerConfig::new(root.clone());
    config.persisted_store = Some(PersistedStoreConfig {
        selector: store.display().to_string(),
        pg_table: "unused_for_redb".to_string(),
    });
    let response = handle_request(
        &config,
        McpRequest {
            max_bytes: Some(4_096),
            ..request("codedb_get_store_summary", None, None)
        },
    )
    .expect("query persisted redb snapshot");

    assert_eq!(
        response.rows[0].get("backend").map(String::as_str),
        Some("redb")
    );
    assert_eq!(
        response.rows[0].get("source_files").map(String::as_str),
        Some("1")
    );
    assert!(
        !serde_json::to_string(&response)
            .expect("serialize response")
            .contains("persisted_only")
    );
    assert!(
        fs::read(&store).expect("read redb snapshot after MCP") == before,
        "MCP snapshot query must not mutate the persisted store"
    );

    let blocked_output = root.join("blocked-materialize");
    let output = Command::new(codedb_binary())
        .args([
            "materialize",
            "--store",
            store.to_str().expect("utf-8 store"),
            "--out-dir",
            blocked_output.to_str().expect("utf-8 output"),
            "--path",
            "opaque.bin",
            "--format",
            "json",
        ])
        .output()
        .expect("attempt blocked-file materialization");
    assert!(
        output.status.success(),
        "materialize stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !blocked_output.join("opaque.bin").exists(),
        "invalid UTF-8 uncertainty must be absent from redb"
    );

    remove_dir(root);
}

#[test]
fn postgresql_snapshot_and_materialize_summary_are_secret_safe_when_explicitly_enabled() {
    let Ok(conn) = std::env::var("CODEDB_PG_CONN") else {
        eprintln!("SKIP: set explicit CODEDB_PG_CONN to run the codedb-mcp PostgreSQL integration");
        return;
    };
    assert!(
        !conn.trim().is_empty(),
        "CODEDB_PG_CONN must not be empty when explicitly set"
    );

    let root = temp_dir("persisted-postgresql");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repository");
    fs::write(repo.join("README.md"), "persisted PostgreSQL snapshot\n").expect("write source");
    let table = format!("cm_{}_{}", std::process::id(), unique_suffix());
    assert!(table.len() <= 40);
    let tables = PgTestTables::new(conn.clone(), table.clone());
    capture_store(&repo, "pg", Some(&table));

    let mut config = McpServerConfig::new(root.clone());
    config.persisted_store = Some(PersistedStoreConfig {
        selector: "pg".to_string(),
        pg_table: table.clone(),
    });
    let response = handle_request(
        &config,
        McpRequest {
            max_bytes: Some(4_096),
            ..request("codedb_get_store_summary", None, None)
        },
    )
    .expect("query persisted PostgreSQL snapshot");
    assert_eq!(
        response.rows[0].get("backend").map(String::as_str),
        Some("postgresql")
    );
    assert_eq!(
        response.rows[0].get("source_files").map(String::as_str),
        Some("1")
    );

    let query_sentinel = "CODEDB_QUERY_SENTINEL_%43%44%42";
    let selector = format!(
        "{}{}application_name={query_sentinel}",
        conn,
        if conn.contains('?') { "&" } else { "?" }
    );
    let output_dir = root.join("materialized");
    let output = Command::new(codedb_binary())
        .args([
            "materialize",
            "--store",
            "pg",
            "--pg-table",
            &table,
            "--out-dir",
            output_dir.to_str().expect("utf-8 output path"),
            "--format",
            "json",
        ])
        .env("CODEDB_PG_CONN", &selector)
        .output()
        .expect("run PostgreSQL materialize");
    assert!(
        output.status.success(),
        "materialize stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf-8 materialize output");
    assert!(stdout.contains("\"table\": \"materialize_summary\""));
    assert!(stdout.contains(&format!("postgresql:{table}")));
    assert!(!stdout.contains(&conn));
    assert!(!stdout.contains("CODEDB_QUERY_SENTINEL"));
    assert!(!stdout.contains("%43%44%42"));
    if let Some(credentials) = connection_credentials(&conn) {
        assert!(
            !stdout.contains(credentials),
            "materialize summary leaked URL credentials"
        );
    }
    assert_eq!(
        fs::read_to_string(output_dir.join("README.md")).expect("read materialized source"),
        "persisted PostgreSQL snapshot\n"
    );

    drop(tables);
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
fn complete_json_rpc_wire_lines_include_envelope_wrapper_and_utf8_in_max_bytes() {
    let root = temp_dir("wire-byte-budget");
    let config = McpServerConfig::new(root.clone());
    let backend = EscapingBackend;
    let budget = 1_024usize;
    let input = format!(
        concat!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}\n",
            "{{\"jsonrpc\":\"2.0\",\"id\":\"escaped-id-\\u2603\",\"method\":\"tools/call\",",
            "\"params\":{{\"name\":\"codedb_list_tables\",\"arguments\":",
            "{{\"limit\":20,\"max_bytes\":{}}}}}}}\n"
        ),
        budget
    );
    let mut output = Vec::new();

    let report = serve_json_rpc_with_backend(
        &config,
        &backend,
        &mut Cursor::new(input.as_bytes()),
        &mut output,
    )
    .expect("serve bounded protocol");
    assert_eq!(report.status, "eof_shutdown");

    let stdout = String::from_utf8(output).expect("UTF-8 JSON-RPC output");
    let messages = stdout.lines().collect::<Vec<_>>();
    assert_eq!(messages.len(), 2);
    for message in &messages {
        let wire_bytes = message.len().saturating_add(1);
        assert!(
            wire_bytes <= budget,
            "complete newline-delimited wire message exceeded {budget} bytes: {}",
            wire_bytes
        );
        let value: serde_json::Value = serde_json::from_str(message).expect("valid JSON-RPC");
        assert_eq!(value["jsonrpc"], "2.0");
    }
    let call: serde_json::Value = serde_json::from_str(messages[1]).expect("tool response");
    assert!(call.get("result").is_some(), "bounded result: {call}");
    assert_eq!(
        call["result"]["structuredContent"]["max_bytes"],
        budget as u64
    );
    assert!(
        call["result"]["structuredContent"]["truncated"]
            .as_bool()
            .expect("truncated bool"),
        "escaping fixture must exercise final-envelope truncation"
    );
    assert!(
        messages[1].contains("\\n") && messages[1].contains("\\\""),
        "fixture must exercise JSON escaping in byte accounting"
    );

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

#[cfg(unix)]
#[test]
fn packaged_binary_ignores_inherited_executable_override_for_store_access() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("no-executable-override");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("create repository");
    fs::write(repo.join("README.md"), "in-process store adapter\n").expect("write source");
    let store = repo.join(".codedb/store.redb");
    capture_store(&repo, store.to_str().expect("utf-8 store"), None);

    let marker = root.join("override-was-executed");
    let fake = root.join("fake-codedb");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nprintf executed > '{}'\nexit 99\n",
            marker.display()
        ),
    )
    .expect("write fake executable");
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).expect("make fake executable");

    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"codedb_get_store_summary\",\"arguments\":{\"limit\":1,\"max_bytes\":4096}}}\n"
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_codedb-mcp"))
        .env("CODEDB_MCP_ALLOWED_ROOT", &root)
        .env("CODEDB_MCP_STORE", &store)
        .env("CODEDB_MCP_CODEDB_BIN", &fake)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn packaged server");
    child
        .stdin
        .take()
        .expect("server stdin")
        .write_all(input.as_bytes())
        .expect("write request stream");
    let output = child.wait_with_output().expect("wait for server");

    assert!(
        output.status.success(),
        "server stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!marker.exists(), "inherited executable override was run");
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 MCP output");
    assert!(stdout.contains("\"backend\":\"redb\""));
    assert!(!stdout.contains("in-process store adapter"));

    remove_dir(root);
}

#[test]
fn exact_checked_in_codex_sample_launches_codedb_mcp_serve_frontdoor() {
    let root = temp_dir("exact-sample-frontdoor");
    fs::write(root.join("README.md"), "exact checked-in MCP sample\n").expect("write source");
    let store = root.join(".codedb/store.redb");
    capture_store(&root, store.to_str().expect("utf-8 store"), None);

    let sample_path = workspace_root().join("examples/codex/codedb_mcp_config.json");
    let sample: serde_json::Value =
        serde_json::from_slice(&fs::read(&sample_path).expect("read exact sample"))
            .expect("parse exact sample");
    let server = &sample["mcpServers"]["codedb"];
    assert_eq!(server["command"], "codedb");
    let command = server["command"].as_str().expect("sample command");
    let args = server["args"]
        .as_array()
        .expect("sample args")
        .iter()
        .map(|value| value.as_str().expect("string arg"))
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            "mcp",
            "serve",
            "--repo-path",
            ".",
            "--store",
            ".codedb/store.redb",
            "--default-limit",
            "50",
            "--max-bytes",
            "65536",
        ]
    );

    let path = format!(
        "{}:{}",
        codedb_binary()
            .parent()
            .expect("codedb binary parent")
            .display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(command)
        .args(&args)
        .current_dir(&root)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("launch exact checked-in MCP config");
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"codedb_get_store_summary\",\"arguments\":{\"limit\":1,\"max_bytes\":4096}}}\n"
    );
    child
        .stdin
        .take()
        .expect("sample stdin")
        .write_all(input.as_bytes())
        .expect("write sample request stream");
    let output = child.wait_with_output().expect("wait for sample EOF");
    assert!(
        output.status.success(),
        "sample stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "sample must keep stderr clean");
    let stdout = String::from_utf8(output.stdout).expect("sample UTF-8 stdout");
    let messages = stdout.lines().collect::<Vec<_>>();
    assert_eq!(messages.len(), 2);
    let response: serde_json::Value =
        serde_json::from_str(messages[1]).expect("sample store response");
    assert_eq!(
        response["result"]["structuredContent"]["rows"][0]["backend"],
        "redb"
    );
    assert_eq!(
        response["result"]["structuredContent"]["rows"][0]["source_files"],
        "1"
    );

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

struct EscapingBackend;

impl ReadOnlyBackend for EscapingBackend {
    fn read(
        &self,
        _tool: &str,
        _table: Option<&str>,
        _repo_path: Option<&Path>,
        _limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError> {
        Ok((0..20)
            .map(|index| {
                BTreeMap::from([
                    ("index".to_string(), index.to_string()),
                    (
                        "payload".to_string(),
                        format!("snowman=\u{2603}; quote=\"; slash=\\\\; line=\\n-{index}-"),
                    ),
                ])
            })
            .collect())
    }
}

#[cfg(target_os = "linux")]
struct SwapThenStaticBackend {
    swap_path: PathBuf,
    held_path: PathBuf,
    replacement_target: PathBuf,
}

#[cfg(target_os = "linux")]
impl ReadOnlyBackend for SwapThenStaticBackend {
    fn read(
        &self,
        tool: &str,
        table: Option<&str>,
        repo_path: Option<&Path>,
        limits: WorkLimits,
    ) -> Result<Vec<Row>, McpError> {
        fs::rename(&self.swap_path, &self.held_path).expect("hold selected path");
        std::os::unix::fs::symlink(&self.replacement_target, &self.swap_path)
            .expect("install outside replacement");
        StaticAnalysisBackend.read(tool, table, repo_path, limits)
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

fn summary_count(rows: &[Row], table: &str) -> usize {
    rows.iter()
        .find(|row| row.get("table").map(String::as_str) == Some(table))
        .and_then(|row| row.get("rows"))
        .and_then(|value| value.parse().ok())
        .expect("summary row count")
}

fn temp_dir(label: &str) -> PathBuf {
    let suffix = unique_suffix();
    let path = std::env::temp_dir().join(format!("codedb_mcp_{label}_{suffix}"));
    fs::create_dir_all(&path).expect("create temp directory");
    path
}

fn remove_dir(path: PathBuf) {
    fs::remove_dir_all(path).expect("remove temp directory");
}

fn unique_suffix() -> u128 {
    static NEXT_SUFFIX: AtomicU64 = AtomicU64::new(0);
    ((std::process::id() as u128) << 64) | (NEXT_SUFFIX.fetch_add(1, Ordering::Relaxed) as u128)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace root")
}

fn codedb_binary() -> &'static PathBuf {
    static CODEDB: OnceLock<PathBuf> = OnceLock::new();
    CODEDB.get_or_init(|| {
        let root = workspace_root();
        let status = Command::new("cargo")
            .args(["build", "--locked", "-p", "codedb"])
            .current_dir(&root)
            .status()
            .expect("build codedb frontdoor");
        assert!(status.success(), "codedb frontdoor build failed");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                }
            })
            .unwrap_or_else(|| root.join("target"));
        target
            .join("debug")
            .join(format!("codedb{}", std::env::consts::EXE_SUFFIX))
    })
}

fn capture_store(repo: &Path, store: &str, pg_table: Option<&str>) -> String {
    let mut command = Command::new(codedb_binary());
    command.args([
        "capture",
        "--repo-path",
        repo.to_str().expect("utf-8 repo"),
        "--store",
        store,
        "--raw-persistence",
        "safe-source",
        "--batch-files",
        "16",
        "--format",
        "json",
    ]);
    if let Some(table) = pg_table {
        command.args(["--pg-table", table]);
    }
    let output = command.output().expect("capture persisted snapshot");
    assert!(
        output.status.success(),
        "capture stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("capture UTF-8 output")
}

fn connection_credentials(conn: &str) -> Option<&str> {
    conn.split_once("://")?
        .1
        .split_once('@')
        .map(|(credentials, _)| credentials)
}

struct PgTestTables {
    conn: String,
    base: String,
}

impl PgTestTables {
    fn new(conn: String, base: String) -> Self {
        let tables = Self { conn, base };
        if let Err(error) = tables.drop_all() {
            panic!("PostgreSQL pre-test cleanup failed: {error}");
        }
        tables
    }

    fn drop_all(&self) -> Result<(), String> {
        let sql = format!(
            "DROP TABLE IF EXISTS {base}_path_refs CASCADE;\
             DROP TABLE IF EXISTS {base}_blobs CASCADE;\
             DROP TABLE IF EXISTS {base}_schema_metadata CASCADE;\
             DROP TABLE IF EXISTS {base} CASCADE;",
            base = self.base
        );
        let output = Command::new("psql")
            .args([
                "-X",
                "--dbname",
                &self.conn,
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                &sql,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .expect("psql is required when CODEDB_PG_CONN enables this integration");
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }
}

impl Drop for PgTestTables {
    fn drop(&mut self) {
        if let Err(error) = self.drop_all() {
            eprintln!("PostgreSQL cleanup failed during Drop: {error}");
        }
    }
}

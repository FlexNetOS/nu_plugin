#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

fn temp_root(name: &str) -> PathBuf {
    let sequence = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "codedb-symlink-{name}-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("reserve symlink integration-test root");
    root
}

fn run_codedb(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args(args)
        .output()
        .expect("run packaged CodeDB binary")
}

fn fixture_with_bun_link(root: &Path, target: &str) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("node_modules/.bin")).expect("create Bun .bin directory");
    fs::create_dir_all(repo.join("node_modules/tool/bin")).expect("create package bin directory");
    fs::write(
        repo.join("node_modules/tool/bin/tool.js"),
        b"#!/usr/bin/env node\nconsole.log('tool');\n",
    )
    .expect("write package executable");
    symlink(target, repo.join("node_modules/.bin/tool")).expect("create Bun-style link");
    repo
}

#[test]
fn capture_store_materialize_preserves_bun_relative_symlink() {
    let root = temp_root("bun-roundtrip");
    let repo = fixture_with_bun_link(&root, "../tool/bin/tool.js");
    let store = root.join("capture.redb");
    let output = root.join("materialized");

    let capture = run_codedb(&[
        "capture",
        repo.to_str().expect("UTF-8 repo path"),
        "--store",
        store.to_str().expect("UTF-8 store path"),
        "--raw-persistence",
        "safe-source",
        "--format",
        "json",
    ]);
    assert!(
        capture.status.success(),
        "capture failed: {}",
        String::from_utf8_lossy(&capture.stderr)
    );
    let rows: serde_json::Value =
        serde_json::from_slice(&capture.stdout).expect("parse capture receipt");
    let link = rows
        .as_array()
        .expect("capture rows")
        .iter()
        .find(|row| {
            row.get("table").and_then(serde_json::Value::as_str) == Some("source_symlinks")
                && row.get("relative_path").and_then(serde_json::Value::as_str)
                    == Some("node_modules/.bin/tool")
        })
        .expect("captured symlink receipt");
    assert_eq!(
        link.get("target").and_then(serde_json::Value::as_str),
        Some("../tool/bin/tool.js")
    );
    assert!(
        link.get("target_sha256")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|digest| digest.len() == 64),
        "symlink target must be checksum-bound: {link}"
    );
    assert_eq!(
        link.get("status").and_then(serde_json::Value::as_str),
        Some("captured")
    );
    assert!(
        !rows.as_array().expect("capture rows").iter().any(|row| {
            row.get("table").and_then(serde_json::Value::as_str) == Some("capture_gaps")
                && row.get("relative_path").and_then(serde_json::Value::as_str)
                    == Some("node_modules/.bin/tool")
        }),
        "captured symlink must not remain a gap: {rows}"
    );

    let materialize = run_codedb(&[
        "materialize",
        "--store",
        store.to_str().expect("UTF-8 store path"),
        "--out-dir",
        output.to_str().expect("UTF-8 output path"),
        "--format",
        "json",
    ]);
    assert!(
        materialize.status.success(),
        "materialize failed: {}",
        String::from_utf8_lossy(&materialize.stderr)
    );

    let restored_link = output.join("node_modules/.bin/tool");
    assert!(
        fs::symlink_metadata(&restored_link)
            .expect("restored symlink metadata")
            .file_type()
            .is_symlink(),
        "captured link was materialized as a regular file"
    );
    assert_eq!(
        fs::read_link(&restored_link).expect("restored target"),
        Path::new("../tool/bin/tool.js")
    );
    assert_eq!(
        fs::read(&restored_link).expect("follow restored Bun link"),
        b"#!/usr/bin/env node\nconsole.log('tool');\n"
    );

    fs::remove_dir_all(root).expect("remove integration-test root");
}

#[test]
fn escaping_symlink_target_is_captured_as_metadata_but_replay_fails_before_output() {
    let root = temp_root("escape-rejection");
    let repo = fixture_with_bun_link(&root, "../../../outside");
    let store = root.join("capture.redb");
    let output = root.join("materialized");

    let capture = run_codedb(&[
        "capture",
        repo.to_str().expect("UTF-8 repo path"),
        "--store",
        store.to_str().expect("UTF-8 store path"),
        "--raw-persistence",
        "safe-source",
        "--format",
        "json",
    ]);
    assert!(
        capture.status.success(),
        "metadata capture failed: {}",
        String::from_utf8_lossy(&capture.stderr)
    );

    let materialize = run_codedb(&[
        "materialize",
        "--store",
        store.to_str().expect("UTF-8 store path"),
        "--out-dir",
        output.to_str().expect("UTF-8 output path"),
        "--format",
        "json",
    ]);
    assert!(
        !materialize.status.success(),
        "escaping target was replayed"
    );
    assert!(
        String::from_utf8_lossy(&materialize.stderr).contains("escapes output root"),
        "unexpected rejection: {}",
        String::from_utf8_lossy(&materialize.stderr)
    );
    assert!(
        !output.exists(),
        "unsafe target validation must finish before any output is published"
    );

    fs::remove_dir_all(root).expect("remove integration-test root");
}

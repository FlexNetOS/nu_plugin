use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codedb-build-cli-{suffix}"))
}

fn source_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, current: &Path, rows: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(current)
            .expect("read source directory")
            .map(|entry| entry.expect("read source entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("source metadata");
            if metadata.is_dir() {
                walk(root, &path, rows);
            } else if metadata.is_file() {
                rows.insert(
                    path.strip_prefix(root)
                        .expect("relative source path")
                        .to_path_buf(),
                    fs::read(path).expect("source bytes"),
                );
            }
        }
    }

    let mut rows = BTreeMap::new();
    walk(root, root, &mut rows);
    rows
}

#[test]
fn approved_cli_capture_persists_receipt_and_never_mutates_source() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let raw_log = evidence.join("capture.log");
    let store = evidence.join("capture.redb");
    fs::create_dir_all(repo.join("src")).expect("create fixture source");
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"codedb-cli-build-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    )
    .expect("write manifest");
    fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 7 }\n").expect("write source");
    fs::write(
        repo.join("build.rs"),
        r#"fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("generated.rs"), b"pub const GENERATED: u8 = 9;\n")
        .expect("write generated source");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-lib=static=codedb_cli_fixture");
    println!("cargo:rustc-link-search=native=vendor/native");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");
}
"#,
    )
    .expect("write build script");
    let before = source_snapshot(&repo);

    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB079",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("run approved capture build");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("capture JSON rows");
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("build_script_runs")
            && row.get("status").map(String::as_str) == Some("captured")
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("out_dir_artifacts")
            && row.get("relative_path").map(String::as_str) == Some("generated.rs")
            && row.get("sha256").is_some_and(|sha256| sha256.len() == 64)
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("native_link_facts")
            && row.get("fact_kind").map(String::as_str) == Some("linked_lib")
            && row.get("value").map(String::as_str) == Some("static=codedb_cli_fixture")
    }));
    let receipt = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("build_capture_receipts"))
        .expect("persisted build receipt row");
    assert_eq!(receipt.get("status").map(String::as_str), Some("persisted"));
    assert!(raw_log.is_file());
    assert!(store.is_file());
    assert_eq!(source_snapshot(&repo), before);

    let report = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "store-report",
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("read build receipt store");
    assert!(report.status.success());
    let report_rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&report.stdout).expect("store report JSON rows");
    assert!(report_rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("source_files")
            && row
                .get("key")
                .is_some_and(|key| key.starts_with("dynamic-build-captures/"))
            && row.get("value") == receipt.get("blob_ref")
    }));

    let artifact_dir = evidence.join("reproduced-out-dir");
    let reproduced = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "reproduce",
            "--approval-id",
            receipt.get("approval_id").expect("receipt approval id"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--artifact-dir",
            artifact_dir.to_str().expect("UTF-8 artifact path"),
            "--format",
            "json",
        ])
        .output()
        .expect("reproduce captured OUT_DIR");
    assert!(
        reproduced.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reproduced.stderr)
    );
    let reproduction_rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&reproduced.stdout).expect("reproduction JSON rows");
    assert!(!reproduction_rows.is_empty());
    assert!(reproduction_rows.iter().all(|row| {
        row.get("table").map(String::as_str) == Some("out_dir_reproduction")
            && row.get("status").map(String::as_str) == Some("verified")
    }));
    assert_eq!(
        fs::read(artifact_dir.join("generated.rs")).expect("reproduced generated source"),
        b"pub const GENERATED: u8 = 9;\n"
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn approved_cli_capture_records_instrumented_proc_macro_tokens() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/proc_macro_consumer");
    for relative in [
        "Cargo.toml",
        "crates/demo_macro/Cargo.toml",
        "crates/demo_macro/src/lib.rs",
        "crates/consumer/Cargo.toml",
        "crates/consumer/src/lib.rs",
    ] {
        let destination = repo.join(relative);
        fs::create_dir_all(destination.parent().expect("fixture parent"))
            .expect("create fixture parent");
        fs::copy(fixture_root.join(relative), destination).expect("copy proc-macro fixture");
    }
    let before = source_snapshot(&repo);
    let raw_log = evidence.join("capture.log");
    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB078",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--format",
            "json",
        ])
        .output()
        .expect("run approved proc-macro capture");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("capture JSON rows");
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("proc_macro_invocations")
            && row.get("status").map(String::as_str) == Some("observed")
            && row.get("macro_name").map(String::as_str) == Some("demo_attr")
    }));
    for table in [
        "proc_macro_input_token_streams",
        "proc_macro_output_token_streams",
    ] {
        assert!(rows.iter().any(|row| {
            row.get("table").map(String::as_str) == Some(table)
                && row.get("sha256").is_some_and(|sha256| sha256.len() == 64)
        }));
    }
    assert_eq!(source_snapshot(&repo), before);
    assert!(raw_log.is_file());
    fs::remove_dir_all(root).expect("remove fixture");
}

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use codedb_build_capture::{BuildCaptureRequest, BuildCaptureStatus, capture_approved_build};

fn temp_root() -> PathBuf {
    // A bare nanosecond timestamp collides across concurrent test threads and
    // across concurrent processes sharing /tmp; compose it with the process id
    // and a per-process atomic sequence so every fixture root is unique.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let pid = std::process::id();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("codedb-approved-frontdoor-{pid}-{suffix}-{seq}"))
}

fn source_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, current: &Path, rows: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(current)
            .expect("read fixture directory")
            .map(|entry| entry.expect("read fixture entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("fixture metadata");
            if metadata.is_dir() {
                walk(root, &path, rows);
            } else if metadata.is_file() {
                rows.insert(
                    path.strip_prefix(root)
                        .expect("relative fixture path")
                        .to_path_buf(),
                    fs::read(path).expect("fixture bytes"),
                );
            }
        }
    }

    let mut rows = BTreeMap::new();
    walk(root, root, &mut rows);
    rows
}

#[test]
fn approved_frontdoor_executes_with_external_log_and_preserves_source_tree() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    fs::create_dir_all(repo.join("src")).expect("create fixture source");
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"approved-frontdoor-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    )
    .expect("write manifest");
    fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 7 }\n").expect("write source");
    fs::write(
        repo.join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
    )
    .expect("write build script");
    let before = source_snapshot(&repo);

    let outcome = capture_approved_build(BuildCaptureRequest {
        repo_path: repo.clone(),
        store_path: None,
        raw_log_path: evidence.join("capture.log"),
        unsafe_execute_build: true,
        approver: Some("integration-test".to_string()),
        task_id: Some("CDB078,CDB079,CDB080,CDB082".to_string()),
        before_state: Some("fixture-snapshot-recorded".to_string()),
        cleanup_plan: Some("remove isolated sandbox after evidence capture".to_string()),
    })
    .expect("approved production frontdoor");

    assert_eq!(outcome.status, BuildCaptureStatus::Captured);
    assert!(evidence.join("capture.log").is_file());
    assert_eq!(source_snapshot(&repo), before);
    let tables = outcome
        .into_rows()
        .into_iter()
        .filter_map(|row| row.get("table").cloned())
        .collect::<Vec<_>>();
    for required in [
        "unsafe_execution_approval",
        "build_script_runs",
        "raw_log_paths",
    ] {
        assert!(
            tables.iter().any(|table| table == required),
            "missing {required}"
        );
    }

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn approved_frontdoor_rejects_raw_log_inside_source_tree() {
    let root = temp_root();
    let repo = root.join("repo");
    fs::create_dir_all(repo.join("src")).expect("create fixture source");
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"in-source-log-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write manifest");
    fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 7 }\n").expect("write source");
    let before = source_snapshot(&repo);

    let error = capture_approved_build(BuildCaptureRequest {
        repo_path: repo.clone(),
        store_path: None,
        raw_log_path: repo.join("capture.log"),
        unsafe_execute_build: true,
        approver: Some("integration-test".to_string()),
        task_id: Some("CDB079".to_string()),
        before_state: Some("fixture-snapshot-recorded".to_string()),
        cleanup_plan: Some("remove isolated sandbox after evidence capture".to_string()),
    })
    .expect_err("source-contained raw log must be rejected");

    assert!(error.to_string().contains("outside the source repository"));
    assert_eq!(source_snapshot(&repo), before);
    fs::remove_dir_all(root).expect("remove fixture");
}

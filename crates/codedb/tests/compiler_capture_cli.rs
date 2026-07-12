use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

fn temp_root() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("codedb-compiler-cli-{suffix}"))
}

fn snapshot_source_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("read source tree")
            .map(|entry| entry.expect("source tree entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let relative = path.strip_prefix(root).expect("source-relative path");
            let file_type = entry.file_type().expect("source entry type");
            if file_type.is_dir() {
                snapshot.insert(relative.to_path_buf(), b"directory".to_vec());
                visit(root, &path, snapshot);
            } else if file_type.is_file() {
                let mut value = b"file\0".to_vec();
                value.extend(fs::read(&path).expect("read source file"));
                snapshot.insert(relative.to_path_buf(), value);
            } else if file_type.is_symlink() {
                let mut value = b"symlink\0".to_vec();
                value.extend(
                    fs::read_link(&path)
                        .expect("read source symlink")
                        .to_string_lossy()
                        .as_bytes(),
                );
                snapshot.insert(relative.to_path_buf(), value);
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn run_approved_capture(
    repo: &Path,
    source: &Path,
    evidence: &Path,
    store: &Path,
) -> Vec<BTreeMap<String, String>> {
    let source_before = snapshot_source_tree(repo);
    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "compiler",
            source.to_str().expect("UTF-8 source path"),
            "--repo-path",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB077,CDB085",
            "--before-state",
            "source-sha256-recorded",
            "--cleanup-plan",
            "remove-isolated-compiler-sandbox",
            "--evidence-dir",
            evidence.to_str().expect("UTF-8 evidence path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--edition",
            "2024",
            "--format",
            "json",
        ])
        .output()
        .expect("run compiler capture");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        snapshot_source_tree(repo),
        source_before,
        "compiler capture mutated its source repository"
    );
    serde_json::from_slice(&output.stdout).expect("compiler capture JSON rows")
}

fn artifact_pin_hashes(rows: &[BTreeMap<String, String>]) -> BTreeMap<String, String> {
    rows.iter()
        .filter(|row| row.get("table").map(String::as_str) == Some("compiler_artifacts"))
        .map(|row| {
            (
                row.get("artifact_kind")
                    .expect("compiler artifact kind")
                    .clone(),
                row.get("pin_sha256")
                    .expect("compiler artifact pin")
                    .clone(),
            )
        })
        .collect()
}

fn compiler_hashes(rows: &[BTreeMap<String, String>]) -> (String, String) {
    let row = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("compiler_semantic_hashes"))
        .expect("compiler semantic hash row");
    (
        row.get("semantic_hash").expect("semantic hash").clone(),
        row.get("public_api_hash").expect("public API hash").clone(),
    )
}

#[test]
fn approved_compiler_cli_preserves_pinned_artifacts_and_source() {
    let root = temp_root();
    let repo = root.join("repo");
    let source = repo.join("src/lib.rs");
    let evidence = root.join("evidence/compiler");
    let store = root.join("evidence/compiler.redb");
    fs::create_dir_all(source.parent().expect("source parent")).expect("create source parent");
    fs::write(
        &source,
        r#"macro_rules! generated_answer {
    () => { 42_u32 };
}

pub fn answer() -> u32 {
    generated_answer!()
}
"#,
    )
    .expect("write compiler fixture");
    let rows = run_approved_capture(&repo, &source, &evidence, &store);
    for kind in [
        "macro_expansion",
        "macro_resolution",
        "macro_hygiene",
        "hir",
        "mir",
        "rustdoc_public_api",
    ] {
        let row = rows
            .iter()
            .find(|row| {
                row.get("table").map(String::as_str) == Some("compiler_artifacts")
                    && row.get("artifact_kind").map(String::as_str) == Some(kind)
            })
            .unwrap_or_else(|| panic!("missing compiler artifact row for {kind}"));
        assert_eq!(
            row.get("status").map(String::as_str),
            Some("compiler_observed")
        );
        if kind == "macro_resolution" {
            assert_eq!(row.get("evidence_path").map(String::as_str), Some(""));
            assert!(
                row.get("evidence_sha256")
                    .is_some_and(|value| value.len() == 64)
            );
            assert!(
                row.get("evidence_bytes")
                    .and_then(|value| value.parse::<usize>().ok())
                    .is_some_and(|bytes| bytes > 0)
            );
        } else {
            let artifact_path = PathBuf::from(row.get("evidence_path").expect("artifact path"));
            let bytes = fs::read(&artifact_path).expect("persisted compiler artifact");
            let sha256 = format!("{:x}", Sha256::digest(&bytes));
            assert_eq!(row.get("evidence_sha256"), Some(&sha256));
        }
        assert!(row.get("pin_sha256").is_some_and(|value| value.len() == 64));
    }
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("compiler_semantic_hashes")
            && row
                .get("semantic_hash")
                .is_some_and(|value| value.len() == 64)
            && row
                .get("public_api_hash")
                .is_some_and(|value| value.len() == 64)
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("compiler_capture_receipts")
            && row.get("status").map(String::as_str) == Some("persisted")
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("raw_log_paths")
            && row.get("status").map(String::as_str) == Some("written")
    }));

    let repeated_rows = run_approved_capture(
        &repo,
        &source,
        &root.join("evidence/repeated-compiler"),
        &root.join("evidence/repeated-compiler.redb"),
    );
    assert_eq!(
        artifact_pin_hashes(&rows),
        artifact_pin_hashes(&repeated_rows),
        "unchanged source produced different compiler artifact pins"
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn compiler_cli_hashes_distinguish_private_implementation_and_public_api_drift() {
    let root = temp_root();
    let repo = root.join("repo");
    let source = repo.join("src/lib.rs");
    fs::create_dir_all(source.parent().expect("source parent")).expect("create source parent");
    fs::write(
        &source,
        "fn implementation() -> u32 { 42 }\npub fn answer() -> u32 { implementation() }\n",
    )
    .expect("write baseline source");
    let baseline = run_approved_capture(
        &repo,
        &source,
        &root.join("evidence/baseline"),
        &root.join("evidence/baseline.redb"),
    );
    let (baseline_semantic, baseline_public_api) = compiler_hashes(&baseline);

    fs::write(
        &source,
        "fn implementation() -> u32 { 43 }\npub fn answer() -> u32 { implementation() }\n",
    )
    .expect("write private implementation drift");
    let private_drift = run_approved_capture(
        &repo,
        &source,
        &root.join("evidence/private-drift"),
        &root.join("evidence/private-drift.redb"),
    );
    let (private_semantic, private_public_api) = compiler_hashes(&private_drift);
    assert_ne!(
        private_semantic, baseline_semantic,
        "private implementation drift did not change the semantic hash"
    );
    assert_eq!(
        private_public_api, baseline_public_api,
        "private implementation drift changed the public API hash"
    );

    fs::write(
        &source,
        "fn implementation() -> u64 { 43 }\npub fn answer() -> u64 { implementation() }\n",
    )
    .expect("write public signature drift");
    let public_drift = run_approved_capture(
        &repo,
        &source,
        &root.join("evidence/public-drift"),
        &root.join("evidence/public-drift.redb"),
    );
    let (_, public_drift_api) = compiler_hashes(&public_drift);
    assert_ne!(
        public_drift_api, private_public_api,
        "public signature drift did not change the public API hash"
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn compiler_cli_refuses_without_unsafe_flag_and_writes_nothing() {
    let root = temp_root();
    let repo = root.join("repo");
    let source = repo.join("src/lib.rs");
    let evidence = root.join("evidence/compiler");
    let store = root.join("evidence/compiler.redb");
    fs::create_dir_all(source.parent().expect("source parent")).expect("create source parent");
    fs::write(&source, "pub fn answer() -> u32 { 42 }\n").expect("write source");
    let source_before = snapshot_source_tree(&repo);

    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "compiler",
            source.to_str().expect("UTF-8 source path"),
            "--repo-path",
            repo.to_str().expect("UTF-8 repo path"),
            "--evidence-dir",
            evidence.to_str().expect("UTF-8 evidence path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("run compiler refusal");
    assert!(output.status.success());
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("refusal JSON rows");
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("validation_errors")
            && row.get("code").map(String::as_str) == Some("unsafe_execution_refused")
    }));
    assert!(!evidence.exists());
    assert!(!store.exists());
    assert_eq!(snapshot_source_tree(&repo), source_before);
    fs::remove_dir_all(root).expect("remove fixture");
}

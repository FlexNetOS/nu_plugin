use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
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
    let source_before = fs::read(&source).expect("source before capture");

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
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("compiler capture JSON rows");
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
        let artifact_path = PathBuf::from(row.get("evidence_path").expect("artifact path"));
        let bytes = fs::read(&artifact_path).expect("persisted compiler artifact");
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(row.get("evidence_sha256"), Some(&sha256));
        assert!(row.get("pin_sha256").is_some_and(|value| value.len() == 64));
    }
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("compiler_semantic_hashes")
            && row.get("semantic_hash").is_some_and(|value| value.len() == 64)
            && row.get("public_api_hash").is_some_and(|value| value.len() == 64)
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("compiler_capture_receipts")
            && row.get("status").map(String::as_str) == Some("persisted")
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("raw_log_paths")
            && row.get("status").map(String::as_str) == Some("written")
    }));
    assert_eq!(fs::read(&source).expect("source after capture"), source_before);

    fs::remove_dir_all(root).expect("remove fixture");
}

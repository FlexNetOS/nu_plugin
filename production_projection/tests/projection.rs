// ARCHBP-006 — envctl production projection engine.
//
// Projects a selected approved branch to disposable .rs/.toml/.nu files with
// exact bytes, module_path metadata, permissions, safe relative links,
// deterministic ordering, source-version traceability, no canonical overwrite,
// and complete receipt-driven cleanup.

use production_projection::{
    is_safe_link, overwrites_canonical, project, cleanup, Branch, ProjectedFile,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn disposable_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("archbp006-{}-{}-{}", tag, std::process::id(), n));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn branch() -> Branch {
    Branch {
        id: "branch-abc".to_string(),
        version: "v1".to_string(),
        files: vec![
            ProjectedFile { rel_path: "src/lib.rs".to_string(), bytes: b"pub mod a;\n".to_vec(), module_path: Some("crate".to_string()), mode: 0o644 },
            ProjectedFile { rel_path: "Cargo.toml".to_string(), bytes: b"[package]\n".to_vec(), module_path: None, mode: 0o644 },
            ProjectedFile { rel_path: "scripts/run.nu".to_string(), bytes: b"def main [] {}\n".to_vec(), module_path: None, mode: 0o755 },
        ],
    }
}

#[test]
fn never_overwrites_canonical_source() {
    assert!(!overwrites_canonical());
}

#[test]
fn repeated_projection_is_byte_identical() {
    let b = branch();
    let d1 = disposable_dir("a");
    let d2 = disposable_dir("b");
    project(&b, &d1).unwrap();
    project(&b, &d2).unwrap();
    for f in &b.files {
        let x = fs::read(d1.join(&f.rel_path)).unwrap();
        let y = fs::read(d2.join(&f.rel_path)).unwrap();
        assert_eq!(x, y, "repeated projection must be byte-identical for {}", f.rel_path);
        assert_eq!(x, f.bytes, "projected bytes must equal source bytes exactly");
    }
    let _ = cleanup(&project(&b, &disposable_dir("c")).unwrap(), &disposable_dir("c"));
    fs::remove_dir_all(&d1).ok();
    fs::remove_dir_all(&d2).ok();
}

#[test]
fn projects_module_paths_and_permissions() {
    let b = branch();
    let d = disposable_dir("perm");
    let receipt = project(&b, &d).unwrap();
    // module path preserved in the receipt metadata
    let lib = receipt.projected.iter().find(|e| e.rel_path == "src/lib.rs").unwrap();
    assert_eq!(lib.module_path.as_deref(), Some("crate"));
    // permissions applied on disk
    let mode = fs::metadata(d.join("scripts/run.nu")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755);
    let mode_rs = fs::metadata(d.join("src/lib.rs")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode_rs, 0o644);
    fs::remove_dir_all(&d).ok();
}

#[test]
fn only_safe_relative_links_are_published() {
    assert!(is_safe_link("./mod.rs"));
    assert!(is_safe_link("sub/child.rs"));
    assert!(!is_safe_link("../escape"));
    assert!(!is_safe_link("/etc/passwd"));
}

#[test]
fn receipt_carries_source_version_traceability() {
    let b = branch();
    let d = disposable_dir("trace");
    let receipt = project(&b, &d).unwrap();
    assert_eq!(receipt.branch_id, "branch-abc");
    assert_eq!(receipt.branch_version, "v1");
    assert!(receipt.projected.iter().all(|e| e.source_version == "v1"));
    // deterministic ordering: receipt entries are sorted by rel_path
    let paths: Vec<&str> = receipt.projected.iter().map(|e| e.rel_path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    fs::remove_dir_all(&d).ok();
}

#[test]
fn path_escape_is_rejected_no_canonical_overwrite() {
    let mut b = branch();
    b.files.push(ProjectedFile { rel_path: "../../etc/evil".to_string(), bytes: b"x".to_vec(), module_path: None, mode: 0o644 });
    let d = disposable_dir("escape");
    let err = project(&b, &d).unwrap_err();
    assert!(err.contains("path-escape") || err.contains("unsafe"));
    fs::remove_dir_all(&d).ok();
}

#[test]
fn receipt_driven_cleanup_is_complete() {
    let b = branch();
    let d = disposable_dir("cleanup");
    let receipt = project(&b, &d).unwrap();
    for f in &b.files {
        assert!(d.join(&f.rel_path).exists());
    }
    let removed = cleanup(&receipt, &d).unwrap();
    assert_eq!(removed, b.files.len());
    for f in &b.files {
        assert!(!d.join(&f.rel_path).exists(), "cleanup must remove every projected file");
    }
    fs::remove_dir_all(&d).ok();
}

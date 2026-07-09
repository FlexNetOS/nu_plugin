//! Two-repo merge planner: the surgical worklist for reconciling divergent
//! forks. Classifies every source file across two repo roots by content hash as
//! `identical` (auto-mergeable), `divergent` (needs resolution), `unique_a`, or
//! `unique_b`, and flags crate-name collisions (same Cargo `[package]` name in
//! both). Source-only: vendor/generated/target/.git are skipped — build
//! artifacts do not merge, only source does.

use crate::{scan_filesystem, FileClassification, ScanError};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// Per-file disposition when overlaying repo B onto repo A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMergeStatus {
    /// Same relative path, identical content hash — merges with no conflict.
    Identical,
    /// Same relative path, different content — the surgical set needing a call.
    Divergent,
    /// Present only in repo A.
    UniqueA,
    /// Present only in repo B.
    UniqueB,
}

impl FileMergeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileMergeStatus::Identical => "identical",
            FileMergeStatus::Divergent => "divergent",
            FileMergeStatus::UniqueA => "unique_a",
            FileMergeStatus::UniqueB => "unique_b",
        }
    }
}

/// One file's cross-repo disposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMerge {
    pub relative_path: String,
    pub status: FileMergeStatus,
    pub sha_a: Option<String>,
    pub sha_b: Option<String>,
}

/// The deterministic merge plan for two repo roots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergePlan {
    pub identical: usize,
    pub divergent: usize,
    pub unique_a: usize,
    pub unique_b: usize,
    /// Cargo `[package]` names declared in BOTH repos (rename/reconcile targets).
    pub crate_collisions: Vec<String>,
    /// The divergent relative paths, sorted — the human/agent worklist.
    pub divergent_paths: Vec<String>,
    /// Full per-file detail, sorted by relative path.
    pub files: Vec<FileMerge>,
}

/// Build the merge plan comparing the source files of two repo roots by content
/// hash. Deterministic: paths are compared in sorted order, so the same inputs
/// always yield the same plan.
pub fn merge_plan(repo_a: &Path, repo_b: &Path) -> Result<MergePlan, ScanError> {
    let a = source_hashes(repo_a)?;
    let b = source_hashes(repo_b)?;

    let mut paths: Vec<&String> = a.keys().chain(b.keys()).collect();
    paths.sort_unstable();
    paths.dedup();

    let mut plan = MergePlan::default();
    for p in paths {
        let sa = a.get(p);
        let sb = b.get(p);
        let status = match (sa, sb) {
            (Some(x), Some(y)) if x == y => {
                plan.identical += 1;
                FileMergeStatus::Identical
            }
            (Some(_), Some(_)) => {
                plan.divergent += 1;
                plan.divergent_paths.push(p.clone());
                FileMergeStatus::Divergent
            }
            (Some(_), None) => {
                plan.unique_a += 1;
                FileMergeStatus::UniqueA
            }
            (None, Some(_)) => {
                plan.unique_b += 1;
                FileMergeStatus::UniqueB
            }
            (None, None) => unreachable!("path came from one of the two maps"),
        };
        plan.files.push(FileMerge {
            relative_path: p.clone(),
            status,
            sha_a: sa.cloned(),
            sha_b: sb.cloned(),
        });
    }

    let names_a = crate_names(repo_a)?;
    let names_b = crate_names(repo_b)?;
    plan.crate_collisions = names_a
        .iter()
        .filter(|n| names_b.contains(*n))
        .cloned()
        .collect();
    plan.crate_collisions.sort_unstable();
    plan.crate_collisions.dedup();

    Ok(plan)
}

/// `relative_path -> sha256` for source-relevant files. Skips vendor/generated
/// classifications plus any `target/` or `.git/` path — those are regenerated,
/// not merged.
fn source_hashes(root: &Path) -> Result<BTreeMap<String, String>, ScanError> {
    let mut map = BTreeMap::new();
    for entry in scan_filesystem(root)? {
        if entry.kind.as_str() != "file" || entry.is_symlink {
            continue;
        }
        if matches!(
            entry.classification,
            FileClassification::Vendor | FileClassification::Generated
        ) {
            continue;
        }
        if is_excluded_path(&entry.relative_path) {
            continue;
        }
        let abs = root.join(&entry.relative_path);
        if let Ok(bytes) = std::fs::read(&abs) {
            map.insert(entry.relative_path, format!("{:x}", Sha256::digest(&bytes)));
        }
    }
    Ok(map)
}

/// Cargo `[package]` names declared under `root` (workspace crate names).
fn crate_names(root: &Path) -> Result<Vec<String>, ScanError> {
    let mut names = Vec::new();
    for entry in scan_filesystem(root)? {
        if entry.classification != FileClassification::CargoManifest {
            continue;
        }
        if is_excluded_path(&entry.relative_path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&entry.relative_path)) else {
            continue;
        };
        if let Some(name) = parse_package_name(&text) {
            names.push(name);
        }
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

fn is_excluded_path(rel: &str) -> bool {
    rel.starts_with(".git/")
        || rel == ".git"
        || rel.starts_with("target/")
        || rel.contains("/target/")
        || rel.contains("/node_modules/")
        || rel.starts_with("node_modules/")
}

/// Extract the `name` from a Cargo manifest's `[package]` table without a toml
/// dependency. Returns None for virtual manifests (workspace-only, no package).
fn parse_package_name(toml: &str) -> Option<String> {
    let mut in_package = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        if t.starts_with('[') {
            in_package = t == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = t.strip_prefix("name") else {
            continue;
        };
        let Some(val) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let v = val.trim().trim_matches('"').trim_matches('\'');
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("codedb-merge-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn classifies_identical_divergent_and_unique() {
        let a = tmp("a");
        let b = tmp("b");
        // identical file (same content in both)
        write(&a, "src/same.rs", "fn same() {}\n");
        write(&b, "src/same.rs", "fn same() {}\n");
        // divergent file (same path, different content)
        write(&a, "src/diff.rs", "fn a() {}\n");
        write(&b, "src/diff.rs", "fn b() {}\n");
        // unique to each
        write(&a, "src/only_a.rs", "fn a() {}\n");
        write(&b, "src/only_b.rs", "fn b() {}\n");

        let plan = merge_plan(&a, &b).unwrap();
        assert_eq!(plan.identical, 1, "same.rs identical");
        assert_eq!(plan.divergent, 1, "diff.rs divergent");
        assert_eq!(plan.unique_a, 1);
        assert_eq!(plan.unique_b, 1);
        assert_eq!(plan.divergent_paths, vec!["src/diff.rs".to_string()]);

        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn detects_crate_name_collisions() {
        let a = tmp("ca");
        let b = tmp("cb");
        write(
            &a,
            "crates/cli/Cargo.toml",
            "[package]\nname = \"shared-cli\"\nversion = \"0.1.0\"\n",
        );
        write(&a, "crates/cli/src/lib.rs", "// a\n");
        write(
            &b,
            "crates/cli/Cargo.toml",
            "[package]\nname = \"shared-cli\"\nversion = \"0.2.0\"\n",
        );
        write(&b, "crates/cli/src/lib.rs", "// b\n");
        write(
            &b,
            "crates/only_b/Cargo.toml",
            "[package]\nname = \"only-b\"\nversion = \"0.1.0\"\n",
        );

        let plan = merge_plan(&a, &b).unwrap();
        assert_eq!(plan.crate_collisions, vec!["shared-cli".to_string()]);
        // the two Cargo.toml at crates/cli diverge (different version) -> divergent
        assert!(plan
            .divergent_paths
            .iter()
            .any(|p| p == "crates/cli/Cargo.toml"));

        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn parse_package_name_skips_virtual_manifest() {
        assert_eq!(
            parse_package_name("[package]\nname = \"x\"\n"),
            Some("x".to_string())
        );
        // virtual workspace manifest: no [package]
        assert_eq!(parse_package_name("[workspace]\nmembers = [\"a\"]\n"), None);
        // name outside [package] must not match
        assert_eq!(
            parse_package_name("[dependencies]\nname = \"nope\"\n"),
            None
        );
    }
}

//! ARCHBP-006 — envctl production projection engine.
//!
//! Projects a selected approved database branch to DISPOSABLE .rs/.toml/.nu
//! files with exact bytes, module_path metadata, permissions, safe relative
//! links, deterministic ordering, and source-version traceability, then a
//! receipt drives complete cleanup. The engine writes only under the given
//! output directory and NEVER overwrites canonical source
//! (overwrites_canonical()=false); unsafe (absolute or parent-escaping) paths
//! are rejected.

#![forbid(unsafe_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub const SCHEMA_VERSION: &str = "production-projection.v0";

#[derive(Clone, Debug)]
pub struct ProjectedFile {
    pub rel_path: String,
    pub bytes: Vec<u8>,
    pub module_path: Option<String>,
    pub mode: u32,
}

#[derive(Clone, Debug)]
pub struct Branch {
    pub id: String,
    pub version: String,
    pub files: Vec<ProjectedFile>,
}

#[derive(Clone, Debug)]
pub struct ProjectedEntry {
    pub rel_path: String,
    pub byte_len: usize,
    pub module_path: Option<String>,
    pub mode: u32,
    pub source_version: String,
}

#[derive(Clone, Debug)]
pub struct ProjectionReceipt {
    pub branch_id: String,
    pub branch_version: String,
    pub projected: Vec<ProjectedEntry>,
}

/// Whether the engine overwrites canonical source. Always false — projection
/// writes only to the given disposable output directory.
pub fn overwrites_canonical() -> bool {
    false
}

/// A publishable link/path is safe only if it is relative and never escapes the
/// output root.
pub fn is_safe_link(target: &str) -> bool {
    !target.starts_with('/') && !target.split('/').any(|seg| seg == "..")
}

/// Deterministically project a branch into `output_dir`.
pub fn project(branch: &Branch, output_dir: &Path) -> Result<ProjectionReceipt, String> {
    // Reject any unsafe path before writing anything (fail closed).
    for f in &branch.files {
        if !is_safe_link(&f.rel_path) {
            return Err(format!("path-escape:{}", f.rel_path));
        }
    }

    // Deterministic ordering by relative path.
    let mut files: Vec<&ProjectedFile> = branch.files.iter().collect();
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    let mut projected = Vec::with_capacity(files.len());
    for f in files {
        let dest = output_dir.join(&f.rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        fs::write(&dest, &f.bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
        fs::set_permissions(&dest, fs::Permissions::from_mode(f.mode))
            .map_err(|e| format!("chmod {}: {e}", dest.display()))?;
        projected.push(ProjectedEntry {
            rel_path: f.rel_path.clone(),
            byte_len: f.bytes.len(),
            module_path: f.module_path.clone(),
            mode: f.mode,
            source_version: branch.version.clone(),
        });
    }

    Ok(ProjectionReceipt {
        branch_id: branch.id.clone(),
        branch_version: branch.version.clone(),
        projected,
    })
}

/// Receipt-driven cleanup: remove exactly the projected files. Returns the count
/// removed.
pub fn cleanup(receipt: &ProjectionReceipt, output_dir: &Path) -> Result<usize, String> {
    let mut removed = 0usize;
    for entry in &receipt.projected {
        let path = output_dir.join(&entry.rel_path);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("rm {}: {e}", path.display()))?;
            removed += 1;
        }
    }
    Ok(removed)
}

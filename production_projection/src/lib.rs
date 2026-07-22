//! ARCHBP-006 — RED STUB. Contract surface only; the projection engine is
//! unimplemented so the projection gate fails closed before the real engine
//! lands.

#![forbid(unsafe_code)]

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
/// writes only to a disposable output directory.
pub fn overwrites_canonical() -> bool {
    true
}

pub fn is_safe_link(_target: &str) -> bool {
    true
}

pub fn project(_branch: &Branch, _output_dir: &Path) -> Result<ProjectionReceipt, String> {
    Err("ARCHBP-006 not implemented".to_string())
}

pub fn cleanup(_receipt: &ProjectionReceipt, _output_dir: &Path) -> Result<usize, String> {
    Ok(0)
}

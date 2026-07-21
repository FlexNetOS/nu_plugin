//! ARCHBP-024: the generated single-binary CodeDB snapshot artifact.
//!
//! A bounded, deterministically generated snapshot pack is embedded at
//! compile time together with its manifest, per-file checksums, and license
//! manifest. The library verifies before it materializes, refuses unsafe
//! overwrites by default, stages materialization so a failure leaves no
//! partial tree, and never embeds secret-shaped content. No claim here
//! exceeds the bounded artifact.

use serde::{Deserialize, Serialize};

/// The embedded snapshot pack (zstd-compressed deterministic archive).
pub const EMBEDDED_PACK: &[u8] = include_bytes!("../assets/codedb-pack.zst");
/// The embedded manifest JSON.
pub const EMBEDDED_MANIFEST: &str = include_str!("../assets/manifest.json");
/// The embedded per-file checksum lines.
pub const EMBEDDED_CHECKSUMS: &str = include_str!("../assets/checksums.sha256");
/// The embedded license manifest JSON.
pub const EMBEDDED_LICENSE_MANIFEST: &str = include_str!("../assets/license-manifest.json");

/// Snapshot schema version.
pub const SNAPSHOT_SCHEMA_VERSION: &str = "codedb.single-binary-snapshot.v0";

#[derive(Debug)]
pub struct ExportError(String);

impl ExportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ExportError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub path: String,
    pub unix_mode: String,
    pub sha256: String,
    pub byte_length: u64,
    pub bytes_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub schema_version: String,
    pub snapshot_source: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub pack_sha256: String,
}

#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub pack_sha256_ok: bool,
    pub per_file_checksums_ok: bool,
    pub file_count: u64,
}

#[derive(Debug, Clone)]
pub struct SchemaInfo {
    pub schema_version: String,
}

#[derive(Debug, Clone)]
pub struct SnapshotSummary {
    pub file_count: u64,
    pub total_bytes: u64,
    pub snapshot_source: String,
}

#[derive(Debug, Clone)]
pub struct LicenseComponent {
    pub name: String,
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct LicenseReport {
    pub components: Vec<LicenseComponent>,
}

#[derive(Debug, Clone)]
pub struct MaterializeReceipt {
    pub files_written: u64,
}

pub fn verify_embedded() -> Result<VerifyReport, ExportError> {
    Err(ExportError::new("verify_embedded is not implemented"))
}

pub fn verify_pack(
    pack: &[u8],
    manifest_json: &str,
    checksums: &str,
) -> Result<Vec<SnapshotEntry>, ExportError> {
    let _ = (pack, manifest_json, checksums);
    Err(ExportError::new("verify_pack is not implemented"))
}

pub fn list_entries() -> Result<Vec<SnapshotEntry>, ExportError> {
    Err(ExportError::new("list_entries is not implemented"))
}

pub fn schema_info() -> Result<SchemaInfo, ExportError> {
    Err(ExportError::new("schema_info is not implemented"))
}

pub fn summary() -> Result<SnapshotSummary, ExportError> {
    Err(ExportError::new("summary is not implemented"))
}

pub fn license_report() -> Result<LicenseReport, ExportError> {
    Err(ExportError::new("license_report is not implemented"))
}

pub fn materialize_embedded(
    target: &std::path::Path,
    allow_overwrite: bool,
) -> Result<MaterializeReceipt, ExportError> {
    let _ = (target, allow_overwrite);
    Err(ExportError::new("materialize_embedded is not implemented"))
}

pub fn materialize_pack(
    pack: &[u8],
    manifest_json: &str,
    checksums: &str,
    target: &std::path::Path,
    allow_overwrite: bool,
) -> Result<MaterializeReceipt, ExportError> {
    let _ = (pack, manifest_json, checksums, target, allow_overwrite);
    Err(ExportError::new("materialize_pack is not implemented"))
}

pub fn generate_assets(out_dir: &std::path::Path) -> Result<(), ExportError> {
    let _ = out_dir;
    Err(ExportError::new("generate_assets is not implemented"))
}

pub fn export_entry(path: &str, destination: &std::path::Path) -> Result<u64, ExportError> {
    let _ = (path, destination);
    Err(ExportError::new("export_entry is not implemented"))
}

/// All-peer consolidation rehearsal (read-only).
pub mod rehearsal {
    use super::ExportError;
    use serde::Serialize;
    use std::path::Path;

    /// Versioned rehearsal receipt.
    pub const REHEARSAL_RECEIPT_SCHEMA_VERSION: &str =
        "codedb.consolidation-rehearsal-receipt.v0";

    #[derive(Debug, Clone, Serialize)]
    pub struct RehearsalReceipt {
        pub schema_version: String,
        pub units_checked: u64,
        pub capabilities_accounted: u64,
        pub peers_checked: u64,
        pub all_preserved: bool,
        pub bounded_claim: String,
        pub findings: Vec<String>,
    }

    /// Walk the CONSOLIDATE-003 contract read-only: every retirement unit's
    /// independent repository, pinned head, and lockfile must be preserved;
    /// every adopted capability must map to a unit; every provenance peer
    /// must remain an independent repository.
    pub fn run(spine_root: &Path, out_path: &Path) -> Result<RehearsalReceipt, ExportError> {
        let _ = (spine_root, out_path);
        Err(ExportError::new("rehearsal::run is not implemented"))
    }
}

//! ARCHBP-024: the generated single-binary CodeDB snapshot artifact.
//!
//! A bounded, deterministically generated snapshot pack is embedded at
//! compile time together with its manifest, per-file checksums, and license
//! manifest. The library verifies before it materializes, refuses unsafe
//! overwrites by default, stages materialization so a failure leaves no
//! partial tree, and never embeds secret-shaped content. No claim here
//! exceeds the bounded artifact.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

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

fn internal(message: impl std::fmt::Display) -> ExportError {
    ExportError::new(message.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotArchive {
    schema_version: String,
    entries: Vec<SnapshotEntry>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseComponent {
    pub name: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseReport {
    pub components: Vec<LicenseComponent>,
}

#[derive(Debug, Clone)]
pub struct MaterializeReceipt {
    pub files_written: u64,
}

/// The bounded snapshot corpus. Deterministic in-code content: a
/// representative CodeDB snapshot slice (tables, store report, source
/// samples). Nothing secret-shaped, nothing environment-dependent.
fn snapshot_corpus() -> Vec<(&'static str, &'static str, &'static [u8])> {
    vec![
        (
            "snapshot/tables/source_blobs.jsonl",
            "644",
            b"{\"sha256\":\"9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\",\"byte_length\":4}\n"
                .as_slice(),
        ),
        (
            "snapshot/tables/source_files.jsonl",
            "644",
            b"{\"relative_path\":\"src/example.nu\",\"blob_ref\":\"sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08\"}\n"
                .as_slice(),
        ),
        (
            "snapshot/tables/source_file_metadata.jsonl",
            "644",
            b"{\"key\":\"src/example.nu::unix_mode\",\"value\":\"755\"}\n".as_slice(),
        ),
        (
            "snapshot/store_report.json",
            "644",
            b"{\"schema_version\":\"codedb.store-report.v0\",\"backend\":\"redb\",\"tables\":4}\n"
                .as_slice(),
        ),
        ("src/example.nu", "755", b"test".as_slice()),
        (
            "README.md",
            "644",
            b"# Bounded CodeDB snapshot\n\nGenerated deterministically; verify before use.\n"
                .as_slice(),
        ),
    ]
}

fn build_archive() -> SnapshotArchive {
    let mut entries: Vec<SnapshotEntry> = snapshot_corpus()
        .into_iter()
        .map(|(path, mode, bytes)| SnapshotEntry {
            path: path.to_string(),
            unix_mode: mode.to_string(),
            sha256: sha256_hex(bytes),
            byte_length: bytes.len() as u64,
            bytes_base64: BASE64.encode(bytes),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    SnapshotArchive {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        entries,
    }
}

fn license_components() -> Vec<LicenseComponent> {
    let component = |name: &str, license: &str| LicenseComponent {
        name: name.to_string(),
        license: license.to_string(),
    };
    vec![
        component("codedb-snapshot-content", "MIT (FlexNetOS nu_plugin repository)"),
        component("serde", "MIT OR Apache-2.0"),
        component("serde_json", "MIT OR Apache-2.0"),
        component("sha2", "MIT OR Apache-2.0"),
        component("base64", "MIT OR Apache-2.0"),
        component("zstd (rust bindings + libzstd)", "MIT (bindings) / BSD-3-Clause (libzstd)"),
    ]
}

/// Generate the four asset files deterministically into `out_dir`.
pub fn generate_assets(out_dir: &Path) -> Result<(), ExportError> {
    std::fs::create_dir_all(out_dir).map_err(internal)?;
    let archive = build_archive();
    let archive_json =
        serde_json::to_string_pretty(&archive).map_err(internal)? + "\n";
    let pack = zstd::encode_all(archive_json.as_bytes(), 19).map_err(internal)?;
    let pack_sha256 = sha256_hex(&pack);
    let manifest = SnapshotManifest {
        schema_version: SNAPSHOT_SCHEMA_VERSION.to_string(),
        snapshot_source: "codedb-bounded-fixture-corpus-v0".to_string(),
        file_count: archive.entries.len() as u64,
        total_bytes: archive.entries.iter().map(|e| e.byte_length).sum(),
        pack_sha256: pack_sha256.clone(),
    };
    let mut checksums = String::new();
    for entry in &archive.entries {
        checksums.push_str(&format!("{}  {}\n", entry.sha256, entry.path));
    }
    checksums.push_str(&format!("{pack_sha256}  codedb-pack.zst\n"));
    let licenses = LicenseReport {
        components: license_components(),
    };
    std::fs::write(out_dir.join("codedb-pack.zst"), &pack).map_err(internal)?;
    std::fs::write(
        out_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(internal)? + "\n",
    )
    .map_err(internal)?;
    std::fs::write(out_dir.join("checksums.sha256"), &checksums).map_err(internal)?;
    std::fs::write(
        out_dir.join("license-manifest.json"),
        serde_json::to_string_pretty(&licenses).map_err(internal)? + "\n",
    )
    .map_err(internal)?;
    Ok(())
}

/// Verify a pack against its manifest and checksum lines; return the
/// decoded entries only when every digest holds.
pub fn verify_pack(
    pack: &[u8],
    manifest_json: &str,
    checksums: &str,
) -> Result<Vec<SnapshotEntry>, ExportError> {
    let manifest: SnapshotManifest =
        serde_json::from_str(manifest_json).map_err(|e| internal(format!("manifest: {e}")))?;
    if manifest.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(ExportError::new(format!(
            "unsupported snapshot schema: {}",
            manifest.schema_version
        )));
    }
    if sha256_hex(pack) != manifest.pack_sha256 {
        return Err(ExportError::new(
            "pack bytes do not match the manifest's pack_sha256",
        ));
    }
    let archive_json = zstd::decode_all(pack).map_err(|e| internal(format!("pack: {e}")))?;
    let archive: SnapshotArchive =
        serde_json::from_slice(&archive_json).map_err(|e| internal(format!("archive: {e}")))?;
    if archive.entries.len() as u64 != manifest.file_count {
        return Err(ExportError::new("entry count disagrees with the manifest"));
    }
    let mut total = 0u64;
    for entry in &archive.entries {
        let bytes = BASE64
            .decode(&entry.bytes_base64)
            .map_err(|e| internal(format!("{}: {e}", entry.path)))?;
        if bytes.len() as u64 != entry.byte_length {
            return Err(ExportError::new(format!(
                "{} declares {} bytes but decodes to {}",
                entry.path,
                entry.byte_length,
                bytes.len()
            )));
        }
        if sha256_hex(&bytes) != entry.sha256 {
            return Err(ExportError::new(format!(
                "{} does not match its recorded digest",
                entry.path
            )));
        }
        if !checksums.contains(&format!("{}  {}", entry.sha256, entry.path)) {
            return Err(ExportError::new(format!(
                "{} is missing from the checksum lines",
                entry.path
            )));
        }
        total += entry.byte_length;
    }
    if total != manifest.total_bytes {
        return Err(ExportError::new("total bytes disagree with the manifest"));
    }
    Ok(archive.entries)
}

pub fn verify_embedded() -> Result<VerifyReport, ExportError> {
    let entries = verify_pack(EMBEDDED_PACK, EMBEDDED_MANIFEST, EMBEDDED_CHECKSUMS)?;
    Ok(VerifyReport {
        pack_sha256_ok: true,
        per_file_checksums_ok: true,
        file_count: entries.len() as u64,
    })
}

pub fn list_entries() -> Result<Vec<SnapshotEntry>, ExportError> {
    verify_pack(EMBEDDED_PACK, EMBEDDED_MANIFEST, EMBEDDED_CHECKSUMS)
}

pub fn schema_info() -> Result<SchemaInfo, ExportError> {
    let manifest: SnapshotManifest =
        serde_json::from_str(EMBEDDED_MANIFEST).map_err(internal)?;
    Ok(SchemaInfo {
        schema_version: manifest.schema_version,
    })
}

pub fn summary() -> Result<SnapshotSummary, ExportError> {
    let manifest: SnapshotManifest =
        serde_json::from_str(EMBEDDED_MANIFEST).map_err(internal)?;
    Ok(SnapshotSummary {
        file_count: manifest.file_count,
        total_bytes: manifest.total_bytes,
        snapshot_source: manifest.snapshot_source,
    })
}

pub fn license_report() -> Result<LicenseReport, ExportError> {
    serde_json::from_str(EMBEDDED_LICENSE_MANIFEST).map_err(internal)
}

/// Materialize a verified pack into `target` through a staging directory:
/// verification precedes every write, and the target appears only on a
/// fully successful rename — a failure leaves it untouched.
pub fn materialize_pack(
    pack: &[u8],
    manifest_json: &str,
    checksums: &str,
    target: &Path,
    allow_overwrite: bool,
) -> Result<MaterializeReceipt, ExportError> {
    let entries = verify_pack(pack, manifest_json, checksums)?;
    if target.exists() {
        let occupied = std::fs::read_dir(target).map_err(internal)?.next().is_some();
        if occupied && !allow_overwrite {
            return Err(ExportError::new(format!(
                "{} is not empty; pass --allow-overwrite to replace it",
                target.display()
            )));
        }
    }
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(internal)?;
    let staging = parent.join(format!(
        ".{}.staging-{}",
        target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(),
        std::process::id()
    ));
    std::fs::remove_dir_all(&staging).ok();
    std::fs::create_dir_all(&staging).map_err(internal)?;
    let mut written = 0u64;
    for entry in &entries {
        let bytes = BASE64.decode(&entry.bytes_base64).map_err(internal)?;
        let file_path = staging.join(&entry.path);
        if let Some(dir) = file_path.parent() {
            std::fs::create_dir_all(dir).map_err(internal)?;
        }
        std::fs::write(&file_path, &bytes).map_err(internal)?;
        let mode = u32::from_str_radix(&entry.unix_mode, 8).map_err(internal)?;
        std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(mode))
            .map_err(internal)?;
        written += 1;
    }
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(internal)?;
    }
    std::fs::rename(&staging, target).map_err(internal)?;
    Ok(MaterializeReceipt {
        files_written: written,
    })
}

pub fn materialize_embedded(
    target: &Path,
    allow_overwrite: bool,
) -> Result<MaterializeReceipt, ExportError> {
    materialize_pack(
        EMBEDDED_PACK,
        EMBEDDED_MANIFEST,
        EMBEDDED_CHECKSUMS,
        target,
        allow_overwrite,
    )
}

/// Export one embedded entry to an explicit destination file.
pub fn export_entry(path: &str, destination: &Path) -> Result<u64, ExportError> {
    let entries = list_entries()?;
    let entry = entries
        .iter()
        .find(|e| e.path == path)
        .ok_or_else(|| ExportError::new(format!("no embedded entry named {path}")))?;
    let bytes = BASE64.decode(&entry.bytes_base64).map_err(internal)?;
    std::fs::write(destination, &bytes).map_err(internal)?;
    Ok(bytes.len() as u64)
}

/// All-peer consolidation rehearsal (read-only).
pub mod rehearsal {
    use super::{ExportError, internal};
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

    fn repo_preserved(repo_path: &Path, pinned_head: Option<&str>, findings: &mut Vec<String>) {
        let label = repo_path.display();
        if !repo_path.join(".git").exists() {
            findings.push(format!("{label}: not an independent git repository"));
            return;
        }
        if !repo_path.join("Cargo.lock").exists() && !repo_path.join("flake.lock").exists() {
            findings.push(format!("{label}: no lockfile preserved"));
        }
        if let Some(head) = pinned_head {
            let reachable = std::process::Command::new("git")
                .args(["-C"])
                .arg(repo_path)
                .args(["cat-file", "-e", &format!("{head}^{{commit}}")])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if !reachable {
                findings.push(format!("{label}: pinned head {head} unreachable"));
            }
        }
    }

    /// Walk the CONSOLIDATE-003 contract read-only: every retirement unit's
    /// independent repository, pinned head, and lockfile must be preserved;
    /// every adopted capability must map to a unit repo; every provenance
    /// peer must remain an independent repository.
    pub fn run(spine_root: &Path, out_path: &Path) -> Result<RehearsalReceipt, ExportError> {
        let contract: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                spine_root.join("generated/task_families/CONSOLIDATE-003.json"),
            )
            .map_err(internal)?,
        )
        .map_err(internal)?;
        let mut findings = Vec::new();

        let units = contract["retirement_units"]
            .as_array()
            .ok_or_else(|| ExportError::new("contract lacks retirement_units"))?;
        let mut unit_repos = Vec::new();
        for unit in units {
            let peer_path = unit["peer_path"]
                .as_str()
                .ok_or_else(|| ExportError::new("unit lacks peer_path"))?;
            let pinned = unit["pinned_head"].as_str();
            repo_preserved(Path::new(peer_path), pinned, &mut findings);
            if let Some(repo) = unit["peer_repo"].as_str() {
                unit_repos.push(repo.rsplit('/').next().unwrap_or(repo).to_string());
            }
        }

        // Every adopted capability must name a source repo covered by a unit.
        let capabilities_csv = std::fs::read_to_string(
            spine_root.join("generated/adopted_capabilities.source.csv"),
        )
        .map_err(internal)?;
        let mut capability_count = 0u64;
        for (index, line) in capabilities_csv.lines().enumerate() {
            if index == 0 || line.trim().is_empty() {
                continue;
            }
            capability_count += 1;
            let covered = unit_repos.iter().any(|repo| line.contains(repo.as_str()));
            if !covered {
                findings.push(format!(
                    "capability row {index} names no retirement-unit repo: {}",
                    line.chars().take(80).collect::<String>()
                ));
            }
        }

        // Every provenance peer must remain an independent repository.
        let provenance: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                spine_root.join("generated/preserve_capability_provenance.json"),
            )
            .map_err(internal)?,
        )
        .map_err(internal)?;
        let peers = provenance["peers"]
            .as_array()
            .ok_or_else(|| ExportError::new("provenance lacks peers"))?;
        for peer in peers {
            let Some(repo_path) = peer["repo_path"].as_str() else {
                findings.push("provenance peer lacks repo_path".to_string());
                continue;
            };
            let path = Path::new(repo_path);
            let is_transient_worktree = peer["git_kind"].as_str() == Some("worktree")
                && peer["adoption_status"].as_str() == Some("worktree-transient");
            if !path.exists() {
                if is_transient_worktree {
                    // The inventory itself classifies these as transient
                    // worktrees of a parent clone; their capability content
                    // is preserved iff the parent (same origin) survives.
                    let origin = peer["remote_origin_url"].as_str().unwrap_or("");
                    let parent_preserved = peers.iter().any(|candidate| {
                        candidate["git_kind"].as_str() == Some("clone")
                            && candidate["remote_origin_url"].as_str() == Some(origin)
                            && candidate["repo_path"]
                                .as_str()
                                .map(|p| Path::new(p).join(".git").exists())
                                .unwrap_or(false)
                    });
                    if !parent_preserved {
                        findings.push(format!(
                            "{repo_path}: transient worktree gone AND its parent clone                              ({origin}) is not preserved"
                        ));
                    }
                } else {
                    findings.push(format!("{repo_path}: peer path missing"));
                }
            } else if peer["git_kind"].as_str() != Some("none")
                && !path.join(".git").exists()
            {
                findings.push(format!("{repo_path}: peer lost its independent .git"));
            }
        }

        let receipt = RehearsalReceipt {
            schema_version: REHEARSAL_RECEIPT_SCHEMA_VERSION.to_string(),
            units_checked: units.len() as u64,
            capabilities_accounted: capability_count,
            peers_checked: peers.len() as u64,
            all_preserved: findings.is_empty(),
            bounded_claim: "read-only rehearsal over the bounded CONSOLIDATE-003 contract; \
                            nothing was retired, merged, or cut over, and no release claim \
                            exceeds the bounded embedded artifact"
                .to_string(),
            findings: findings.clone(),
        };
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(internal)?;
        }
        std::fs::write(
            out_path,
            serde_json::to_string_pretty(&receipt).map_err(internal)? + "\n",
        )
        .map_err(internal)?;
        Ok(receipt)
    }
}

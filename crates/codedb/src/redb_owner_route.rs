//! Route CodeDB's redb landing through `flexnetos-redb-owner` (ARCHBP D09/D10).
//!
//! The blueprint's D10 names the owner the "only writable Database handle" and
//! has CodeDB reach it by "authenticated UDS mutation"; D09 puts CodeDB's
//! "backend-neutral mapping" immediately before the "redb owner client
//! protocol". This module is that mapping: one ingest file becomes a set of
//! namespaced key/value entries applied in a single owner transaction, so the
//! record commits atomically under one `local_seq` and publishes exactly one
//! ordered commit event.
//!
//! When no owner is running the caller keeps its direct-store bootstrap path
//! (blueprint §1.1); an owner that is running takes over the write (§1.2).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use flexnetos_redb_owner::{OwnerClient, StateEntry};

/// Key namespaces mirroring the store tables the direct path writes.
const BLAKE3_INDEX_NS: &str = "codedb/blake3-index";
const SOURCE_BLOB_NS: &str = "codedb/source-blob";
const SOURCE_FILE_NS: &str = "codedb/source-file";
const SOURCE_FILE_METADATA_NS: &str = "codedb/source-file-metadata";
const RAW_OBJECT_NS: &str = "codedb/raw-object";

/// Artifact kind marking rows written by `codedb ingest-envelope`, matching
/// the direct-store path so both routes describe the same artifact.
const INGEST_ARTIFACT_KIND: &str = "ingest_envelope_file";

/// One owner-routed ingest write.
#[derive(Debug, Clone)]
pub struct OwnerIngestRow {
    pub relative_path: String,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
    pub deduplicated: bool,
    /// The owner's post-commit `local_seq` for this record.
    pub local_seq: u64,
}

/// Resolve the owner root that this process should mutate through, if one is
/// actually serving. `FLEXNETOS_REDB_OWNER_ROOT` overrides the profile default.
/// A root without a live `owner.sock` yields `None` so the caller stays on the
/// bootstrap path instead of failing.
pub fn active_owner_root() -> Option<PathBuf> {
    let root = match std::env::var_os("FLEXNETOS_REDB_OWNER_ROOT") {
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(std::env::var_os("HOME")?)
            .join("meta")
            .join("var")
            .join("lib")
            .join("redb"),
    };
    root.join("owner.sock").exists().then_some(root)
}

/// One owner-routed raw-object write.
#[derive(Debug, Clone)]
pub struct OwnerRawRow {
    pub deduplicated: bool,
    /// The owner's post-commit `local_seq` for this raw object.
    pub local_seq: u64,
}

/// Land one canonical raw object (D07's rtk_nu tee path) in the owner
/// transaction: the exact bytes and their metadata commit together, so the
/// "canonical raw-object linkage" edge cannot observe a half-written object.
pub fn persist_raw_object_via_owner(
    owner_root: &Path,
    raw_object_id: &str,
    bytes: &[u8],
    metadata_json: &str,
) -> Result<OwnerRawRow, String> {
    let sha256 = raw_object_id
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("raw_object_id {raw_object_id} is not sha256-prefixed"))?;
    let mut client = OwnerClient::connect(owner_root)
        .map_err(|error| format!("owner connect failed: {error}"))?;

    let deduplicated = client
        .get(&format!("{SOURCE_BLOB_NS}/{sha256}"))
        .map_err(|error| format!("owner dedup probe failed: {error}"))?
        .is_some();

    let local_seq = client
        .put_many(&[
            StateEntry {
                key: format!("{SOURCE_BLOB_NS}/{sha256}"),
                value: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
            StateEntry {
                key: format!("{RAW_OBJECT_NS}/{raw_object_id}"),
                value: metadata_json.to_string(),
            },
        ])
        .map_err(|error| format!("owner write failed: {error}"))?;

    Ok(OwnerRawRow {
        deduplicated,
        local_seq,
    })
}

/// Map one ingest file onto backend-neutral entries and apply them through the
/// authenticated owner socket in a single transaction.
pub fn persist_ingest_file_via_owner(
    owner_root: &Path,
    relative_path: &str,
    bytes: &[u8],
    sha256: &str,
    blake3: &str,
    unix_mode: &str,
    module_path: &str,
    ast_json: &str,
) -> Result<OwnerIngestRow, String> {
    let blob_ref = format!("sha256:{sha256}");
    let mut client = OwnerClient::connect(owner_root)
        .map_err(|error| format!("owner connect failed: {error}"))?;

    let deduplicated = client
        .get(&format!("{BLAKE3_INDEX_NS}/{blake3}"))
        .map_err(|error| format!("owner dedup probe failed: {error}"))?
        .is_some();

    let mut entries = vec![
        StateEntry {
            key: format!("{BLAKE3_INDEX_NS}/{blake3}"),
            value: sha256.to_string(),
        },
        StateEntry {
            key: format!("{SOURCE_BLOB_NS}/{sha256}"),
            value: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        StateEntry {
            key: format!("{SOURCE_FILE_NS}/{relative_path}"),
            value: blob_ref.clone(),
        },
    ];
    for (field, value) in [
        ("artifact_kind", INGEST_ARTIFACT_KIND),
        ("permission_capture", "unix_mode"),
        ("unix_mode", unix_mode),
        ("module_path", module_path),
        ("blake3", blake3),
        ("nu_ast", ast_json),
    ] {
        entries.push(StateEntry {
            key: format!("{SOURCE_FILE_METADATA_NS}/{relative_path}#{field}"),
            value: value.to_string(),
        });
    }

    let local_seq = client
        .put_many(&entries)
        .map_err(|error| format!("owner write failed: {error}"))?;

    Ok(OwnerIngestRow {
        relative_path: relative_path.to_string(),
        blob_ref,
        sha256: sha256.to_string(),
        bytes: bytes.len() as u64,
        deduplicated,
        local_seq,
    })
}

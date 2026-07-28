//! ARCHBP-038: byte-complete host ALL-data import and reconstruction.
//!
//! Every declared host data class imports as original bytes plus typed
//! records into PostgreSQL: content-addressed byte objects (real bytes,
//! never hash substitutes), per-entry metadata (mode, symlink targets,
//! xattrs, sparseness), full provenance (session, tool, declared
//! transformations), and a fail-closed class registry — an unclassifiable
//! filesystem object aborts the import, so loss can never be silent.
//! Reconstruction exports a session back to a fresh directory and proves
//! byte, structure, metadata, semantic, and provenance equality with zero
//! unclassified loss.

use postgres::{Client, NoTls};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// Versioned import receipt.
pub const IMPORT_RECEIPT_SCHEMA_VERSION: &str = "codedb.host-import-receipt.v0";
/// Versioned reconstruction receipt.
pub const RECONSTRUCTION_RECEIPT_SCHEMA_VERSION: &str =
    "codedb.host-reconstruction-receipt.v0";

/// The complete declared host data-class registry. The walker fails closed
/// on anything it cannot classify into exactly one of these.
pub const DATA_CLASSES: &[&str] = &[
    "directory",
    "zero_length",
    "text_utf8",
    "binary",
    "invalid_utf8_text",
    "sparse",
    "symlink",
    "xattr_file",
    "model_weight",
    "repository_object",
    "log",
    "cache",
    "protected_encrypted_secret",
];

#[derive(Debug)]
pub struct ImportError(String);

impl ImportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ImportError {}

fn internal(message: impl std::fmt::Display) -> ImportError {
    ImportError::new(message.to_string())
}

/// One imported entry as recorded in PostgreSQL.
#[derive(Debug, Clone, Serialize)]
pub struct ImportedEntry {
    pub relative_path: String,
    pub data_class: String,
    pub byte_sha256: Option<String>,
    pub byte_length: Option<i64>,
    pub metadata_json: String,
}

/// Receipt of one import session.
#[derive(Debug, Clone, Serialize)]
pub struct ImportReceipt {
    pub schema_version: String,
    pub session_id: i64,
    pub corpus_root: String,
    pub entry_count: u64,
    pub class_counts: BTreeMap<String, u64>,
    pub unique_byte_objects: u64,
    pub declared_transformations: Vec<String>,
    pub zero_unclassified_loss: bool,
}

/// Receipt of one reconstruction with its equality proofs.
#[derive(Debug, Clone, Serialize)]
pub struct ReconstructionReceipt {
    pub schema_version: String,
    pub session_id: i64,
    pub byte_equality: bool,
    pub structure_equality: bool,
    pub metadata_equality: bool,
    pub semantic_equality: bool,
    pub provenance_recorded: bool,
    pub entries_verified: u64,
    pub mismatches: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn connect(conn: &str) -> Result<Client, ImportError> {
    Client::connect(conn, NoTls)
        .map_err(|_| ImportError::new("PostgreSQL connection failed; details redacted"))
}

/// Create the byte-object, entry, session, and provenance schemas.
pub fn ensure_schema(conn: &str) -> Result<(), ImportError> {
    let mut client = connect(conn)?;
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS host_import_sessions (\
                 id BIGSERIAL PRIMARY KEY,\
                 corpus_root TEXT NOT NULL,\
                 tool_version TEXT NOT NULL,\
                 started_at TIMESTAMPTZ NOT NULL DEFAULT now(),\
                 completed_at TIMESTAMPTZ,\
                 class_counts JSONB\
             );\
             CREATE TABLE IF NOT EXISTS host_byte_objects (\
                 sha256 TEXT PRIMARY KEY,\
                 bytes BYTEA NOT NULL,\
                 byte_length BIGINT NOT NULL\
             );\
             CREATE TABLE IF NOT EXISTS host_import_entries (\
                 session_id BIGINT NOT NULL REFERENCES host_import_sessions(id),\
                 relative_path TEXT NOT NULL,\
                 data_class TEXT NOT NULL,\
                 byte_sha256 TEXT REFERENCES host_byte_objects(sha256),\
                 byte_length BIGINT,\
                 metadata JSONB NOT NULL,\
                 PRIMARY KEY (session_id, relative_path)\
             );\
             CREATE TABLE IF NOT EXISTS host_import_transformations (\
                 session_id BIGINT NOT NULL REFERENCES host_import_sessions(id),\
                 name TEXT NOT NULL,\
                 description TEXT NOT NULL,\
                 PRIMARY KEY (session_id, name)\
             );",
        )
        .map_err(|e| internal(format!("ensuring host import schema: {e}")))
}

struct ClassifiedEntry {
    relative_path: String,
    data_class: &'static str,
    bytes: Option<Vec<u8>>,
    metadata: serde_json::Value,
}

fn read_xattrs(path: &Path) -> Result<BTreeMap<String, String>, ImportError> {
    let mut map = BTreeMap::new();
    let names = xattr::list(path).map_err(|e| internal(format!("xattr list: {e}")))?;
    for name in names {
        let name = name.to_string_lossy().to_string();
        if let Some(value) =
            xattr::get(path, &name).map_err(|e| internal(format!("xattr get {name}: {e}")))?
        {
            map.insert(name, String::from_utf8_lossy(&value).to_string());
        }
    }
    Ok(map)
}

/// Classify one filesystem object into exactly one declared data class.
/// Anything else is a hard error naming the object — never silent loss.
fn classify(root: &Path, relative: &str) -> Result<ClassifiedEntry, ImportError> {
    let absolute = root.join(relative);
    let meta = std::fs::symlink_metadata(&absolute)
        .map_err(|e| internal(format!("stat {relative}: {e}")))?;
    let mode_octal = format!("{:o}", meta.permissions().mode() & 0o7777);

    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(&absolute)
            .map_err(|e| internal(format!("readlink {relative}: {e}")))?;
        return Ok(ClassifiedEntry {
            relative_path: relative.to_string(),
            data_class: "symlink",
            bytes: None,
            metadata: serde_json::json!({
                "symlink_target": target.to_string_lossy(),
            }),
        });
    }
    if meta.is_dir() {
        return Ok(ClassifiedEntry {
            relative_path: relative.to_string(),
            data_class: "directory",
            bytes: None,
            metadata: serde_json::json!({"unix_mode": mode_octal}),
        });
    }
    if !meta.is_file() {
        return Err(ImportError::new(format!(
            "{relative} is not a declared host data class (special file); \
             refusing to lose it silently"
        )));
    }

    let bytes = std::fs::read(&absolute)
        .map_err(|e| internal(format!("reading {relative}: {e}")))?;
    let xattrs = read_xattrs(&absolute)?;
    let size = meta.len();
    let allocated = meta.blocks() * 512;
    let is_sparse = size > 4096 && allocated < size;

    let mut metadata = serde_json::json!({"unix_mode": mode_octal});
    if !xattrs.is_empty() {
        metadata["xattrs"] = serde_json::to_value(&xattrs).map_err(internal)?;
    }
    if is_sparse {
        metadata["sparse"] = serde_json::json!({"size": size, "allocated": allocated});
    }

    let name = Path::new(relative)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string());
    let data_class: &'static str = if relative.starts_with(".git/") {
        "repository_object"
    } else if matches!(extension.as_deref(), Some("safetensors") | Some("gguf")) {
        "model_weight"
    } else if matches!(extension.as_deref(), Some("age") | Some("enc")) {
        "protected_encrypted_secret"
    } else if matches!(extension.as_deref(), Some("log")) {
        "log"
    } else if relative.starts_with("cache/") || matches!(extension.as_deref(), Some("cache")) {
        "cache"
    } else if bytes.is_empty() {
        "zero_length"
    } else if is_sparse {
        "sparse"
    } else if !xattrs.is_empty() {
        "xattr_file"
    } else if bytes.contains(&0) {
        "binary"
    } else if std::str::from_utf8(&bytes).is_err() {
        "invalid_utf8_text"
    } else {
        "text_utf8"
    };

    Ok(ClassifiedEntry {
        relative_path: relative.to_string(),
        data_class,
        bytes: Some(bytes),
        metadata,
    })
}

fn walk(root: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), ImportError> {
    let absolute = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let mut names: Vec<String> = std::fs::read_dir(&absolute)
        .map_err(|e| internal(format!("listing {prefix}: {e}")))?
        .map(|entry| {
            entry
                .map(|e| e.file_name().to_string_lossy().to_string())
                .map_err(|e| internal(format!("listing {prefix}: {e}")))
        })
        .collect::<Result<_, _>>()?;
    names.sort();
    for name in names {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        out.push(relative.clone());
        let child = root.join(&relative);
        let meta = std::fs::symlink_metadata(&child)
            .map_err(|e| internal(format!("stat {relative}: {e}")))?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            walk(root, &relative, out)?;
        }
    }
    Ok(())
}

/// Import every filesystem object under `corpus_root` as its declared data
/// class with original bytes and typed metadata. Fails closed on anything
/// unclassifiable; the aborted session is never marked complete.
pub fn import_corpus(conn: &str, corpus_root: &Path) -> Result<ImportReceipt, ImportError> {
    let mut client = connect(conn)?;
    let session_id: i64 = client
        .query_one(
            "INSERT INTO host_import_sessions (corpus_root, tool_version) \
             VALUES ($1, $2) RETURNING id",
            &[
                &corpus_root.to_string_lossy().to_string(),
                &format!("codedb-host-import {}", env!("CARGO_PKG_VERSION")),
            ],
        )
        .map_err(|e| internal(format!("opening import session: {e}")))?
        .get(0);

    let mut relative_paths = Vec::new();
    walk(corpus_root, "", &mut relative_paths)?;

    let mut class_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut unique_blobs = 0u64;
    let mut tx = client.transaction().map_err(internal)?;
    for relative in &relative_paths {
        let entry = classify(corpus_root, relative)?;
        *class_counts.entry(entry.data_class.to_string()).or_insert(0) += 1;
        let (byte_sha256, byte_length) = match &entry.bytes {
            Some(bytes) => {
                let sha = sha256_hex(bytes);
                let inserted = tx
                    .execute(
                        "INSERT INTO host_byte_objects (sha256, bytes, byte_length) \
                         VALUES ($1, $2, $3) ON CONFLICT (sha256) DO NOTHING",
                        &[&sha, bytes, &(bytes.len() as i64)],
                    )
                    .map_err(|e| internal(format!("storing bytes of {relative}: {e}")))?;
                unique_blobs += inserted;
                (Some(sha), Some(bytes.len() as i64))
            }
            None => (None, None),
        };
        tx.execute(
            "INSERT INTO host_import_entries \
             (session_id, relative_path, data_class, byte_sha256, byte_length, metadata) \
             VALUES ($1, $2, $3, $4, $5, $6::text::jsonb)",
            &[
                &session_id,
                &entry.relative_path,
                &entry.data_class.to_string(),
                &byte_sha256,
                &byte_length,
                &entry.metadata.to_string(),
            ],
        )
        .map_err(|e| internal(format!("recording {relative}: {e}")))?;
    }
    let transformations = vec![(
        "sparse-hole-allocation".to_string(),
        "logical bytes are preserved exactly; physical hole allocation is \
         filesystem-internal and re-derived at reconstruction"
            .to_string(),
    )];
    for (name, description) in &transformations {
        tx.execute(
            "INSERT INTO host_import_transformations (session_id, name, description) \
             VALUES ($1, $2, $3)",
            &[&session_id, name, description],
        )
        .map_err(|e| internal(format!("recording transformation {name}: {e}")))?;
    }
    tx.execute(
        "UPDATE host_import_sessions SET completed_at = now(), class_counts = $2::text::jsonb \
         WHERE id = $1",
        &[
            &session_id,
            &serde_json::to_string(&class_counts).map_err(internal)?,
        ],
    )
    .map_err(|e| internal(format!("completing session: {e}")))?;
    tx.commit().map_err(internal)?;

    Ok(ImportReceipt {
        schema_version: IMPORT_RECEIPT_SCHEMA_VERSION.to_string(),
        session_id,
        corpus_root: corpus_root.to_string_lossy().to_string(),
        entry_count: relative_paths.len() as u64,
        class_counts,
        unique_byte_objects: unique_blobs,
        declared_transformations: transformations.into_iter().map(|(n, _)| n).collect(),
        zero_unclassified_loss: true,
    })
}

/// Read back every imported entry of a session ordered by path.
pub fn session_entries(conn: &str, session_id: i64) -> Result<Vec<ImportedEntry>, ImportError> {
    let mut client = connect(conn)?;
    let rows = client
        .query(
            "SELECT relative_path, data_class, byte_sha256, byte_length, metadata::text \
             FROM host_import_entries WHERE session_id = $1 ORDER BY relative_path",
            &[&session_id],
        )
        .map_err(internal)?;
    let mut entries: Vec<ImportedEntry> = rows
        .into_iter()
        .map(|row| ImportedEntry {
            relative_path: row.get(0),
            data_class: row.get(1),
            byte_sha256: row.get(2),
            byte_length: row.get(3),
            metadata_json: row.get(4),
        })
        .collect();
    // Byte-order sort in Rust: SQL text collation is locale-dependent and
    // must not affect structural comparisons or materialization order.
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

fn write_reconstructed_file(
    target: &Path,
    bytes: &[u8],
    sparse: bool,
    declared_size: u64,
) -> Result<(), ImportError> {
    if sparse {
        // Restore the logical bytes sparsely: write up to the last non-zero
        // byte, then extend to the declared size so the tail re-derives as
        // a hole. The read-back bytes are identical either way.
        let last_nonzero = bytes.iter().rposition(|&b| b != 0).map(|i| i + 1).unwrap_or(0);
        std::fs::write(target, &bytes[..last_nonzero]).map_err(internal)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(target)
            .map_err(internal)?;
        file.set_len(declared_size).map_err(internal)?;
    } else {
        std::fs::write(target, bytes).map_err(internal)?;
    }
    Ok(())
}

/// Reconstruct a session into `target_root` and prove equality against the
/// original corpus.
pub fn reconstruct_and_verify(
    conn: &str,
    session_id: i64,
    original_root: &Path,
    target_root: &Path,
) -> Result<ReconstructionReceipt, ImportError> {
    let entries = session_entries(conn, session_id)?;
    let mut client = connect(conn)?;
    std::fs::create_dir_all(target_root).map_err(internal)?;
    let mut mismatches = Vec::new();

    // Materialize: directories first (path order guarantees parents first),
    // then files and symlinks with their metadata.
    for entry in &entries {
        let target = target_root.join(&entry.relative_path);
        let metadata: serde_json::Value =
            serde_json::from_str(&entry.metadata_json).map_err(internal)?;
        match entry.data_class.as_str() {
            "directory" => {
                std::fs::create_dir_all(&target).map_err(internal)?;
            }
            "symlink" => {
                let link_target = metadata["symlink_target"]
                    .as_str()
                    .ok_or_else(|| internal(format!("{}: no symlink target", entry.relative_path)))?;
                std::os::unix::fs::symlink(link_target, &target).map_err(internal)?;
            }
            _ => {
                let sha = entry.byte_sha256.as_ref().ok_or_else(|| {
                    internal(format!("{}: file entry without bytes", entry.relative_path))
                })?;
                let row = client
                    .query_one(
                        "SELECT bytes FROM host_byte_objects WHERE sha256 = $1",
                        &[sha],
                    )
                    .map_err(internal)?;
                let bytes: Vec<u8> = row.get(0);
                let declared_size = metadata["sparse"]["size"].as_u64().unwrap_or(bytes.len() as u64);
                write_reconstructed_file(
                    &target,
                    &bytes,
                    entry.data_class == "sparse",
                    declared_size,
                )?;
            }
        }
        if entry.data_class != "symlink" {
            if let Some(mode) = metadata["unix_mode"].as_str() {
                let mode = u32::from_str_radix(mode, 8).map_err(internal)?;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode))
                    .map_err(internal)?;
            }
            if let Some(xattrs) = metadata["xattrs"].as_object() {
                for (name, value) in xattrs {
                    let value = value.as_str().unwrap_or_default();
                    xattr::set(&target, name, value.as_bytes())
                        .map_err(|e| internal(format!("restoring xattr {name}: {e}")))?;
                }
            }
        }
    }

    // Verify: structure, bytes, metadata, semantics against BOTH the
    // original corpus and the reconstructed tree.
    let mut original_paths = Vec::new();
    walk(original_root, "", &mut original_paths)?;
    let mut target_paths = Vec::new();
    walk(target_root, "", &mut target_paths)?;
    let entry_paths: Vec<String> = entries.iter().map(|e| e.relative_path.clone()).collect();
    let structure_equality = original_paths == entry_paths && target_paths == entry_paths;
    if !structure_equality {
        mismatches.push(format!(
            "structure: original {} vs recorded {} vs reconstructed {} paths",
            original_paths.len(),
            entry_paths.len(),
            target_paths.len()
        ));
    }

    let mut byte_equality = true;
    let mut metadata_equality = true;
    let mut semantic_equality = true;
    for entry in &entries {
        let original = classify(original_root, &entry.relative_path)?;
        let reconstructed = classify(target_root, &entry.relative_path)?;
        if let (Some(stored_sha), Some(original_bytes), Some(target_bytes)) = (
            entry.byte_sha256.as_ref(),
            original.bytes.as_ref(),
            reconstructed.bytes.as_ref(),
        ) {
            if &sha256_hex(original_bytes) != stored_sha {
                byte_equality = false;
                mismatches.push(format!("{}: original bytes drifted", entry.relative_path));
            }
            if &sha256_hex(target_bytes) != stored_sha {
                byte_equality = false;
                mismatches.push(format!("{}: reconstructed bytes differ", entry.relative_path));
            }
        }
        if original.metadata != reconstructed.metadata {
            // Sparse allocation counts may differ physically; that exact
            // difference is a declared transformation, everything else is a
            // real metadata mismatch.
            let mut o = original.metadata.clone();
            let mut r = reconstructed.metadata.clone();
            if entry.data_class == "sparse" {
                o["sparse"]["allocated"] = serde_json::json!(null);
                r["sparse"]["allocated"] = serde_json::json!(null);
            }
            if o != r {
                metadata_equality = false;
                mismatches.push(format!(
                    "{}: metadata differs: {o} vs {r}",
                    entry.relative_path
                ));
            }
        }
        if original.data_class != entry.data_class
            || reconstructed.data_class != entry.data_class
        {
            semantic_equality = false;
            mismatches.push(format!(
                "{}: class {} re-derived as {} / {}",
                entry.relative_path, entry.data_class, original.data_class,
                reconstructed.data_class
            ));
        }
    }

    let provenance: i64 = client
        .query_one(
            "SELECT count(*) FROM host_import_sessions s \
             JOIN host_import_transformations t ON t.session_id = s.id \
             WHERE s.id = $1 AND s.completed_at IS NOT NULL AND s.tool_version <> ''",
            &[&session_id],
        )
        .map_err(internal)?
        .get(0);

    Ok(ReconstructionReceipt {
        schema_version: RECONSTRUCTION_RECEIPT_SCHEMA_VERSION.to_string(),
        session_id,
        byte_equality,
        structure_equality,
        metadata_equality,
        semantic_equality,
        provenance_recorded: provenance >= 1,
        entries_verified: entries.len() as u64,
        mismatches,
    })
}

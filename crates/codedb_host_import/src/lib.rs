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

use serde::Serialize;
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
    pub class_counts: std::collections::BTreeMap<String, u64>,
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

/// Create the byte-object, chunk, entry, session, and provenance schemas.
pub fn ensure_schema(conn: &str) -> Result<(), ImportError> {
    let _ = conn;
    Err(ImportError::new("ensure_schema is not implemented"))
}

/// Import every filesystem object under `corpus_root` as its declared data
/// class with original bytes and typed metadata. Fails closed on anything
/// unclassifiable.
pub fn import_corpus(conn: &str, corpus_root: &Path) -> Result<ImportReceipt, ImportError> {
    let _ = (conn, corpus_root);
    Err(ImportError::new("import_corpus is not implemented"))
}

/// Read back every imported entry of a session ordered by path.
pub fn session_entries(conn: &str, session_id: i64) -> Result<Vec<ImportedEntry>, ImportError> {
    let _ = (conn, session_id);
    Err(ImportError::new("session_entries is not implemented"))
}

/// Reconstruct a session into `target_root` and prove equality against the
/// original corpus.
pub fn reconstruct_and_verify(
    conn: &str,
    session_id: i64,
    original_root: &Path,
    target_root: &Path,
) -> Result<ReconstructionReceipt, ImportError> {
    let _ = (conn, session_id, original_root, target_root);
    Err(ImportError::new("reconstruct_and_verify is not implemented"))
}

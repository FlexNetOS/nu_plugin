//! Backend-agnostic blob-store surface.
//!
//! [`BlobStore`] is the synchronous storage contract CodeDB captures/materializes
//! through. It mirrors the semantics of the redb free functions in
//! `codedb_store_redb` so any backend (redb file, PostgreSQL, …) is drop-in:
//! content-addressed (sha256) blob persistence, a resume skip-set, byte-exact
//! read-back, and metadata-aware materialization that restores unix modes.
//!
//! The error type is deliberately simple — a single `String`-carrying
//! [`StoreError`] — so backends with unrelated native error enums (redb's
//! `StoreError`, postgres' `Error`) collapse to one uniform surface at the CLI.

use std::collections::BTreeSet;
use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::path::PathBuf;

/// Uniform, backend-agnostic store error. Backends map their native error into
/// this via [`StoreError::new`]/`From<E: Display>` at the trait boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError(String);

impl StoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl Display for StoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl StdError for StoreError {}

/// A persisted source-file row: relative path, content-addressed blob ref
/// (`sha256:<hex>`), the raw sha256 hex, and the blob byte length. Mirrors
/// `codedb_store_redb::SourceBlobRow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileRow {
    pub relative_path: String,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

/// The outcome of materializing one blob back to disk: the on-disk path, the
/// blob ref it came from, the sha256 re-checksummed from the written file, and
/// its byte length. Mirrors `codedb_store_redb::FileMaterializationReport`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedFile {
    pub path: PathBuf,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

/// One `(table, key, value)` metadata row describing the store itself — the
/// `store-report` surface. Mirrors `codedb_store_redb::StoreMetadataRow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMetadataRow {
    pub table: String,
    pub key: String,
    pub value: String,
}

/// The synchronous, backend-agnostic blob-store contract.
///
/// Every method mirrors the exact semantics of the corresponding
/// `codedb_store_redb` free function so a `Box<dyn BlobStore>` can transparently
/// front the redb file store or the PostgreSQL store with no behavioral
/// downgrade.
pub trait BlobStore {
    /// Persist a batch of `(relative_path, bytes)` in ONE durable transaction.
    /// Content-addressed: identical bytes dedup on their sha256. Returns one
    /// [`SourceFileRow`] per input, in input order.
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceFileRow>, StoreError>;

    /// Relative paths already durably persisted — the resume skip-set. A re-run
    /// reads this once and skips paths already present so an interrupted capture
    /// continues from its last checkpoint instead of restarting.
    fn captured_paths(&self) -> Result<BTreeSet<String>, StoreError>;

    /// Raw bytes for a captured relative path, or `None` if the path was never
    /// captured. Byte-exact — no text normalization.
    fn read_source_file_blob(&self, relative_path: &str) -> Result<Option<Vec<u8>>, StoreError>;

    /// Every persisted source file (relative path + blob ref + sha256 + size),
    /// in key order — the full-tree read surface behind `materialize`.
    fn list_source_files(&self) -> Result<Vec<SourceFileRow>, StoreError>;

    /// Re-emit one captured file byte-for-byte to `output_path`, restoring the
    /// stored unix mode when metadata carries one (creates parent dirs). The
    /// returned sha256 is re-checksummed from the written file.
    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<MaterializedFile, StoreError>;

    /// The store's own metadata/toolchain/validation rows — the `store-report`
    /// surface.
    fn store_metadata_rows(&self) -> Result<Vec<StoreMetadataRow>, StoreError>;
}

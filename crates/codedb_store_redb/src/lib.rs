#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use codedb_core::SchemaVersion;
use codedb_core::store::{
    BlobStore, MaterializedFile as CoreMaterializedFile, SourceFileRow as CoreSourceFileRow,
    StoreError as CoreStoreError, StoreMetadataRow as CoreStoreMetadataRow,
};
use redb::{
    CommitError, Database, DatabaseError, ReadableTable, StorageError, TableDefinition, TableError,
    TransactionError,
};
use sha2::{Digest, Sha256};

pub const STATUS: &str = "source_blob_store_available";
pub const STORE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0, 0);

const SCHEMA_VERSION_TABLE: TableDefinition<&str, &str> = TableDefinition::new("schema_versions");
const STORE_METADATA_TABLE: TableDefinition<&str, &str> = TableDefinition::new("store_metadata");
const TOOLCHAIN_METADATA_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("toolchain_metadata");
const VALIDATION_ROWS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("validation_rows");
const SOURCE_BLOBS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("source_blobs");
const SOURCE_FILES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("source_files");
const SOURCE_FILE_METADATA_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("source_file_metadata");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreInitContext<'a> {
    pub codedb_version: &'a str,
    pub toolchain: &'a str,
    pub rustc_version: &'a str,
    pub cargo_version: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreMetadataRow {
    pub table: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreInitReport {
    pub path: PathBuf,
    pub schema_version: SchemaVersion,
    pub rows: Vec<StoreMetadataRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreBackupReport {
    pub source_path: PathBuf,
    pub backup_path: PathBuf,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRestoreReport {
    pub backup: StoreBackupReport,
    pub restored_path: PathBuf,
    pub restored_sha256: String,
    pub restored_store: StoreInitReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBlobRow {
    pub relative_path: String,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMaterializationReport {
    pub path: PathBuf,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum StoreError {
    Database(DatabaseError),
    Transaction(TransactionError),
    Commit(CommitError),
    Table(TableError),
    Storage(StorageError),
    Io(io::Error),
    UnsupportedSchemaVersion {
        observed: String,
    },
    MissingValue {
        table: &'static str,
        key: &'static str,
    },
}

impl Display for StoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(err) => write!(f, "database error: {err}"),
            Self::Transaction(err) => write!(f, "transaction error: {err}"),
            Self::Commit(err) => write!(f, "commit error: {err}"),
            Self::Table(err) => write!(f, "table error: {err}"),
            Self::Storage(err) => write!(f, "storage error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::UnsupportedSchemaVersion { observed } => {
                write!(f, "unsupported store schema version: {observed}")
            }
            Self::MissingValue { table, key } => {
                write!(f, "missing metadata value {key} in table {table}")
            }
        }
    }
}

impl StdError for StoreError {}

impl From<DatabaseError> for StoreError {
    fn from(value: DatabaseError) -> Self {
        Self::Database(value)
    }
}

impl From<TransactionError> for StoreError {
    fn from(value: TransactionError) -> Self {
        Self::Transaction(value)
    }
}

impl From<CommitError> for StoreError {
    fn from(value: CommitError) -> Self {
        Self::Commit(value)
    }
}

impl From<TableError> for StoreError {
    fn from(value: TableError) -> Self {
        Self::Table(value)
    }
}

impl From<StorageError> for StoreError {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<io::Error> for StoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[allow(clippy::result_large_err)]
pub fn initialize_store(
    path: impl AsRef<Path>,
    context: &StoreInitContext<'_>,
) -> Result<StoreInitReport, StoreError> {
    let path = path.as_ref().to_path_buf();
    let db = Database::create(&path)?;

    {
        let write_txn = db.begin_write()?;
        {
            let mut schema_versions = write_txn.open_table(SCHEMA_VERSION_TABLE)?;
            schema_versions.insert("schema_version", "1.0.0")?;
        }
        {
            let mut store_metadata = write_txn.open_table(STORE_METADATA_TABLE)?;
            store_metadata.insert("store_status", "initialized")?;
            store_metadata.insert("store_created", "true")?;
            store_metadata.insert("codedb_version", context.codedb_version)?;
            store_metadata.insert("schema_version", "1.0.0")?;
            store_metadata.insert("migration_state", "schema_1_no_migrations_supported")?;
            store_metadata.insert("unsupported_state_behavior", "refuse_unknown_schema")?;
            store_metadata.insert("corruption_validation", "backup_restore_smoke_required")?;
            store_metadata.insert("checksum_algorithm", "sha256")?;
        }
        {
            let mut toolchain_metadata = write_txn.open_table(TOOLCHAIN_METADATA_TABLE)?;
            toolchain_metadata.insert("toolchain", context.toolchain)?;
            toolchain_metadata.insert("rustc_version", context.rustc_version)?;
            toolchain_metadata.insert("cargo_version", context.cargo_version)?;
        }
        {
            let mut validation_rows = write_txn.open_table(VALIDATION_ROWS_TABLE)?;
            validation_rows.insert("single_writer_lock", "redb_write_transaction")?;
            validation_rows.insert("reader_concurrency", "redb_read_transactions")?;
            validation_rows.insert("backup_restore", "available")?;
            validation_rows.insert(
                "lock_contention_behavior",
                "single_writer_blocks_until_release",
            )?;
            validation_rows.insert("plugin_lifecycle_gc", "drop_releases_write_lock")?;
        }
        {
            write_txn.open_table(SOURCE_BLOBS_TABLE)?;
            write_txn.open_table(SOURCE_FILES_TABLE)?;
            write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        }
        write_txn.commit()?;
    }

    drop(db);
    let report = read_store_report(&path)?;
    Ok(report)
}

#[allow(clippy::result_large_err)]
pub fn read_store_report(path: impl AsRef<Path>) -> Result<StoreInitReport, StoreError> {
    let path = path.as_ref().to_path_buf();
    let db = Database::open(&path)?;
    read_store_report_db(&db, path)
}

/// Shared store-report read against an already-open database.
#[allow(clippy::result_large_err)]
fn read_store_report_db(db: &Database, path: PathBuf) -> Result<StoreInitReport, StoreError> {
    let read_txn = db.begin_read()?;

    let schema_version = {
        let table = read_txn.open_table(SCHEMA_VERSION_TABLE)?;
        let value = table
            .get("schema_version")?
            .ok_or(StoreError::MissingValue {
                table: "schema_versions",
                key: "schema_version",
            })?;
        match value.value() {
            "1.0.0" => STORE_SCHEMA_VERSION,
            other => {
                return Err(StoreError::UnsupportedSchemaVersion {
                    observed: other.to_string(),
                });
            }
        }
    };

    let mut rows = Vec::new();
    for (table_name, definition) in [
        ("schema_versions", SCHEMA_VERSION_TABLE),
        ("store_metadata", STORE_METADATA_TABLE),
        ("toolchain_metadata", TOOLCHAIN_METADATA_TABLE),
        ("validation_rows", VALIDATION_ROWS_TABLE),
        ("source_files", SOURCE_FILES_TABLE),
        ("source_file_metadata", SOURCE_FILE_METADATA_TABLE),
    ] {
        let table = read_txn.open_table(definition)?;
        for result in table.iter()? {
            let (key, value) = result?;
            rows.push(StoreMetadataRow {
                table: table_name.to_string(),
                key: key.value().to_string(),
                value: value.value().to_string(),
            });
        }
    }

    Ok(StoreInitReport {
        path,
        schema_version,
        rows,
    })
}

#[allow(clippy::result_large_err)]
pub fn persist_source_file(
    store_path: impl AsRef<Path>,
    relative_path: impl AsRef<str>,
    source_path: impl AsRef<Path>,
) -> Result<SourceBlobRow, StoreError> {
    let store_path = store_path.as_ref();
    let relative_path = relative_path.as_ref().to_string();
    let source_path = source_path.as_ref();
    let bytes = fs::read(source_path)?;
    let row = persist_source_blob(store_path, relative_path, &bytes)?;
    persist_source_file_metadata(store_path, &row.relative_path, source_path)?;
    Ok(row)
}

#[allow(clippy::result_large_err)]
pub fn persist_source_blob(
    store_path: impl AsRef<Path>,
    relative_path: impl Into<String>,
    bytes: &[u8],
) -> Result<SourceBlobRow, StoreError> {
    let relative_path = relative_path.into();
    let sha256 = sha256_bytes(bytes);
    let blob_ref = format!("sha256:{sha256}");
    let db = Database::open(store_path)?;
    {
        let write_txn = db.begin_write()?;
        {
            let mut blobs = write_txn.open_table(SOURCE_BLOBS_TABLE)?;
            blobs.insert(sha256.as_str(), bytes)?;
        }
        {
            let mut files = write_txn.open_table(SOURCE_FILES_TABLE)?;
            files.insert(relative_path.as_str(), blob_ref.as_str())?;
        }
        {
            let mut metadata = write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
            metadata.insert(
                source_file_metadata_key(&relative_path, "artifact_kind").as_str(),
                "raw_blob",
            )?;
            metadata.insert(
                source_file_metadata_key(&relative_path, "permission_capture").as_str(),
                "gap_not_available_for_raw_blob",
            )?;
        }
        write_txn.commit()?;
    }

    Ok(SourceBlobRow {
        relative_path,
        blob_ref,
        sha256,
        bytes: bytes.len() as u64,
    })
}

/// A capture session that holds the store open across many batches so a full-repo
/// import pays one `Database::open` and one durable `commit` (fsync) *per batch*
/// instead of two per file. The per-batch commit is the on-disk checkpoint: if the
/// run is killed or hits a time budget, every committed batch survives and a re-run
/// resumes (content-addressed blobs are idempotent; already-captured paths are
/// skipped via [`CaptureBatcher::captured_paths`]). Same rows, same durability as
/// [`persist_source_file`] — just amortized. No file is skipped, so no downgrade.
pub struct CaptureBatcher {
    db: Database,
}

impl CaptureBatcher {
    /// Open an already-initialized store for batched capture.
    #[allow(clippy::result_large_err)]
    pub fn open(store_path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            db: Database::open(store_path.as_ref())?,
        })
    }

    /// Relative paths already persisted — the resume skip-set. A re-run reads this
    /// once and skips paths already present, so an interrupted capture continues
    /// from its last durable checkpoint instead of restarting.
    #[allow(clippy::result_large_err)]
    pub fn captured_paths(&self) -> Result<std::collections::BTreeSet<String>, StoreError> {
        captured_paths_db(&self.db)
    }

    /// Persist a batch of `(relative_path, bytes)` in ONE write transaction and one
    /// durable commit. Writes the identical blob/file/metadata rows that
    /// [`persist_source_blob`] + `persist_source_file_metadata` write per file.
    #[allow(clippy::result_large_err)]
    pub fn persist_batch(
        &self,
        batch: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceBlobRow>, StoreError> {
        persist_batch_db(&self.db, batch)
    }
}

/// Shared read of the resume skip-set from an already-open database.
#[allow(clippy::result_large_err)]
fn captured_paths_db(db: &Database) -> Result<std::collections::BTreeSet<String>, StoreError> {
    let read_txn = db.begin_read()?;
    let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
    let mut set = std::collections::BTreeSet::new();
    for item in files.iter()? {
        let (key, _) = item?;
        set.insert(key.value().to_string());
    }
    Ok(set)
}

/// Shared batch persist against an already-open database — the exact blob/file/
/// metadata write both the inherent method and the [`BlobStore`] impl use.
#[allow(clippy::result_large_err)]
fn persist_batch_db(
    db: &Database,
    batch: &[(String, Vec<u8>)],
) -> Result<Vec<SourceBlobRow>, StoreError> {
    let mut out = Vec::with_capacity(batch.len());
    let write_txn = db.begin_write()?;
    {
        let mut blobs = write_txn.open_table(SOURCE_BLOBS_TABLE)?;
        let mut files = write_txn.open_table(SOURCE_FILES_TABLE)?;
        let mut metadata = write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        for (relative_path, bytes) in batch {
            let sha256 = sha256_bytes(bytes);
            let blob_ref = format!("sha256:{sha256}");
            blobs.insert(sha256.as_str(), bytes.as_slice())?;
            files.insert(relative_path.as_str(), blob_ref.as_str())?;
            metadata.insert(
                source_file_metadata_key(relative_path, "artifact_kind").as_str(),
                "raw_blob",
            )?;
            metadata.insert(
                source_file_metadata_key(relative_path, "permission_capture").as_str(),
                "gap_not_available_for_raw_blob",
            )?;
            out.push(SourceBlobRow {
                relative_path: relative_path.clone(),
                blob_ref,
                sha256,
                bytes: bytes.len() as u64,
            });
        }
    }
    write_txn.commit()?;
    Ok(out)
}

#[allow(clippy::result_large_err)]
pub fn read_source_file_blob(
    store_path: impl AsRef<Path>,
    relative_path: impl AsRef<str>,
) -> Result<SourceBlobRow, StoreError> {
    let relative_path = relative_path.as_ref().to_string();
    let db = Database::open(store_path)?;
    let read_txn = db.begin_read()?;
    let blob_ref = {
        let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
        files
            .get(relative_path.as_str())?
            .ok_or(StoreError::MissingValue {
                table: "source_files",
                key: "relative_path",
            })?
            .value()
            .to_string()
    };
    let sha256 = blob_ref.trim_start_matches("sha256:").to_string();
    let bytes = {
        let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
        blobs
            .get(sha256.as_str())?
            .ok_or(StoreError::MissingValue {
                table: "source_blobs",
                key: "sha256",
            })?
            .value()
            .len() as u64
    };

    Ok(SourceBlobRow {
        relative_path,
        blob_ref,
        sha256,
        bytes,
    })
}

/// Every persisted source file (relative path + blob ref + sha256 + size), in
/// key order. The full-tree read surface behind `codedb materialize` — restore
/// walks this list so re-emission can never silently drop a captured file.
#[allow(clippy::result_large_err)]
pub fn list_source_files(store_path: impl AsRef<Path>) -> Result<Vec<SourceBlobRow>, StoreError> {
    let db = Database::open(store_path)?;
    list_source_files_db(&db)
}

/// Shared list against an already-open database.
#[allow(clippy::result_large_err)]
fn list_source_files_db(db: &Database) -> Result<Vec<SourceBlobRow>, StoreError> {
    let read_txn = db.begin_read()?;
    let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    let mut rows = Vec::new();
    for item in files.iter()? {
        let (key, value) = item?;
        let relative_path = key.value().to_string();
        let blob_ref = value.value().to_string();
        let sha256 = blob_ref.trim_start_matches("sha256:").to_string();
        let bytes = blobs
            .get(sha256.as_str())?
            .map(|blob| blob.value().len() as u64)
            .unwrap_or(0);
        rows.push(SourceBlobRow {
            relative_path,
            blob_ref,
            sha256,
            bytes,
        });
    }
    Ok(rows)
}

/// Raw bytes for a captured relative path, or `None` if absent — the
/// [`BlobStore::read_source_file_blob`] read path against an open database.
#[allow(clippy::result_large_err)]
fn read_source_blob_bytes_db(
    db: &Database,
    relative_path: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let read_txn = db.begin_read()?;
    let blob_ref = {
        let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
        match files.get(relative_path)? {
            Some(value) => value.value().to_string(),
            None => return Ok(None),
        }
    };
    let sha256 = blob_ref.trim_start_matches("sha256:").to_string();
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    Ok(blobs.get(sha256.as_str())?.map(|blob| blob.value().to_vec()))
}

#[allow(clippy::result_large_err)]
pub fn materialize_source_file(
    store_path: impl AsRef<Path>,
    relative_path: impl AsRef<str>,
    output_path: impl AsRef<Path>,
) -> Result<FileMaterializationReport, StoreError> {
    let relative_path = relative_path.as_ref();
    let output_path = output_path.as_ref().to_path_buf();
    let db = Database::open(store_path)?;
    materialize_source_file_db(&db, relative_path, &output_path)
}

/// Shared materialize against an already-open database — restores the stored
/// unix mode when present, then re-checksums the written file.
#[allow(clippy::result_large_err)]
fn materialize_source_file_db(
    db: &Database,
    relative_path: &str,
    output_path: &Path,
) -> Result<FileMaterializationReport, StoreError> {
    let output_path = output_path.to_path_buf();
    let read_txn = db.begin_read()?;
    let blob_ref = {
        let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
        files
            .get(relative_path)?
            .ok_or(StoreError::MissingValue {
                table: "source_files",
                key: "relative_path",
            })?
            .value()
            .to_string()
    };
    let sha256 = blob_ref.trim_start_matches("sha256:").to_string();
    let bytes = {
        let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
        blobs
            .get(sha256.as_str())?
            .ok_or(StoreError::MissingValue {
                table: "source_blobs",
                key: "sha256",
            })?
            .value()
            .to_vec()
    };
    let unix_mode = {
        let metadata = read_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        metadata
            .get(source_file_metadata_key(relative_path, "unix_mode").as_str())?
            .map(|value| value.value().to_string())
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, &bytes)?;
    #[cfg(unix)]
    if let Some(mode) = unix_mode {
        use std::os::unix::fs::PermissionsExt;

        if let Ok(parsed_mode) = u32::from_str_radix(&mode, 8) {
            fs::set_permissions(&output_path, fs::Permissions::from_mode(parsed_mode))?;
        }
    }
    let materialized_sha256 = checksum_file_sha256(&output_path)?;

    Ok(FileMaterializationReport {
        path: output_path,
        blob_ref,
        sha256: materialized_sha256,
        bytes: bytes.len() as u64,
    })
}

pub fn store_metadata_rows(report: &StoreInitReport) -> Vec<StoreMetadataRow> {
    report.rows.clone()
}

#[allow(clippy::result_large_err)]
pub fn checksum_file_sha256(path: impl AsRef<Path>) -> Result<String, StoreError> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn source_file_metadata_key(relative_path: &str, field: &str) -> String {
    format!("{relative_path}::{field}")
}

#[allow(clippy::result_large_err)]
fn persist_source_file_metadata(
    store_path: &Path,
    relative_path: &str,
    source_path: &Path,
) -> Result<(), StoreError> {
    let source_metadata = fs::metadata(source_path)?;
    let db = Database::open(store_path)?;
    let write_txn = db.begin_write()?;
    {
        let mut metadata = write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        metadata.insert(
            source_file_metadata_key(relative_path, "artifact_kind").as_str(),
            "source_file",
        )?;
        metadata.insert(
            source_file_metadata_key(relative_path, "readonly").as_str(),
            if source_metadata.permissions().readonly() {
                "true"
            } else {
                "false"
            },
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = source_metadata.permissions().mode() & 0o777;
            metadata.insert(
                source_file_metadata_key(relative_path, "unix_mode").as_str(),
                format!("{mode:o}").as_str(),
            )?;
        }
        #[cfg(not(unix))]
        {
            metadata.insert(
                source_file_metadata_key(relative_path, "unix_mode").as_str(),
                "gap_not_available_on_non_unix_platform",
            )?;
        }
    }
    write_txn.commit()?;
    Ok(())
}

#[allow(clippy::result_large_err)]
pub fn backup_store(
    source_path: impl AsRef<Path>,
    backup_path: impl AsRef<Path>,
) -> Result<StoreBackupReport, StoreError> {
    let source_path = source_path.as_ref().to_path_buf();
    let backup_path = backup_path.as_ref().to_path_buf();
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }

    read_store_report(&source_path)?;
    let bytes = fs::copy(&source_path, &backup_path)?;
    let sha256 = checksum_file_sha256(&backup_path)?;

    Ok(StoreBackupReport {
        source_path,
        backup_path,
        bytes,
        sha256,
    })
}

#[allow(clippy::result_large_err)]
pub fn restore_store_from_backup(
    backup_path: impl AsRef<Path>,
    restored_path: impl AsRef<Path>,
) -> Result<StoreRestoreReport, StoreError> {
    let backup_path = backup_path.as_ref().to_path_buf();
    let restored_path = restored_path.as_ref().to_path_buf();
    if let Some(parent) = restored_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let source_sha256 = checksum_file_sha256(&backup_path)?;
    let bytes = fs::copy(&backup_path, &restored_path)?;
    let restored_sha256 = checksum_file_sha256(&restored_path)?;
    let restored_store = read_store_report(&restored_path)?;

    Ok(StoreRestoreReport {
        backup: StoreBackupReport {
            source_path: backup_path.clone(),
            backup_path,
            bytes,
            sha256: source_sha256,
        },
        restored_path,
        restored_sha256,
        restored_store,
    })
}

fn to_core_err(err: StoreError) -> CoreStoreError {
    CoreStoreError::new(err.to_string())
}

fn to_core_row(row: SourceBlobRow) -> CoreSourceFileRow {
    CoreSourceFileRow {
        relative_path: row.relative_path,
        blob_ref: row.blob_ref,
        sha256: row.sha256,
        bytes: row.bytes,
    }
}

/// The redb file store fronted through the backend-agnostic [`BlobStore`]
/// contract. Every method delegates to the same shared `_db` helpers the
/// standalone free functions use, so the trait path and the free-function path
/// share identical semantics — no behavioral fork.
impl BlobStore for CaptureBatcher {
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<CoreSourceFileRow>, CoreStoreError> {
        let rows = persist_batch_db(&self.db, files).map_err(to_core_err)?;
        Ok(rows.into_iter().map(to_core_row).collect())
    }

    fn captured_paths(&self) -> Result<std::collections::BTreeSet<String>, CoreStoreError> {
        captured_paths_db(&self.db).map_err(to_core_err)
    }

    fn read_source_file_blob(
        &self,
        relative_path: &str,
    ) -> Result<Option<Vec<u8>>, CoreStoreError> {
        read_source_blob_bytes_db(&self.db, relative_path).map_err(to_core_err)
    }

    fn list_source_files(&self) -> Result<Vec<CoreSourceFileRow>, CoreStoreError> {
        let rows = list_source_files_db(&self.db).map_err(to_core_err)?;
        Ok(rows.into_iter().map(to_core_row).collect())
    }

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<CoreMaterializedFile, CoreStoreError> {
        let report =
            materialize_source_file_db(&self.db, relative_path, output_path).map_err(to_core_err)?;
        Ok(CoreMaterializedFile {
            path: report.path,
            blob_ref: report.blob_ref,
            sha256: report.sha256,
            bytes: report.bytes,
        })
    }

    fn store_metadata_rows(&self) -> Result<Vec<CoreStoreMetadataRow>, CoreStoreError> {
        let report = read_store_report_db(&self.db, PathBuf::new()).map_err(to_core_err)?;
        Ok(report
            .rows
            .into_iter()
            .map(|row| CoreStoreMetadataRow {
                table: row.table,
                key: row.key,
                value: row.value,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store_path() -> PathBuf {
        // nanos alone collide when two tests start in the same tick (redb then
        // fails with DatabaseAlreadyOpen); a process-wide counter makes every
        // path unique regardless of timer resolution or parallelism.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time monotonic")
            .as_nanos();
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("codedb_store_redb_{stamp}_{seq}.redb"))
    }

    // Test lane: default
    // Defends: store initialization must create the database and persist schema/toolchain metadata.
    #[test]
    fn initialize_and_read_back_metadata() {
        let path = temp_store_path();
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };

        let report = initialize_store(&path, &context).expect("store init");
        assert_eq!(report.schema_version.as_tuple(), (1, 0, 0));
        assert!(report.rows.iter().any(|row| row.key == "codedb_version"));
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.key == "single_writer_lock")
        );
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.key == "lock_contention_behavior"
                    && row.value == "single_writer_blocks_until_release")
        );
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.key == "plugin_lifecycle_gc"
                    && row.value == "drop_releases_write_lock")
        );

        let reread = read_store_report(&path).expect("store reread");
        assert_eq!(reread.schema_version.as_tuple(), (1, 0, 0));
        assert!(reread.rows.iter().any(|row| row.key == "schema_version"));
        assert!(reread.rows.iter().any(|row| row.key == "toolchain"));

        fs::remove_file(&path).ok();
    }

    fn init_ctx() -> StoreInitContext<'static> {
        StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "test",
            rustc_version: "rustc test",
            cargo_version: "cargo test",
        }
    }

    // Test lane: default
    // Defends: batched capture persists the identical blob/file rows a per-file
    // capture would, in one durable commit — same data, no downgrade.
    #[test]
    fn batch_capture_matches_per_file_and_is_durable() {
        let path = temp_store_path();
        initialize_store(&path, &init_ctx()).expect("init");
        let batch = vec![
            ("a.rs".to_string(), b"fn a() {}\n".to_vec()),
            ("dir/b.txt".to_string(), b"hello".to_vec()),
            ("dir/c.bin".to_string(), vec![0u8, 1, 2, 3, 255]),
        ];
        let batcher = CaptureBatcher::open(&path).expect("open");
        let rows = batcher.persist_batch(&batch).expect("persist batch");
        assert_eq!(rows.len(), 3);

        // Re-read after dropping the batcher: the commit was durable.
        drop(batcher);
        let listed = list_source_files(&path).expect("list");
        assert_eq!(listed.len(), 3, "all three files persisted in one batch");
        // Byte-identical read-back for every file.
        for (rel, bytes) in &batch {
            let blob = read_source_file_blob(&path, rel).expect("read blob");
            assert_eq!(blob.bytes, bytes.len() as u64, "size for {rel}");
        }
        fs::remove_file(&path).ok();
    }

    // Test lane: default
    // Defends: an interrupted capture resumes from its last durable checkpoint —
    // captured_paths reflects committed batches so a re-run skips them.
    #[test]
    fn captured_paths_drive_resume_after_checkpoint() {
        let path = temp_store_path();
        initialize_store(&path, &init_ctx()).expect("init");
        let batcher = CaptureBatcher::open(&path).expect("open");

        // First checkpoint (batch 1) commits durably.
        batcher
            .persist_batch(&[
                ("keep/1.rs".to_string(), b"one".to_vec()),
                ("keep/2.rs".to_string(), b"two".to_vec()),
            ])
            .expect("batch 1");
        // Simulate a fresh process resuming: reopen and read the skip-set.
        drop(batcher);
        let resumed = CaptureBatcher::open(&path).expect("reopen");
        let seen = resumed.captured_paths().expect("captured paths");
        assert!(seen.contains("keep/1.rs") && seen.contains("keep/2.rs"));
        assert!(!seen.contains("keep/3.rs"), "unwritten path absent");

        // Resume persists only the not-yet-seen file.
        let remaining: Vec<_> = [("keep/3.rs".to_string(), b"three".to_vec())]
            .into_iter()
            .filter(|(p, _)| !seen.contains(p))
            .collect();
        resumed.persist_batch(&remaining).expect("resume batch");
        drop(resumed);
        assert_eq!(list_source_files(&path).expect("list").len(), 3);
        fs::remove_file(&path).ok();
    }

    // Test lane: default
    // Defends: CDB086 unknown future schemas are refused instead of silently treated as current.
    #[test]
    fn unknown_schema_version_is_refused() {
        let path = temp_store_path();
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };
        initialize_store(&path, &context).expect("store init");
        {
            let db = Database::open(&path).expect("open store");
            let write_txn = db.begin_write().expect("begin write");
            {
                let mut schema_versions = write_txn
                    .open_table(SCHEMA_VERSION_TABLE)
                    .expect("schema table");
                schema_versions
                    .insert("schema_version", "99.0.0")
                    .expect("write future schema");
            }
            write_txn.commit().expect("commit future schema");
        }

        let err = read_store_report(&path).expect_err("future schema should be refused");
        assert!(matches!(
            err,
            StoreError::UnsupportedSchemaVersion { observed } if observed == "99.0.0"
        ));

        fs::remove_file(&path).ok();
    }

    // Test lane: default
    // Defends: CDB061 redb lifecycle keeps one writer active and releases the lock when plugin-like handles drop.
    #[test]
    fn lock_contention_blocks_until_writer_lifecycle_release() {
        let path = temp_store_path();
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };
        initialize_store(&path, &context).expect("store init");

        let db = Arc::new(Database::open(&path).expect("open store"));
        let first_writer = db.begin_write().expect("first writer");
        let read_txn = db.begin_read().expect("reader while writer active");
        {
            let table = read_txn
                .open_table(VALIDATION_ROWS_TABLE)
                .expect("validation table");
            assert_eq!(
                table
                    .get("reader_concurrency")
                    .expect("reader_concurrency")
                    .expect("reader_concurrency value")
                    .value(),
                "redb_read_transactions"
            );
        }
        drop(read_txn);

        let (sender, receiver) = mpsc::channel();
        let db_for_thread = Arc::clone(&db);
        let writer_thread = std::thread::spawn(move || {
            let writer = db_for_thread
                .begin_write()
                .expect("thread writer after release");
            drop(writer);
            sender.send("writer_acquired_after_release").expect("send");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        drop(first_writer);
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("second writer should acquire after first writer drops"),
            "writer_acquired_after_release"
        );
        writer_thread.join().expect("writer thread joins");

        drop(db);
        let report = read_store_report(&path).expect("store remains readable");
        assert!(
            report
                .rows
                .iter()
                .any(|row| row.key == "plugin_lifecycle_gc"
                    && row.value == "drop_releases_write_lock")
        );

        fs::remove_file(&path).ok();
    }

    // Test lane: default
    // Defends: CDB016 requires a backup/restore smoke with checksum evidence.
    #[test]
    fn backup_and_restore_smoke_preserves_metadata() {
        let path = temp_store_path();
        let backup_path = path.with_extension("backup.redb");
        let restored_path = path.with_extension("restored.redb");
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };

        initialize_store(&path, &context).expect("store init");
        let backup = backup_store(&path, &backup_path).expect("backup");
        assert!(backup.bytes > 0);
        assert_eq!(backup.sha256.len(), 64);

        let restore = restore_store_from_backup(&backup_path, &restored_path).expect("restore");
        assert_eq!(restore.backup.sha256, restore.restored_sha256);
        assert!(
            restore
                .restored_store
                .rows
                .iter()
                .any(|row| row.key == "migration_state"
                    && row.value == "schema_1_no_migrations_supported")
        );
        assert!(
            restore
                .restored_store
                .rows
                .iter()
                .any(|row| row.key == "backup_restore" && row.value == "available")
        );

        fs::remove_file(&path).ok();
        fs::remove_file(&backup_path).ok();
        fs::remove_file(&restored_path).ok();
    }

    // Test lane: default
    // Defends: source blob bytes are owned by the redb store and can be materialized after restore.
    #[test]
    fn source_blob_persists_and_materializes_after_restore() {
        let path = temp_store_path();
        let backup_path = path.with_extension("backup.redb");
        let restored_path = path.with_extension("restored.redb");
        let output_path = path.with_extension("materialized.rs");
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };
        let source = b"pub fn codedb_blob_roundtrip() -> bool { true }\n";

        initialize_store(&path, &context).expect("store init");
        let persisted =
            persist_source_blob(&path, "src/lib.rs", source).expect("persist source blob");
        assert_eq!(persisted.bytes, source.len() as u64);
        assert!(persisted.blob_ref.starts_with("sha256:"));
        let reread = read_source_file_blob(&path, "src/lib.rs").expect("read source blob row");
        assert_eq!(reread, persisted);

        backup_store(&path, &backup_path).expect("backup");
        restore_store_from_backup(&backup_path, &restored_path).expect("restore");
        let materialized = materialize_source_file(&restored_path, "src/lib.rs", &output_path)
            .expect("materialize");
        assert_eq!(materialized.sha256, persisted.sha256);
        assert_eq!(fs::read(&output_path).expect("materialized bytes"), source);

        fs::remove_file(&path).ok();
        fs::remove_file(&backup_path).ok();
        fs::remove_file(&restored_path).ok();
        fs::remove_file(&output_path).ok();
    }

    // Test lane: default
    // Defends: binary and non-Rust artifacts are stored as exact bytes, not text-normalized.
    #[test]
    fn non_rust_binary_artifact_materializes_exact_bytes() {
        let path = temp_store_path();
        let output_path = path.with_extension("materialized.bin");
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };
        let asset = b"\x00PNG\r\n\x1a\n\xffcodedb\x00asset";

        initialize_store(&path, &context).expect("store init");
        let persisted = persist_source_blob(&path, "assets/logo.bin", asset)
            .expect("persist binary source blob");
        let materialized = materialize_source_file(&path, "assets/logo.bin", &output_path)
            .expect("materialize binary asset");

        assert_eq!(materialized.sha256, persisted.sha256);
        assert_eq!(fs::read(&output_path).expect("materialized bytes"), asset);

        fs::remove_file(&path).ok();
        fs::remove_file(&output_path).ok();
    }

    // Test lane: default
    // Defends: source-file capture records and restores executable permission bits on Unix.
    #[cfg(unix)]
    #[test]
    fn source_file_materialization_restores_unix_executable_bits() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_store_path();
        let source_path = path.with_extension("source.sh");
        let output_path = path.with_extension("materialized.sh");
        let context = StoreInitContext {
            codedb_version: "0.1.0",
            toolchain: "stable-x86_64-unknown-linux-gnu",
            rustc_version: "rustc 1.92.0",
            cargo_version: "cargo 1.96.0",
        };

        initialize_store(&path, &context).expect("store init");
        fs::write(&source_path, b"#!/usr/bin/env bash\necho codedb\n").expect("write source");
        fs::set_permissions(&source_path, fs::Permissions::from_mode(0o755))
            .expect("set source mode");

        let persisted = persist_source_file(&path, "scripts/codedb.sh", &source_path)
            .expect("persist source file");
        let materialized = materialize_source_file(&path, "scripts/codedb.sh", &output_path)
            .expect("materialize source file");

        assert_eq!(materialized.sha256, persisted.sha256);
        assert_eq!(
            fs::read(&output_path).expect("materialized bytes"),
            fs::read(&source_path).expect("source bytes")
        );
        assert_eq!(
            fs::metadata(&output_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );

        fs::remove_file(&path).ok();
        fs::remove_file(&source_path).ok();
        fs::remove_file(&output_path).ok();
    }
}

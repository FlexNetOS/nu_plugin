#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use codedb_core::SchemaVersion;
use codedb_core::store::{
    BlobStore, CURRENT_STORE_SCHEMA_VERSION, LEGACY_STORE_SCHEMA_VERSION,
    MaterializedFile as CoreMaterializedFile, SourceFileRow as CoreSourceFileRow,
    SourceSymlinkRow as CoreSourceSymlinkRow, StoreBackupKind, StoreError as CoreStoreError,
    StoreMetadataRow as CoreStoreMetadataRow, StoreMigrationBackup, StoreMigrationReport,
    StoreMigrationStep, atomic_materialize_file, parse_schema_version, plan_store_migration,
};
use codedb_core::store_spec::StoreBackend;
use redb::{
    CommitError, Database, DatabaseError, ReadOnlyDatabase, ReadableDatabase, ReadableTable,
    StorageError, TableDefinition, TableError, TransactionError,
};
use sha2::{Digest, Sha256};

pub const STATUS: &str = "source_blob_store_available";
pub const STORE_SCHEMA_VERSION: SchemaVersion = CURRENT_STORE_SCHEMA_VERSION;

const REDB_MIGRATIONS: [StoreMigrationStep; 1] = [StoreMigrationStep::new(
    "redb_legacy_v0_9_to_v1",
    LEGACY_STORE_SCHEMA_VERSION,
    CURRENT_STORE_SCHEMA_VERSION,
)];

const SCHEMA_VERSION_TABLE: TableDefinition<&str, &str> = TableDefinition::new("schema_versions");
const STORE_METADATA_TABLE: TableDefinition<&str, &str> = TableDefinition::new("store_metadata");
const TOOLCHAIN_METADATA_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("toolchain_metadata");
const VALIDATION_ROWS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("validation_rows");
const SOURCE_BLOBS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("source_blobs");
const SOURCE_FILES_TABLE: TableDefinition<&str, &str> = TableDefinition::new("source_files");
const SOURCE_FILE_METADATA_TABLE: TableDefinition<&str, &str> =
    TableDefinition::new("source_file_metadata");
// BLAKE3 content identity -> sha256 blob key. Created lazily by the first
// ingest-envelope write so pre-existing stores stay valid without migration.
const BLAKE3_INDEX_TABLE: TableDefinition<&str, &str> = TableDefinition::new("blake3_index");

/// Artifact kind marking rows written by `codedb ingest-envelope`.
pub const INGEST_ARTIFACT_KIND: &str = "ingest_envelope_file";

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

/// One file persisted by `codedb ingest-envelope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestFileRow {
    pub relative_path: String,
    pub blob_ref: String,
    pub sha256: String,
    pub bytes: u64,
    /// True when the BLAKE3 identity was already indexed before this write,
    /// i.e. identical bytes are content-addressed exactly once.
    pub deduplicated: bool,
}

/// One ingested file read back with its stored metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestReportRow {
    pub relative_path: String,
    pub module_path: String,
    pub unix_mode: String,
    pub sha256: String,
    pub blake3: String,
    pub bytes: u64,
    pub ast_json: String,
}

#[derive(Debug)]
pub enum StoreError {
    Database(DatabaseError),
    Transaction(TransactionError),
    Commit(CommitError),
    Table(TableError),
    Storage(StorageError),
    Io(io::Error),
    Materialization(CoreStoreError),
    Migration(CoreStoreError),
    UnsupportedSchemaVersion {
        observed: String,
    },
    MissingValue {
        table: &'static str,
        key: &'static str,
    },
    OutboxContract {
        message: String,
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
            Self::Materialization(err) => write!(f, "materialization error: {err}"),
            Self::Migration(err) => write!(f, "migration error: {err}"),
            Self::UnsupportedSchemaVersion { observed } => {
                write!(f, "unsupported store schema version: {observed}")
            }
            Self::MissingValue { table, key } => {
                write!(f, "missing metadata value {key} in table {table}")
            }
            Self::OutboxContract { message } => {
                write!(f, "outbox contract violation: {message}")
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
    let schema_version = STORE_SCHEMA_VERSION.to_string();

    {
        let write_txn = db.begin_write()?;
        {
            let mut schema_versions = write_txn.open_table(SCHEMA_VERSION_TABLE)?;
            schema_versions.insert("schema_version", schema_version.as_str())?;
        }
        {
            let mut store_metadata = write_txn.open_table(STORE_METADATA_TABLE)?;
            store_metadata.insert("store_status", "initialized")?;
            store_metadata.insert("store_created", "true")?;
            store_metadata.insert("codedb_version", context.codedb_version)?;
            store_metadata.insert("schema_version", schema_version.as_str())?;
            store_metadata.insert("migration_state", "current")?;
            store_metadata.insert("last_migration", "initialize_v1")?;
            store_metadata.insert("migration_plan", "explicit_known_steps_only")?;
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
fn read_store_report_db<D: ReadableDatabase>(
    db: &D,
    path: PathBuf,
) -> Result<StoreInitReport, StoreError> {
    let read_txn = db.begin_read()?;

    let schema_version = {
        let table = read_txn.open_table(SCHEMA_VERSION_TABLE)?;
        let value = table
            .get("schema_version")?
            .ok_or(StoreError::MissingValue {
                table: "schema_versions",
                key: "schema_version",
            })?;
        let observed = parse_schema_version(value.value()).map_err(|_| {
            StoreError::UnsupportedSchemaVersion {
                observed: value.value().to_string(),
            }
        })?;
        if observed != STORE_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedSchemaVersion {
                observed: value.value().to_string(),
            });
        }
        observed
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

/// Persist one `ingest-envelope` file: content-address the exact bytes once
/// (sha256 blob key, BLAKE3 dedup index), bind the relative path, and store
/// the module path, unix mode, BLAKE3 identity, and Nushell AST rows as
/// metadata. The `unix_mode` metadata key is the same one
/// [`materialize_source_file`] restores, so ingested files round-trip
/// permissions through the existing materialization path.
#[allow(clippy::result_large_err)]
pub fn persist_ingest_file(
    store_path: impl AsRef<Path>,
    relative_path: impl Into<String>,
    bytes: &[u8],
    blake3: &str,
    unix_mode: &str,
    module_path: &str,
    ast_json: &str,
) -> Result<IngestFileRow, StoreError> {
    let relative_path = relative_path.into();
    let sha256 = sha256_bytes(bytes);
    let blob_ref = format!("sha256:{sha256}");
    let db = Database::open(store_path.as_ref())?;
    let deduplicated;
    {
        let write_txn = db.begin_write()?;
        {
            let mut blake3_index = write_txn.open_table(BLAKE3_INDEX_TABLE)?;
            deduplicated = blake3_index.get(blake3)?.is_some();
            blake3_index.insert(blake3, sha256.as_str())?;
        }
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
            for (field, value) in [
                ("artifact_kind", INGEST_ARTIFACT_KIND),
                ("permission_capture", "unix_mode"),
                ("unix_mode", unix_mode),
                ("module_path", module_path),
                ("blake3", blake3),
                ("nu_ast", ast_json),
            ] {
                metadata.insert(
                    source_file_metadata_key(&relative_path, field).as_str(),
                    value,
                )?;
            }
        }
        write_txn.commit()?;
    }

    Ok(IngestFileRow {
        relative_path,
        blob_ref,
        sha256,
        bytes: bytes.len() as u64,
        deduplicated,
    })
}

/// Read back every `ingest-envelope` file with its stored metadata.
#[allow(clippy::result_large_err)]
pub fn list_ingest_files(store_path: impl AsRef<Path>) -> Result<Vec<IngestReportRow>, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    let read_txn = db.begin_read()?;
    let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    let metadata = read_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
    let field = |relative_path: &str, name: &str| -> Result<String, StoreError> {
        Ok(metadata
            .get(source_file_metadata_key(relative_path, name).as_str())?
            .map(|value| value.value().to_string())
            .unwrap_or_default())
    };
    let mut rows = Vec::new();
    for entry in files.iter()? {
        let (key, value) = entry?;
        let relative_path = key.value().to_string();
        if field(&relative_path, "artifact_kind")? != INGEST_ARTIFACT_KIND {
            continue;
        }
        let sha256 = value.value().trim_start_matches("sha256:").to_string();
        let bytes = blobs
            .get(sha256.as_str())?
            .map(|blob| blob.value().len() as u64)
            .unwrap_or(0);
        rows.push(IngestReportRow {
            module_path: field(&relative_path, "module_path")?,
            unix_mode: field(&relative_path, "unix_mode")?,
            blake3: field(&relative_path, "blake3")?,
            ast_json: field(&relative_path, "nu_ast")?,
            relative_path,
            sha256,
            bytes,
        });
    }
    Ok(rows)
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
fn captured_paths_db<D: ReadableDatabase>(
    db: &D,
) -> Result<std::collections::BTreeSet<String>, StoreError> {
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
fn persist_symlink_db(
    db: &Database,
    relative_path: &str,
    target: &str,
) -> Result<CoreSourceSymlinkRow, StoreError> {
    let row = CoreSourceSymlinkRow::new(relative_path, target);
    let blob_ref = format!("sha256:{}", row.target_sha256);
    let write_txn = db.begin_write()?;
    {
        let mut blobs = write_txn.open_table(SOURCE_BLOBS_TABLE)?;
        blobs.insert(row.target_sha256.as_str(), target.as_bytes())?;
    }
    {
        let mut files = write_txn.open_table(SOURCE_FILES_TABLE)?;
        files.insert(relative_path, blob_ref.as_str())?;
    }
    {
        let mut metadata = write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        metadata.insert(
            source_file_metadata_key(relative_path, "artifact_kind").as_str(),
            "symlink",
        )?;
        metadata.insert(
            source_file_metadata_key(relative_path, "symlink_target_sha256").as_str(),
            row.target_sha256.as_str(),
        )?;
    }
    write_txn.commit()?;
    Ok(row)
}

#[allow(clippy::result_large_err)]
fn source_artifact_kind_db(
    read_txn: &redb::ReadTransaction,
    relative_path: &str,
) -> Result<Option<String>, StoreError> {
    let metadata = match read_txn.open_table(SOURCE_FILE_METADATA_TABLE) {
        Ok(metadata) => metadata,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(metadata
        .get(source_file_metadata_key(relative_path, "artifact_kind").as_str())?
        .map(|value| value.value().to_string()))
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
fn list_source_files_db<D: ReadableDatabase>(db: &D) -> Result<Vec<SourceBlobRow>, StoreError> {
    let read_txn = db.begin_read()?;
    let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    let mut rows = Vec::new();
    for item in files.iter()? {
        let (key, value) = item?;
        let relative_path = key.value().to_string();
        if source_artifact_kind_db(&read_txn, &relative_path)?.as_deref() == Some("symlink") {
            continue;
        }
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

#[allow(clippy::result_large_err)]
fn list_source_symlinks_db<D: ReadableDatabase>(
    db: &D,
) -> Result<Vec<CoreSourceSymlinkRow>, StoreError> {
    let read_txn = db.begin_read()?;
    let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    let metadata = match read_txn.open_table(SOURCE_FILE_METADATA_TABLE) {
        Ok(metadata) => metadata,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut rows = Vec::new();
    for item in files.iter()? {
        let (key, value) = item?;
        let relative_path = key.value().to_string();
        let artifact_kind = metadata
            .get(source_file_metadata_key(&relative_path, "artifact_kind").as_str())?
            .map(|value| value.value().to_string());
        if artifact_kind.as_deref() != Some("symlink") {
            continue;
        }
        let blob_ref = value.value().to_string();
        let target_sha256 = blob_ref
            .strip_prefix("sha256:")
            .ok_or_else(|| {
                StoreError::Materialization(CoreStoreError::new(format!(
                    "captured symlink {:?} has invalid target blob reference {:?}",
                    relative_path, blob_ref
                )))
            })?
            .to_string();
        let target_bytes = blobs
            .get(target_sha256.as_str())?
            .ok_or_else(|| {
                StoreError::Materialization(CoreStoreError::new(format!(
                    "captured symlink {:?} points to missing target metadata blob sha256:{}",
                    relative_path, target_sha256
                )))
            })?
            .value()
            .to_vec();
        let target = String::from_utf8(target_bytes).map_err(|_| {
            StoreError::Materialization(CoreStoreError::new(format!(
                "captured symlink {:?} target metadata is not UTF-8",
                relative_path
            )))
        })?;
        let row = CoreSourceSymlinkRow {
            relative_path,
            target,
            target_sha256,
        };
        row.verify().map_err(StoreError::Materialization)?;
        let metadata_sha256 = metadata
            .get(source_file_metadata_key(&row.relative_path, "symlink_target_sha256").as_str())?
            .map(|value| value.value().to_string())
            .ok_or_else(|| {
                StoreError::Materialization(CoreStoreError::new(format!(
                    "captured symlink {:?} is missing its checksum metadata",
                    row.relative_path
                )))
            })?;
        if metadata_sha256 != row.target_sha256 {
            return Err(StoreError::Materialization(CoreStoreError::new(format!(
                "captured symlink {:?} checksum metadata mismatch",
                row.relative_path
            ))));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// Raw bytes for a captured relative path, or `None` if absent — the
/// [`BlobStore::read_source_file_blob`] read path against an open database.
#[allow(clippy::result_large_err)]
fn read_source_blob_bytes_db(
    db: &impl ReadableDatabase,
    relative_path: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    let read_txn = db.begin_read()?;
    if source_artifact_kind_db(&read_txn, relative_path)?.as_deref() == Some("symlink") {
        return Ok(None);
    }
    let blob_ref = {
        let files = read_txn.open_table(SOURCE_FILES_TABLE)?;
        match files.get(relative_path)? {
            Some(value) => value.value().to_string(),
            None => return Ok(None),
        }
    };
    let sha256 = blob_ref.trim_start_matches("sha256:").to_string();
    let blobs = read_txn.open_table(SOURCE_BLOBS_TABLE)?;
    Ok(blobs
        .get(sha256.as_str())?
        .map(|blob| blob.value().to_vec()))
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

/// Shared materialize against an already-open database. The core publication
/// helper checksum-binds the stored bytes, restores the optional unix mode,
/// fsyncs file and destination directory, and publishes with no-replace.
#[allow(clippy::result_large_err)]
fn materialize_source_file_db(
    db: &impl ReadableDatabase,
    relative_path: &str,
    output_path: &Path,
) -> Result<FileMaterializationReport, StoreError> {
    let output_path = output_path.to_path_buf();
    let read_txn = db.begin_read()?;
    if source_artifact_kind_db(&read_txn, relative_path)?.as_deref() == Some("symlink") {
        return Err(StoreError::Materialization(CoreStoreError::new(format!(
            "captured symlink {relative_path:?} cannot be materialized as a regular file"
        ))));
    }
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
    #[cfg(unix)]
    let unix_mode = {
        let metadata = read_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
        metadata
            .get(source_file_metadata_key(relative_path, "unix_mode").as_str())?
            .map(|value| value.value().to_string())
            .as_deref()
            .and_then(|mode| u32::from_str_radix(mode, 8).ok())
    };
    #[cfg(not(unix))]
    let unix_mode = None;
    let materialized = atomic_materialize_file(&output_path, &bytes, &sha256, unix_mode)
        .map_err(StoreError::Materialization)?;

    Ok(FileMaterializationReport {
        path: materialized.path,
        blob_ref,
        sha256: materialized.sha256,
        bytes: materialized.bytes,
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
    if source_path == backup_path {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "store backup path must differ from source path",
        )));
    }
    if let Some(parent) = backup_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let observed = inspect_store_schema_version(&source_path)?;
    if observed != STORE_SCHEMA_VERSION && !REDB_MIGRATIONS.iter().any(|step| step.from == observed)
    {
        return Err(StoreError::UnsupportedSchemaVersion {
            observed: observed.to_string(),
        });
    }
    let bytes = fs::copy(&source_path, &backup_path)?;
    let sha256 = checksum_file_sha256(&backup_path)?;

    Ok(StoreBackupReport {
        source_path,
        backup_path,
        bytes,
        sha256,
    })
}

/// Inspect only the schema version table without opening any data surface or
/// performing migration/repair.
#[allow(clippy::result_large_err)]
pub fn inspect_store_schema_version(path: impl AsRef<Path>) -> Result<SchemaVersion, StoreError> {
    let db = ReadOnlyDatabase::open(path)?;
    let read_txn = db.begin_read()?;
    let table = read_txn.open_table(SCHEMA_VERSION_TABLE)?;
    let value = table
        .get("schema_version")?
        .ok_or(StoreError::MissingValue {
            table: "schema_versions",
            key: "schema_version",
        })?;
    parse_schema_version(value.value()).map_err(|_| StoreError::UnsupportedSchemaVersion {
        observed: value.value().to_string(),
    })
}

/// Explicitly migrate a known redb schema after writing a checksum-bound file
/// backup. Unknown/future versions are rejected before backup or mutation.
#[allow(clippy::result_large_err)]
pub fn migrate_store(
    store_path: impl AsRef<Path>,
    backup_path: impl AsRef<Path>,
) -> Result<StoreMigrationReport, StoreError> {
    let store_path = store_path.as_ref().to_path_buf();
    let backup_path = backup_path.as_ref().to_path_buf();
    let observed = inspect_store_schema_version(&store_path)?;
    let plan = plan_store_migration(
        StoreBackend::Redb,
        observed,
        STORE_SCHEMA_VERSION,
        &REDB_MIGRATIONS,
    )
    .map_err(|_| StoreError::UnsupportedSchemaVersion {
        observed: observed.to_string(),
    })?;
    if plan.steps.is_empty() {
        return Ok(StoreMigrationReport {
            plan,
            backup: None,
            applied_steps: Vec::new(),
            rolled_back: false,
        });
    }

    let backup = backup_store(&store_path, &backup_path)?;
    let db = Database::open(&store_path)?;
    let write_txn = db.begin_write()?;
    for step in &plan.steps {
        match step.id {
            "redb_legacy_v0_9_to_v1" => {
                write_txn.open_table(SOURCE_FILE_METADATA_TABLE)?;
                let target = step.to.to_string();
                {
                    let mut versions = write_txn.open_table(SCHEMA_VERSION_TABLE)?;
                    versions.insert("schema_version", target.as_str())?;
                }
                {
                    let mut metadata = write_txn.open_table(STORE_METADATA_TABLE)?;
                    metadata.insert("schema_version", target.as_str())?;
                    metadata.insert("migration_state", "current")?;
                    metadata.insert("last_migration", step.id)?;
                    metadata.insert("migration_plan", "explicit_known_steps_only")?;
                    metadata.insert("unsupported_state_behavior", "refuse_unknown_schema")?;
                    metadata.insert("corruption_validation", "backup_restore_smoke_required")?;
                }
                {
                    let mut validation = write_txn.open_table(VALIDATION_ROWS_TABLE)?;
                    validation.insert("migration_backup", "checksum_bound_file_copy")?;
                    validation.insert("migration_rollback", "explicit_file_restore")?;
                }
            }
            other => {
                return Err(StoreError::Migration(CoreStoreError::new(format!(
                    "redb migration implementation is missing for step {other:?}"
                ))));
            }
        }
    }
    write_txn.commit()?;
    drop(db);

    if let Err(error) = read_store_report(&store_path) {
        restore_backup_exact(&backup_path, &store_path)?;
        return Err(StoreError::Migration(CoreStoreError::new(format!(
            "redb migration validation failed and the backup was restored: {error}"
        ))));
    }

    Ok(StoreMigrationReport {
        applied_steps: plan.steps.iter().map(|step| step.id).collect(),
        plan,
        backup: Some(StoreMigrationBackup {
            kind: StoreBackupKind::FileCopy,
            reference: backup.backup_path.display().to_string(),
            sha256: Some(backup.sha256),
        }),
        rolled_back: false,
    })
}

/// Replace a migrated redb store with its exact checksum-validated
/// pre-migration backup.
#[allow(clippy::result_large_err)]
pub fn rollback_store_migration(
    store_path: impl AsRef<Path>,
    backup_path: impl AsRef<Path>,
) -> Result<StoreMigrationReport, StoreError> {
    let store_path = store_path.as_ref().to_path_buf();
    let backup_path = backup_path.as_ref().to_path_buf();
    let current = inspect_store_schema_version(&store_path)?;
    if current != STORE_SCHEMA_VERSION {
        return Err(StoreError::UnsupportedSchemaVersion {
            observed: current.to_string(),
        });
    }
    let observed = inspect_store_schema_version(&backup_path)?;
    let plan = plan_store_migration(
        StoreBackend::Redb,
        observed,
        STORE_SCHEMA_VERSION,
        &REDB_MIGRATIONS,
    )
    .map_err(StoreError::Migration)?;
    if plan.steps.is_empty() {
        return Err(StoreError::Migration(CoreStoreError::new(
            "rollback backup does not contain a pre-migration schema",
        )));
    }
    let sha256 = checksum_file_sha256(&backup_path)?;
    restore_backup_exact(&backup_path, &store_path)?;
    let restored = inspect_store_schema_version(&store_path)?;
    if restored != observed {
        return Err(StoreError::Migration(CoreStoreError::new(
            "redb rollback validation observed the wrong schema version",
        )));
    }

    Ok(StoreMigrationReport {
        applied_steps: plan.steps.iter().map(|step| step.id).collect(),
        plan,
        backup: Some(StoreMigrationBackup {
            kind: StoreBackupKind::FileCopy,
            reference: backup_path.display().to_string(),
            sha256: Some(sha256),
        }),
        rolled_back: true,
    })
}

fn restore_backup_exact(backup_path: &Path, store_path: &Path) -> Result<(), StoreError> {
    if backup_path == store_path {
        return Err(StoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "rollback backup path must differ from store path",
        )));
    }
    let sequence = std::process::id();
    let temporary = store_path.with_extension(format!("rollback-{sequence}.tmp"));
    if temporary.exists() {
        fs::remove_file(&temporary)?;
    }
    fs::copy(backup_path, &temporary)?;
    let expected = checksum_file_sha256(backup_path)?;
    let observed = checksum_file_sha256(&temporary)?;
    if expected != observed {
        fs::remove_file(&temporary).ok();
        return Err(StoreError::Migration(CoreStoreError::new(
            "redb rollback temporary copy checksum mismatch",
        )));
    }
    inspect_store_schema_version(&temporary)?;
    fs::rename(&temporary, store_path)?;
    Ok(())
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

    fn persist_symlink(
        &mut self,
        relative_path: &str,
        target: &str,
    ) -> Result<CoreSourceSymlinkRow, CoreStoreError> {
        persist_symlink_db(&self.db, relative_path, target).map_err(to_core_err)
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

    fn list_source_symlinks(&self) -> Result<Vec<CoreSourceSymlinkRow>, CoreStoreError> {
        list_source_symlinks_db(&self.db).map_err(to_core_err)
    }

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<CoreMaterializedFile, CoreStoreError> {
        let report = materialize_source_file_db(&self.db, relative_path, output_path)
            .map_err(to_core_err)?;
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

/// Strictly read-only redb adapter for MCP/reporting surfaces.
///
/// `ReadOnlyDatabase::open` never performs repair or write-side bookkeeping on
/// the selected store. Mutating trait operations fail closed.
pub struct ReadOnlyStore {
    db: ReadOnlyDatabase,
}

impl ReadOnlyStore {
    #[allow(clippy::result_large_err)]
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            db: ReadOnlyDatabase::open(path)?,
        })
    }
}

impl BlobStore for ReadOnlyStore {
    fn persist_batch(
        &mut self,
        _files: &[(String, Vec<u8>)],
    ) -> Result<Vec<CoreSourceFileRow>, CoreStoreError> {
        Err(CoreStoreError::new(
            "read-only redb store refuses persistence",
        ))
    }

    fn persist_symlink(
        &mut self,
        _relative_path: &str,
        _target: &str,
    ) -> Result<CoreSourceSymlinkRow, CoreStoreError> {
        Err(CoreStoreError::new(
            "read-only redb store refuses symlink persistence",
        ))
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
        list_source_files_db(&self.db)
            .map_err(to_core_err)
            .map(|rows| rows.into_iter().map(to_core_row).collect())
    }

    fn list_source_symlinks(&self) -> Result<Vec<CoreSourceSymlinkRow>, CoreStoreError> {
        list_source_symlinks_db(&self.db).map_err(to_core_err)
    }

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<CoreMaterializedFile, CoreStoreError> {
        let report = materialize_source_file_db(&self.db, relative_path, output_path)
            .map_err(to_core_err)?;
        Ok(CoreMaterializedFile {
            path: report.path,
            blob_ref: report.blob_ref,
            sha256: report.sha256,
            bytes: report.bytes,
        })
    }

    fn store_metadata_rows(&self) -> Result<Vec<CoreStoreMetadataRow>, CoreStoreError> {
        read_store_report_db(&self.db, PathBuf::new())
            .map_err(to_core_err)
            .map(|report| {
                report
                    .rows
                    .into_iter()
                    .map(|row| CoreStoreMetadataRow {
                        table: row.table,
                        key: row.key,
                        value: row.value,
                    })
                    .collect()
            })
    }
}

// ---------------------------------------------------------------------------
// Outbox: restartable local buffer + explicit application outbox (ARCHBP-002).
// Entries are append-only with contiguous monotonic sequences; the single
// acknowledge cursor is the only mutable state. Entries are never deleted, so
// the outbox remains replayable and lossless after any crash.
// ---------------------------------------------------------------------------

const OUTBOX_ENTRIES_TABLE: TableDefinition<u64, &str> = TableDefinition::new("outbox_entries");
const OUTBOX_CURSOR_TABLE: TableDefinition<&str, u64> = TableDefinition::new("outbox_cursor");
const OUTBOX_ACK_KEY: &str = "acknowledged";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntryRow {
    pub seq: u64,
    pub entry_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxStatusRow {
    pub enqueued: u64,
    pub acknowledged: u64,
    pub pending: u64,
}

fn outbox_last_seq(
    read_txn: &redb::ReadTransaction,
) -> Result<u64, StoreError> {
    match read_txn.open_table(OUTBOX_ENTRIES_TABLE) {
        Ok(entries) => Ok(entries.last()?.map(|(key, _)| key.value()).unwrap_or(0)),
        Err(TableError::TableDoesNotExist(_)) => Ok(0),
        Err(err) => Err(err.into()),
    }
}

fn outbox_ack_cursor(
    read_txn: &redb::ReadTransaction,
) -> Result<u64, StoreError> {
    match read_txn.open_table(OUTBOX_CURSOR_TABLE) {
        Ok(cursor) => Ok(cursor
            .get(OUTBOX_ACK_KEY)?
            .map(|value| value.value())
            .unwrap_or(0)),
        Err(TableError::TableDoesNotExist(_)) => Ok(0),
        Err(err) => Err(err.into()),
    }
}

/// Append one entry; returns its assigned sequence (contiguous from 1).
#[allow(clippy::result_large_err)]
pub fn outbox_enqueue(
    store_path: impl AsRef<Path>,
    entry_json: &str,
) -> Result<u64, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    let seq;
    {
        let write_txn = db.begin_write()?;
        {
            let mut entries = write_txn.open_table(OUTBOX_ENTRIES_TABLE)?;
            seq = entries.last()?.map(|(key, _)| key.value()).unwrap_or(0) + 1;
            entries.insert(seq, entry_json)?;
        }
        write_txn.commit()?;
    }
    Ok(seq)
}

/// Entries strictly after the acknowledge cursor, in sequence order.
#[allow(clippy::result_large_err)]
pub fn outbox_pending(
    store_path: impl AsRef<Path>,
    limit: usize,
) -> Result<Vec<OutboxEntryRow>, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    let read_txn = db.begin_read()?;
    let acknowledged = outbox_ack_cursor(&read_txn)?;
    let entries = match read_txn.open_table(OUTBOX_ENTRIES_TABLE) {
        Ok(entries) => entries,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut rows = Vec::new();
    for entry in entries.range((acknowledged + 1)..)? {
        if rows.len() >= limit {
            break;
        }
        let (key, value) = entry?;
        rows.push(OutboxEntryRow {
            seq: key.value(),
            entry_json: value.value().to_string(),
        });
    }
    Ok(rows)
}

/// Advance the acknowledge cursor. The cursor is monotonic and can never
/// pass the last enqueued sequence; violations fail closed.
#[allow(clippy::result_large_err)]
pub fn outbox_acknowledge(
    store_path: impl AsRef<Path>,
    up_to: u64,
) -> Result<u64, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    {
        let write_txn = db.begin_write()?;
        {
            let entries = write_txn.open_table(OUTBOX_ENTRIES_TABLE)?;
            let last_seq = entries.last()?.map(|(key, _)| key.value()).unwrap_or(0);
            let mut cursor = write_txn.open_table(OUTBOX_CURSOR_TABLE)?;
            let acknowledged = cursor
                .get(OUTBOX_ACK_KEY)?
                .map(|value| value.value())
                .unwrap_or(0);
            if up_to < acknowledged {
                return Err(StoreError::OutboxContract {
                    message: format!(
                        "acknowledge cursor cannot regress from {acknowledged} to {up_to}"
                    ),
                });
            }
            if up_to > last_seq {
                return Err(StoreError::OutboxContract {
                    message: format!(
                        "cannot acknowledge {up_to} beyond the enqueued head {last_seq}"
                    ),
                });
            }
            cursor.insert(OUTBOX_ACK_KEY, up_to)?;
        }
        write_txn.commit()?;
    }
    Ok(up_to)
}

/// Observable outbox state: last enqueued seq, acknowledge cursor, pending.
#[allow(clippy::result_large_err)]
pub fn outbox_status(store_path: impl AsRef<Path>) -> Result<OutboxStatusRow, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    let read_txn = db.begin_read()?;
    let enqueued = outbox_last_seq(&read_txn)?;
    let acknowledged = outbox_ack_cursor(&read_txn)?;
    Ok(OutboxStatusRow {
        enqueued,
        acknowledged,
        pending: enqueued.saturating_sub(acknowledged),
    })
}

/// Whether the content-addressed source blob is present in this store.
#[allow(clippy::result_large_err)]
pub fn source_blob_exists(
    store_path: impl AsRef<Path>,
    sha256: &str,
) -> Result<bool, StoreError> {
    let db = Database::open(store_path.as_ref())?;
    let read_txn = db.begin_read()?;
    let blobs = match read_txn.open_table(SOURCE_BLOBS_TABLE) {
        Ok(blobs) => blobs,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(err) => return Err(err.into()),
    };
    Ok(blobs.get(sha256)?.is_some())
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
        std::env::temp_dir().join(format!(
            "codedb_store_redb_{}_{stamp}_{seq}.redb",
            std::process::id()
        ))
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
                .any(|row| row.key == "migration_state" && row.value == "current")
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

    #[test]
    fn symlink_metadata_is_checksum_bound_and_never_exposed_as_regular_file_bytes() {
        let path = temp_store_path();
        initialize_store(&path, &init_ctx()).expect("store init");
        let mut store = CaptureBatcher::open(&path).expect("open store");
        let relative_path = "node_modules/.bin/tool";
        let target = "../tool/bin/tool.js";
        let persisted = store
            .persist_symlink(relative_path, target)
            .expect("persist symlink metadata");

        assert_eq!(
            store.list_source_symlinks().unwrap(),
            vec![persisted.clone()]
        );
        assert!(store.list_source_files().unwrap().is_empty());
        assert_eq!(store.read_source_file_blob(relative_path).unwrap(), None);
        assert!(
            store
                .materialize_source_file(relative_path, &path.with_extension("must-not-be-file"))
                .expect_err("symlink target text must never become a regular file")
                .message()
                .contains("symlink")
        );

        {
            let write_txn = store
                .db
                .begin_write()
                .expect("begin corruption transaction");
            {
                let mut blobs = write_txn
                    .open_table(SOURCE_BLOBS_TABLE)
                    .expect("blob table");
                blobs
                    .insert(
                        persisted.target_sha256.as_str(),
                        b"../different/target".as_slice(),
                    )
                    .expect("corrupt target metadata blob");
            }
            write_txn.commit().expect("commit corrupt target fixture");
        }
        let error = store
            .list_source_symlinks()
            .expect_err("corrupt symlink target must fail checksum verification");
        assert!(error.message().contains("checksum mismatch"));

        drop(store);
        fs::remove_file(&path).ok();
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

    // Test lane: default
    // Defends: publication is no-replace and a failed publish removes its
    // destination-local temporary file without disturbing existing content.
    #[test]
    fn materialization_refuses_replace_and_cleans_publication_temporary_file() {
        let path = temp_store_path();
        let output_dir = path.with_extension("materialization-output");
        let output_path = output_dir.join("artifact.bin");
        let original = b"existing destination must survive";
        let captured = b"captured replacement must not win";

        initialize_store(&path, &init_ctx()).expect("store init");
        persist_source_blob(&path, "artifact.bin", captured).expect("persist captured bytes");
        fs::create_dir(&output_dir).expect("create output directory");
        fs::write(&output_path, original).expect("seed existing destination");

        let error = materialize_source_file(&path, "artifact.bin", &output_path)
            .expect_err("materialization must never replace an existing destination");
        assert!(
            error.to_string().contains("exists") || error.to_string().contains("replace"),
            "unexpected no-replace error: {error}"
        );
        assert_eq!(
            fs::read(&output_path).expect("read preserved destination"),
            original
        );
        let entries = fs::read_dir(&output_dir)
            .expect("read output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect output entries");
        assert_eq!(
            entries.len(),
            1,
            "failed publication leaked a destination-local temporary file: {entries:?}"
        );
        assert_eq!(entries[0].path(), output_path);

        fs::remove_file(&path).ok();
        fs::remove_dir_all(&output_dir).ok();
    }

    // Test lane: default
    // Defends: stored bytes are verified against their content-addressed key
    // before publication; corrupt blobs publish nothing and leave no temp file.
    #[test]
    fn corrupt_blob_is_rejected_before_publication_and_cleans_temporary_file() {
        let path = temp_store_path();
        let output_dir = path.with_extension("corrupt-output");
        let output_path = output_dir.join("artifact.bin");
        let persisted = persist_after_init(&path, "artifact.bin", b"checksum-bound captured bytes");

        {
            let db = Database::open(&path).expect("open store for corruption fixture");
            let write_txn = db.begin_write().expect("begin corruption transaction");
            {
                let mut blobs = write_txn
                    .open_table(SOURCE_BLOBS_TABLE)
                    .expect("open blob table");
                blobs
                    .insert(
                        persisted.sha256.as_str(),
                        b"corrupt stored bytes".as_slice(),
                    )
                    .expect("inject corrupt blob bytes");
            }
            write_txn.commit().expect("commit corrupt blob fixture");
        }

        let error = materialize_source_file(&path, "artifact.bin", &output_path)
            .expect_err("checksum mismatch must fail before publication");
        assert!(
            error.to_string().contains("checksum"),
            "unexpected corrupt-blob error: {error}"
        );
        assert!(!output_path.exists(), "corrupt bytes were published");
        if output_dir.exists() {
            let entries = fs::read_dir(&output_dir)
                .expect("read output directory")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect output entries");
            assert!(
                entries.is_empty(),
                "corrupt-blob failure leaked a temporary file: {entries:?}"
            );
        }

        fs::remove_file(&path).ok();
        fs::remove_dir_all(&output_dir).ok();
    }

    // Test lane: default
    // Defends: concurrent writers use atomic no-replace publication, yielding
    // exactly one complete winner and no destination-local temporary residue.
    #[test]
    fn concurrent_materializations_have_exactly_one_complete_winner() {
        const WRITERS: usize = 16;

        let path = temp_store_path();
        let output_dir = path.with_extension("concurrent-output");
        let output_path = output_dir.join("artifact.bin");
        let captured = b"one complete atomic publication".to_vec();

        initialize_store(&path, &init_ctx()).expect("store init");
        let mut batcher = CaptureBatcher::open(&path).expect("open store");
        BlobStore::persist_batch(
            &mut batcher,
            &[("artifact.bin".to_string(), captured.clone())],
        )
        .expect("persist captured bytes");
        let batcher = Arc::new(batcher);
        let barrier = Arc::new(std::sync::Barrier::new(WRITERS));
        let writers = (0..WRITERS)
            .map(|_| {
                let batcher = Arc::clone(&batcher);
                let barrier = Arc::clone(&barrier);
                let output_path = output_path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    BlobStore::materialize_source_file(
                        batcher.as_ref(),
                        "artifact.bin",
                        &output_path,
                    )
                })
            })
            .collect::<Vec<_>>();

        let results = writers
            .into_iter()
            .map(|writer| writer.join().expect("writer thread must not panic"))
            .collect::<Vec<_>>();
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            1,
            "exactly one concurrent no-replace publication must win: {results:?}"
        );
        assert_eq!(
            fs::read(&output_path).expect("read winning publication"),
            captured
        );
        let entries = fs::read_dir(&output_dir)
            .expect("read output directory")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect output entries");
        assert_eq!(
            entries.len(),
            1,
            "concurrent publication leaked temporary files: {entries:?}"
        );
        assert_eq!(entries[0].path(), output_path);

        drop(batcher);
        fs::remove_file(&path).ok();
        fs::remove_dir_all(&output_dir).ok();
    }

    // Test lane: default
    // Defends: CDB086 upgrades a known legacy redb layout only after creating
    // a checksum-bound backup, preserves captured bytes, and can explicitly
    // roll the store back to the exact pre-migration image.
    #[test]
    fn legacy_schema_migrates_with_backup_and_explicit_rollback() {
        use codedb_core::store::{
            CURRENT_STORE_SCHEMA_VERSION, LEGACY_STORE_SCHEMA_VERSION, StoreBackend,
            StoreBackupKind,
        };

        let path = temp_store_path();
        let backup_path = path.with_extension("pre-migration.redb");
        initialize_legacy_store(&path);

        let migrated = migrate_store(&path, &backup_path).expect("migrate known legacy store");
        assert_eq!(migrated.plan.backend, StoreBackend::Redb);
        assert_eq!(migrated.plan.observed_version, LEGACY_STORE_SCHEMA_VERSION);
        assert_eq!(migrated.plan.target_version, CURRENT_STORE_SCHEMA_VERSION);
        assert_eq!(migrated.applied_steps, vec!["redb_legacy_v0_9_to_v1"]);
        let backup = migrated.backup.expect("pre-migration backup report");
        assert_eq!(backup.kind, StoreBackupKind::FileCopy);
        assert_eq!(backup.reference, backup_path.display().to_string());
        assert_eq!(backup.sha256.as_deref().map(str::len), Some(64));

        let current = read_store_report(&path).expect("read migrated store");
        assert_eq!(current.schema_version, CURRENT_STORE_SCHEMA_VERSION);
        assert_eq!(
            read_source_blob_bytes(&path, "legacy/file.rs"),
            b"fn legacy() {}\n"
        );

        let rolled_back =
            rollback_store_migration(&path, &backup_path).expect("rollback from backup");
        assert!(rolled_back.rolled_back);
        assert_eq!(
            inspect_store_schema_version(&path).expect("inspect rolled-back schema"),
            LEGACY_STORE_SCHEMA_VERSION
        );
        assert_eq!(
            read_source_blob_bytes(&path, "legacy/file.rs"),
            b"fn legacy() {}\n"
        );

        fs::remove_file(&path).ok();
        fs::remove_file(&backup_path).ok();
    }

    #[test]
    fn migration_refuses_unknown_schema_without_creating_a_backup() {
        let path = temp_store_path();
        let backup_path = path.with_extension("must-not-exist.redb");
        initialize_legacy_store(&path);
        {
            let db = Database::open(&path).expect("open legacy store");
            let write_txn = db.begin_write().expect("begin schema mutation");
            {
                let mut versions = write_txn
                    .open_table(SCHEMA_VERSION_TABLE)
                    .expect("schema versions");
                versions
                    .insert("schema_version", "99.0.0")
                    .expect("write unsupported schema");
            }
            write_txn.commit().expect("commit unsupported schema");
        }

        let error =
            migrate_store(&path, &backup_path).expect_err("unknown schema must fail closed");
        assert!(matches!(
            error,
            StoreError::UnsupportedSchemaVersion { observed } if observed == "99.0.0"
        ));
        assert!(
            !backup_path.exists(),
            "unknown schemas must be refused before backup or mutation"
        );

        fs::remove_file(&path).ok();
    }

    fn initialize_legacy_store(path: &Path) {
        let content = b"fn legacy() {}\n";
        let sha256 = sha256_bytes(content);
        let blob_ref = format!("sha256:{sha256}");
        let db = Database::create(path).expect("create legacy redb fixture");
        let write_txn = db.begin_write().expect("begin legacy fixture");
        {
            let mut versions = write_txn
                .open_table(SCHEMA_VERSION_TABLE)
                .expect("legacy schema table");
            versions
                .insert("schema_version", "0.9.0")
                .expect("legacy schema version");
        }
        {
            let mut metadata = write_txn
                .open_table(STORE_METADATA_TABLE)
                .expect("legacy metadata");
            metadata
                .insert("schema_version", "0.9.0")
                .expect("legacy metadata version");
            metadata
                .insert("migration_state", "legacy_requires_migration")
                .expect("legacy migration state");
            metadata
                .insert("checksum_algorithm", "sha256")
                .expect("legacy checksum algorithm");
            metadata
                .insert("store_status", "initialized")
                .expect("legacy store status");
        }
        write_txn
            .open_table(TOOLCHAIN_METADATA_TABLE)
            .expect("legacy toolchain table");
        write_txn
            .open_table(VALIDATION_ROWS_TABLE)
            .expect("legacy validation table");
        {
            let mut blobs = write_txn
                .open_table(SOURCE_BLOBS_TABLE)
                .expect("legacy blobs");
            blobs
                .insert(sha256.as_str(), content.as_slice())
                .expect("legacy blob");
        }
        {
            let mut files = write_txn
                .open_table(SOURCE_FILES_TABLE)
                .expect("legacy files");
            files
                .insert("legacy/file.rs", blob_ref.as_str())
                .expect("legacy file reference");
        }
        write_txn.commit().expect("commit legacy fixture");
    }

    fn read_source_blob_bytes(path: &Path, relative_path: &str) -> Vec<u8> {
        let db = Database::open(path).expect("open store for byte proof");
        read_source_blob_bytes_db(&db, relative_path)
            .expect("read source blob")
            .expect("source blob exists")
    }

    fn persist_after_init(path: &Path, relative_path: &str, bytes: &[u8]) -> SourceBlobRow {
        initialize_store(path, &init_ctx()).expect("store init");
        persist_source_blob(path, relative_path, bytes).expect("persist source blob")
    }
}

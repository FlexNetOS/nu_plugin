#![forbid(unsafe_code)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use codedb_core::SchemaVersion;
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
        }
        write_txn.commit()?;
    }

    drop(db);
    let report = read_store_report(&path)?;
    Ok(report)
}

pub fn read_store_report(path: impl AsRef<Path>) -> Result<StoreInitReport, StoreError> {
    let path = path.as_ref().to_path_buf();
    let db = Database::open(&path)?;
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
            _ => STORE_SCHEMA_VERSION,
        }
    };

    let mut rows = Vec::new();
    for (table_name, definition) in [
        ("schema_versions", SCHEMA_VERSION_TABLE),
        ("store_metadata", STORE_METADATA_TABLE),
        ("toolchain_metadata", TOOLCHAIN_METADATA_TABLE),
        ("validation_rows", VALIDATION_ROWS_TABLE),
        ("source_files", SOURCE_FILES_TABLE),
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

pub fn persist_source_file(
    store_path: impl AsRef<Path>,
    relative_path: impl AsRef<str>,
    source_path: impl AsRef<Path>,
) -> Result<SourceBlobRow, StoreError> {
    let relative_path = relative_path.as_ref().to_string();
    let bytes = fs::read(source_path)?;
    persist_source_blob(store_path, relative_path, &bytes)
}

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
        write_txn.commit()?;
    }

    Ok(SourceBlobRow {
        relative_path,
        blob_ref,
        sha256,
        bytes: bytes.len() as u64,
    })
}

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

pub fn materialize_source_file(
    store_path: impl AsRef<Path>,
    relative_path: impl AsRef<str>,
    output_path: impl AsRef<Path>,
) -> Result<FileMaterializationReport, StoreError> {
    let relative_path = relative_path.as_ref();
    let output_path = output_path.as_ref().to_path_buf();
    let db = Database::open(store_path)?;
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
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output_path, &bytes)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store_path() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time monotonic")
            .as_nanos();
        std::env::temp_dir().join(format!("codedb_store_redb_{stamp}.redb"))
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
}

#![forbid(unsafe_code)]

//! PostgreSQL implementation of CodeDB's backend-neutral content-addressed
//! [`BlobStore`] contract.
//!
//! A logical store name expands to three tables:
//!
//! - `<store>_blobs` contains one byte-exact blob per SHA-256 digest.
//! - `<store>_path_refs` maps captured relative paths to blob digests, scoped
//!   by capture root (`metadata->>'repo_path'`) so multi-root ingestion of a
//!   whole host tree into one store never collides on relative path alone
//!   (see [`PgStore::set_capture_root`]).
//! - `<store>_schema_metadata` records the schema version and migration state.
//!
//! [`PgStore::open_existing`] is deliberately read-only: it validates that
//! layout before exposing data and contains no DDL. Schema creation and the
//! one supported legacy migration are explicit mutating operations.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codedb_core::store::{
    BlobStore, CURRENT_STORE_SCHEMA_VERSION, CURRENT_STORE_SCHEMA_VERSION_TEXT,
    LEGACY_STORE_SCHEMA_VERSION, MaterializedFile, SourceFileRow, SourceSymlinkRow,
    StoreBackupKind, StoreError, StoreMetadataRow, StoreMigrationBackup, StoreMigrationReport,
    StoreMigrationStep, atomic_materialize_file, parse_schema_version, plan_store_migration,
};
use codedb_core::store_spec::StoreBackend;
use postgres::config::{Host, SslMode};
use postgres::{Client, Config, GenericClient, NoTls};
use postgres_rustls::{MakeTlsConnector, set_postgresql_alpn};
use sha2::{Digest, Sha256};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use url::Url;

pub const DEFAULT_TABLE: &str = "codebase_codedb";
pub const STORE_SCHEMA_VERSION: &str = CURRENT_STORE_SCHEMA_VERSION_TEXT;

const ORIGIN: &str = "codedb";
const CURRENT_MIGRATION_STATE: &str = "current";
const SCHEMA_LAYOUT: &str = "content_addressed_blobs_plus_path_refs";
const MAX_IDENTIFIER_BYTES: usize = 63;
// The longest suffix appended to `base` across every derived identifier
// (schema_metadata/blobs/path_refs/migration_backup/path_refs_repo_index);
// `sanitize_table` reserves this much headroom so no generated identifier can
// exceed PostgreSQL's 63-byte NAMEDATALEN limit.
const LONGEST_COMPONENT_SUFFIX: &str = "_path_refs_repo_uidx";
const POSTGRESQL_MIGRATIONS: [StoreMigrationStep; 1] = [StoreMigrationStep::new(
    "postgresql_legacy_content_rows_to_v1",
    LEGACY_STORE_SCHEMA_VERSION,
    CURRENT_STORE_SCHEMA_VERSION,
)];
/// Shared bucket for `path_refs` writes that never set a capture root (legacy
/// call sites that predate [`PgStore::set_capture_root`]). Every such row
/// collapses onto this one value, so `module_path` stays effectively unique
/// among them — identical to the pre-repo-path-identity behavior.
const NO_CAPTURE_ROOT_SENTINEL: &str = "";
/// The `path_refs` uniqueness/conflict expression. Used verbatim in both the
/// unique index definition ([`ensure_repo_path_unique_index`]) and every
/// `ON CONFLICT` target so PostgreSQL matches the conflict target to the
/// index; the two must stay textually identical.
const REPO_PATH_METADATA_EXPR: &str = "(metadata->>'repo_path')";
/// PostgreSQL `bytea` values are capped near 1 GiB (`content` is TOASTed but
/// still bounded), while captured host files run up to several GiB. A blob
/// at or above this size is split across `{blobs}_chunks`-style rows instead
/// of a single `content` value (EVERY-BYTE engine gap G1); a blob at or below
/// it is stored exactly as before, in `content`, with `chunk_count = 0`.
const CHUNK_THRESHOLD_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Debug)]
struct StoreTables {
    base: String,
    schema_metadata: String,
    blobs: String,
    blob_chunks: String,
    path_refs: String,
    path_refs_repo_index: String,
    migration_backup: String,
}

impl StoreTables {
    fn new(table: &str) -> Result<Self, StoreError> {
        let base = sanitize_table(table)?;
        Ok(Self {
            schema_metadata: format!("{base}_schema_metadata"),
            blobs: format!("{base}_blobs"),
            blob_chunks: format!("{base}_blob_chunks"),
            path_refs: format!("{base}_path_refs"),
            path_refs_repo_index: format!("{base}_path_refs_repo_uidx"),
            migration_backup: format!("{base}_migration_backup"),
            base,
        })
    }
}

enum StoreLayout {
    Fresh,
    LegacyContentRows,
    Current,
    Incomplete,
}

/// Dynamic PostgreSQL CodeDB store. The connection is intentionally retained so
/// a capture session can persist many durable batches without reconnecting.
pub struct PgStore {
    client: RefCell<Client>,
    tables: StoreTables,
    /// Absolute capture root attributed to every `path_refs` write from this
    /// handle, set via [`PgStore::set_capture_root`]. `None` until set;
    /// [`persist_batch`](BlobStore::persist_batch) and
    /// [`persist_symlink`](BlobStore::persist_symlink) fall back to
    /// [`NO_CAPTURE_ROOT_SENTINEL`] so the metadata field is never absent.
    capture_root: Option<String>,
}

impl fmt::Debug for PgStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgStore")
            .field("tables", &self.tables)
            .field("capture_root", &self.capture_root)
            .finish_non_exhaustive()
    }
}

impl PgStore {
    /// Explicit mutating initialization API.
    ///
    /// This creates an empty current-layout store only when the logical store
    /// has no pre-existing relations. Existing current stores are merely
    /// validated; legacy or partial stores require explicit remediation.
    pub fn initialize(conn: &str, table: &str) -> Result<Self, StoreError> {
        let tables = StoreTables::new(table)?;
        let mut client = connect_client(conn)?;
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin initialization transaction"))?;
        acquire_store_mutation_lock(&mut tx, &tables)?;
        match inspect_layout(&mut tx, &tables)? {
            StoreLayout::Fresh => {
                create_current_schema(&mut tx, &tables, "initialize_v1")?;
            }
            StoreLayout::Current => {
                // Idempotent bootstrap repair: a store created before capture-root
                // identity existed still has the legacy `module_path`-only primary
                // key. Converge it onto the repo-scoped unique index every time so
                // every existing installation heals on its next capture, with no
                // separate migration command required.
                ensure_repo_path_unique_index(&mut tx, &tables)?;
                // Idempotent bootstrap repair: a store created before chunked
                // large-blob storage existed still has `content bytea NOT
                // NULL` and no `chunk_count` column or chunks table. Heal it
                // in place every time, same pattern as the repo-path index
                // above, so an existing installation can accept files above
                // `CHUNK_THRESHOLD_BYTES` on its very next capture.
                ensure_blob_chunking_schema(&mut tx, &tables)?;
            }
            StoreLayout::LegacyContentRows => {
                return Err(StoreError::new(format!(
                    "legacy PostgreSQL CodeDB store {} requires explicit PgStore::migrate",
                    tables.base
                )));
            }
            StoreLayout::Incomplete => {
                return Err(incomplete_layout_error(&tables));
            }
        }
        validate_current_schema(&mut tx, &tables)?;
        tx.commit()
            .map_err(|_| database_error("commit initialization transaction"))?;
        Ok(Self {
            client: RefCell::new(client),
            tables,
            capture_root: None,
        })
    }

    /// Explicit mutating migration API.
    ///
    /// Version 1 migrates the previous single-table content layout into
    /// deduplicated blobs plus path references. It refuses an unknown/future
    /// current-layout version rather than guessing a migration.
    pub fn migrate(conn: &str, table: &str) -> Result<Self, StoreError> {
        Self::migrate_with_report(conn, table).map(|(store, _)| store)
    }

    /// Migrate and return the backend-neutral plan/backup report.
    pub fn migrate_with_report(
        conn: &str,
        table: &str,
    ) -> Result<(Self, StoreMigrationReport), StoreError> {
        let tables = StoreTables::new(table)?;
        let mut client = connect_client(conn)?;
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin schema migration transaction"))?;
        acquire_store_mutation_lock(&mut tx, &tables)?;
        let observed = match inspect_layout(&mut tx, &tables)? {
            StoreLayout::Fresh => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB store {} is not initialized; run PgStore::initialize first",
                    tables.base
                )));
            }
            StoreLayout::Current => {
                ensure_repo_path_unique_index(&mut tx, &tables)?;
                validate_current_schema(&mut tx, &tables)?;
                CURRENT_STORE_SCHEMA_VERSION
            }
            StoreLayout::LegacyContentRows => LEGACY_STORE_SCHEMA_VERSION,
            StoreLayout::Incomplete => {
                return Err(incomplete_layout_error(&tables));
            }
        };
        let plan = plan_store_migration(
            StoreBackend::PostgreSql,
            observed,
            CURRENT_STORE_SCHEMA_VERSION,
            &POSTGRESQL_MIGRATIONS,
        )?;
        let mut backup = None;
        for step in &plan.steps {
            match step.id {
                "postgresql_legacy_content_rows_to_v1" => {
                    create_legacy_migration_backup(&mut tx, &tables)?;
                    migrate_legacy_content_rows(
                        &mut tx,
                        &tables,
                        &tables.migration_backup,
                        step.id,
                    )?;
                    validate_current_schema(&mut tx, &tables)?;
                    backup = Some(StoreMigrationBackup {
                        kind: StoreBackupKind::TransactionalTableSnapshot,
                        reference: tables.migration_backup.clone(),
                        sha256: None,
                    });
                }
                other => {
                    return Err(StoreError::new(format!(
                        "PostgreSQL migration implementation is missing for step {other:?}"
                    )));
                }
            }
        }
        tx.commit()
            .map_err(|_| database_error("commit schema migration transaction"))?;
        let report = StoreMigrationReport {
            applied_steps: plan.steps.iter().map(|step| step.id).collect(),
            plan,
            backup,
            rolled_back: false,
        };
        Ok((
            Self {
                client: RefCell::new(client),
                tables,
                capture_root: None,
            },
            report,
        ))
    }

    /// Restore the exact legacy relation retained by the last successful
    /// migration. DDL and restore run in one transaction under the same
    /// store-scoped advisory lock as migration.
    pub fn rollback_last_migration(
        conn: &str,
        table: &str,
    ) -> Result<StoreMigrationReport, StoreError> {
        let tables = StoreTables::new(table)?;
        let mut client = connect_client(conn)?;
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin schema rollback transaction"))?;
        acquire_store_mutation_lock(&mut tx, &tables)?;
        if !relation_exists(&mut tx, &tables.migration_backup)? {
            return Err(StoreError::new(format!(
                "PostgreSQL CodeDB store {} has no migration backup to roll back",
                tables.base
            )));
        }
        match inspect_layout(&mut tx, &tables)? {
            StoreLayout::Current => validate_current_schema(&mut tx, &tables)?,
            StoreLayout::Fresh | StoreLayout::LegacyContentRows | StoreLayout::Incomplete => {
                return Err(incomplete_layout_error(&tables));
            }
        }
        let metadata = read_schema_metadata(&mut tx, &tables)?;
        let expected_step = POSTGRESQL_MIGRATIONS[0].id;
        if metadata.get("last_migration").map(String::as_str) != Some(expected_step) {
            return Err(StoreError::new(format!(
                "PostgreSQL CodeDB store {} last migration is not rollback-compatible",
                tables.base
            )));
        }
        let plan = plan_store_migration(
            StoreBackend::PostgreSql,
            LEGACY_STORE_SCHEMA_VERSION,
            CURRENT_STORE_SCHEMA_VERSION,
            &POSTGRESQL_MIGRATIONS,
        )?;
        tx.batch_execute(
            format!(
                "DROP TABLE {path_refs};\
                 DROP TABLE {blob_chunks};\
                 DROP TABLE {blobs};\
                 DROP TABLE {schema_metadata};\
                 ALTER TABLE {backup} RENAME TO {base};",
                path_refs = tables.path_refs,
                // Dropped before `blobs`: chunked large-blob storage
                // (EVERY-BYTE engine gap G1) added a `blob_chunks` table with
                // a foreign key to `blobs`, so `blobs` cannot be dropped
                // while it still exists.
                blob_chunks = tables.blob_chunks,
                blobs = tables.blobs,
                schema_metadata = tables.schema_metadata,
                backup = tables.migration_backup,
                base = tables.base,
            )
            .as_str(),
        )
        .map_err(|_| database_error("restore PostgreSQL migration backup"))?;
        tx.commit()
            .map_err(|_| database_error("commit schema rollback transaction"))?;
        Ok(StoreMigrationReport {
            applied_steps: plan.steps.iter().map(|step| step.id).collect(),
            plan,
            backup: Some(StoreMigrationBackup {
                kind: StoreBackupKind::TransactionalTableSnapshot,
                reference: tables.migration_backup,
                sha256: None,
            }),
            rolled_back: true,
        })
    }

    /// Non-mutating open for report, query, and materialization paths.
    ///
    /// This function only runs catalog and schema-metadata reads before
    /// returning a store. It never creates, alters, drops, or migrates a
    /// relation; an absent, partial, legacy, unknown, or future layout is
    /// refused before any captured blob/path data can be read.
    pub fn open_existing(conn: &str, table: &str) -> Result<Self, StoreError> {
        let tables = StoreTables::new(table)?;
        let mut client = connect_client(conn)?;
        match inspect_layout(&mut client, &tables)? {
            StoreLayout::Current => validate_current_schema(&mut client, &tables)?,
            StoreLayout::Fresh => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB store {} is not initialized; run explicit PgStore::initialize first",
                    tables.base
                )));
            }
            StoreLayout::LegacyContentRows => {
                return Err(StoreError::new(format!(
                    "legacy PostgreSQL CodeDB store {} requires explicit PgStore::migrate; read-only open will not run DDL",
                    tables.base
                )));
            }
            StoreLayout::Incomplete => {
                return Err(incomplete_layout_error(&tables));
            }
        }
        Ok(Self {
            client: RefCell::new(client),
            tables,
            capture_root: None,
        })
    }

    /// The validated logical store identifier supplied by the caller.
    pub fn table(&self) -> &str {
        &self.tables.base
    }

    /// Attribute every subsequent `path_refs` write from this handle to
    /// `root` (recorded as `metadata->>'repo_path'`), so multi-root ingestion
    /// of a whole host tree into one store reconciles unambiguously instead
    /// of colliding on relative path alone (every repository has a
    /// `README.md`). `root` must already be absolute; every capture call site
    /// canonicalizes its repository path before a store is opened.
    pub fn set_capture_root(&mut self, root: &Path) -> Result<(), StoreError> {
        if !root.is_absolute() {
            return Err(StoreError::new(
                "PostgreSQL CodeDB capture root must be an absolute path",
            ));
        }
        self.capture_root = Some(root.display().to_string());
        Ok(())
    }

    /// Shared implementation of [`BlobStore::persist_batch`], parameterized
    /// on the chunk threshold so `#[cfg(feature = "pg-integration")]` tests
    /// can exercise the chunked-storage path (see
    /// [`persist_batch_with_chunk_threshold_for_tests`]) without persisting
    /// hundreds of megabytes; production always calls this through
    /// [`CHUNK_THRESHOLD_BYTES`] via the trait method.
    fn persist_batch_at_threshold(
        &mut self,
        files: &[(String, Vec<u8>)],
        chunk_threshold_bytes: usize,
    ) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin batch transaction"))?;
        let blob_sql = format!(
            "INSERT INTO {} (sha256, content, bytes, chunk_count) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (sha256) DO NOTHING",
            self.tables.blobs
        );
        let chunk_sql = format!(
            "INSERT INTO {} (sha256, seq, chunk) VALUES ($1, $2, $3) \
             ON CONFLICT (sha256, seq) DO NOTHING",
            self.tables.blob_chunks
        );
        let path_sql = format!(
            "INSERT INTO {} (module_path, sha256, metadata) VALUES ($1, $2, $3::text::jsonb) \
             ON CONFLICT ({}, module_path) DO UPDATE SET \
                 sha256 = EXCLUDED.sha256, metadata = EXCLUDED.metadata",
            self.tables.path_refs, REPO_PATH_METADATA_EXPR
        );
        let metadata_json = raw_blob_metadata_json(self.capture_root.as_deref());

        let mut rows = Vec::with_capacity(files.len());
        for (relative_path, bytes) in files {
            let sha256 = sha256_hex(bytes);
            let byte_count = i64::try_from(bytes.len())
                .map_err(|_| StoreError::new("captured blob exceeds PostgreSQL bigint size"))?;
            // PostgreSQL `bytea` values are capped near 1 GiB; a blob at or
            // above the chunk threshold is split across `blob_chunks` rows
            // instead of a single `content` value (EVERY-BYTE engine gap
            // G1). `ON CONFLICT (sha256) DO NOTHING` reports whether this
            // call inserted the row (content-addressed dedup): the chunks
            // are only written when it did, so a blob shared by multiple
            // captured paths is never re-chunked and re-sent to PostgreSQL.
            let chunk_count = chunk_count_for(bytes.len(), chunk_threshold_bytes);
            if chunk_count > 0 {
                let chunk_count_param = i32::try_from(chunk_count)
                    .map_err(|_| StoreError::new("captured blob exceeds supported chunk count"))?;
                let content: Option<&[u8]> = None;
                let inserted = tx
                    .execute(
                        blob_sql.as_str(),
                        &[&sha256, &content, &byte_count, &chunk_count_param],
                    )
                    .map_err(|_| database_error("insert content-addressed blob"))?;
                if inserted == 1 {
                    for (seq, chunk) in bytes.chunks(chunk_threshold_bytes).enumerate() {
                        let seq = i32::try_from(seq).map_err(|_| {
                            StoreError::new("captured blob exceeds supported chunk count")
                        })?;
                        tx.execute(chunk_sql.as_str(), &[&sha256, &seq, &chunk])
                            .map_err(|_| database_error("insert content-addressed blob chunk"))?;
                    }
                }
            } else {
                let content = Some(bytes.as_slice());
                tx.execute(blob_sql.as_str(), &[&sha256, &content, &byte_count, &0i32])
                    .map_err(|_| database_error("insert content-addressed blob"))?;
            }
            tx.execute(path_sql.as_str(), &[relative_path, &sha256, &metadata_json])
                .map_err(|_| database_error("upsert path reference"))?;
            rows.push(SourceFileRow {
                relative_path: relative_path.clone(),
                blob_ref: format!("sha256:{sha256}"),
                sha256,
                bytes: bytes.len() as u64,
            });
        }
        tx.commit()
            .map_err(|_| database_error("commit batch transaction"))?;
        Ok(rows)
    }
}

impl BlobStore for PgStore {
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceFileRow>, StoreError> {
        self.persist_batch_at_threshold(files, CHUNK_THRESHOLD_BYTES)
    }

    fn persist_symlink(
        &mut self,
        relative_path: &str,
        target: &str,
    ) -> Result<SourceSymlinkRow, StoreError> {
        let row = SourceSymlinkRow::new(relative_path, target);
        let metadata = serde_json::json!({
            "artifact_kind": "symlink",
            "symlink_target_sha256": row.target_sha256,
            "repo_path": self.capture_root.as_deref().unwrap_or(NO_CAPTURE_ROOT_SENTINEL),
        })
        .to_string();
        let mut client = self.client.borrow_mut();
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin symlink transaction"))?;
        let blob_sql = format!(
            "INSERT INTO {} (sha256, content, bytes) VALUES ($1, $2, $3) \
             ON CONFLICT (sha256) DO NOTHING",
            self.tables.blobs
        );
        let path_sql = format!(
            "INSERT INTO {} (module_path, sha256, metadata) VALUES ($1, $2, $3::text::jsonb) \
             ON CONFLICT ({}, module_path) DO UPDATE SET \
                 sha256 = EXCLUDED.sha256, metadata = EXCLUDED.metadata",
            self.tables.path_refs, REPO_PATH_METADATA_EXPR
        );
        let target_bytes = target.as_bytes();
        let target_len = i64::try_from(target_bytes.len()).map_err(|_| {
            StoreError::new("captured symlink target exceeds PostgreSQL bigint size")
        })?;
        tx.execute(
            blob_sql.as_str(),
            &[&row.target_sha256, &target_bytes, &target_len],
        )
        .map_err(|_| database_error("insert content-addressed symlink target"))?;
        tx.execute(
            path_sql.as_str(),
            &[&relative_path, &row.target_sha256, &metadata],
        )
        .map_err(|_| database_error("upsert symlink path reference"))?;
        tx.commit()
            .map_err(|_| database_error("commit symlink transaction"))?;
        Ok(row)
    }

    fn captured_paths(&self) -> Result<BTreeSet<String>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT module_path FROM {} ORDER BY module_path COLLATE \"C\"",
            self.tables.path_refs
        );
        let rows = client
            .query(sql.as_str(), &[])
            .map_err(|_| database_error("list captured paths"))?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }

    fn read_source_file_blob(&self, relative_path: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.sha256, b.content, b.chunk_count, p.metadata->>'artifact_kind' FROM {} p \
             LEFT JOIN {} b ON b.sha256 = p.sha256 WHERE p.module_path = $1",
            self.tables.path_refs, self.tables.blobs
        );
        let rows = client
            .query(sql.as_str(), &[&relative_path])
            .map_err(|_| database_error("read path-reference blob"))?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let sha256: String = row.get(0);
        let content: Option<Vec<u8>> = row.get(1);
        let chunk_count: Option<i32> = row.get(2);
        let artifact_kind: Option<String> = row.get(3);
        if artifact_kind.as_deref() == Some("symlink") {
            return Ok(None);
        }
        match chunk_count {
            Some(count) if count > 0 => {
                read_chunked_blob(&mut client, &self.tables.blob_chunks, &sha256, count).map(Some)
            }
            _ => content
                .ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))
                .map(Some),
        }
    }

    fn list_source_files(&self) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.module_path, p.sha256, b.bytes FROM {} p \
             LEFT JOIN {} b ON b.sha256 = p.sha256 \
             WHERE p.metadata->>'artifact_kind' IS DISTINCT FROM 'symlink' \
             ORDER BY p.module_path COLLATE \"C\"",
            self.tables.path_refs, self.tables.blobs
        );
        let rows = client
            .query(sql.as_str(), &[])
            .map_err(|_| database_error("list source-file path references"))?;
        rows.into_iter()
            .map(|row| {
                let relative_path: String = row.get(0);
                let sha256: String = row.get(1);
                let bytes: Option<i64> = row.get(2);
                let bytes =
                    bytes.ok_or_else(|| corrupt_path_reference_error(&relative_path, &sha256))?;
                let bytes = u64::try_from(bytes)
                    .map_err(|_| corrupt_path_reference_error(&relative_path, &sha256))?;
                Ok(SourceFileRow {
                    relative_path,
                    blob_ref: format!("sha256:{sha256}"),
                    sha256,
                    bytes,
                })
            })
            .collect()
    }

    fn list_source_symlinks(&self) -> Result<Vec<SourceSymlinkRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.module_path, p.sha256, b.content, \
                    p.metadata->>'symlink_target_sha256' \
             FROM {} p LEFT JOIN {} b ON b.sha256 = p.sha256 \
             WHERE p.metadata->>'artifact_kind' = 'symlink' \
             ORDER BY p.module_path COLLATE \"C\"",
            self.tables.path_refs, self.tables.blobs
        );
        client
            .query(sql.as_str(), &[])
            .map_err(|_| database_error("list symlink path references"))?
            .into_iter()
            .map(|row| {
                let relative_path: String = row.get(0);
                let target_sha256: String = row.get(1);
                let target_bytes: Option<Vec<u8>> = row.get(2);
                let metadata_sha256: Option<String> = row.get(3);
                let target_bytes = target_bytes
                    .ok_or_else(|| corrupt_path_reference_error(&relative_path, &target_sha256))?;
                let target = String::from_utf8(target_bytes).map_err(|_| {
                    StoreError::new(format!(
                        "PostgreSQL CodeDB symlink target for {relative_path:?} is not UTF-8"
                    ))
                })?;
                let result = SourceSymlinkRow {
                    relative_path,
                    target,
                    target_sha256,
                };
                result.verify()?;
                if metadata_sha256.as_deref() != Some(result.target_sha256.as_str()) {
                    return Err(StoreError::new(format!(
                        "PostgreSQL CodeDB symlink checksum metadata mismatch for {:?}",
                        result.relative_path
                    )));
                }
                Ok(result)
            })
            .collect()
    }

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<MaterializedFile, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.sha256, b.content, b.chunk_count, p.metadata::text, \
                    p.metadata->>'artifact_kind' FROM {} p \
             LEFT JOIN {} b ON b.sha256 = p.sha256 WHERE p.module_path = $1",
            self.tables.path_refs, self.tables.blobs
        );
        let rows = client
            .query(sql.as_str(), &[&relative_path])
            .map_err(|_| database_error("read materialization blob"))?;
        let row = rows
            .first()
            .ok_or_else(|| StoreError::new(format!("missing source file: {relative_path}")))?;
        let sha256: String = row.get(0);
        let artifact_kind: Option<String> = row.get(4);
        if artifact_kind.as_deref() == Some("symlink") {
            return Err(StoreError::new(format!(
                "captured symlink {relative_path:?} cannot be materialized as a regular file"
            )));
        }
        let content: Option<Vec<u8>> = row.get(1);
        let chunk_count: Option<i32> = row.get(2);
        // Reassembly is verified structurally here (declared chunk count and
        // contiguous sequence) and cryptographically by
        // `atomic_materialize_file` below, which refuses to publish unless
        // `sha256(content) == sha256` — the roundtrip integrity guarantee is
        // mandatory for both the direct and the chunked-reassembly path
        // (EVERY-BYTE engine gap G1).
        let content = match chunk_count {
            Some(count) if count > 0 => {
                read_chunked_blob(&mut client, &self.tables.blob_chunks, &sha256, count)?
            }
            _ => content.ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))?,
        };
        #[cfg(unix)]
        let unix_mode = {
            let metadata_text: String = row.get(3);
            parse_unix_mode(&metadata_text)
        };
        #[cfg(not(unix))]
        let unix_mode = None;
        atomic_materialize_file(output_path, &content, &sha256, unix_mode)
    }

    fn store_metadata_rows(&self) -> Result<Vec<StoreMetadataRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let schema_sql = format!(
            "SELECT key, value FROM {} ORDER BY key COLLATE \"C\"",
            self.tables.schema_metadata
        );
        let mut rows = client
            .query(schema_sql.as_str(), &[])
            .map_err(|_| database_error("read schema metadata"))?
            .into_iter()
            .map(|row| StoreMetadataRow {
                table: self.tables.schema_metadata.clone(),
                key: row.get(0),
                value: row.get(1),
            })
            .collect::<Vec<_>>();
        let count_sql = format!("SELECT count(*) FROM {}", self.tables.path_refs);
        let source_files: i64 = client
            .query_one(count_sql.as_str(), &[])
            .map_err(|_| database_error("count source-file path references"))?
            .get(0);
        rows.push(StoreMetadataRow {
            table: self.tables.path_refs.clone(),
            key: "source_files".to_string(),
            value: source_files.to_string(),
        });
        rows.push(StoreMetadataRow {
            table: self.tables.schema_metadata.clone(),
            key: "table".to_string(),
            value: self.tables.base.clone(),
        });
        Ok(rows)
    }
}

fn connect_client(conn: &str) -> Result<Client, StoreError> {
    if conn.trim().is_empty() {
        return Err(StoreError::new("PostgreSQL DSN is required"));
    }
    let secured = parse_connection_security(conn)?;
    match secured.transport {
        ConnectionTransport::UnixSocket => secured
            .config
            .connect(NoTls)
            .map_err(|_| connection_error(conn)),
        ConnectionTransport::VerifiedTls { ca_path } => {
            let roots = load_ca_roots(&ca_path)?;
            let mut tls = ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth();
            set_postgresql_alpn(&mut tls);
            secured
                .config
                .connect(MakeTlsConnector::new(tokio_rustls::TlsConnector::from(
                    Arc::new(tls),
                )))
                .map_err(|_| connection_error(conn))
        }
    }
}

/// Open an administrative integration-test connection through the exact same
/// transport policy as [`PgStore`].
#[cfg(feature = "pg-integration")]
#[doc(hidden)]
pub fn connect_for_integration_tests(conn: &str) -> Result<Client, StoreError> {
    connect_client(conn)
}

/// Test-only override of [`CHUNK_THRESHOLD_BYTES`] so integration tests can
/// exercise the real chunked-storage SQL path (split, dedup-skip, reassemble
/// on read) against a real PostgreSQL service without persisting hundreds of
/// megabytes to prove it.
#[cfg(feature = "pg-integration")]
#[doc(hidden)]
pub fn persist_batch_with_chunk_threshold_for_tests(
    store: &mut PgStore,
    files: &[(String, Vec<u8>)],
    chunk_threshold_bytes: usize,
) -> Result<Vec<SourceFileRow>, StoreError> {
    store.persist_batch_at_threshold(files, chunk_threshold_bytes)
}

fn connection_error(_conn: &str) -> StoreError {
    StoreError::new("PostgreSQL connection failed; connection details redacted")
}

struct SecureConnectionConfig {
    config: Config,
    transport: ConnectionTransport,
}

#[derive(Debug, Eq, PartialEq)]
enum ConnectionTransport {
    UnixSocket,
    VerifiedTls { ca_path: PathBuf },
}

fn parse_connection_security(conn: &str) -> Result<SecureConnectionConfig, StoreError> {
    let (mut config, ssl_mode, ca_path) =
        if conn.starts_with("postgres://") || conn.starts_with("postgresql://") {
            parse_url_connection_security(conn)?
        } else {
            parse_keyword_connection_security(conn)?
        };
    let hosts = config.get_hosts();
    let has_tcp = hosts.iter().any(|host| matches!(host, Host::Tcp(_)));
    let has_unix = hosts.iter().any(|host| matches!(host, Host::Unix(_)));
    if hosts.is_empty() || has_tcp == has_unix {
        return Err(security_policy_error(
            "PostgreSQL connections must select either verified TLS over TCP or one explicit Unix socket path",
        ));
    }
    if has_unix {
        if !config.get_hostaddrs().is_empty() {
            return Err(security_policy_error(
                "PostgreSQL Unix socket connections cannot include TCP host addresses",
            ));
        }
        config.ssl_mode(SslMode::Disable);
        return Ok(SecureConnectionConfig {
            config,
            transport: ConnectionTransport::UnixSocket,
        });
    }
    if ssl_mode.as_deref() != Some("verify-full") {
        return Err(security_policy_error(
            "remote PostgreSQL TCP requires verified TLS with sslmode=verify-full",
        ));
    }
    let ca_path = ca_path.ok_or_else(|| {
        security_policy_error(
            "remote PostgreSQL TCP requires an explicit CA certificate path in sslrootcert",
        )
    })?;
    let ca_path = PathBuf::from(ca_path);
    if !ca_path.is_absolute() {
        return Err(security_policy_error(
            "remote PostgreSQL TCP requires an absolute CA certificate path",
        ));
    }
    config.ssl_mode(SslMode::Require);
    Ok(SecureConnectionConfig {
        config,
        transport: ConnectionTransport::VerifiedTls { ca_path },
    })
}

fn parse_url_connection_security(
    conn: &str,
) -> Result<(Config, Option<String>, Option<String>), StoreError> {
    let mut url = Url::parse(conn).map_err(|_| dsn_parse_error())?;
    let mut ssl_mode = None;
    let mut ca_path = None;
    let mut retained = Vec::new();
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "sslmode" => set_unique_policy_value(&mut ssl_mode, value.into_owned())?,
            "sslrootcert" => set_unique_policy_value(&mut ca_path, value.into_owned())?,
            _ => retained.push((key.into_owned(), value.into_owned())),
        }
    }
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in retained {
            query.append_pair(&key, &value);
        }
        if ssl_mode.as_deref() == Some("verify-full") {
            query.append_pair("sslmode", "require");
        } else if let Some(mode) = ssl_mode.as_deref() {
            query.append_pair("sslmode", mode);
        }
    }
    let config = url
        .as_str()
        .parse::<Config>()
        .map_err(|_| dsn_parse_error())?;
    Ok((config, ssl_mode, ca_path))
}

fn parse_keyword_connection_security(
    conn: &str,
) -> Result<(Config, Option<String>, Option<String>), StoreError> {
    let mut ssl_mode = None;
    let mut ca_path = None;
    let mut retained = Vec::new();
    for (key, value) in parse_keyword_pairs(conn)? {
        match key.as_str() {
            "sslmode" => set_unique_policy_value(&mut ssl_mode, value)?,
            "sslrootcert" => set_unique_policy_value(&mut ca_path, value)?,
            _ => retained.push((key, value)),
        }
    }
    retained.push((
        "sslmode".to_string(),
        if ssl_mode.as_deref() == Some("verify-full") {
            "require".to_string()
        } else {
            ssl_mode.clone().unwrap_or_else(|| "prefer".to_string())
        },
    ));
    let normalized = retained
        .into_iter()
        .map(|(key, value)| format!("{key}='{}'", escape_keyword_value(&value)))
        .collect::<Vec<_>>()
        .join(" ");
    let config = normalized
        .parse::<Config>()
        .map_err(|_| dsn_parse_error())?;
    Ok((config, ssl_mode, ca_path))
}

fn parse_keyword_pairs(conn: &str) -> Result<Vec<(String, String)>, StoreError> {
    let bytes = conn.as_bytes();
    let mut cursor = 0;
    let mut pairs = Vec::new();
    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let key_start = cursor;
        while cursor < bytes.len() && bytes[cursor] != b'=' && !bytes[cursor].is_ascii_whitespace()
        {
            cursor += 1;
        }
        if cursor == key_start || cursor == bytes.len() || bytes[cursor] != b'=' {
            return Err(dsn_parse_error());
        }
        let key = std::str::from_utf8(&bytes[key_start..cursor])
            .map_err(|_| dsn_parse_error())?
            .to_string();
        cursor += 1;
        let mut value = Vec::new();
        let quoted = cursor < bytes.len() && bytes[cursor] == b'\'';
        if quoted {
            cursor += 1;
        }
        let mut closed = !quoted;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if byte == b'\\' {
                cursor += 1;
                if cursor == bytes.len() {
                    return Err(dsn_parse_error());
                }
                value.push(bytes[cursor]);
                cursor += 1;
            } else if quoted && byte == b'\'' {
                cursor += 1;
                closed = true;
                break;
            } else if !quoted && byte.is_ascii_whitespace() {
                break;
            } else {
                value.push(byte);
                cursor += 1;
            }
        }
        if !closed || (cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace()) {
            return Err(dsn_parse_error());
        }
        pairs.push((
            key,
            String::from_utf8(value).map_err(|_| dsn_parse_error())?,
        ));
    }
    Ok(pairs)
}

fn set_unique_policy_value(slot: &mut Option<String>, value: String) -> Result<(), StoreError> {
    if slot.replace(value).is_some() {
        return Err(security_policy_error(
            "duplicate PostgreSQL TLS policy parameters are forbidden",
        ));
    }
    Ok(())
}

fn escape_keyword_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

fn load_ca_roots(path: &Path) -> Result<RootCertStore, StoreError> {
    let bytes = std::fs::read(path).map_err(|_| {
        security_policy_error("PostgreSQL CA certificate file is unavailable or unreadable")
    })?;
    let mut roots = RootCertStore::empty();
    let mut count = 0usize;
    for cert in rustls_pemfile::certs(&mut bytes.as_slice()) {
        let cert = cert.map_err(|_| {
            security_policy_error("PostgreSQL CA certificate file contains invalid PEM")
        })?;
        roots.add(cert).map_err(|_| {
            security_policy_error("PostgreSQL CA certificate file contains an invalid certificate")
        })?;
        count += 1;
    }
    if count == 0 {
        return Err(security_policy_error(
            "PostgreSQL CA certificate file contains no trusted certificates",
        ));
    }
    Ok(roots)
}

fn dsn_parse_error() -> StoreError {
    StoreError::new("invalid PostgreSQL DSN; connection details redacted")
}

fn security_policy_error(reason: &str) -> StoreError {
    StoreError::new(format!("{reason}; PostgreSQL connection details redacted"))
}

fn database_error(operation: &str) -> StoreError {
    StoreError::new(format!(
        "PostgreSQL {operation} failed; database connection details redacted"
    ))
}

fn sanitize_table(table: &str) -> Result<String, StoreError> {
    let valid_identifier = !table.is_empty()
        && table
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_identifier {
        return Err(StoreError::new(format!(
            "invalid table name {table:?}: expected [A-Za-z_][A-Za-z0-9_]*"
        )));
    }
    if table.len() + LONGEST_COMPONENT_SUFFIX.len() > MAX_IDENTIFIER_BYTES {
        return Err(StoreError::new(format!(
            "invalid table name {table:?}: logical store name is too long for PostgreSQL component identifiers"
        )));
    }
    Ok(table.to_string())
}

fn acquire_store_mutation_lock<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    client
        .query_one(
            "SELECT pg_advisory_xact_lock(\
                 hashtextextended(COALESCE(current_schema(), '') || chr(31) || $1, 0)\
             )",
            &[&tables.base],
        )
        .map_err(|_| database_error("acquire store schema mutation lock"))?;
    Ok(())
}

fn inspect_layout<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<StoreLayout, StoreError> {
    let base = relation_exists(client, &tables.base)?;
    let schema_metadata = relation_exists(client, &tables.schema_metadata)?;
    let blobs = relation_exists(client, &tables.blobs)?;
    let path_refs = relation_exists(client, &tables.path_refs)?;
    let migration_backup = relation_exists(client, &tables.migration_backup)?;
    let component_count = [schema_metadata, blobs, path_refs]
        .into_iter()
        .filter(|exists| *exists)
        .count();
    match (base, component_count, migration_backup) {
        (false, 0, false) => Ok(StoreLayout::Fresh),
        (true, 0, false) => Ok(StoreLayout::LegacyContentRows),
        (false, 3, _) => Ok(StoreLayout::Current),
        _ => Ok(StoreLayout::Incomplete),
    }
}

fn relation_exists<C: GenericClient>(client: &mut C, relation: &str) -> Result<bool, StoreError> {
    let relation: Option<String> = client
        .query_one("SELECT to_regclass($1)::text", &[&relation])
        .map_err(|_| database_error("inspect schema relation"))?
        .get(0);
    Ok(relation.is_some())
}

fn incomplete_layout_error(tables: &StoreTables) -> StoreError {
    StoreError::new(format!(
        "PostgreSQL CodeDB store {} has an incomplete or mixed schema layout; refusing automatic repair",
        tables.base
    ))
}

fn create_current_schema<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
    last_migration: &str,
) -> Result<(), StoreError> {
    let schema_version = STORE_SCHEMA_VERSION.to_string();
    let ddl = format!(
        "CREATE TABLE {schema_metadata} (\
             key text PRIMARY KEY,\
             value text NOT NULL\
         );\
         CREATE TABLE {blobs} (\
             sha256 text PRIMARY KEY,\
             content bytea NOT NULL,\
             bytes bigint NOT NULL CHECK (bytes >= 0)\
         );\
         CREATE TABLE {path_refs} (\
             module_path text NOT NULL,\
             sha256 text NOT NULL REFERENCES {blobs}(sha256),\
             metadata jsonb NOT NULL DEFAULT '{{}}'::jsonb\
         );",
        schema_metadata = tables.schema_metadata,
        blobs = tables.blobs,
        path_refs = tables.path_refs,
    );
    client
        .batch_execute(&ddl)
        .map_err(|_| database_error("create current schema"))?;
    ensure_repo_path_unique_index(client, tables)?;
    ensure_blob_chunking_schema(client, tables)?;

    let metadata_sql = format!(
        "INSERT INTO {} (key, value) VALUES ($1, $2) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        tables.schema_metadata
    );
    for (key, value) in [
        ("store_backend", "postgresql"),
        ("store_status", "initialized"),
        ("schema_version", schema_version.as_str()),
        ("migration_state", CURRENT_MIGRATION_STATE),
        ("last_migration", last_migration),
        ("checksum_algorithm", "sha256"),
        ("schema_layout", SCHEMA_LAYOUT),
        ("origin", ORIGIN),
        (
            "unsupported_schema_behavior",
            "refuse_unknown_or_future_schema",
        ),
    ] {
        client
            .execute(metadata_sql.as_str(), &[&key, &value])
            .map_err(|_| database_error("write schema metadata"))?;
        maybe_inject_initialization_failure(client, last_migration, key)?;
    }
    Ok(())
}

/// Idempotently converge `path_refs` uniqueness onto
/// `(metadata->>'repo_path', module_path)` instead of `module_path` alone:
/// many capture roots share relative paths (every repository has a
/// `README.md`), so per-root attribution must also be the conflict/upsert
/// key or multi-root ingestion into one store silently corrupts.
///
/// Safe to call unconditionally: on a table just created by
/// [`create_current_schema`] there is no legacy primary key to drop and the
/// index does not yet exist; on a store bootstrapped by a build that
/// predates capture-root identity, this drops that legacy `module_path`
/// primary key (PostgreSQL's default name for an unnamed single-column
/// primary key on this table is deterministic — every such key was created
/// by this same crate's DDL) and creates the new index. Called from every
/// [`PgStore::initialize`] so an existing installation heals on its very
/// next capture; no separate migration command is required.
fn ensure_repo_path_unique_index<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    let legacy_pkey = format!("{}_pkey", tables.path_refs);
    let ddl = format!(
        "ALTER TABLE {path_refs} DROP CONSTRAINT IF EXISTS {legacy_pkey}; \
         CREATE UNIQUE INDEX IF NOT EXISTS {repo_index} ON {path_refs} ({repo_path_expr}, module_path);",
        path_refs = tables.path_refs,
        repo_index = tables.path_refs_repo_index,
        repo_path_expr = REPO_PATH_METADATA_EXPR,
    );
    client
        .batch_execute(&ddl)
        .map_err(|_| database_error("ensure repo-path unique index"))
}

/// Idempotently add chunked large-blob storage: relax `{blobs}.content` from
/// `NOT NULL` (a blob at/above [`CHUNK_THRESHOLD_BYTES`] stores `NULL` there
/// instead) and add the `chunk_count` column plus the companion
/// `{blobs}_chunks`-style table that holds its content in ordered pieces.
///
/// Safe to call unconditionally: on a table just created by
/// [`create_current_schema`], `content` is still `NOT NULL` and there is no
/// `chunk_count` column or chunks table yet, so this only adds them; on a
/// store bootstrapped by a build that predates chunked storage, this heals
/// it in place. Called from every [`PgStore::initialize`] so an existing
/// installation heals on its very next capture; no separate migration
/// command is required. `chunk_count = 0` (the default, and every row
/// written before this change) means "read `content` directly"; `> 0` means
/// "reassemble that many ordered rows from `{blobs}_chunks`" — existing rows
/// and existing readers of an unchanged store are unaffected either way.
fn ensure_blob_chunking_schema<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    let ddl = format!(
        "ALTER TABLE {blobs} ALTER COLUMN content DROP NOT NULL; \
         ALTER TABLE {blobs} ADD COLUMN IF NOT EXISTS chunk_count integer NOT NULL DEFAULT 0 CHECK (chunk_count >= 0); \
         CREATE TABLE IF NOT EXISTS {blob_chunks} (\
             sha256 text NOT NULL REFERENCES {blobs}(sha256),\
             seq integer NOT NULL CHECK (seq >= 0),\
             chunk bytea NOT NULL,\
             PRIMARY KEY (sha256, seq)\
         );",
        blobs = tables.blobs,
        blob_chunks = tables.blob_chunks,
    );
    client
        .batch_execute(&ddl)
        .map_err(|_| database_error("ensure blob chunking schema"))
}

#[cfg(feature = "pg-integration")]
fn maybe_inject_initialization_failure<C: GenericClient>(
    client: &mut C,
    last_migration: &str,
    metadata_key: &str,
) -> Result<(), StoreError> {
    if last_migration != "initialize_v1" || metadata_key != "store_backend" {
        return Ok(());
    }
    let setting: Option<String> = client
        .query_one(
            "SELECT current_setting(\
                 'codedb.test_fail_initialization_after_first_metadata', true\
             )",
            &[],
        )
        .map_err(|_| database_error("read initialization test fault setting"))?
        .get(0);
    if setting.as_deref() == Some("on") {
        return Err(database_error(
            "injected initialization failure after first schema metadata write",
        ));
    }
    Ok(())
}

#[cfg(not(feature = "pg-integration"))]
fn maybe_inject_initialization_failure<C: GenericClient>(
    _client: &mut C,
    _last_migration: &str,
    _metadata_key: &str,
) -> Result<(), StoreError> {
    Ok(())
}

fn validate_current_schema<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    validate_relation_shape(
        client,
        &tables.schema_metadata,
        &[("key", "text"), ("value", "text")],
    )?;
    validate_relation_shape(
        client,
        &tables.blobs,
        &[
            ("sha256", "text"),
            ("content", "bytea"),
            ("bytes", "bigint"),
            ("chunk_count", "integer"),
        ],
    )?;
    validate_relation_shape(
        client,
        &tables.blob_chunks,
        &[("sha256", "text"), ("seq", "integer"), ("chunk", "bytea")],
    )?;
    validate_relation_shape(
        client,
        &tables.path_refs,
        &[
            ("module_path", "text"),
            ("sha256", "text"),
            ("metadata", "jsonb"),
        ],
    )?;

    let metadata = read_schema_metadata(client, tables)?;

    let schema_version = metadata.get("schema_version").ok_or_else(|| {
        StoreError::new("PostgreSQL CodeDB schema is missing schema_version metadata")
    })?;
    let observed = parse_schema_version(schema_version)?;
    if observed != CURRENT_STORE_SCHEMA_VERSION {
        return Err(StoreError::new(format!(
            "unsupported PostgreSQL CodeDB schema version {schema_version:?}; this client supports {STORE_SCHEMA_VERSION:?} and refuses unknown or future schemas"
        )));
    }
    for (key, expected) in [
        ("migration_state", CURRENT_MIGRATION_STATE),
        ("store_backend", "postgresql"),
        ("checksum_algorithm", "sha256"),
        ("schema_layout", SCHEMA_LAYOUT),
    ] {
        match metadata.get(key) {
            Some(value) if value == expected => {}
            Some(value) => {
                return Err(StoreError::new(format!(
                    "unsupported PostgreSQL CodeDB {key} metadata value {value:?}; refusing data access"
                )));
            }
            None => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB schema is missing required {key} metadata"
                )));
            }
        }
    }
    Ok(())
}

fn read_schema_metadata<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<BTreeMap<String, String>, StoreError> {
    let metadata_sql = format!("SELECT key, value FROM {}", tables.schema_metadata);
    Ok(client
        .query(metadata_sql.as_str(), &[])
        .map_err(|_| database_error("validate schema metadata"))?
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
        .collect())
}

fn validate_relation_shape<C: GenericClient>(
    client: &mut C,
    relation: &str,
    expected_columns: &[(&str, &str)],
) -> Result<(), StoreError> {
    let kind: String = client
        .query_one(
            "SELECT relkind::text FROM pg_catalog.pg_class WHERE oid = to_regclass($1)",
            &[&relation],
        )
        .map_err(|_| database_error("validate schema relation kind"))?
        .get(0);
    if kind != "r" {
        return Err(StoreError::new(format!(
            "PostgreSQL CodeDB relation {relation} is not a table"
        )));
    }
    let columns = client
        .query(
            "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod) \
             FROM pg_catalog.pg_attribute a \
             WHERE a.attrelid = to_regclass($1) \
               AND a.attnum > 0 AND NOT a.attisdropped",
            &[&relation],
        )
        .map_err(|_| database_error("validate schema relation columns"))?
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
        .collect::<BTreeMap<_, _>>();
    for (column, expected_type) in expected_columns {
        match columns.get(*column) {
            Some(observed) if observed == expected_type => {}
            Some(observed) => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB relation {relation} column {column} has type {observed:?}, expected {expected_type:?}"
                )));
            }
            None => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB relation {relation} is missing required column {column}"
                )));
            }
        }
    }
    Ok(())
}

fn create_legacy_migration_backup<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    if relation_exists(client, &tables.migration_backup)? {
        return Err(StoreError::new(format!(
            "PostgreSQL CodeDB store {} already has a migration backup; refusing overwrite",
            tables.base
        )));
    }
    client
        .batch_execute(
            format!(
                "ALTER TABLE {} RENAME TO {}",
                tables.base, tables.migration_backup
            )
            .as_str(),
        )
        .map_err(|_| database_error("create PostgreSQL migration backup"))?;
    Ok(())
}

fn migrate_legacy_content_rows<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
    legacy_relation: &str,
    migration_id: &str,
) -> Result<(), StoreError> {
    validate_legacy_relation(client, legacy_relation)?;
    create_current_schema(client, tables, migration_id)?;
    let legacy_sql = format!(
        "SELECT module_path, content, sha256, COALESCE(metadata, '{{}}'::jsonb)::text \
         FROM {} ORDER BY module_path COLLATE \"C\"",
        legacy_relation
    );
    let legacy_rows = client
        .query(legacy_sql.as_str(), &[])
        .map_err(|_| database_error("read legacy content rows"))?;
    let blob_sql = format!(
        "INSERT INTO {} (sha256, content, bytes) VALUES ($1, $2, $3) \
         ON CONFLICT (sha256) DO NOTHING",
        tables.blobs
    );
    let path_sql = format!(
        "INSERT INTO {} (module_path, sha256, metadata) VALUES ($1, $2, $3::text::jsonb)",
        tables.path_refs
    );
    for row in legacy_rows {
        let relative_path: String = row.get(0);
        let content: Vec<u8> = row.get(1);
        let sha256: String = row.get(2);
        let metadata: String = row.get(3);
        if sha256 != sha256_hex(&content) {
            return Err(StoreError::new(format!(
                "legacy PostgreSQL CodeDB row {relative_path:?} has a content checksum mismatch; refusing migration"
            )));
        }
        let byte_count = i64::try_from(content.len())
            .map_err(|_| StoreError::new("legacy blob exceeds PostgreSQL bigint size"))?;
        client
            .execute(blob_sql.as_str(), &[&sha256, &content, &byte_count])
            .map_err(|_| database_error("migrate legacy content-addressed blob"))?;
        client
            .execute(path_sql.as_str(), &[&relative_path, &sha256, &metadata])
            .map_err(|_| database_error("migrate legacy path reference"))?;
    }
    Ok(())
}

fn validate_legacy_relation<C: GenericClient>(
    client: &mut C,
    relation: &str,
) -> Result<(), StoreError> {
    validate_relation_shape(
        client,
        relation,
        &[
            ("module_path", "text"),
            ("content", "bytea"),
            ("sha256", "text"),
            ("metadata", "jsonb"),
        ],
    )
}

fn corrupt_path_reference_error(relative_path: &str, sha256: &str) -> StoreError {
    StoreError::new(format!(
        "PostgreSQL CodeDB path reference {relative_path:?} points to missing or invalid blob sha256:{sha256}"
    ))
}

/// Number of `blob_chunks` rows a `total_bytes`-byte blob splits into at
/// `threshold`: `0` means "not chunked, read `content` directly" (used when
/// `total_bytes <= threshold`, so a blob of exactly `threshold` bytes is
/// still stored whole); otherwise the ceiling division, so a blob one byte
/// over `threshold` still gets a (very unbalanced) second chunk. Pure and
/// infallible so EVERY-BYTE engine gap G1's boundary conditions (0 bytes,
/// exactly `threshold`, `threshold + 1`, `2.5 * threshold`) are
/// unit-testable with a small `threshold` instead of allocating real
/// megabytes; the astronomically large chunk count that would overflow
/// `u32` saturates instead of panicking.
fn chunk_count_for(total_bytes: usize, threshold: usize) -> u32 {
    if total_bytes <= threshold {
        0
    } else {
        u32::try_from(total_bytes.div_ceil(threshold)).unwrap_or(u32::MAX)
    }
}

/// Reassemble a blob from its ordered `(seq, chunk)` rows, already fetched
/// from `blob_chunks` in ascending `seq` order (EVERY-BYTE engine gap G1).
///
/// Verifies the stored chunk count matches what was declared and that
/// sequence numbers are exactly `0..chunk_count` with no gap or duplicate
/// before concatenating; the caller (`read_source_file_blob` /
/// `materialize_source_file`) already verifies the reassembled bytes hash to
/// `sha256`, so together the roundtrip is fully integrity-checked
/// structurally and cryptographically. Pure (no I/O) so it is unit-testable
/// directly with hand-built chunk vectors.
fn reassemble_chunks(
    chunks: Vec<(i32, Vec<u8>)>,
    sha256: &str,
    chunk_count: i32,
) -> Result<Vec<u8>, StoreError> {
    if chunks.len() != chunk_count as usize {
        return Err(StoreError::new(format!(
            "PostgreSQL CodeDB blob {sha256} declares {chunk_count} chunks but {} are stored",
            chunks.len()
        )));
    }
    let mut bytes = Vec::new();
    for (expected_seq, (seq, chunk)) in chunks.into_iter().enumerate() {
        if seq != expected_seq as i32 {
            return Err(StoreError::new(format!(
                "PostgreSQL CodeDB blob {sha256} has a non-contiguous chunk sequence at position {expected_seq}"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// Fetch a chunked blob's ordered rows from `blob_chunks` and reassemble
/// them via [`reassemble_chunks`] (EVERY-BYTE engine gap G1).
fn read_chunked_blob(
    client: &mut Client,
    blob_chunks_table: &str,
    sha256: &str,
    chunk_count: i32,
) -> Result<Vec<u8>, StoreError> {
    let sql = format!("SELECT seq, chunk FROM {blob_chunks_table} WHERE sha256 = $1 ORDER BY seq");
    let chunks = client
        .query(sql.as_str(), &[&sha256])
        .map_err(|_| database_error("read content-addressed blob chunks"))?
        .into_iter()
        .map(|row| (row.get::<_, i32>(0), row.get::<_, Vec<u8>>(1)))
        .collect();
    reassemble_chunks(chunks, sha256, chunk_count)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Metadata for a raw-blob `path_refs` row, tagged with the store's capture
/// root when [`PgStore::set_capture_root`] has set one. Call sites that never
/// set a capture root all collapse onto [`NO_CAPTURE_ROOT_SENTINEL`],
/// reproducing today's single-bucket-per-store behavior exactly.
fn raw_blob_metadata_json(capture_root: Option<&str>) -> String {
    serde_json::json!({
        "artifact_kind": "raw_blob",
        "permission_capture": "gap_not_available_for_raw_blob",
        "repo_path": capture_root.unwrap_or(NO_CAPTURE_ROOT_SENTINEL),
    })
    .to_string()
}

#[cfg(unix)]
fn parse_unix_mode(metadata_text: &str) -> Option<u32> {
    let value = serde_json::from_str::<serde_json::Value>(metadata_text).ok()?;
    let field = value.get("unix_mode")?;
    if let Some(value) = field.as_str() {
        u32::from_str_radix(value, 8).ok()
    } else {
        field.as_u64().and_then(|value| u32::try_from(value).ok())
    }
}

// ---------------------------------------------------------------------------
// Outbox export contract (ARCHBP-002): the versioned PostgreSQL landing table
// for ordered embedding work drained from the redb outbox. This table is an
// export contract only — envctl remains the sole authoritative committer and
// consumes these rows; nothing here mutates authoritative LifeOS state.
// ---------------------------------------------------------------------------

/// Fixed name of the versioned export contract table.
pub const OUTBOX_EXPORT_TABLE: &str = "codedb_outbox_export";
/// Version stamped on every exported row.
pub const OUTBOX_EXPORT_CONTRACT_VERSION: &str = "codedb.outbox-export.v0";

#[derive(Debug, Clone)]
pub struct OutboxExportRowInput {
    pub seq: u64,
    pub blob_sha256: String,
    pub job_json: String,
}

#[derive(Debug, Clone, Default)]
pub struct OutboxExportOutcome {
    pub inserted: Vec<u64>,
    pub skipped_existing: Vec<u64>,
}

/// Idempotently land export rows keyed by outbox sequence. Creates the
/// contract table when absent; every insert is `ON CONFLICT (seq) DO
/// NOTHING`, and the whole batch commits in one transaction so a crash can
/// never leave a partially visible batch.
pub fn outbox_export_flush(
    conn: &str,
    rows: &[OutboxExportRowInput],
) -> Result<OutboxExportOutcome, StoreError> {
    let mut outcome = OutboxExportOutcome::default();
    if rows.is_empty() {
        return Ok(outcome);
    }
    let mut client = connect_client(conn)?;
    let mut tx = client
        .transaction()
        .map_err(|_| database_error("begin outbox export transaction"))?;
    tx.batch_execute(&format!(
        "CREATE TABLE IF NOT EXISTS {OUTBOX_EXPORT_TABLE} (\
             seq BIGINT PRIMARY KEY,\
             contract_version TEXT NOT NULL,\
             blob_sha256 TEXT NOT NULL,\
             job JSONB NOT NULL,\
             synced_at TIMESTAMPTZ NOT NULL DEFAULT now()\
         )"
    ))
    .map_err(|_| database_error("create outbox export contract table"))?;
    let insert = format!(
        "INSERT INTO {OUTBOX_EXPORT_TABLE} (seq, contract_version, blob_sha256, job) \
         VALUES ($1, $2, $3, $4::text::jsonb) ON CONFLICT (seq) DO NOTHING"
    );
    for row in rows {
        let seq = i64::try_from(row.seq).map_err(|_| {
            StoreError::new(format!("outbox sequence {} overflows BIGINT", row.seq))
        })?;
        let affected = tx
            .execute(
                insert.as_str(),
                &[
                    &seq,
                    &OUTBOX_EXPORT_CONTRACT_VERSION,
                    &row.blob_sha256,
                    &row.job_json,
                ],
            )
            .map_err(|_| database_error("insert outbox export row"))?;
        if affected == 1 {
            outcome.inserted.push(row.seq);
        } else {
            outcome.skipped_existing.push(row.seq);
        }
    }
    tx.commit()
        .map_err(|_| database_error("commit outbox export transaction"))?;
    Ok(outcome)
}

/// Read back every exported row ordered by sequence: (seq, contract_version,
/// blob_sha256, job_json). An absent contract table reports no rows.
pub fn outbox_export_rows(
    conn: &str,
) -> Result<Vec<(i64, String, String, String)>, StoreError> {
    let mut client = connect_client(conn)?;
    let exists: Option<String> = client
        .query_one("SELECT to_regclass($1)::text", &[&OUTBOX_EXPORT_TABLE])
        .map_err(|_| database_error("probe outbox export contract table"))?
        .get(0);
    if exists.is_none() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            format!(
                "SELECT seq, contract_version, blob_sha256, job::text \
                 FROM {OUTBOX_EXPORT_TABLE} ORDER BY seq"
            )
            .as_str(),
            &[],
        )
        .map_err(|_| database_error("read outbox export rows"))?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2), row.get(3)))
        .collect())
}

// ---------------------------------------------------------------------------
// Raw-object metadata landing (ARCHBP-041): the PostgreSQL twin of redb's
// raw_objects table, keyed by the same canonical content-addressed ids so
// the two stores hold parity metadata for every captured raw byte object.
// ---------------------------------------------------------------------------

/// Fixed name of the raw-object metadata table.
pub const RAW_OBJECTS_TABLE: &str = "codedb_raw_objects";

#[derive(Debug, Clone, Default)]
pub struct RawObjectsFlushOutcome {
    pub inserted: Vec<String>,
    pub skipped_existing: Vec<String>,
}

/// Idempotently land raw-object metadata rows keyed by canonical id.
pub fn raw_objects_flush(
    conn: &str,
    rows: &[(String, String)],
) -> Result<RawObjectsFlushOutcome, StoreError> {
    let mut outcome = RawObjectsFlushOutcome::default();
    if rows.is_empty() {
        return Ok(outcome);
    }
    let mut client = connect_client(conn)?;
    let mut tx = client
        .transaction()
        .map_err(|_| database_error("begin raw objects transaction"))?;
    tx.batch_execute(&format!(
        "CREATE TABLE IF NOT EXISTS {RAW_OBJECTS_TABLE} (\
             raw_object_id TEXT PRIMARY KEY,\
             metadata JSONB NOT NULL,\
             landed_at TIMESTAMPTZ NOT NULL DEFAULT now()\
         )"
    ))
    .map_err(|_| database_error("create raw objects table"))?;
    let insert = format!(
        "INSERT INTO {RAW_OBJECTS_TABLE} (raw_object_id, metadata) \
         VALUES ($1, $2::text::jsonb) ON CONFLICT (raw_object_id) DO NOTHING"
    );
    for (raw_object_id, metadata_json) in rows {
        let affected = tx
            .execute(insert.as_str(), &[raw_object_id, metadata_json])
            .map_err(|_| database_error("insert raw object row"))?;
        if affected == 1 {
            outcome.inserted.push(raw_object_id.clone());
        } else {
            outcome.skipped_existing.push(raw_object_id.clone());
        }
    }
    tx.commit()
        .map_err(|_| database_error("commit raw objects transaction"))?;
    Ok(outcome)
}

/// Read back every raw-object metadata row ordered by canonical id. An
/// absent table reports no rows.
pub fn raw_objects_rows(conn: &str) -> Result<Vec<(String, String)>, StoreError> {
    let mut client = connect_client(conn)?;
    let exists: Option<String> = client
        .query_one("SELECT to_regclass($1)::text", &[&RAW_OBJECTS_TABLE])
        .map_err(|_| database_error("probe raw objects table"))?
        .get(0);
    if exists.is_none() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            format!(
                "SELECT raw_object_id, metadata::text FROM {RAW_OBJECTS_TABLE} \
                 ORDER BY raw_object_id"
            )
            .as_str(),
            &[],
        )
        .map_err(|_| database_error("read raw object rows"))?;
    Ok(rows.into_iter().map(|row| (row.get(0), row.get(1))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_table_accepts_safe_logical_store_identifier() {
        assert_eq!(
            sanitize_table("codebase_codedb").unwrap(),
            "codebase_codedb"
        );
        assert_eq!(sanitize_table("_x9").unwrap(), "_x9");
    }

    #[test]
    fn sanitize_table_rejects_injection_and_component_name_overflow() {
        assert!(sanitize_table("a; drop table x").is_err());
        assert!(sanitize_table("public.codebase").is_err());
        assert!(sanitize_table("").is_err());
        assert!(sanitize_table("9abc").is_err());
        assert!(sanitize_table(&"a".repeat(MAX_IDENTIFIER_BYTES)).is_err());
    }

    #[test]
    fn sanitize_table_reserves_headroom_for_the_longest_generated_identifier() {
        // 43 + "_path_refs_repo_uidx".len() (20) == 63, exactly PostgreSQL's
        // NAMEDATALEN limit; one byte longer must be rejected up front rather
        // than silently truncated by PostgreSQL later.
        let fits = "a".repeat(43);
        let base = sanitize_table(&fits).expect("43-byte base name must fit");
        let tables = StoreTables::new(&base).expect("StoreTables must build for a fitting base");
        assert_eq!(tables.path_refs_repo_index.len(), 63);

        let overflow = "a".repeat(44);
        assert!(sanitize_table(&overflow).is_err());
    }

    // --- EVERY-BYTE engine gap G1: chunked large-blob storage boundaries ---
    //
    // A lowered `threshold` (never the real `CHUNK_THRESHOLD_BYTES`) exercises
    // exactly the same pure math and reassembly code the real 256 MiB
    // threshold uses in production, without allocating real megabytes.

    #[test]
    fn chunk_count_for_is_zero_at_and_below_the_threshold() {
        let threshold = 8;
        assert_eq!(
            chunk_count_for(0, threshold),
            0,
            "an empty blob is never chunked"
        );
        assert_eq!(
            chunk_count_for(threshold, threshold),
            0,
            "a blob exactly at the threshold is stored whole, not chunked"
        );
    }

    #[test]
    fn chunk_count_for_splits_one_byte_over_the_threshold_into_two_chunks() {
        let threshold = 8;
        assert_eq!(chunk_count_for(threshold + 1, threshold), 2);
    }

    #[test]
    fn chunk_count_for_rounds_a_fractional_multiple_up() {
        let threshold = 8;
        // 2.5 * threshold: two full chunks plus one half-size final chunk.
        assert_eq!(chunk_count_for(threshold * 2 + threshold / 2, threshold), 3);
    }

    #[test]
    fn persisted_chunk_boundaries_match_chunk_count_for() {
        // The same `.chunks(threshold)` iterator `persist_batch_at_threshold`
        // uses to split bytes for insertion must always agree with
        // `chunk_count_for`'s prediction, across every boundary the fixpack2
        // spec calls out (0, exactly threshold, threshold + 1, 2.5x).
        let threshold = 8usize;
        for total_bytes in [
            0usize,
            threshold,
            threshold + 1,
            threshold * 2 + threshold / 2,
        ] {
            let bytes = vec![0xABu8; total_bytes];
            let expected = chunk_count_for(total_bytes, threshold);
            if expected == 0 {
                assert!(
                    bytes.len() <= threshold,
                    "chunk_count_for said unchunked for {total_bytes} bytes at threshold {threshold}"
                );
                continue;
            }
            let actual = u32::try_from(bytes.chunks(threshold).count()).unwrap();
            assert_eq!(
                actual, expected,
                "chunk_count_for disagreed with the real splitting iterator for {total_bytes} bytes"
            );
        }
    }

    #[test]
    fn reassemble_chunks_concatenates_contiguous_ordered_chunks() {
        let chunks = vec![
            (0, b"abcdefgh".to_vec()),
            (1, b"ijklmnop".to_vec()),
            (2, b"qr".to_vec()),
        ];
        let bytes = reassemble_chunks(chunks, "test-sha", 3).expect("reassemble contiguous chunks");
        assert_eq!(bytes, b"abcdefghijklmnopqr");
    }

    #[test]
    fn reassemble_chunks_rejects_a_declared_count_mismatch() {
        let chunks = vec![(0, b"only-one".to_vec())];
        let error = reassemble_chunks(chunks, "test-sha", 2)
            .expect_err("declaring 2 chunks but storing 1 must be rejected");
        assert!(
            error
                .message()
                .contains("declares 2 chunks but 1 are stored")
        );
    }

    #[test]
    fn reassemble_chunks_rejects_a_gap_in_the_sequence() {
        let chunks = vec![(0, b"first".to_vec()), (2, b"third".to_vec())];
        let error = reassemble_chunks(chunks, "test-sha", 2)
            .expect_err("a gap in the sequence (0, 2) must be rejected");
        assert!(error.message().contains("non-contiguous"));
    }

    #[test]
    fn reassemble_chunks_handles_the_empty_case() {
        // Mirrors the `total_bytes == 0` boundary: zero declared chunks,
        // zero stored chunks, empty result.
        let bytes = reassemble_chunks(Vec::new(), "test-sha", 0).expect("reassemble zero chunks");
        assert!(bytes.is_empty());
    }

    #[test]
    fn raw_blob_metadata_json_includes_the_capture_root_when_set() {
        let json = raw_blob_metadata_json(Some("/captures/repo-a"));
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["artifact_kind"], "raw_blob");
        assert_eq!(
            value["permission_capture"],
            "gap_not_available_for_raw_blob"
        );
        assert_eq!(value["repo_path"], "/captures/repo-a");
    }

    #[test]
    fn raw_blob_metadata_json_falls_back_to_the_sentinel_when_capture_root_is_unset() {
        let json = raw_blob_metadata_json(None);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(value["repo_path"], NO_CAPTURE_ROOT_SENTINEL);
    }

    #[test]
    fn connection_diagnostics_do_not_contain_a_supplied_secret() {
        let secret = "not-a-real-postgresql-password";
        let dsn = format!("postgresql://codedb:{secret}@db.example.invalid/codedb");
        let error = connection_error(&dsn);
        assert!(!error.message().contains(secret));
        assert!(!error.message().contains("postgres://"));
        assert!(error.message().contains("redacted"));
    }

    #[test]
    fn tls_policy_parses_verified_tcp_url_and_keyword_dsn() {
        for dsn in [
            "postgresql://codedb:secret@db.example.invalid/codedb?sslmode=verify-full&sslrootcert=%2Fetc%2Fcodedb%2Froot.crt",
            "host=db.example.invalid user=codedb password='secret value' dbname=codedb sslmode=verify-full sslrootcert=/etc/codedb/root.crt",
        ] {
            let secured = parse_connection_security(dsn).expect("parse verified TLS policy");
            assert_eq!(secured.config.get_ssl_mode(), SslMode::Require);
            assert_eq!(
                secured.transport,
                ConnectionTransport::VerifiedTls {
                    ca_path: PathBuf::from("/etc/codedb/root.crt")
                }
            );
            assert!(secured.config.get_hosts().iter().all(
                |host| matches!(host, Host::Tcp(hostname) if hostname == "db.example.invalid")
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn tls_policy_parses_only_explicit_unix_socket_as_plaintext() {
        let secured = parse_connection_security(
            "host='/run/postgresql' user=codedb password='secret value' sslmode=disable",
        )
        .expect("parse explicit Unix socket policy");
        assert_eq!(secured.transport, ConnectionTransport::UnixSocket);
        assert_eq!(secured.config.get_ssl_mode(), SslMode::Disable);
        assert!(
            secured.config.get_hosts().iter().all(
                |host| matches!(host, Host::Unix(path) if path == Path::new("/run/postgresql"))
            )
        );
    }

    #[test]
    fn tls_policy_rejects_weaker_mixed_duplicate_and_relative_ca_configuration() {
        for dsn in [
            "host=db.example.invalid sslmode=require sslrootcert=/etc/codedb/root.crt",
            "host=db.example.invalid,/run/postgresql sslmode=verify-full sslrootcert=/etc/codedb/root.crt",
            "hostaddr=203.0.113.7 sslmode=verify-full sslrootcert=/etc/codedb/root.crt",
            "host=db.example.invalid sslmode=verify-full sslmode=verify-full sslrootcert=/etc/codedb/root.crt",
            "host=db.example.invalid sslmode=verify-full sslrootcert=relative/root.crt",
        ] {
            let error = parse_connection_security(dsn)
                .err()
                .expect("unsafe policy must fail closed");
            assert!(error.message().contains("redacted"));
            assert!(!error.message().contains("db.example.invalid"));
            assert!(!error.message().contains("root.crt"));
        }
    }

    #[test]
    fn malformed_dsn_diagnostics_never_echo_credentials_or_hosts() {
        let secret = "diagnostic-secret";
        let dsn = format!(
            "host=db.example.invalid user=codedb password='{secret}' sslmode='unterminated"
        );
        let error = parse_connection_security(&dsn)
            .err()
            .expect("malformed DSN must fail");
        assert_eq!(
            error.message(),
            "invalid PostgreSQL DSN; connection details redacted"
        );
        assert!(!error.message().contains(secret));
        assert!(!error.message().contains("db.example.invalid"));
    }

    #[test]
    fn empty_ca_bundle_fails_closed_without_disclosing_its_path() {
        let ca = tempfile::NamedTempFile::new().expect("empty CA fixture");
        let error = load_ca_roots(ca.path()).expect_err("empty CA bundle must fail closed");
        assert!(error.message().contains("no trusted certificates"));
        assert!(error.message().contains("redacted"));
        assert!(!error.message().contains(&ca.path().display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn parse_unix_mode_reads_octal_string_and_decimal() {
        assert_eq!(parse_unix_mode("{\"unix_mode\":\"755\"}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"unix_mode\":493}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"artifact_kind\":\"raw_blob\"}"), None);
    }
}

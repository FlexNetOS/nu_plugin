#![forbid(unsafe_code)]

//! PostgreSQL implementation of CodeDB's backend-neutral content-addressed
//! [`BlobStore`] contract.
//!
//! A logical store name expands to three tables:
//!
//! - `<store>_blobs` contains one byte-exact blob per SHA-256 digest.
//! - `<store>_path_refs` maps captured relative paths to blob digests.
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
const LONGEST_COMPONENT_SUFFIX: &str = "_migration_backup";
const POSTGRESQL_MIGRATIONS: [StoreMigrationStep; 1] = [StoreMigrationStep::new(
    "postgresql_legacy_content_rows_to_v1",
    LEGACY_STORE_SCHEMA_VERSION,
    CURRENT_STORE_SCHEMA_VERSION,
)];
const BATCH_METADATA_JSON: &str =
    "{\"artifact_kind\":\"raw_blob\",\"permission_capture\":\"gap_not_available_for_raw_blob\"}";

#[derive(Clone, Debug)]
struct StoreTables {
    base: String,
    schema_metadata: String,
    blobs: String,
    path_refs: String,
    migration_backup: String,
}

impl StoreTables {
    fn new(table: &str) -> Result<Self, StoreError> {
        let base = sanitize_table(table)?;
        Ok(Self {
            schema_metadata: format!("{base}_schema_metadata"),
            blobs: format!("{base}_blobs"),
            path_refs: format!("{base}_path_refs"),
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
}

impl fmt::Debug for PgStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgStore")
            .field("tables", &self.tables)
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
            StoreLayout::Current => {}
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
                 DROP TABLE {blobs};\
                 DROP TABLE {schema_metadata};\
                 ALTER TABLE {backup} RENAME TO {base};",
                path_refs = tables.path_refs,
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
        })
    }

    /// The validated logical store identifier supplied by the caller.
    pub fn table(&self) -> &str {
        &self.tables.base
    }
}

impl BlobStore for PgStore {
    fn persist_batch(
        &mut self,
        files: &[(String, Vec<u8>)],
    ) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin batch transaction"))?;
        let blob_sql = format!(
            "INSERT INTO {} (sha256, content, bytes) VALUES ($1, $2, $3) \
             ON CONFLICT (sha256) DO NOTHING",
            self.tables.blobs
        );
        let path_sql = format!(
            "INSERT INTO {} (module_path, sha256, metadata) VALUES ($1, $2, $3::text::jsonb) \
             ON CONFLICT (module_path) DO UPDATE SET \
                 sha256 = EXCLUDED.sha256, metadata = EXCLUDED.metadata",
            self.tables.path_refs
        );

        let mut rows = Vec::with_capacity(files.len());
        for (relative_path, bytes) in files {
            let sha256 = sha256_hex(bytes);
            let content = bytes.as_slice();
            let byte_count = i64::try_from(bytes.len())
                .map_err(|_| StoreError::new("captured blob exceeds PostgreSQL bigint size"))?;
            tx.execute(blob_sql.as_str(), &[&sha256, &content, &byte_count])
                .map_err(|_| database_error("insert content-addressed blob"))?;
            tx.execute(
                path_sql.as_str(),
                &[relative_path, &sha256, &BATCH_METADATA_JSON],
            )
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

    fn persist_symlink(
        &mut self,
        relative_path: &str,
        target: &str,
    ) -> Result<SourceSymlinkRow, StoreError> {
        let row = SourceSymlinkRow::new(relative_path, target);
        let metadata = serde_json::json!({
            "artifact_kind": "symlink",
            "symlink_target_sha256": row.target_sha256,
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
             ON CONFLICT (module_path) DO UPDATE SET \
                 sha256 = EXCLUDED.sha256, metadata = EXCLUDED.metadata",
            self.tables.path_refs
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
            "SELECT p.sha256, b.content, p.metadata->>'artifact_kind' FROM {} p \
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
        let artifact_kind: Option<String> = row.get(2);
        if artifact_kind.as_deref() == Some("symlink") {
            return Ok(None);
        }
        content
            .ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))
            .map(Some)
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
            "SELECT p.sha256, b.content, p.metadata::text, \
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
        let artifact_kind: Option<String> = row.get(3);
        if artifact_kind.as_deref() == Some("symlink") {
            return Err(StoreError::new(format!(
                "captured symlink {relative_path:?} cannot be materialized as a regular file"
            )));
        }
        let content: Option<Vec<u8>> = row.get(1);
        let content =
            content.ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))?;
        #[cfg(unix)]
        let unix_mode = {
            let metadata_text: String = row.get(2);
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
             module_path text PRIMARY KEY,\
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
        ],
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

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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

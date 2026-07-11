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
use std::fs;
use std::path::Path;

use codedb_core::store::{
    BlobStore, MaterializedFile, SourceFileRow, StoreError, StoreMetadataRow,
};
use postgres::{Client, GenericClient, NoTls};
use sha2::{Digest, Sha256};

pub const DEFAULT_TABLE: &str = "codebase_codedb";
pub const STORE_SCHEMA_VERSION: &str = "1.0.0";

const ORIGIN: &str = "codedb";
const CURRENT_MIGRATION_STATE: &str = "current";
const SCHEMA_LAYOUT: &str = "content_addressed_blobs_plus_path_refs";
const MAX_IDENTIFIER_BYTES: usize = 63;
const LONGEST_COMPONENT_SUFFIX: &str = "_schema_metadata";
const BATCH_METADATA_JSON: &str =
    "{\"artifact_kind\":\"raw_blob\",\"permission_capture\":\"gap_not_available_for_raw_blob\"}";

#[derive(Clone, Debug)]
struct StoreTables {
    base: String,
    schema_metadata: String,
    blobs: String,
    path_refs: String,
}

impl StoreTables {
    fn new(table: &str) -> Result<Self, StoreError> {
        let base = sanitize_table(table)?;
        Ok(Self {
            schema_metadata: format!("{base}_schema_metadata"),
            blobs: format!("{base}_blobs"),
            path_refs: format!("{base}_path_refs"),
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
        let tables = StoreTables::new(table)?;
        let mut client = connect_client(conn)?;
        let mut tx = client
            .transaction()
            .map_err(|_| database_error("begin schema migration transaction"))?;
        acquire_store_mutation_lock(&mut tx, &tables)?;
        match inspect_layout(&mut tx, &tables)? {
            StoreLayout::Fresh => {
                return Err(StoreError::new(format!(
                    "PostgreSQL CodeDB store {} is not initialized; run PgStore::initialize first",
                    tables.base
                )));
            }
            StoreLayout::Current => {
                validate_current_schema(&mut tx, &tables)?;
            }
            StoreLayout::LegacyContentRows => {
                migrate_legacy_content_rows(&mut tx, &tables)?;
                validate_current_schema(&mut tx, &tables)?;
            }
            StoreLayout::Incomplete => {
                return Err(incomplete_layout_error(&tables));
            }
        }
        tx.commit()
            .map_err(|_| database_error("commit schema migration transaction"))?;
        Ok(Self {
            client: RefCell::new(client),
            tables,
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
            "SELECT p.sha256, b.content FROM {} p \
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
        content
            .ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))
            .map(Some)
    }

    fn list_source_files(&self) -> Result<Vec<SourceFileRow>, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.module_path, p.sha256, b.bytes FROM {} p \
             LEFT JOIN {} b ON b.sha256 = p.sha256 \
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

    fn materialize_source_file(
        &self,
        relative_path: &str,
        output_path: &Path,
    ) -> Result<MaterializedFile, StoreError> {
        let mut client = self.client.borrow_mut();
        let sql = format!(
            "SELECT p.sha256, b.content, p.metadata::text FROM {} p \
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
        let content: Option<Vec<u8>> = row.get(1);
        let content =
            content.ok_or_else(|| corrupt_path_reference_error(relative_path, &sha256))?;
        let metadata_text: String = row.get(2);

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|_| {
                StoreError::new("failed to create materialization output directory")
            })?;
        }
        fs::write(output_path, &content)
            .map_err(|_| StoreError::new("failed to write materialized source file"))?;

        #[cfg(unix)]
        if let Some(mode) = parse_unix_mode(&metadata_text) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(output_path, fs::Permissions::from_mode(mode)).map_err(|_| {
                StoreError::new("failed to restore materialized source permissions")
            })?;
        }

        let materialized_sha256 = sha256_file(output_path)?;
        Ok(MaterializedFile {
            path: output_path.to_path_buf(),
            blob_ref: format!("sha256:{sha256}"),
            sha256: materialized_sha256,
            bytes: content.len() as u64,
        })
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
    Client::connect(conn, NoTls).map_err(|_| connection_error(conn))
}

fn connection_error(_conn: &str) -> StoreError {
    StoreError::new("PostgreSQL connection failed; connection details redacted")
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
    let component_count = [schema_metadata, blobs, path_refs]
        .into_iter()
        .filter(|exists| *exists)
        .count();
    match (base, component_count) {
        (false, 0) => Ok(StoreLayout::Fresh),
        (true, 0) => Ok(StoreLayout::LegacyContentRows),
        (false, 3) => Ok(StoreLayout::Current),
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
        ("schema_version", STORE_SCHEMA_VERSION),
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

    let metadata_sql = format!("SELECT key, value FROM {}", tables.schema_metadata);
    let metadata = client
        .query(metadata_sql.as_str(), &[])
        .map_err(|_| database_error("validate schema metadata"))?
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
        .collect::<BTreeMap<_, _>>();

    let schema_version = metadata.get("schema_version").ok_or_else(|| {
        StoreError::new("PostgreSQL CodeDB schema is missing schema_version metadata")
    })?;
    if schema_version != STORE_SCHEMA_VERSION {
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

fn migrate_legacy_content_rows<C: GenericClient>(
    client: &mut C,
    tables: &StoreTables,
) -> Result<(), StoreError> {
    validate_legacy_relation(client, &tables.base)?;
    create_current_schema(client, tables, "legacy_content_rows_to_v1")?;
    let legacy_sql = format!(
        "SELECT module_path, content, sha256, COALESCE(metadata, '{{}}'::jsonb)::text \
         FROM {} ORDER BY module_path COLLATE \"C\"",
        tables.base
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
    client
        .batch_execute(format!("DROP TABLE {}", tables.base).as_str())
        .map_err(|_| database_error("remove migrated legacy table"))?;
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

fn sha256_file(path: &Path) -> Result<String, StoreError> {
    let bytes = fs::read(path)
        .map_err(|_| StoreError::new("failed to checksum materialized source file"))?;
    Ok(sha256_hex(&bytes))
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

    #[cfg(unix)]
    #[test]
    fn parse_unix_mode_reads_octal_string_and_decimal() {
        assert_eq!(parse_unix_mode("{\"unix_mode\":\"755\"}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"unix_mode\":493}"), Some(0o755));
        assert_eq!(parse_unix_mode("{\"artifact_kind\":\"raw_blob\"}"), None);
    }
}

#![cfg(feature = "pg-integration")]

//! Integration tests for the PostgreSQL implementation of the backend-neutral
//! `BlobStore` contract.
//!
//! Every test uses `CODEDB_PG_CONN` explicitly. It is intentionally the only
//! accepted test connection setting: test runs must target a disposable
//! PostgreSQL service and never inherit a developer's default DSN.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use codedb_core::store::{BlobStore, StoreError};
use codedb_store_pg::{PgStore, STORE_SCHEMA_VERSION};
use postgres::{Client, NoTls};
use sha2::{Digest, Sha256};

fn fixture_batch() -> Vec<(String, Vec<u8>)> {
    vec![
        ("src/main.rs".to_string(), b"fn main() {}\n".to_vec()),
        ("src/lib.rs".to_string(), b"shared content\n".to_vec()),
        ("README.md".to_string(), b"shared content\n".to_vec()),
        (
            "nested/deep/notes.txt".to_string(),
            b"deep content\n".to_vec(),
        ),
        ("empty.txt".to_string(), Vec::new()),
    ]
}

fn disposable_conn() -> String {
    std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must select the explicit disposable PostgreSQL test service")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn require_error(result: Result<PgStore, StoreError>, expectation: &str) -> StoreError {
    match result {
        Ok(_) => panic!("{expectation}"),
        Err(error) => error,
    }
}

static TABLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestTables {
    conn: String,
    base: String,
}

impl TestTables {
    fn new() -> Self {
        let conn = disposable_conn();
        let sequence = TABLE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let base = format!("codedb_pg_test_{}_{}", std::process::id(), sequence);
        assert_ne!(base, "codebase");
        assert_ne!(base, "codebase_codedb");
        let tables = Self { conn, base };
        tables.drop_all();
        tables
    }

    fn schema_metadata(&self) -> String {
        format!("{}_schema_metadata", self.base)
    }

    fn blobs(&self) -> String {
        format!("{}_blobs", self.base)
    }

    fn path_refs(&self) -> String {
        format!("{}_path_refs", self.base)
    }

    fn connection(&self) -> Client {
        Client::connect(&self.conn, NoTls).expect("connect to disposable PostgreSQL test service")
    }

    fn relation_exists(&self, relation: &str) -> bool {
        let mut client = self.connection();
        let exists: Option<String> = client
            .query_one("SELECT to_regclass($1)::text", &[&relation])
            .expect("inspect test relation")
            .get(0);
        exists.is_some()
    }

    fn drop_all(&self) {
        let mut client = self.connection();
        let sql = format!(
            "DROP TABLE IF EXISTS {} CASCADE;\
             DROP TABLE IF EXISTS {} CASCADE;\
             DROP TABLE IF EXISTS {} CASCADE;\
             DROP TABLE IF EXISTS {} CASCADE;",
            self.path_refs(),
            self.blobs(),
            self.schema_metadata(),
            self.base
        );
        client
            .batch_execute(&sql)
            .expect("clean disposable PostgreSQL test relations");
    }
}

impl Drop for TestTables {
    fn drop(&mut self) {
        self.drop_all();
    }
}

#[test]
fn pg_blobstore_contract_is_content_addressed_and_reopenable() {
    let tables = TestTables::new();
    let batch = fixture_batch();
    let expected_paths: BTreeSet<String> = batch.iter().map(|(path, _)| path.clone()).collect();

    let mut pg = PgStore::initialize(&tables.conn, &tables.base).expect("initialize pg store");
    assert_eq!(pg.table(), tables.base);
    let persisted = pg.persist_batch(&batch).expect("persist first batch");
    assert_eq!(persisted.len(), batch.len());
    assert_eq!(
        persisted
            .iter()
            .find(|row| row.relative_path == "src/lib.rs")
            .expect("src/lib.rs row")
            .blob_ref,
        persisted
            .iter()
            .find(|row| row.relative_path == "README.md")
            .expect("README.md row")
            .blob_ref
    );

    assert_eq!(pg.captured_paths().expect("captured paths"), expected_paths);
    assert_eq!(
        pg.read_source_file_blob("src/main.rs")
            .expect("read main")
            .expect("main captured"),
        b"fn main() {}\n"
    );
    assert_eq!(
        pg.read_source_file_blob("missing.rs")
            .expect("read missing"),
        None
    );

    let listed = pg.list_source_files().expect("list source files");
    assert_eq!(
        listed
            .iter()
            .map(|row| row.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec![
            "README.md",
            "empty.txt",
            "nested/deep/notes.txt",
            "src/lib.rs",
            "src/main.rs",
        ]
    );
    assert!(
        listed
            .iter()
            .all(|row| row.blob_ref == format!("sha256:{}", row.sha256))
    );
    assert_eq!(
        listed
            .iter()
            .find(|row| row.relative_path == "empty.txt")
            .expect("empty row")
            .bytes,
        0
    );

    let output = tempfile::tempdir().expect("output directory");
    let output_path = output.path().join("nested/deep/notes.txt");
    let materialized = pg
        .materialize_source_file("nested/deep/notes.txt", &output_path)
        .expect("materialize captured source");
    assert_eq!(
        std::fs::read(&output_path).expect("read output"),
        b"deep content\n"
    );
    assert_eq!(materialized.path, output_path);
    assert_eq!(materialized.sha256, sha256_hex(b"deep content\n"));
    assert_eq!(materialized.bytes, b"deep content\n".len() as u64);
    assert!(
        pg.materialize_source_file("missing.rs", &output.path().join("missing.rs"))
            .is_err()
    );

    let metadata = pg
        .store_metadata_rows()
        .expect("schema metadata from pg store")
        .into_iter()
        .map(|row| (row.key, row.value))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        metadata.get("schema_version").map(String::as_str),
        Some(STORE_SCHEMA_VERSION)
    );
    assert_eq!(
        metadata.get("migration_state").map(String::as_str),
        Some("current")
    );
    assert_eq!(
        metadata.get("schema_layout").map(String::as_str),
        Some("content_addressed_blobs_plus_path_refs")
    );
    assert_eq!(metadata.get("source_files").map(String::as_str), Some("5"));

    drop(pg);

    let mut reopened = PgStore::open_existing(&tables.conn, &tables.base)
        .expect("validated readonly reopen of initialized pg store");
    assert_eq!(
        reopened
            .read_source_file_blob("README.md")
            .expect("read after reopen"),
        Some(b"shared content\n".to_vec())
    );

    reopened
        .persist_batch(&[("src/main.rs".to_string(), b"changed\n".to_vec())])
        .expect("overwrite existing path");
    assert_eq!(
        reopened
            .read_source_file_blob("src/main.rs")
            .expect("read overwritten path"),
        Some(b"changed\n".to_vec())
    );

    let mut admin = tables.connection();
    let blob_count: i64 = admin
        .query_one(
            format!("SELECT count(*) FROM {}", tables.blobs()).as_str(),
            &[],
        )
        .expect("count distinct content-addressed blobs")
        .get(0);
    let path_count: i64 = admin
        .query_one(
            format!("SELECT count(*) FROM {}", tables.path_refs()).as_str(),
            &[],
        )
        .expect("count path references")
        .get(0);
    assert_eq!(
        blob_count, 5,
        "identical bytes share one blob; overwritten content is retained"
    );
    assert_eq!(
        path_count, 5,
        "one path reference remains per captured path"
    );
}

#[test]
fn readonly_open_never_initializes_or_runs_migrations() {
    let tables = TestTables::new();

    let error = require_error(
        PgStore::open_existing(&tables.conn, &tables.base),
        "read-only open must reject an uninitialized PostgreSQL store",
    );
    assert!(error.message().contains("not initialized"));
    assert!(!tables.relation_exists(&tables.base));
    assert!(!tables.relation_exists(&tables.schema_metadata()));
    assert!(!tables.relation_exists(&tables.blobs()));
    assert!(!tables.relation_exists(&tables.path_refs()));
}

#[test]
fn future_or_unknown_schema_is_refused_before_blob_access() {
    let tables = TestTables::new();
    let store = PgStore::initialize(&tables.conn, &tables.base).expect("initialize current schema");
    drop(store);

    let mut admin = tables.connection();
    admin
        .execute(
            format!(
                "UPDATE {} SET value = '99.0.0' WHERE key = 'schema_version'",
                tables.schema_metadata()
            )
            .as_str(),
            &[],
        )
        .expect("mark schema as a future version");

    let error = require_error(
        PgStore::open_existing(&tables.conn, &tables.base),
        "future schema must not be opened",
    );
    assert!(
        error
            .message()
            .contains("unsupported PostgreSQL CodeDB schema version")
    );
    assert!(error.message().contains("99.0.0"));
    assert!(PgStore::migrate(&tables.conn, &tables.base).is_err());
}

#[test]
fn explicit_migration_converts_legacy_content_rows_to_blobs_and_path_refs() {
    let tables = TestTables::new();
    let mut admin = tables.connection();
    admin
        .batch_execute(
            format!(
                "CREATE TABLE {} (\
                    block_id bigserial PRIMARY KEY,\
                    module_path text NOT NULL UNIQUE,\
                    block_type text NOT NULL DEFAULT 'file',\
                    origin text NOT NULL DEFAULT 'codedb',\
                    content bytea NOT NULL,\
                    sha256 text NOT NULL,\
                    metadata jsonb\
                )",
                tables.base
            )
            .as_str(),
        )
        .expect("create legacy table");
    for (path, content) in [
        ("legacy/a.txt", b"legacy shared\n".as_slice()),
        ("legacy/b.txt", b"legacy shared\n".as_slice()),
        ("legacy/c.txt", b"legacy different\n".as_slice()),
    ] {
        let digest = sha256_hex(content);
        admin
            .execute(
                format!(
                    "INSERT INTO {} (module_path, content, sha256, metadata) \
                     VALUES ($1, $2, $3, '{{\"artifact_kind\":\"raw_blob\"}}'::jsonb)",
                    tables.base
                )
                .as_str(),
                &[&path, &content, &digest],
            )
            .expect("seed legacy row");
    }

    let legacy_error = require_error(
        PgStore::open_existing(&tables.conn, &tables.base),
        "read-only open must not silently migrate legacy storage",
    );
    assert!(legacy_error.message().contains("legacy"));

    let migrated = PgStore::migrate(&tables.conn, &tables.base)
        .expect("explicit migration should convert the known legacy layout");
    assert_eq!(
        migrated.captured_paths().expect("migrated path refs"),
        BTreeSet::from([
            "legacy/a.txt".to_string(),
            "legacy/b.txt".to_string(),
            "legacy/c.txt".to_string(),
        ])
    );
    assert_eq!(
        migrated
            .read_source_file_blob("legacy/b.txt")
            .expect("read migrated blob"),
        Some(b"legacy shared\n".to_vec())
    );
    drop(migrated);

    assert!(!tables.relation_exists(&tables.base));
    assert!(tables.relation_exists(&tables.schema_metadata()));
    assert!(tables.relation_exists(&tables.blobs()));
    assert!(tables.relation_exists(&tables.path_refs()));

    let reopened =
        PgStore::open_existing(&tables.conn, &tables.base).expect("readonly open after migration");
    let metadata = reopened
        .store_metadata_rows()
        .expect("migrated store metadata")
        .into_iter()
        .map(|row| (row.key, row.value))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        metadata.get("last_migration").map(String::as_str),
        Some("legacy_content_rows_to_v1")
    );
    assert_eq!(
        metadata.get("migration_state").map(String::as_str),
        Some("current")
    );
}

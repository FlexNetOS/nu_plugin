//! Differential BlobStore parity: PostgreSQL backend vs the redb reference.

use std::collections::BTreeSet;

use codedb_core::store::BlobStore;
use codedb_store_pg::PgStore;
use codedb_store_redb::{CaptureBatcher, StoreInitContext, initialize_store};
use postgres::{Client, NoTls};

fn fixture_batch() -> Vec<(String, Vec<u8>)> {
    vec![
        (
            "src/main.rs".to_string(),
            b"fn main() { println!(\"hi\"); }\n".to_vec(),
        ),
        (
            "src/lib.rs".to_string(),
            b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n".to_vec(),
        ),
        (
            "README.md".to_string(),
            b"# codedb\n\nblob-store parity fixture.\n".to_vec(),
        ),
        (
            "nested/deep/notes.txt".to_string(),
            b"deep content\nline two\n".to_vec(),
        ),
        ("empty.txt".to_string(), Vec::new()),
    ]
}

fn snapshot(store: &dyn BlobStore) -> (BTreeSet<(String, String)>, BTreeSet<String>) {
    let files = store
        .list_source_files()
        .expect("list_source_files")
        .into_iter()
        .map(|row| (row.relative_path, row.sha256))
        .collect();
    let paths = store.captured_paths().expect("captured_paths");
    (files, paths)
}

#[test]
#[ignore = "legacy manual lane; mandatory CI parity is implemented in a dedicated PostgreSQL service job"]
fn pg_redb_blobstore_parity() {
    if std::env::var("CODEDB_PG_TEST").as_deref() != Ok("1") {
        eprintln!("SKIP: CODEDB_PG_TEST != 1");
        return;
    }
    let conn = std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must explicitly select the disposable test database");
    let batch = fixture_batch();

    let dir = tempfile::tempdir().expect("tempdir");
    let redb_path = dir.path().join("parity.redb");
    let ctx = StoreInitContext {
        codedb_version: "t5-parity",
        toolchain: "stable-x86_64-unknown-linux-gnu",
        rustc_version: "rustc",
        cargo_version: "cargo",
    };
    initialize_store(&redb_path, &ctx).expect("initialize redb store");
    let mut redb = CaptureBatcher::open(&redb_path).expect("open redb store");
    (&mut redb as &mut dyn BlobStore)
        .persist_batch(&batch)
        .expect("redb persist_batch");
    let (redb_files, redb_paths) = snapshot(&redb);

    let table = format!("codedb_t5_{}", std::process::id());
    assert_ne!(table, "codebase");
    assert_ne!(table, "codebase_codedb");
    let mut pg = PgStore::initialize(&conn, &table).expect("initialize pg store");
    (&mut pg as &mut dyn BlobStore)
        .persist_batch(&batch)
        .expect("pg persist_batch");
    let (pg_files, pg_paths) = snapshot(&pg);

    drop(pg);
    let mut admin = Client::connect(&conn, NoTls).expect("admin connect for cleanup");
    admin
        .batch_execute(&format!("DROP TABLE IF EXISTS {table}"))
        .expect("drop temp table");

    let expected_paths: BTreeSet<String> = batch.iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(redb_paths, expected_paths);
    assert_eq!(pg_paths, expected_paths);
    assert_eq!(pg_paths, redb_paths);
    assert_eq!(pg_files, redb_files);
    assert_eq!(pg_files.len(), batch.len());
}

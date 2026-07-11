#![cfg(feature = "pg-integration")]

//! Mandatory differential BlobStore parity: PostgreSQL vs redb.

use std::collections::BTreeSet;

use codedb_core::store::BlobStore;
use codedb_store_pg::PgStore;
use codedb_store_redb::{CaptureBatcher, StoreInitContext, initialize_store};
use postgres::{Client, NoTls};

fn fixture_batch() -> Vec<(String, Vec<u8>)> {
    vec![
        ("src/main.rs".to_string(), b"fn main() {}\n".to_vec()),
        (
            "src/lib.rs".to_string(),
            b"pub fn add(a: i32, b: i32) -> i32 { a + b }\n".to_vec(),
        ),
        ("README.md".to_string(), b"# codedb\n".to_vec()),
        (
            "nested/deep/notes.txt".to_string(),
            b"deep content\n".to_vec(),
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
fn pg_redb_blobstore_parity() {
    let conn = std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must select the disposable CI PostgreSQL service");
    let batch = fixture_batch();

    let dir = tempfile::tempdir().expect("tempdir");
    let redb_path = dir.path().join("parity.redb");
    initialize_store(
        &redb_path,
        &StoreInitContext {
            codedb_version: "mandatory-parity",
            toolchain: "ci",
            rustc_version: "rustc",
            cargo_version: "cargo",
        },
    )
    .expect("initialize redb store");
    let mut redb = CaptureBatcher::open(&redb_path).expect("open redb store");
    redb.persist_batch(&batch).expect("redb persist batch");
    let (redb_files, redb_paths) = snapshot(&redb);

    let table = format!("codedb_ci_parity_{}", std::process::id());
    assert_ne!(table, "codebase");
    assert_ne!(table, "codebase_codedb");
    let mut pg = PgStore::initialize(&conn, &table).expect("initialize pg store");
    pg.persist_batch(&batch).expect("pg persist batch");
    let (pg_files, pg_paths) = snapshot(&pg);

    drop(pg);
    let mut admin = Client::connect(&conn, NoTls).expect("admin cleanup connection");
    admin
        .batch_execute(&format!("DROP TABLE IF EXISTS {table}"))
        .expect("drop temp parity table");

    let expected_paths: BTreeSet<String> = batch.iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(redb_paths, expected_paths);
    assert_eq!(pg_paths, expected_paths);
    assert_eq!(pg_paths, redb_paths);
    assert_eq!(pg_files, redb_files);
    assert_eq!(pg_files.len(), batch.len());
}

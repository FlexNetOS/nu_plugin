//! T5 — differential BlobStore parity: PostgreSQL backend vs the redb reference.
//!
//! The two backends implement the same `codedb_core::store::BlobStore` trait. For
//! an identical input batch they must produce the identical content-addressed
//! result: the same set of `(relative_path, sha256)` from `list_source_files()`
//! and the same `captured_paths()`. This test drives both and asserts equality —
//! a regression-lock against the PostgreSQL backend drifting from the reference.
//!
//! Live + opt-in + ignored. It talks to the LIVE PostgreSQL cluster over its unix
//! socket, so it is:
//!   * `#[ignore]`d — never runs in the default `cargo test` sweep; and
//!   * additionally guarded on `CODEDB_PG_TEST=1` — a bare `-- --ignored` run
//!     without the env var skips cleanly instead of hitting the socket.
//!
//! Safety: it uses a dedicated per-process temp table `codedb_t5_<pid>` — NEVER
//! the production `codebase` nor the shared `codebase_codedb` — and DROPs it at
//! the end (before asserting, so a mismatch cannot leak it). The redb side lives
//! in a `tempfile::tempdir()` removed on drop.
//!
//! Run it:
//!   CODEDB_PG_TEST=1 cargo test -p codedb-store-pg --test blobstore_parity \
//!       -- --ignored --nocapture

use std::collections::BTreeSet;

use codedb_core::store::BlobStore;
use codedb_store_pg::{DEFAULT_CONN, PgStore};
use codedb_store_redb::{CaptureBatcher, StoreInitContext, initialize_store};
use postgres::{Client, NoTls};

/// The identical input both backends persist.
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
        // an empty file — sha256 of zero bytes must agree across backends too
        ("empty.txt".to_string(), Vec::new()),
    ]
}

/// `(relative_path, sha256)` set from `list_source_files()`, plus `captured_paths()`
/// — both read through the trait object so the exact same surface is compared.
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
#[ignore = "drives the live PostgreSQL cluster; opt in with CODEDB_PG_TEST=1 -- --ignored"]
fn pg_redb_blobstore_parity() {
    if std::env::var("CODEDB_PG_TEST").as_deref() != Ok("1") {
        eprintln!(
            "SKIP: CODEDB_PG_TEST != 1 — set it to run the live PostgreSQL/redb differential parity test"
        );
        return;
    }

    let batch = fixture_batch();

    // --- redb reference: fresh store in a temp dir (auto-removed on drop) ---
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

    // --- pg under test: dedicated per-process temp table, dropped at the end ---
    let table = format!("codedb_t5_{}", std::process::id());
    assert_ne!(table, "codebase", "must never touch the production table");
    assert_ne!(
        table, "codebase_codedb",
        "must never touch the shared codedb table"
    );
    let mut pg = PgStore::connect(DEFAULT_CONN, &table).expect("connect pg store");
    (&mut pg as &mut dyn BlobStore)
        .persist_batch(&batch)
        .expect("pg persist_batch");
    let (pg_files, pg_paths) = snapshot(&pg);

    // Drop the temp table BEFORE asserting so a parity mismatch never leaks it.
    drop(pg);
    let mut admin = Client::connect(DEFAULT_CONN, NoTls).expect("admin connect for cleanup");
    admin
        .batch_execute(&format!("DROP TABLE IF EXISTS {table}"))
        .expect("drop temp table");

    // --- differential parity ---
    let expected_paths: BTreeSet<String> = batch.iter().map(|(p, _)| p.clone()).collect();
    assert_eq!(
        redb_paths, expected_paths,
        "redb captured_paths != fixture paths"
    );
    assert_eq!(
        pg_paths, expected_paths,
        "pg captured_paths != fixture paths"
    );
    assert_eq!(
        pg_paths, redb_paths,
        "captured_paths differ between PgStore and redb"
    );
    assert_eq!(
        pg_files, redb_files,
        "list_source_files (relative_path, sha256) differ between PgStore and redb"
    );
    assert_eq!(
        pg_files.len(),
        batch.len(),
        "unexpected persisted row count"
    );

    eprintln!(
        "T5 GREEN: PgStore == redb parity on {} files (relative_path + sha256); temp table {} dropped",
        pg_files.len(),
        table
    );
}

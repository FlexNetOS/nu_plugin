#![cfg(feature = "pg-integration")]

//! ARCHBP-041 parity: raw-object metadata persisted in redb and landed in
//! PostgreSQL agree row-for-row under the same canonical content-addressed
//! ids — one identity space, two stores, zero divergence.

use codedb_store_pg::{
    RAW_OBJECTS_TABLE, connect_for_integration_tests, raw_objects_flush, raw_objects_rows,
};
use codedb_store_redb::{
    StoreInitContext, initialize_store, list_raw_objects, persist_raw_object,
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);
static PARITY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn disposable_conn() -> String {
    std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must select the explicit disposable PostgreSQL test service")
}

fn reset_pg(conn: &str) {
    let mut client = connect_for_integration_tests(conn).expect("connect for reset");
    client
        .batch_execute(&format!("DROP TABLE IF EXISTS {RAW_OBJECTS_TABLE}"))
        .expect("drop raw objects table");
}

fn temp_store() -> PathBuf {
    let unique = STORE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "codedb-raw-parity-{}-{unique}.redb",
        std::process::id()
    ));
    initialize_store(
        &path,
        &StoreInitContext {
            codedb_version: "test",
            toolchain: "test",
            rustc_version: "test",
            cargo_version: "test",
        },
    )
    .expect("store init");
    path
}

fn raw_object(bytes: &[u8], stream: &str) -> (String, String) {
    let sha = format!("{:x}", Sha256::digest(bytes));
    let id = format!("sha256:{sha}");
    let metadata = serde_json::json!({
        "stream": stream,
        "byte_length": bytes.len(),
        "frame_count": 1,
        "idempotency_key": "parity-key",
        "sha256": sha,
        "exit": {"code": 0, "signal": null, "success": true},
    })
    .to_string();
    (id, metadata)
}

#[test]
fn redb_and_postgresql_hold_identical_raw_object_metadata() {
    let _guard = PARITY_LOCK.lock().expect("parity lock");
    let conn = disposable_conn();
    reset_pg(&conn);
    let store = temp_store();

    let corpus = vec![
        (b"hello world".to_vec(), "stdout"),
        (b"warn: disk".to_vec(), "stderr"),
        (b"".to_vec(), "stdout"),
    ];
    let mut rows = Vec::new();
    for (bytes, stream) in &corpus {
        let (id, metadata) = raw_object(bytes, stream);
        persist_raw_object(&store, &id, bytes, &metadata).expect("redb persist");
        rows.push((id, metadata));
    }

    let outcome = raw_objects_flush(&conn, &rows).expect("pg flush");
    assert_eq!(outcome.inserted.len(), rows.len());

    // Replay lands nothing twice.
    let replay = raw_objects_flush(&conn, &rows).expect("pg replay");
    assert!(replay.inserted.is_empty());
    assert_eq!(replay.skipped_existing.len(), rows.len());

    // Parity: identical id sets, and metadata equal as parsed JSON values
    // (jsonb normalizes formatting, so parity is semantic, not textual).
    // Both sides sort by byte order here because PostgreSQL text collation
    // is locale-dependent and must not affect the parity verdict.
    let mut redb_rows = list_raw_objects(&store).expect("redb read");
    let mut pg_rows = raw_objects_rows(&conn).expect("pg read");
    redb_rows.sort_by(|a, b| a.0.cmp(&b.0));
    pg_rows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        redb_rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        pg_rows.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        "both stores must hold the same canonical id set"
    );
    for ((redb_id, redb_meta), (pg_id, pg_meta)) in redb_rows.iter().zip(pg_rows.iter()) {
        assert_eq!(redb_id, pg_id);
        let redb_value: serde_json::Value =
            serde_json::from_str(redb_meta).expect("redb metadata json");
        let pg_value: serde_json::Value = serde_json::from_str(pg_meta).expect("pg metadata json");
        assert_eq!(
            redb_value, pg_value,
            "metadata for {redb_id} must be semantically identical in both stores"
        );
    }

    reset_pg(&conn);
    std::fs::remove_file(&store).ok();
}

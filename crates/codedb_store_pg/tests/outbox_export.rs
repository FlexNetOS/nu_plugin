#![cfg(feature = "pg-integration")]

//! ARCHBP-002 integration tests: the versioned outbox export contract lands
//! idempotently in a real PostgreSQL service. Every test uses
//! `CODEDB_PG_CONN` explicitly and targets a disposable service, matching
//! the blobstore parity suite's convention.

use codedb_store_pg::{
    OUTBOX_EXPORT_CONTRACT_VERSION, OUTBOX_EXPORT_TABLE, OutboxExportRowInput,
    connect_for_integration_tests, outbox_export_flush, outbox_export_rows,
};

/// Both tests own the single shared contract table; serialize them.
static EXPORT_TABLE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn disposable_conn() -> String {
    std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must select the explicit disposable PostgreSQL test service")
}

fn reset_export_table(conn: &str) {
    let mut client = connect_for_integration_tests(conn).expect("connect for reset");
    client
        .batch_execute(&format!("DROP TABLE IF EXISTS {OUTBOX_EXPORT_TABLE}"))
        .expect("drop export table");
}

fn row(seq: u64) -> OutboxExportRowInput {
    OutboxExportRowInput {
        seq,
        blob_sha256: format!("{:064x}", seq),
        job_json: format!("{{\"seq\":{seq}}}"),
    }
}

#[test]
fn flush_lands_rows_in_order_and_refluses_are_skipped_not_duplicated() {
    let _guard = EXPORT_TABLE_LOCK.lock().expect("export table lock");
    let conn = disposable_conn();
    reset_export_table(&conn);

    let rows = vec![row(1), row(2), row(3)];
    let first = outbox_export_flush(&conn, &rows).expect("first flush");
    assert_eq!(first.inserted, vec![1, 2, 3]);
    assert_eq!(first.skipped_existing, Vec::<u64>::new());

    // Replay after a simulated crash-before-acknowledge: identical rows.
    let second = outbox_export_flush(&conn, &rows).expect("second flush");
    assert_eq!(second.inserted, Vec::<u64>::new());
    assert_eq!(second.skipped_existing, vec![1, 2, 3]);

    // A mixed batch (old + new) inserts only the new sequence.
    let third = outbox_export_flush(&conn, &[row(3), row(4)]).expect("third flush");
    assert_eq!(third.inserted, vec![4]);
    assert_eq!(third.skipped_existing, vec![3]);

    let landed = outbox_export_rows(&conn).expect("read back");
    assert_eq!(
        landed.iter().map(|(seq, ..)| *seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "read-back must be ordered by sequence with exactly one row each"
    );
    for (seq, contract, blob_sha256, job_json) in &landed {
        assert_eq!(contract, OUTBOX_EXPORT_CONTRACT_VERSION);
        assert_eq!(blob_sha256, &format!("{:064x}", seq));
        assert!(job_json.contains(&format!("\"seq\":{seq}")) || job_json.contains(&format!("\"seq\": {seq}")));
    }
    reset_export_table(&conn);
}

#[test]
fn empty_flush_is_a_no_op_and_read_back_of_missing_table_is_empty() {
    let _guard = EXPORT_TABLE_LOCK.lock().expect("export table lock");
    let conn = disposable_conn();
    reset_export_table(&conn);

    let outcome = outbox_export_flush(&conn, &[]).expect("empty flush");
    assert!(outcome.inserted.is_empty());
    assert!(outcome.skipped_existing.is_empty());

    // Reading an absent contract table reports no rows rather than erroring:
    // observability must not depend on a prior flush.
    reset_export_table(&conn);
    let landed = outbox_export_rows(&conn).expect("read back with no table");
    assert!(landed.is_empty());
}

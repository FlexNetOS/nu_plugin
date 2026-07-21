//! ARCHBP-002 red tests: redb is the restartable local buffer and explicit
//! application outbox. Entries are append-only with contiguous monotonic
//! sequences; the acknowledge cursor is monotonic, bounded by the enqueued
//! head, and survives process crashes (simulated by dropping every handle
//! and reopening the store file).

use codedb_store_redb::{
    StoreInitContext, initialize_store, outbox_acknowledge, outbox_enqueue, outbox_pending,
    outbox_status, persist_source_blob, source_blob_exists,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_store_path(label: &str) -> PathBuf {
    let unique = STORE_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "codedb-outbox-{label}-{}-{unique}.redb",
        std::process::id()
    ))
}

fn init_store(label: &str) -> PathBuf {
    let path = temp_store_path(label);
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

fn job(i: u64) -> String {
    format!("{{\"job\":{i}}}")
}

#[test]
fn enqueue_assigns_contiguous_monotonic_sequences_from_one() {
    let path = init_store("seq");
    for expected in 1..=5u64 {
        let seq = outbox_enqueue(&path, &job(expected)).expect("enqueue");
        assert_eq!(seq, expected, "sequences must be contiguous from 1");
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn pending_returns_entries_after_cursor_in_order_and_respects_limit() {
    let path = init_store("pending");
    for i in 1..=4u64 {
        outbox_enqueue(&path, &job(i)).expect("enqueue");
    }
    let all = outbox_pending(&path, 100).expect("pending");
    assert_eq!(
        all.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(all[0].entry_json, job(1));

    outbox_acknowledge(&path, 2).expect("ack");
    let rest = outbox_pending(&path, 100).expect("pending after ack");
    assert_eq!(rest.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3, 4]);

    let limited = outbox_pending(&path, 1).expect("pending limited");
    assert_eq!(limited.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3]);
    std::fs::remove_file(&path).ok();
}

#[test]
fn empty_store_reports_zero_status_and_no_pending() {
    let path = init_store("empty");
    let status = outbox_status(&path).expect("status");
    assert_eq!((status.enqueued, status.acknowledged, status.pending), (0, 0, 0));
    assert!(outbox_pending(&path, 10).expect("pending").is_empty());
    std::fs::remove_file(&path).ok();
}

#[test]
fn acknowledge_cursor_is_monotonic_and_bounded_by_enqueued_head() {
    let path = init_store("ack");
    for i in 1..=3u64 {
        outbox_enqueue(&path, &job(i)).expect("enqueue");
    }
    assert_eq!(outbox_acknowledge(&path, 2).expect("ack 2"), 2);
    // Re-acknowledging the same cursor is idempotent (replay-safe).
    assert_eq!(outbox_acknowledge(&path, 2).expect("ack 2 again"), 2);
    // Regression fails closed.
    let regress = outbox_acknowledge(&path, 1);
    assert!(regress.is_err(), "cursor regression must fail closed");
    // Acknowledging beyond the enqueued head fails closed.
    let beyond = outbox_acknowledge(&path, 99);
    assert!(beyond.is_err(), "ack beyond enqueued head must fail closed");
    // State is unchanged after both rejected attempts.
    let status = outbox_status(&path).expect("status");
    assert_eq!((status.enqueued, status.acknowledged, status.pending), (3, 2, 1));
    std::fs::remove_file(&path).ok();
}

#[test]
fn outbox_survives_crash_and_reopen_without_losing_entries_or_cursor() {
    let path = init_store("crash");
    for i in 1..=3u64 {
        outbox_enqueue(&path, &job(i)).expect("enqueue");
    }
    outbox_acknowledge(&path, 1).expect("ack 1");
    // Simulated crash: every handle in this process is already dropped after
    // each call (the API opens the store per operation). Reopen and verify
    // the exact pre-crash state.
    let status = outbox_status(&path).expect("status after reopen");
    assert_eq!((status.enqueued, status.acknowledged, status.pending), (3, 1, 2));
    let pending = outbox_pending(&path, 10).expect("pending after reopen");
    assert_eq!(pending.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(pending[0].entry_json, job(2));
    std::fs::remove_file(&path).ok();
}

#[test]
fn entries_are_append_only_identical_content_gets_a_new_sequence() {
    let path = init_store("append");
    let first = outbox_enqueue(&path, &job(7)).expect("enqueue");
    let second = outbox_enqueue(&path, &job(7)).expect("enqueue same content");
    assert_eq!((first, second), (1, 2), "identical content must append, not overwrite");
    let all = outbox_pending(&path, 10).expect("pending");
    assert_eq!(all.len(), 2);
    std::fs::remove_file(&path).ok();
}

#[test]
fn source_blob_existence_is_checkable_for_enqueue_linkage() {
    let path = init_store("blob");
    let row = persist_source_blob(&path, "a.rs", b"fn main() {}").expect("persist");
    assert!(source_blob_exists(&path, &row.sha256).expect("exists"));
    assert!(
        !source_blob_exists(&path, &"0".repeat(64)).expect("missing lookup"),
        "unknown blob must report absent, not error"
    );
    std::fs::remove_file(&path).ok();
}

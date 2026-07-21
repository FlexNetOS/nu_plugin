//! ARCHBP-039 red tests: one supervised service holds the only writable
//! redb handle, serves versioned authenticated Unix-domain mutation/query
//! commands, and atomically publishes checksummed read-only mmap projection
//! generations plus ordered commit notifications — with single-opener
//! denial, monotonic local_seq, crash replay, spool preservation, and
//! corruption fallback.

use flexnetos_redb_owner::{
    OwnerClient, OwnerError, OwnerService, PROTOCOL_VERSION, ProjectionReader, read_events,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(label: &str) -> PathBuf {
    let unique = ROOT_COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "redb-owner-{label}-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("create test root");
    root
}

fn cleanup(root: &PathBuf) {
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn second_opener_is_denied_while_the_owner_holds_the_database() {
    let root = temp_root("single-opener");
    let owner = OwnerService::start(&root).expect("first owner starts");
    let denied = OwnerService::start(&root);
    assert!(
        matches!(denied, Err(OwnerError::AlreadyOwned(_))),
        "a second owner over the same root must be denied: {denied:?}"
    );
    drop(owner);
    // After a clean shutdown the root can be owned again.
    let reopened = OwnerService::start(&root).expect("reopen after shutdown");
    drop(reopened);
    cleanup(&root);
}

#[test]
fn mutations_require_the_exact_protocol_version_and_auth_token() {
    let root = temp_root("auth");
    let _owner = OwnerService::start(&root).expect("owner starts");
    let mut client = OwnerClient::connect(&root).expect("client connects");

    // Happy path.
    let seq = client.put("k1", "v1").expect("authenticated put");
    assert_eq!(seq, 1);
    assert_eq!(client.get("k1").expect("get"), Some("v1".to_string()));

    // Wrong token fails closed.
    let mut bad_token = OwnerClient::connect(&root).expect("client connects");
    bad_token.override_token("not-the-token");
    assert!(matches!(
        bad_token.put("k2", "v2"),
        Err(OwnerError::Rejected(_))
    ));

    // Wrong protocol version fails closed.
    let mut bad_version = OwnerClient::connect(&root).expect("client connects");
    bad_version.override_protocol_version("flexnetos.redb-owner.v999");
    assert!(matches!(
        bad_version.put("k3", "v3"),
        Err(OwnerError::Rejected(_))
    ));

    // Neither rejected request consumed a sequence.
    let mut client2 = OwnerClient::connect(&root).expect("client connects");
    assert_eq!(client2.put("k4", "v4").expect("next put"), 2);
    cleanup(&root);
}

#[test]
fn local_seq_is_monotonic_and_projections_flip_atomically_with_checksums() {
    let root = temp_root("projection");
    let _owner = OwnerService::start(&root).expect("owner starts");
    let mut client = OwnerClient::connect(&root).expect("client connects");

    for i in 1..=5u64 {
        let seq = client.put(&format!("key{i}"), &format!("value{i}")).expect("put");
        assert_eq!(seq, i, "local_seq must be contiguous and monotonic");
        // Every commit publishes a readable projection at exactly that seq.
        let projection = ProjectionReader::read(&root).expect("projection readable");
        assert_eq!(projection.local_seq, i);
        assert_eq!(
            projection.entries.get(&format!("key{i}")),
            Some(&format!("value{i}"))
        );
    }
    // The projection is served from a checksummed mmap slot.
    let projection = ProjectionReader::read(&root).expect("read");
    assert_eq!(projection.entries.len(), 5);
    assert!(!projection.checksum.is_empty());
    cleanup(&root);
}

#[test]
fn commit_notifications_are_ordered_and_gap_detectable_after_reconnect() {
    let root = temp_root("events");
    let _owner = OwnerService::start(&root).expect("owner starts");
    let mut client = OwnerClient::connect(&root).expect("client connects");
    for i in 1..=4u64 {
        client.put(&format!("k{i}"), "v").expect("put");
    }
    let events = read_events(&root, 0).expect("read events from origin");
    assert_eq!(
        events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4],
        "notifications must be ordered and complete"
    );
    // A subscriber that saw seq 2 reconnects and reads only the gap.
    let tail = read_events(&root, 2).expect("read events after 2");
    assert_eq!(tail.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![3, 4]);
    // Every event names the projection generation it published.
    for event in &events {
        assert!(!event.checksum.is_empty());
        assert!(event.slot == "a" || event.slot == "b");
    }
    cleanup(&root);
}

#[test]
fn crash_between_commit_and_publish_replays_on_restart() {
    let root = temp_root("crash-replay");
    {
        let owner = OwnerService::start(&root).expect("owner starts");
        let mut client = OwnerClient::connect(&root).expect("client connects");
        client.put("stable", "1").expect("put");
        // Simulate a crash after the redb commit but before the projection
        // flip: the injected failpoint makes publication die once.
        owner.inject_publish_crash();
        let result = client.put("dark", "2");
        assert!(result.is_err(), "the crashed publication surfaces an error");
        // The projection still shows seq 1 — the flip never happened.
        let projection = ProjectionReader::read(&root).expect("read");
        assert_eq!(projection.local_seq, 1);
        drop(owner);
    }
    // Restart: the owner detects the projection lagging the database and
    // replays the missing publication before serving.
    let _owner = OwnerService::start(&root).expect("owner restarts");
    let projection = ProjectionReader::read(&root).expect("read after replay");
    assert_eq!(projection.local_seq, 2, "replay must republish the lost commit");
    assert_eq!(projection.entries.get("dark"), Some(&"2".to_string()));
    // The spool was preserved and extended, never truncated.
    let events = read_events(&root, 0).expect("events");
    let seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![1, 2], "spool preserves history across restarts");
    cleanup(&root);
}

#[test]
fn state_and_sequence_survive_restart_exactly() {
    let root = temp_root("restart");
    {
        let _owner = OwnerService::start(&root).expect("owner starts");
        let mut client = OwnerClient::connect(&root).expect("client connects");
        client.put("a", "1").expect("put");
        client.put("b", "2").expect("put");
    }
    let _owner = OwnerService::start(&root).expect("owner restarts");
    let mut client = OwnerClient::connect(&root).expect("client reconnects");
    assert_eq!(client.get("a").expect("get"), Some("1".to_string()));
    assert_eq!(client.put("c", "3").expect("put"), 3, "sequence continues, never resets");
    cleanup(&root);
}

#[test]
fn corrupted_active_slot_falls_back_to_the_previous_generation() {
    let root = temp_root("corruption");
    let _owner = OwnerService::start(&root).expect("owner starts");
    let mut client = OwnerClient::connect(&root).expect("client connects");
    client.put("gen1", "old").expect("put");
    client.put("gen2", "new").expect("put");

    // Corrupt the active slot's bytes on disk.
    let projection = ProjectionReader::read(&root).expect("read");
    assert_eq!(projection.local_seq, 2);
    let active_slot_path = root.join(format!("projection.{}.slot", projection.slot));
    let mut bytes = std::fs::read(&active_slot_path).expect("read slot");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&active_slot_path, &bytes).expect("corrupt slot");

    // The reader detects the checksum mismatch and falls back to the
    // previous witnessed generation instead of serving corrupt bytes.
    let fallback = ProjectionReader::read(&root).expect("fallback read");
    assert_eq!(fallback.local_seq, 1, "fallback serves the previous generation");
    assert!(fallback.degraded, "fallback must be visibly degraded, never silent");
    assert_eq!(fallback.entries.get("gen1"), Some(&"old".to_string()));
    cleanup(&root);
}

#[test]
fn commit_to_read_latency_is_benchmarked_with_raw_samples() {
    let root = temp_root("latency");
    let _owner = OwnerService::start(&root).expect("owner starts");
    let mut client = OwnerClient::connect(&root).expect("client connects");
    let mut samples_us = Vec::new();
    for i in 0..30u64 {
        let started = std::time::Instant::now();
        let seq = client.put(&format!("bench{i}"), "x").expect("put");
        let projection = ProjectionReader::read(&root).expect("read");
        assert_eq!(projection.local_seq, seq);
        samples_us.push(started.elapsed().as_micros() as u64);
    }
    assert_eq!(samples_us.len(), 30);
    // No sub-millisecond claim is assumed: the samples are recorded raw and
    // only sanity-bounded (a commit+read round trip under 5 seconds).
    assert!(samples_us.iter().all(|&us| us < 5_000_000));
    let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../logs/redb-owner-latency");
    std::fs::create_dir_all(&out_dir).expect("create latency log dir");
    std::fs::write(
        out_dir.join("samples.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": "flexnetos.redb-owner.latency-samples.v0",
            "unit": "microseconds",
            "protocol_version": PROTOCOL_VERSION,
            "samples": samples_us,
        }))
        .expect("serialize samples"),
    )
    .expect("write samples");
    cleanup(&root);
}

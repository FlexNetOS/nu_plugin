//! ARCHBP-002: the redb outbox and PostgreSQL/RuVector synchronization seam.
//!
//! redb is the restartable local buffer and explicit application outbox for
//! ordered embedding work. Synchronization drains the outbox in sequence
//! order through an [`ExportSink`], flushing idempotently into the versioned
//! PostgreSQL export contract that envctl (the sole authoritative committer)
//! later consumes. Crash at any boundary — enqueue, embed, flush,
//! acknowledge — replays losslessly: entries are append-only, the export is
//! keyed by sequence, and the acknowledge cursor only advances after a
//! successful flush.

use serde::{Deserialize, Serialize};

/// Versioned embedding-job contract carried by every outbox entry.
pub const EMBEDDING_JOB_SCHEMA_VERSION: &str = "codedb.embedding-job.v0";
/// Versioned PostgreSQL export contract consumed by envctl.
pub const EXPORT_CONTRACT_VERSION: &str = "codedb.outbox-export.v0";
/// Versioned receipt emitted by every synchronization run.
pub const SYNC_RECEIPT_SCHEMA_VERSION: &str = "codedb.outbox-sync-receipt.v0";
/// Versioned receipt emitted by every enqueue.
pub const ENQUEUE_RECEIPT_SCHEMA_VERSION: &str = "codedb.outbox-enqueue-receipt.v0";
/// Upper bound on entries drained per flush batch.
pub const MAX_SYNC_BATCH: usize = 512;

#[derive(Debug)]
pub struct OutboxError(String);

impl OutboxError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for OutboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for OutboxError {}

/// One unit of ordered embedding work. The job records model and version
/// provenance up front; ARCHBP-003 attaches the live embedding computation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingJob {
    pub schema_version: String,
    /// Content address of the exact ingested bytes this job embeds.
    pub blob_sha256: String,
    /// Store-relative path of the source file (observability only).
    pub relative_path: String,
    /// Embedding model identity (provenance).
    pub model_name: String,
    /// Embedding model revision/digest (provenance).
    pub model_revision: String,
    /// Digest of the exact payload the model will consume.
    pub payload_digest: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnqueueReceipt {
    pub schema_version: String,
    pub seq: u64,
    pub blob_sha256: String,
    pub relative_path: String,
}

/// One row of the versioned PostgreSQL export contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRow {
    pub seq: u64,
    pub blob_sha256: String,
    pub job_json: String,
}

/// Outcome of one idempotent flush.
#[derive(Debug, Clone, Default)]
pub struct FlushOutcome {
    pub inserted: Vec<u64>,
    pub skipped_existing: Vec<u64>,
}

/// Where export rows land. The PostgreSQL sink writes the versioned export
/// table; tests inject in-memory sinks with crash failpoints.
pub trait ExportSink {
    fn flush(&mut self, rows: &[ExportRow]) -> Result<FlushOutcome, OutboxError>;
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReceipt {
    pub schema_version: String,
    pub contract_version: String,
    pub synced: Vec<u64>,
    pub skipped_existing: Vec<u64>,
    pub acknowledged_up_to: u64,
    pub pending_remaining: u64,
}

fn require_hex_digest(value: &str, field: &str) -> Result<(), OutboxError> {
    if value.len() != 64 || !value.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(OutboxError::new(format!(
            "{field} must be 64 lowercase hex characters"
        )));
    }
    Ok(())
}

/// Fail-closed validation of one embedding job JSON.
pub fn validate_job(json: &str) -> Result<EmbeddingJob, OutboxError> {
    let job: EmbeddingJob = serde_json::from_str(json)
        .map_err(|e| OutboxError::new(format!("embedding job does not parse: {e}")))?;
    if job.schema_version != EMBEDDING_JOB_SCHEMA_VERSION {
        return Err(OutboxError::new(format!(
            "unsupported embedding job schema version: {} (expected {})",
            job.schema_version, EMBEDDING_JOB_SCHEMA_VERSION
        )));
    }
    require_hex_digest(&job.blob_sha256, "blob_sha256")?;
    require_hex_digest(&job.payload_digest, "payload_digest")?;
    if job.model_name.trim().is_empty() {
        return Err(OutboxError::new("model_name must not be empty"));
    }
    if job.model_revision.trim().is_empty() {
        return Err(OutboxError::new("model_revision must not be empty"));
    }
    if job.relative_path.is_empty() {
        return Err(OutboxError::new("relative_path must not be empty"));
    }
    if job.relative_path.starts_with('/') || job.relative_path.contains('\\') {
        return Err(OutboxError::new(
            "relative_path must be a clean relative path",
        ));
    }
    if job
        .relative_path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(OutboxError::new(
            "relative_path must not contain empty, '.', or '..' components",
        ));
    }
    Ok(job)
}

/// Validate, verify blob linkage against the store, and append to the outbox.
pub fn enqueue_job(
    store_path: &std::path::Path,
    json: &str,
) -> Result<EnqueueReceipt, OutboxError> {
    let job = validate_job(json)?;
    let present = codedb_store_redb::source_blob_exists(store_path, &job.blob_sha256)
        .map_err(|e| OutboxError::new(e.to_string()))?;
    if !present {
        return Err(OutboxError::new(format!(
            "embedding job references blob sha256:{} which is not in the store",
            job.blob_sha256
        )));
    }
    // Canonical serialization: the outbox stores the parsed contract, not the
    // caller's raw bytes, so replayed entries are byte-stable.
    let canonical = serde_json::to_string(&job)
        .map_err(|e| OutboxError::new(format!("serializing job: {e}")))?;
    let seq = codedb_store_redb::outbox_enqueue(store_path, &canonical)
        .map_err(|e| OutboxError::new(e.to_string()))?;
    Ok(EnqueueReceipt {
        schema_version: ENQUEUE_RECEIPT_SCHEMA_VERSION.to_string(),
        seq,
        blob_sha256: job.blob_sha256,
        relative_path: job.relative_path,
    })
}

/// Drain pending entries in order through the sink, acknowledging after each
/// successful flush. Restart-safe at every boundary: entries are append-only,
/// export rows are keyed by sequence, and the cursor only advances after the
/// sink accepted the whole batch. A corrupt entry aborts the run before its
/// batch is flushed — the valid prefix stays acknowledged, nothing is
/// silently dropped.
pub fn run_sync(
    store_path: &std::path::Path,
    sink: &mut dyn ExportSink,
    max_batch: usize,
) -> Result<SyncReceipt, OutboxError> {
    let mut synced = Vec::new();
    let mut skipped_existing = Vec::new();
    loop {
        let pending = codedb_store_redb::outbox_pending(store_path, max_batch)
            .map_err(|e| OutboxError::new(e.to_string()))?;
        if pending.is_empty() {
            break;
        }
        // Embed boundary: rebuild every export row deterministically from the
        // stored contract. A corrupt entry fails the run closed, but the
        // valid prefix before it still lands and acknowledges — nothing is
        // silently dropped and nothing valid is held hostage.
        let mut rows = Vec::with_capacity(pending.len());
        let mut corrupt: Option<OutboxError> = None;
        for entry in &pending {
            match validate_job(&entry.entry_json) {
                Ok(job) => rows.push(ExportRow {
                    seq: entry.seq,
                    blob_sha256: job.blob_sha256,
                    job_json: entry.entry_json.clone(),
                }),
                Err(e) => {
                    corrupt = Some(OutboxError::new(format!(
                        "outbox entry seq {} violates the embedding-job contract: {e}",
                        entry.seq
                    )));
                    break;
                }
            }
        }
        if let Some(last_seq) = rows.last().map(|row| row.seq) {
            // Flush boundary: idempotent landing keyed by sequence.
            let outcome = sink.flush(&rows)?;
            synced.extend(outcome.inserted);
            skipped_existing.extend(outcome.skipped_existing);
            // Acknowledge boundary: only after the whole batch landed.
            codedb_store_redb::outbox_acknowledge(store_path, last_seq)
                .map_err(|e| OutboxError::new(e.to_string()))?;
        }
        if let Some(err) = corrupt {
            return Err(err);
        }
    }
    let status = codedb_store_redb::outbox_status(store_path)
        .map_err(|e| OutboxError::new(e.to_string()))?;
    Ok(SyncReceipt {
        schema_version: SYNC_RECEIPT_SCHEMA_VERSION.to_string(),
        contract_version: EXPORT_CONTRACT_VERSION.to_string(),
        synced,
        skipped_existing,
        acknowledged_up_to: status.acknowledged,
        pending_remaining: status.pending,
    })
}

/// PostgreSQL sink over the versioned export contract table. The DSN is
/// held privately and never surfaces in errors or receipts.
pub struct PgExportSink {
    conn: String,
}

impl PgExportSink {
    pub fn new(conn: &str) -> Self {
        Self {
            conn: conn.to_string(),
        }
    }
}

impl ExportSink for PgExportSink {
    fn flush(&mut self, rows: &[ExportRow]) -> Result<FlushOutcome, OutboxError> {
        let input: Vec<codedb_store_pg::OutboxExportRowInput> = rows
            .iter()
            .map(|row| codedb_store_pg::OutboxExportRowInput {
                seq: row.seq,
                blob_sha256: row.blob_sha256.clone(),
                job_json: row.job_json.clone(),
            })
            .collect();
        let outcome = codedb_store_pg::outbox_export_flush(&self.conn, &input)
            .map_err(|e| OutboxError::new(e.to_string()))?;
        Ok(FlushOutcome {
            inserted: outcome.inserted,
            skipped_existing: outcome.skipped_existing,
        })
    }
}

/// Observable outbox status row for the CLI.
#[derive(Debug, Clone, Serialize)]
pub struct OutboxStatus {
    pub enqueued: u64,
    pub acknowledged: u64,
    pub pending: u64,
}

pub fn outbox_status(store_path: &std::path::Path) -> Result<OutboxStatus, OutboxError> {
    let status = codedb_store_redb::outbox_status(store_path)
        .map_err(|e| OutboxError::new(e.to_string()))?;
    Ok(OutboxStatus {
        enqueued: status.enqueued,
        acknowledged: status.acknowledged,
        pending: status.pending,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codedb_store_redb::{StoreInitContext, initialize_store, persist_source_blob};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_store(label: &str) -> PathBuf {
        let unique = STORE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "codedb-outbox-seam-{label}-{}-{unique}.redb",
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

    fn seeded_job(store: &PathBuf, path_label: &str) -> String {
        let bytes = format!("source bytes for {path_label}");
        let row = persist_source_blob(store, path_label, bytes.as_bytes()).expect("persist blob");
        serde_json::to_string(&EmbeddingJob {
            schema_version: EMBEDDING_JOB_SCHEMA_VERSION.to_string(),
            blob_sha256: row.sha256,
            relative_path: path_label.to_string(),
            model_name: "all-MiniLM-L6-v2".to_string(),
            model_revision: "sha256:model-revision-digest".to_string(),
            payload_digest: "b".repeat(64),
        })
        .expect("serialize job")
    }

    /// In-memory sink recording flushed rows, with idempotent seq keying and
    /// an optional injected crash after N accepted rows.
    struct MemorySink {
        rows: std::collections::BTreeMap<u64, ExportRow>,
        fail_after_total_rows: Option<usize>,
        accepted: usize,
    }

    impl MemorySink {
        fn new() -> Self {
            Self {
                rows: std::collections::BTreeMap::new(),
                fail_after_total_rows: None,
                accepted: 0,
            }
        }

        fn failing_after(rows: usize) -> Self {
            Self {
                fail_after_total_rows: Some(rows),
                ..Self::new()
            }
        }
    }

    impl ExportSink for MemorySink {
        fn flush(&mut self, rows: &[ExportRow]) -> Result<FlushOutcome, OutboxError> {
            let mut outcome = FlushOutcome::default();
            for row in rows {
                if let Some(limit) = self.fail_after_total_rows {
                    if self.accepted >= limit {
                        // Crash boundary: rows accepted so far are durable in
                        // the sink; this flush attempt dies mid-batch.
                        return Err(OutboxError::new("injected sink crash"));
                    }
                }
                if self.rows.contains_key(&row.seq) {
                    outcome.skipped_existing.push(row.seq);
                } else {
                    self.rows.insert(row.seq, row.clone());
                    outcome.inserted.push(row.seq);
                }
                self.accepted += 1;
            }
            Ok(outcome)
        }
    }

    // -- job contract validation (fail-closed) ---------------------------

    #[test]
    fn validate_accepts_a_complete_well_formed_job() {
        let json = serde_json::json!({
            "schema_version": EMBEDDING_JOB_SCHEMA_VERSION,
            "blob_sha256": "a".repeat(64),
            "relative_path": "src/lib.rs",
            "model_name": "all-MiniLM-L6-v2",
            "model_revision": "sha256:abc",
            "payload_digest": "b".repeat(64),
        })
        .to_string();
        let job = validate_job(&json).expect("valid job");
        assert_eq!(job.model_name, "all-MiniLM-L6-v2");
    }

    #[test]
    fn validate_rejects_wrong_schema_version_unknown_fields_and_bad_digests() {
        let base = serde_json::json!({
            "schema_version": EMBEDDING_JOB_SCHEMA_VERSION,
            "blob_sha256": "a".repeat(64),
            "relative_path": "src/lib.rs",
            "model_name": "m",
            "model_revision": "r",
            "payload_digest": "b".repeat(64),
        });
        let mutate = |key: &str, value: serde_json::Value| {
            let mut v = base.clone();
            v[key] = value;
            v.to_string()
        };
        assert!(validate_job(&mutate("schema_version", "codedb.embedding-job.v9".into())).is_err());
        assert!(validate_job(&mutate("blob_sha256", "not-hex".into())).is_err());
        assert!(validate_job(&mutate("blob_sha256", "A".repeat(64).into())).is_err());
        assert!(validate_job(&mutate("payload_digest", "b".repeat(63).into())).is_err());
        assert!(validate_job(&mutate("model_name", "".into())).is_err());
        assert!(validate_job(&mutate("model_revision", "".into())).is_err());
        assert!(validate_job(&mutate("relative_path", "/abs/path".into())).is_err());
        assert!(validate_job(&mutate("relative_path", "../escape".into())).is_err());
        assert!(validate_job(&mutate("relative_path", "".into())).is_err());
        let mut unknown = base.clone();
        unknown["surprise"] = serde_json::json!(1);
        assert!(validate_job(&unknown.to_string()).is_err());
    }

    // -- enqueue boundary -------------------------------------------------

    #[test]
    fn enqueue_links_jobs_to_existing_blobs_and_fails_closed_on_unknown_blob() {
        let store = temp_store("enqueue");
        let json = seeded_job(&store, "src/known.rs");
        let receipt = enqueue_job(&store, &json).expect("enqueue known blob");
        assert_eq!(receipt.seq, 1);
        assert_eq!(receipt.schema_version, ENQUEUE_RECEIPT_SCHEMA_VERSION);

        let orphan = serde_json::json!({
            "schema_version": EMBEDDING_JOB_SCHEMA_VERSION,
            "blob_sha256": "f".repeat(64),
            "relative_path": "src/orphan.rs",
            "model_name": "m",
            "model_revision": "r",
            "payload_digest": "b".repeat(64),
        })
        .to_string();
        assert!(
            enqueue_job(&store, &orphan).is_err(),
            "jobs must reference bytes that are actually in the store"
        );
        // The failed enqueue must not have consumed a sequence.
        let second = enqueue_job(&store, &seeded_job(&store, "src/next.rs")).expect("enqueue");
        assert_eq!(second.seq, 2, "rejected enqueue must not burn a sequence");
        std::fs::remove_file(&store).ok();
    }

    // -- ordered, idempotent, lossless sync -------------------------------

    #[test]
    fn sync_drains_in_order_acknowledges_and_is_idempotent_on_rerun() {
        let store = temp_store("sync");
        for label in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            enqueue_job(&store, &seeded_job(&store, label)).expect("enqueue");
        }
        let mut sink = MemorySink::new();
        let receipt = run_sync(&store, &mut sink, MAX_SYNC_BATCH).expect("sync");
        assert_eq!(receipt.synced, vec![1, 2, 3]);
        assert_eq!(receipt.skipped_existing, Vec::<u64>::new());
        assert_eq!(receipt.acknowledged_up_to, 3);
        assert_eq!(receipt.pending_remaining, 0);
        assert_eq!(receipt.contract_version, EXPORT_CONTRACT_VERSION);
        assert_eq!(
            sink.rows.keys().copied().collect::<Vec<_>>(),
            vec![1, 2, 3],
            "export rows must be keyed by sequence in order"
        );

        // Re-running against an already-synced outbox does nothing new.
        let rerun = run_sync(&store, &mut sink, MAX_SYNC_BATCH).expect("rerun");
        assert_eq!(rerun.synced, Vec::<u64>::new());
        assert_eq!(rerun.acknowledged_up_to, 3);
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn crash_after_flush_before_acknowledge_replays_without_duplication() {
        let store = temp_store("crash-ack");
        for label in ["src/a.rs", "src/b.rs"] {
            enqueue_job(&store, &seeded_job(&store, label)).expect("enqueue");
        }
        // Flush both rows into the sink, then "crash" before acknowledging:
        // simulate by flushing manually without touching the cursor.
        let mut sink = MemorySink::new();
        let pending = codedb_store_redb::outbox_pending(&store, 100).expect("pending");
        let rows: Vec<ExportRow> = pending
            .iter()
            .map(|e| {
                let job = validate_job(&e.entry_json).expect("entry validates");
                ExportRow {
                    seq: e.seq,
                    blob_sha256: job.blob_sha256.clone(),
                    job_json: e.entry_json.clone(),
                }
            })
            .collect();
        sink.flush(&rows).expect("manual flush");

        // Replay after the crash: the sink already has both rows; sync must
        // skip them as existing, still acknowledge, and lose nothing.
        let receipt = run_sync(&store, &mut sink, MAX_SYNC_BATCH).expect("replay");
        assert_eq!(receipt.synced, Vec::<u64>::new());
        assert_eq!(receipt.skipped_existing, vec![1, 2]);
        assert_eq!(receipt.acknowledged_up_to, 2);
        assert_eq!(sink.rows.len(), 2, "exactly one export row per sequence");
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn crash_mid_flush_leaves_cursor_behind_and_replay_completes_exactly_once() {
        let store = temp_store("crash-mid");
        for label in ["src/a.rs", "src/b.rs", "src/c.rs"] {
            enqueue_job(&store, &seeded_job(&store, label)).expect("enqueue");
        }
        // The sink crashes after durably accepting one row.
        let mut crashing = MemorySink::failing_after(1);
        let err = run_sync(&store, &mut crashing, MAX_SYNC_BATCH);
        assert!(err.is_err(), "mid-flush crash surfaces as an error");
        // Nothing was acknowledged: the cursor still points before seq 1.
        let status = outbox_status(&store).expect("status");
        assert_eq!(status.acknowledged, 0);
        assert_eq!(status.pending, 3);

        // Replay with a healthy sink that kept the one durable row.
        let mut healthy = MemorySink::new();
        healthy.rows = crashing.rows.clone();
        let receipt = run_sync(&store, &mut healthy, MAX_SYNC_BATCH).expect("replay");
        let mut all: Vec<u64> = receipt.synced.clone();
        all.extend(&receipt.skipped_existing);
        all.sort_unstable();
        assert_eq!(all, vec![1, 2, 3], "every sequence lands exactly once");
        assert_eq!(receipt.skipped_existing, vec![1], "the durable row is skipped, not duplicated");
        assert_eq!(receipt.acknowledged_up_to, 3);
        assert_eq!(healthy.rows.len(), 3);
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn sync_fails_closed_on_a_corrupt_outbox_entry_without_acknowledging_it() {
        let store = temp_store("corrupt");
        enqueue_job(&store, &seeded_job(&store, "src/good.rs")).expect("enqueue good");
        codedb_store_redb::outbox_enqueue(&store, "{\"not\":\"a job\"}").expect("raw corrupt entry");
        let mut sink = MemorySink::new();
        let result = run_sync(&store, &mut sink, MAX_SYNC_BATCH);
        assert!(result.is_err(), "corrupt entries must abort, never be dropped silently");
        let status = outbox_status(&store).expect("status");
        assert_eq!(
            status.acknowledged, 1,
            "the valid prefix is synced and acknowledged; the corrupt entry stays pending"
        );
        assert_eq!(status.pending, 1);
        assert_eq!(sink.rows.len(), 1);
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn sync_respects_batch_limit_across_multiple_flushes() {
        let store = temp_store("batches");
        for i in 0..5 {
            enqueue_job(&store, &seeded_job(&store, &format!("src/f{i}.rs"))).expect("enqueue");
        }
        let mut sink = MemorySink::new();
        let receipt = run_sync(&store, &mut sink, 2).expect("sync batched");
        assert_eq!(receipt.synced, vec![1, 2, 3, 4, 5]);
        assert_eq!(receipt.acknowledged_up_to, 5);
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn model_and_version_provenance_survive_into_the_export_rows() {
        let store = temp_store("provenance");
        enqueue_job(&store, &seeded_job(&store, "src/p.rs")).expect("enqueue");
        let mut sink = MemorySink::new();
        run_sync(&store, &mut sink, MAX_SYNC_BATCH).expect("sync");
        let row = sink.rows.get(&1).expect("exported row");
        let job: EmbeddingJob = serde_json::from_str(&row.job_json).expect("job json");
        assert_eq!(job.model_name, "all-MiniLM-L6-v2");
        assert_eq!(job.model_revision, "sha256:model-revision-digest");
        assert_eq!(job.blob_sha256, row.blob_sha256);
        std::fs::remove_file(&store).ok();
    }
}

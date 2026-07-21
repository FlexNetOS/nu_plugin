//! ARCHBP-041: typed ingest of rtk_nu raw envelopes with canonical
//! raw-object linkage.
//!
//! rtk_nu (ARCHBP-040) tees ordered byte-exact stdout/stderr frames into
//! versioned envelopes (JSON aggregate, JSONL event stream, or native Nu
//! records over the plugin protocol) with `canonical_raw_object_id: null`.
//! This module validates those envelopes fail-closed — per-frame sha256 and
//! byte_length recomputation, contiguous per-stream offsets, strictly
//! monotonic sequences, completion totals — then reassembles each stream,
//! assigns the canonical content-addressed `raw_object_id`
//! (`sha256:<digest>` over the exact reassembled bytes, shared with the
//! typed blob identity space: no raw/typed identity split), persists it
//! idempotently into the redb store, and returns a typed receipt.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The rtk_nu envelope contract this module accepts.
pub const RTK_NU_ENVELOPE_SCHEMA_VERSION: &str = "flexnetos.rtk_nu.envelope.v1";
/// Versioned receipt for raw-envelope ingestion.
pub const RAW_RECEIPT_SCHEMA_VERSION: &str = "codedb.raw-ingest-receipt.v0";
/// Frame-count bound per envelope (fail-closed).
pub const MAX_RAW_FRAMES: usize = 100_000;
/// Reassembled per-stream byte bound (fail-closed).
pub const MAX_RAW_STREAM_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug)]
pub struct RawEnvelopeError(String);

impl RawEnvelopeError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for RawEnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for RawEnvelopeError {}

#[derive(Debug, Clone, Deserialize)]
pub struct RawFrame {
    pub sequence: u64,
    pub provisional_frame_id: String,
    pub provisional_content_id: String,
    pub canonical_raw_object_id: Option<String>,
    pub stream: String,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub payload_base64: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawExit {
    pub code: Option<i64>,
    pub signal: Option<i64>,
    pub success: bool,
    #[serde(default)]
    pub launch_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawCompletion {
    pub frame_count: u64,
    pub stdout_byte_length: u64,
    pub stderr_byte_length: u64,
    pub exit: RawExit,
}

/// The metadata fields this seam validates and echoes. rtk_nu attaches more
/// (identity, argv, timings); those pass through untouched in the stored
/// metadata JSON but are not contract-bound here.
#[derive(Debug, Clone, Deserialize)]
pub struct RawMetadata {
    pub schema_version: String,
    pub idempotency_key: String,
    pub rtk_filter: String,
    pub rtk_filter_revision: String,
    pub parser_name: String,
    pub parser_status: String,
}

/// One validated raw stream, reassembled to exact bytes.
#[derive(Debug, Clone)]
pub struct ValidatedStream {
    pub stream: String,
    pub bytes: Vec<u8>,
    pub frame_count: u64,
    /// The canonical id every frame of this stream must carry (or null).
    pub raw_object_id: String,
}

#[derive(Debug, Clone)]
pub struct ValidatedRawEnvelope {
    pub streams: Vec<ValidatedStream>,
    pub metadata_json: String,
    pub idempotency_key: String,
    pub exit: (Option<i64>, Option<i64>, bool),
}

#[derive(Debug, Clone, Serialize)]
pub struct RawObjectReceiptRow {
    pub stream: String,
    pub raw_object_id: String,
    pub byte_length: u64,
    pub frame_count: u64,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawIngestReceipt {
    pub schema_version: String,
    pub idempotency_key: String,
    pub raw_objects: Vec<RawObjectReceiptRow>,
    pub exit_code: Option<i64>,
    pub exit_signal: Option<i64>,
    pub exit_success: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawReportRow {
    pub raw_object_id: String,
    pub stream: String,
    pub byte_length: u64,
    pub frame_count: u64,
    pub idempotency_key: String,
    pub sha256: String,
}

/// Validate one rtk_nu JSON aggregate envelope fail-closed.
pub fn validate_raw_envelope(json: &str) -> Result<ValidatedRawEnvelope, RawEnvelopeError> {
    let _ = json;
    Err(RawEnvelopeError::new("validate_raw_envelope is not implemented"))
}

/// Validate a rtk_nu JSONL event stream (raw_frame* + execution_complete)
/// by assembling it into the aggregate shape first.
pub fn validate_raw_jsonl(text: &str) -> Result<ValidatedRawEnvelope, RawEnvelopeError> {
    let _ = text;
    Err(RawEnvelopeError::new("validate_raw_jsonl is not implemented"))
}

/// Validate a JSON array of JSONL event objects (the plugin's list input).
pub fn validate_raw_event_array(json: &str) -> Result<ValidatedRawEnvelope, RawEnvelopeError> {
    let _ = json;
    Err(RawEnvelopeError::new("validate_raw_event_array is not implemented"))
}

/// Persist every validated stream as a canonical raw object (idempotent) and
/// return the typed receipt.
pub fn run_raw_ingest(
    store_path: &std::path::Path,
    validated: &ValidatedRawEnvelope,
) -> Result<RawIngestReceipt, RawEnvelopeError> {
    let _ = (store_path, validated);
    Err(RawEnvelopeError::new("run_raw_ingest is not implemented"))
}

/// Read back every stored raw object with its metadata.
pub fn raw_report(store_path: &std::path::Path) -> Result<Vec<RawReportRow>, RawEnvelopeError> {
    let _ = store_path;
    Err(RawEnvelopeError::new("raw_report is not implemented"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[allow(dead_code)]
fn decode_frame_payload(frame: &RawFrame) -> Result<Vec<u8>, RawEnvelopeError> {
    BASE64
        .decode(&frame.payload_base64)
        .map_err(|e| RawEnvelopeError::new(format!("frame {} payload: {e}", frame.sequence)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codedb_store_redb::{StoreInitContext, initialize_store};
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static STORE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_store(label: &str) -> PathBuf {
        let unique = STORE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "codedb-raw-envelope-{label}-{}-{unique}.redb",
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

    fn frame(
        sequence: u64,
        stream: &str,
        byte_offset: u64,
        payload: &[u8],
    ) -> serde_json::Value {
        json!({
            "sequence": sequence,
            "provisional_frame_id": format!("provisional:frame:{sequence}"),
            "provisional_content_id": format!("provisional:content:{}", super::sha256_hex(payload)),
            "canonical_raw_object_id": null,
            "stream": stream,
            "byte_offset": byte_offset,
            "byte_length": payload.len(),
            "payload_base64": base64::engine::general_purpose::STANDARD.encode(payload),
            "sha256": super::sha256_hex(payload),
        })
    }

    fn metadata() -> serde_json::Value {
        json!({
            "schema_version": RTK_NU_ENVELOPE_SCHEMA_VERSION,
            "identity": {"execution_id": "exec-1", "task_id": "task-1", "branch_id": "branch-1"},
            "argv": ["echo", "hi"],
            "argv_bytes_base64": ["ZWNobw==", "aGk="],
            "cwd": "/tmp",
            "selected_environment_digest": "d".repeat(64),
            "idempotency_key": "idem-key-1",
            "rtk_filter": "none",
            "rtk_filter_revision": "0.43.0",
            "parser_name": "none",
            "parser_revision": "0",
            "parser_status": "not_attempted",
            "parser_error": null,
            "compact_representation": null,
            "typed_payload": null,
            "provenance_witness_seed": "seed",
            "started_at_unix_ms": 0,
        })
    }

    fn aggregate(frames: Vec<serde_json::Value>) -> serde_json::Value {
        let stdout_len: u64 = frames
            .iter()
            .filter(|f| f["stream"] == "stdout")
            .map(|f| f["byte_length"].as_u64().unwrap())
            .sum();
        let stderr_len: u64 = frames
            .iter()
            .filter(|f| f["stream"] == "stderr")
            .map(|f| f["byte_length"].as_u64().unwrap())
            .sum();
        json!({
            "schema_version": RTK_NU_ENVELOPE_SCHEMA_VERSION,
            "event_type": "execution",
            "metadata": metadata(),
            "frames": frames,
            "completion": {
                "frame_count": 0, // patched by callers via frames.len() below
                "stdout_byte_length": stdout_len,
                "stderr_byte_length": stderr_len,
                "completed_at_unix_ms": 1,
                "duration_ms": 1,
                "exit": {"code": 0, "signal": null, "success": true, "launch_error": null},
            },
        })
    }

    fn aggregate_json(frames: Vec<serde_json::Value>) -> String {
        let count = frames.len();
        let mut envelope = aggregate(frames);
        envelope["completion"]["frame_count"] = json!(count);
        envelope.to_string()
    }

    fn two_stream_frames() -> Vec<serde_json::Value> {
        vec![
            frame(1, "stdout", 0, b"hello "),
            frame(2, "stderr", 0, b"warn:"),
            frame(3, "stdout", 6, b"world"),
            frame(4, "stderr", 5, b" disk"),
        ]
    }

    // -- validation ------------------------------------------------------

    #[test]
    fn validates_a_well_formed_aggregate_and_reassembles_streams_exactly() {
        let validated = validate_raw_envelope(&aggregate_json(two_stream_frames()))
            .expect("valid envelope");
        assert_eq!(validated.idempotency_key, "idem-key-1");
        assert_eq!(validated.exit, (Some(0), None, true));
        let stdout = validated
            .streams
            .iter()
            .find(|s| s.stream == "stdout")
            .expect("stdout stream");
        assert_eq!(stdout.bytes, b"hello world");
        assert_eq!(stdout.frame_count, 2);
        assert_eq!(
            stdout.raw_object_id,
            format!("sha256:{}", super::sha256_hex(b"hello world"))
        );
        let stderr = validated
            .streams
            .iter()
            .find(|s| s.stream == "stderr")
            .expect("stderr stream");
        assert_eq!(stderr.bytes, b"warn: disk");
    }

    #[test]
    fn rejects_wrong_schema_version_and_wrong_event_type() {
        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["schema_version"] = json!("flexnetos.rtk_nu.envelope.v2");
        assert!(validate_raw_envelope(&envelope.to_string()).is_err());

        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["event_type"] = json!("raw_frame");
        assert!(validate_raw_envelope(&envelope.to_string()).is_err());
    }

    #[test]
    fn rejects_digest_and_length_mismatches_fail_closed() {
        // Corrupt sha256.
        let mut frames = two_stream_frames();
        frames[0]["sha256"] = json!("0".repeat(64));
        assert!(
            validate_raw_envelope(&aggregate_json(frames)).is_err(),
            "frame digest mismatch must be rejected"
        );
        // Corrupt byte_length (completion totals patched to match so only
        // the frame-level check can catch it).
        let mut frames = two_stream_frames();
        frames[0]["byte_length"] = json!(3);
        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["frames"] = json!(frames);
        assert!(
            validate_raw_envelope(&envelope.to_string()).is_err(),
            "frame length mismatch must be rejected"
        );
    }

    #[test]
    fn rejects_gapped_offsets_nonmonotonic_sequences_and_total_mismatches() {
        // Offset gap in stdout.
        let frames = vec![
            frame(1, "stdout", 0, b"hello "),
            frame(2, "stdout", 7, b"world"),
        ];
        assert!(validate_raw_envelope(&aggregate_json(frames)).is_err());

        // Non-monotonic sequence.
        let frames = vec![
            frame(2, "stdout", 0, b"hello "),
            frame(2, "stdout", 6, b"world"),
        ];
        assert!(validate_raw_envelope(&aggregate_json(frames)).is_err());

        // Completion totals disagreeing with frames.
        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["completion"]["stdout_byte_length"] = json!(1);
        assert!(validate_raw_envelope(&envelope.to_string()).is_err());

        // frame_count disagreeing with frames.
        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["completion"]["frame_count"] = json!(1);
        assert!(validate_raw_envelope(&envelope.to_string()).is_err());
    }

    #[test]
    fn accepts_preassigned_canonical_ids_only_when_they_match() {
        // Idempotent re-ingest: correct canonical id passes.
        let mut frames = two_stream_frames();
        let stdout_id = format!("sha256:{}", super::sha256_hex(b"hello world"));
        frames[0]["canonical_raw_object_id"] = json!(stdout_id.clone());
        frames[2]["canonical_raw_object_id"] = json!(stdout_id);
        validate_raw_envelope(&aggregate_json(frames)).expect("matching canonical id passes");

        // A wrong pre-assigned id is an identity violation.
        let mut frames = two_stream_frames();
        frames[0]["canonical_raw_object_id"] = json!(format!("sha256:{}", "0".repeat(64)));
        assert!(validate_raw_envelope(&aggregate_json(frames)).is_err());
    }

    #[test]
    fn validates_jsonl_event_stream_equivalently() {
        let frames = two_stream_frames();
        let meta = metadata();
        let mut lines = Vec::new();
        for f in &frames {
            lines.push(
                json!({"event_type": "raw_frame", "metadata": meta, "frame": f}).to_string(),
            );
        }
        lines.push(
            json!({
                "event_type": "execution_complete",
                "metadata": meta,
                "frame_count": frames.len(),
                "stdout_byte_length": 11,
                "stderr_byte_length": 10,
                "completed_at_unix_ms": 1,
                "duration_ms": 1,
                "exit": {"code": 0, "signal": null, "success": true, "launch_error": null},
            })
            .to_string(),
        );
        let text = lines.join("\n");
        let validated = validate_raw_jsonl(&text).expect("valid JSONL");
        let stdout = validated
            .streams
            .iter()
            .find(|s| s.stream == "stdout")
            .expect("stdout");
        assert_eq!(stdout.bytes, b"hello world");

        // The same events as a JSON array (the plugin's list input).
        let array = format!("[{}]", lines.join(","));
        let validated = validate_raw_event_array(&array).expect("valid event array");
        assert_eq!(validated.streams.len(), 2);

        // Truncated stream (no completion) fails closed.
        let truncated = lines[..frames.len()].join("\n");
        assert!(validate_raw_jsonl(&truncated).is_err());
    }

    // -- ingest ----------------------------------------------------------

    #[test]
    fn ingest_assigns_canonical_ids_idempotently_and_marks_dedup() {
        let store = temp_store("idempotent");
        let validated = validate_raw_envelope(&aggregate_json(two_stream_frames())).unwrap();
        let first = run_raw_ingest(&store, &validated).expect("first ingest");
        assert_eq!(first.schema_version, RAW_RECEIPT_SCHEMA_VERSION);
        assert_eq!(first.raw_objects.len(), 2);
        assert!(first.raw_objects.iter().all(|o| !o.deduplicated));
        let stdout_id = first
            .raw_objects
            .iter()
            .find(|o| o.stream == "stdout")
            .unwrap()
            .raw_object_id
            .clone();
        assert_eq!(
            stdout_id,
            format!("sha256:{}", super::sha256_hex(b"hello world"))
        );

        // Re-ingest: identical canonical ids, dedup marked.
        let second = run_raw_ingest(&store, &validated).expect("second ingest");
        assert!(second.raw_objects.iter().all(|o| o.deduplicated));
        assert_eq!(
            second
                .raw_objects
                .iter()
                .find(|o| o.stream == "stdout")
                .unwrap()
                .raw_object_id,
            stdout_id
        );
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn raw_and_typed_bytes_share_one_identity_space() {
        let store = temp_store("identity");
        // Persist the same bytes as a typed source blob first.
        let row =
            codedb_store_redb::persist_source_blob(&store, "src/out.txt", b"hello world")
                .expect("typed blob");
        let validated = validate_raw_envelope(&aggregate_json(two_stream_frames())).unwrap();
        let receipt = run_raw_ingest(&store, &validated).expect("ingest");
        let stdout = receipt
            .raw_objects
            .iter()
            .find(|o| o.stream == "stdout")
            .unwrap();
        assert_eq!(
            stdout.raw_object_id,
            format!("sha256:{}", row.sha256),
            "raw object ids live in the same content-addressed space as typed blobs"
        );
        assert!(
            stdout.deduplicated,
            "identical bytes already in the store must dedup, not fork identity"
        );
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn report_reads_back_stream_metadata_and_exact_digests() {
        let store = temp_store("report");
        let validated = validate_raw_envelope(&aggregate_json(two_stream_frames())).unwrap();
        run_raw_ingest(&store, &validated).expect("ingest");
        let rows = raw_report(&store).expect("report");
        assert_eq!(rows.len(), 2);
        let stdout = rows.iter().find(|r| r.stream == "stdout").expect("stdout row");
        assert_eq!(stdout.byte_length, 11);
        assert_eq!(stdout.frame_count, 2);
        assert_eq!(stdout.idempotency_key, "idem-key-1");
        assert_eq!(stdout.sha256, super::sha256_hex(b"hello world"));
        assert_eq!(stdout.raw_object_id, format!("sha256:{}", stdout.sha256));
        std::fs::remove_file(&store).ok();
    }

    #[test]
    fn signal_death_envelopes_ingest_with_truthful_exit_metadata() {
        let mut envelope: serde_json::Value =
            serde_json::from_str(&aggregate_json(two_stream_frames())).unwrap();
        envelope["completion"]["exit"] =
            json!({"code": null, "signal": 6, "success": false, "launch_error": null});
        let validated = validate_raw_envelope(&envelope.to_string()).expect("signal envelope");
        assert_eq!(validated.exit, (None, Some(6), false));
        let store = temp_store("signal");
        let receipt = run_raw_ingest(&store, &validated).expect("ingest");
        assert_eq!(receipt.exit_signal, Some(6));
        assert!(!receipt.exit_success);
        std::fs::remove_file(&store).ok();
    }
}

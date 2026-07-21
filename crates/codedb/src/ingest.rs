//! Typed `codedb ingest-envelope` ingestion (ARCHBP-001).
//!
//! Bounded native-Nushell ingestion: a typed envelope of source files with
//! exact bytes (base64), relative paths, unix modes, module paths, sha256
//! identities, and flattened Nushell AST rows. Bytes are content-addressed in
//! the selected redb store exactly once; BLAKE3 identities index duplicate
//! content. Hashes and AST rows supplement the stored bytes; they never
//! replace them.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

pub const ENVELOPE_SCHEMA_VERSION: &str = "codedb.ingest-envelope.v0";
pub const RECEIPT_SCHEMA_VERSION: &str = "codedb.ingest-receipt.v0";
pub const MAX_ENVELOPE_FILES: usize = 512;
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
pub const MAX_AST_ROWS_PER_FILE: usize = 10_000;

#[derive(Debug)]
pub struct IngestError(String);

impl IngestError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for IngestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for IngestError {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstRow {
    pub content: String,
    pub shape: String,
    pub span_start: u64,
    pub span_end: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvelopeFile {
    pub path: String,
    pub module_path: String,
    pub unix_mode: String,
    pub content_base64: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blake3: Option<String>,
    #[serde(default)]
    pub ast: Vec<AstRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestEnvelope {
    pub schema_version: String,
    pub files: Vec<EnvelopeFile>,
}

/// One validated envelope file with its decoded exact bytes.
#[derive(Debug, Clone)]
pub struct ValidatedFile {
    pub file: EnvelopeFile,
    pub bytes: Vec<u8>,
    pub blake3: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptFile {
    pub path: String,
    pub sha256: String,
    pub blake3: String,
    pub blob_ref: String,
    pub bytes: u64,
    pub deduplicated: bool,
    pub ast_rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiptSummary {
    pub file_count: u64,
    pub unique_blob_count: u64,
    pub dedup_hit_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReceipt {
    pub schema_version: String,
    pub files: Vec<ReceiptFile>,
    pub summary: ReceiptSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestReportRow {
    pub path: String,
    pub module_path: String,
    pub unix_mode: String,
    pub sha256: String,
    pub blake3: String,
    pub bytes: u64,
    pub ast: Vec<AstRow>,
}

/// Parse and validate a typed ingest envelope.
///
/// Fail-closed validation: exact schema version; bounded file count, decoded
/// size, and AST row count; clean relative paths (no traversal, no absolute
/// paths, no backslashes, no duplicates); valid base64; sha256 recomputed
/// over the decoded bytes must equal the declared identity; a supplied
/// blake3 must equal the recomputed identity; unix modes are 3-4 octal
/// digits.
pub fn validate_envelope(json: &str) -> Result<Vec<ValidatedFile>, IngestError> {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use sha2::{Digest, Sha256};

    let envelope: IngestEnvelope = serde_json::from_str(json)
        .map_err(|error| IngestError::new(format!("invalid envelope JSON: {error}")))?;
    if envelope.schema_version != ENVELOPE_SCHEMA_VERSION {
        return Err(IngestError::new(format!(
            "unsupported schema_version {:?}; expected {ENVELOPE_SCHEMA_VERSION:?}",
            envelope.schema_version
        )));
    }
    if envelope.files.is_empty() {
        return Err(IngestError::new("envelope contains no files"));
    }
    if envelope.files.len() > MAX_ENVELOPE_FILES {
        return Err(IngestError::new(format!(
            "envelope exceeds the {MAX_ENVELOPE_FILES}-file bound: {}",
            envelope.files.len()
        )));
    }

    let mut seen_paths = std::collections::BTreeSet::new();
    let mut validated = Vec::with_capacity(envelope.files.len());
    for file in envelope.files {
        validate_relative_path(&file.path)?;
        if !seen_paths.insert(file.path.clone()) {
            return Err(IngestError::new(format!(
                "duplicate path in envelope: {:?}",
                file.path
            )));
        }
        if !(3..=4).contains(&file.unix_mode.len())
            || !file.unix_mode.bytes().all(|b| (b'0'..=b'7').contains(&b))
        {
            return Err(IngestError::new(format!(
                "{}: unix_mode must be 3-4 octal digits, got {:?}",
                file.path, file.unix_mode
            )));
        }
        if file.ast.len() > MAX_AST_ROWS_PER_FILE {
            return Err(IngestError::new(format!(
                "{}: AST rows exceed the {MAX_AST_ROWS_PER_FILE}-row bound: {}",
                file.path,
                file.ast.len()
            )));
        }
        let bytes = BASE64.decode(&file.content_base64).map_err(|error| {
            IngestError::new(format!("{}: invalid content base64: {error}", file.path))
        })?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(IngestError::new(format!(
                "{}: decoded content exceeds the {MAX_FILE_BYTES}-byte bound: {} bytes",
                file.path,
                bytes.len()
            )));
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if sha256 != file.sha256 {
            return Err(IngestError::new(format!(
                "{}: declared sha256 {} does not match decoded bytes ({sha256})",
                file.path, file.sha256
            )));
        }
        let blake3 = blake3::hash(&bytes).to_hex().to_string();
        if let Some(declared) = &file.blake3 {
            if declared != &blake3 {
                return Err(IngestError::new(format!(
                    "{}: declared blake3 {declared} does not match decoded bytes ({blake3})",
                    file.path
                )));
            }
        }
        validated.push(ValidatedFile { file, bytes, blake3 });
    }
    Ok(validated)
}

fn validate_relative_path(path: &str) -> Result<(), IngestError> {
    if path.is_empty() {
        return Err(IngestError::new("empty path in envelope"));
    }
    if path.contains('\\') {
        return Err(IngestError::new(format!(
            "path {path:?} must use forward slashes"
        )));
    }
    if path.starts_with('/') {
        return Err(IngestError::new(format!(
            "path {path:?} must be relative"
        )));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(IngestError::new(format!(
                "path {path:?} contains a forbidden component {component:?}"
            )));
        }
    }
    Ok(())
}

/// Persist validated files into the redb store at `store_path`,
/// content-addressing duplicate bytes exactly once, and return the typed
/// receipt.
pub fn run_ingest(
    store_path: &std::path::Path,
    files: &[ValidatedFile],
) -> Result<IngestReceipt, IngestError> {
    let mut receipt_files = Vec::with_capacity(files.len());
    let mut dedup_hit_count = 0u64;
    for validated in files {
        let ast_json = serde_json::to_string(&validated.file.ast)
            .map_err(|error| IngestError::new(format!("serialize AST rows: {error}")))?;
        let row = codedb_store_redb::persist_ingest_file(
            store_path,
            validated.file.path.as_str(),
            &validated.bytes,
            &validated.blake3,
            &validated.file.unix_mode,
            &validated.file.module_path,
            &ast_json,
        )
        .map_err(|error| {
            IngestError::new(format!("{}: store write failed: {error}", validated.file.path))
        })?;
        if row.deduplicated {
            dedup_hit_count += 1;
        }
        receipt_files.push(ReceiptFile {
            path: row.relative_path,
            sha256: row.sha256,
            blake3: validated.blake3.clone(),
            blob_ref: row.blob_ref,
            bytes: row.bytes,
            deduplicated: row.deduplicated,
            ast_rows: validated.file.ast.len() as u64,
        });
    }
    let file_count = receipt_files.len() as u64;
    Ok(IngestReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        files: receipt_files,
        summary: ReceiptSummary {
            file_count,
            unique_blob_count: file_count - dedup_hit_count,
            dedup_hit_count,
        },
    })
}

/// Read back every ingested file's stored metadata (module path, unix mode,
/// hashes, AST rows) from the redb store.
pub fn ingest_report(store_path: &std::path::Path) -> Result<Vec<IngestReportRow>, IngestError> {
    let rows = codedb_store_redb::list_ingest_files(store_path)
        .map_err(|error| IngestError::new(format!("store read failed: {error}")))?;
    rows.into_iter()
        .map(|row| {
            let ast: Vec<AstRow> = if row.ast_json.is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&row.ast_json).map_err(|error| {
                    IngestError::new(format!(
                        "{}: stored AST rows are not valid JSON: {error}",
                        row.relative_path
                    ))
                })?
            };
            Ok(IngestReportRow {
                path: row.relative_path,
                module_path: row.module_path,
                unix_mode: row.unix_mode,
                sha256: row.sha256,
                blake3: row.blake3,
                bytes: row.bytes,
                ast,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }

    fn envelope_json(files: &[EnvelopeFile]) -> String {
        serde_json::to_string(&IngestEnvelope {
            schema_version: ENVELOPE_SCHEMA_VERSION.to_string(),
            files: files.to_vec(),
        })
        .expect("serialize envelope")
    }

    fn file_entry(path: &str, bytes: &[u8]) -> EnvelopeFile {
        EnvelopeFile {
            path: path.to_string(),
            module_path: path.replace('/', "::"),
            unix_mode: "644".to_string(),
            content_base64: BASE64.encode(bytes),
            sha256: sha256_hex(bytes),
            blake3: None,
            ast: vec![AstRow {
                content: "let".to_string(),
                shape: "shape_internalcall".to_string(),
                span_start: 0,
                span_end: 3,
            }],
        }
    }

    fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = dir.path().join("codedb.redb");
        codedb_store_redb::initialize_store(
            &store,
            &codedb_store_redb::StoreInitContext {
                codedb_version: "test",
                toolchain: "test",
                rustc_version: "test",
                cargo_version: "test",
            },
        )
        .expect("initialize store");
        (dir, store)
    }

    #[test]
    fn validates_and_decodes_a_correct_envelope() {
        let files = [file_entry("mod.nu", b"let x = 1\n")];
        let validated = validate_envelope(&envelope_json(&files)).expect("valid envelope");
        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].bytes, b"let x = 1\n");
        assert_eq!(
            validated[0].blake3,
            blake3::hash(b"let x = 1\n").to_hex().to_string()
        );
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let json = envelope_json(&[file_entry("mod.nu", b"x")])
            .replace(ENVELOPE_SCHEMA_VERSION, "codedb.ingest-envelope.v999");
        let error = validate_envelope(&json).expect_err("wrong schema must fail");
        assert!(error.to_string().contains("schema_version"), "{error}");
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in ["../escape.nu", "/etc/passwd", "a/../b.nu", "a\\b.nu", ""] {
            let json = envelope_json(&[file_entry(bad, b"x")]);
            let error = validate_envelope(&json).expect_err(&format!("{bad:?} must fail"));
            assert!(error.to_string().contains("path"), "{bad:?}: {error}");
        }
    }

    #[test]
    fn rejects_sha256_mismatch() {
        let mut entry = file_entry("mod.nu", b"real bytes");
        entry.sha256 = sha256_hex(b"other bytes");
        let error = validate_envelope(&envelope_json(&[entry])).expect_err("sha mismatch");
        assert!(error.to_string().contains("sha256"), "{error}");
    }

    #[test]
    fn rejects_supplied_blake3_mismatch() {
        let mut entry = file_entry("mod.nu", b"real bytes");
        entry.blake3 = Some(blake3::hash(b"other bytes").to_hex().to_string());
        let error = validate_envelope(&envelope_json(&[entry])).expect_err("blake3 mismatch");
        assert!(error.to_string().contains("blake3"), "{error}");
    }

    #[test]
    fn rejects_invalid_base64_and_oversize_content() {
        let mut entry = file_entry("mod.nu", b"x");
        entry.content_base64 = "!!!not-base64!!!".to_string();
        let error = validate_envelope(&envelope_json(&[entry])).expect_err("bad base64");
        assert!(error.to_string().contains("base64"), "{error}");

        let big = vec![b'a'; MAX_FILE_BYTES + 1];
        let error = validate_envelope(&envelope_json(&[file_entry("big.nu", &big)]))
            .expect_err("oversize content");
        assert!(error.to_string().contains("bytes"), "{error}");
    }

    #[test]
    fn rejects_duplicate_paths_and_bad_unix_mode() {
        let json = envelope_json(&[file_entry("mod.nu", b"a"), file_entry("mod.nu", b"b")]);
        let error = validate_envelope(&json).expect_err("duplicate path");
        assert!(error.to_string().contains("path"), "{error}");

        let mut entry = file_entry("mod.nu", b"a");
        entry.unix_mode = "rwxr-xr-x".to_string();
        let error = validate_envelope(&envelope_json(&[entry])).expect_err("bad mode");
        assert!(error.to_string().contains("unix_mode"), "{error}");
    }

    #[test]
    fn ingests_and_deduplicates_identical_bytes_once() {
        let (_dir, store) = temp_store();
        let files = [
            file_entry("dup/copy_one.nu", b"identical bytes\n"),
            file_entry("dup/copy_two.nu", b"identical bytes\n"),
            file_entry("unique.nu", b"different bytes\n"),
        ];
        let validated = validate_envelope(&envelope_json(&files)).expect("valid");
        let receipt = run_ingest(&store, &validated).expect("ingest");

        assert_eq!(receipt.schema_version, RECEIPT_SCHEMA_VERSION);
        assert_eq!(receipt.summary.file_count, 3);
        assert_eq!(receipt.summary.unique_blob_count, 2);
        assert_eq!(receipt.summary.dedup_hit_count, 1);
        let one = &receipt.files[0];
        let two = &receipt.files[1];
        assert_eq!(one.blake3, two.blake3);
        assert!(!one.deduplicated);
        assert!(two.deduplicated);

        // Re-ingesting the same envelope is idempotent: everything dedups.
        let receipt2 = run_ingest(&store, &validated).expect("re-ingest");
        assert_eq!(receipt2.summary.dedup_hit_count, 3);
        assert_eq!(receipt2.summary.unique_blob_count, 0);
    }

    #[test]
    fn round_trips_metadata_and_exact_bytes_through_the_store() {
        let (dir, store) = temp_store();
        let source = b"def build [] { print \"ok\" }\n";
        let mut entry = file_entry("scripts/build.nu", source);
        entry.unix_mode = "755".to_string();
        let validated = validate_envelope(&envelope_json(&[entry.clone()])).expect("valid");
        run_ingest(&store, &validated).expect("ingest");

        let report = ingest_report(&store).expect("report");
        assert_eq!(report.len(), 1);
        let row = &report[0];
        assert_eq!(row.path, "scripts/build.nu");
        assert_eq!(row.module_path, entry.module_path);
        assert_eq!(row.unix_mode, "755");
        assert_eq!(row.sha256, entry.sha256);
        assert_eq!(row.blake3, blake3::hash(source).to_hex().to_string());
        assert_eq!(row.ast, entry.ast);

        // The stored bytes materialize byte-exactly with the captured mode.
        let out = dir.path().join("restored/scripts/build.nu");
        codedb_store_redb::materialize_source_file(&store, "scripts/build.nu", &out)
            .expect("materialize");
        assert_eq!(std::fs::read(&out).expect("read restored"), source);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&out).expect("metadata").permissions().mode() & 0o7777;
            assert_eq!(mode, 0o755);
        }
    }
}

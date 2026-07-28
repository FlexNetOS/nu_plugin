#![cfg(feature = "pg-integration")]

//! ARCHBP-038 red tests: every declared host data class imports as original
//! bytes plus typed records, every byte and transformation is queryable,
//! reconstruction proves byte/structure/metadata/semantic/provenance
//! equality, hash substitution is impossible, and unclassifiable objects
//! fail the import closed. Uses `CODEDB_PG_CONN` against a disposable
//! PostgreSQL service.

use codedb_host_import::{
    IMPORT_RECEIPT_SCHEMA_VERSION, RECONSTRUCTION_RECEIPT_SCHEMA_VERSION, ensure_schema,
    import_corpus, reconstruct_and_verify, session_entries,
};
use postgres::{Client, NoTls};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static PG_LOCK: Mutex<()> = Mutex::new(());

fn disposable_conn() -> String {
    std::env::var("CODEDB_PG_CONN")
        .expect("CODEDB_PG_CONN must select the explicit disposable PostgreSQL test service")
}

fn reset(conn: &str) {
    let mut client = Client::connect(conn, NoTls).expect("connect");
    client
        .batch_execute(
            "DROP TABLE IF EXISTS host_import_entries;\
             DROP TABLE IF EXISTS host_byte_objects;\
             DROP TABLE IF EXISTS host_import_transformations;\
             DROP TABLE IF EXISTS host_import_sessions;",
        )
        .expect("reset");
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Build the complete gate corpus: zero-length, binary, invalid UTF-8,
/// sparse, symlink, xattr, model, repository, log, cache, and
/// protected-encrypted-secret fixtures.
fn build_corpus(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("dirs");
    std::fs::create_dir_all(root.join(".git/objects/ab")).expect("git dirs");
    std::fs::create_dir_all(root.join("cache")).expect("cache dir");

    std::fs::write(root.join("empty.touch"), b"").expect("zero-length");
    std::fs::write(root.join("src/main.rs"), b"fn main() { println!(\"hi\"); }\n")
        .expect("text");
    std::fs::write(root.join("logo.bin"), [0u8, 159, 146, 150, 255, 0, 1]).expect("binary");
    std::fs::write(root.join("mangled.txt"), [b'h', b'i', 0xff, 0xfe, b'!']).expect("invalid utf8");

    // Sparse: a real hole via truncate beyond the written prefix.
    let sparse_path = root.join("sparse.dat");
    std::fs::write(&sparse_path, b"prefix").expect("sparse prefix");
    let sparse = std::fs::OpenOptions::new()
        .write(true)
        .open(&sparse_path)
        .expect("open sparse");
    sparse.set_len(1024 * 1024).expect("truncate sparse");
    drop(sparse);

    std::os::unix::fs::symlink("src/main.rs", root.join("entry.link")).expect("symlink");
    std::os::unix::fs::symlink("missing-target", root.join("dangling.link")).expect("dangling");

    std::fs::write(root.join("tagged.conf"), b"key=value\n").expect("xattr file");
    xattr::set(root.join("tagged.conf"), "user.codedb_class", b"gate-fixture")
        .expect("set xattr (tmpfs must support user xattrs)");

    std::fs::write(root.join("model.safetensors"), [7u8; 256]).expect("model");
    std::fs::write(
        root.join(".git/objects/ab/cdef0123456789"),
        [0x78, 0x9c, 1, 2, 3],
    )
    .expect("repo object");
    std::fs::write(root.join("service.log"), b"2026-07-21T00:00:00Z started\n").expect("log");
    std::fs::write(root.join("cache/blob.cache"), [9u8; 64]).expect("cache");
    // An already-encrypted secret imports as its exact ciphertext bytes.
    std::fs::write(root.join("vault.secret.age"), [0xA5u8, 0x5A, 0x42, 0x42, 0x99])
        .expect("encrypted secret");

    let mut perms = std::fs::metadata(root.join("src/main.rs")).expect("meta").permissions();
    perms.set_mode(0o750);
    std::fs::set_permissions(root.join("src/main.rs"), perms).expect("chmod");
}

fn temp_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "codedb-host-import-{label}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&root).ok();
    std::fs::create_dir_all(&root).expect("root");
    root
}

#[test]
fn every_declared_class_imports_with_real_bytes_and_typed_metadata() {
    let _guard = PG_LOCK.lock().expect("pg lock");
    let conn = disposable_conn();
    reset(&conn);
    ensure_schema(&conn).expect("schema");
    let corpus = temp_root("classes");
    build_corpus(&corpus);

    let receipt = import_corpus(&conn, &corpus).expect("import");
    assert_eq!(receipt.schema_version, IMPORT_RECEIPT_SCHEMA_VERSION);
    assert!(receipt.zero_unclassified_loss, "nothing may be unclassified");
    for class in [
        "zero_length",
        "text_utf8",
        "binary",
        "invalid_utf8_text",
        "sparse",
        "symlink",
        "xattr_file",
        "model_weight",
        "repository_object",
        "log",
        "cache",
        "protected_encrypted_secret",
        "directory",
    ] {
        assert!(
            receipt.class_counts.get(class).copied().unwrap_or(0) >= 1,
            "class {class} missing from the import: {:?}",
            receipt.class_counts
        );
    }

    // Hash substitution is impossible: the stored bytes ARE the source
    // bytes, re-verified by recomputing the digest inside PostgreSQL.
    let mut client = Client::connect(&conn, NoTls).expect("connect");
    let rows = client
        .query(
            "SELECT sha256, octet_length(bytes), byte_length, encode(sha256(bytes), 'hex') \
             FROM host_byte_objects",
            &[],
        )
        .expect("byte objects");
    assert!(!rows.is_empty());
    for row in rows {
        let stored_sha: String = row.get(0);
        let octet_len: i32 = row.get(1);
        let declared_len: i64 = row.get(2);
        let recomputed: String = row.get(3);
        assert_eq!(octet_len as i64, declared_len, "bytes must be complete");
        assert_eq!(stored_sha, recomputed, "stored bytes must address to their digest");
    }

    // Typed metadata is queryable per class through plain SQL.
    let symlinks: i64 = client
        .query_one(
            "SELECT count(*) FROM host_import_entries WHERE data_class='symlink' \
             AND metadata->>'symlink_target' IS NOT NULL",
            &[],
        )
        .expect("symlink query")
        .get(0);
    assert_eq!(symlinks, 2, "both symlinks carry their targets");
    let xattrs: i64 = client
        .query_one(
            "SELECT count(*) FROM host_import_entries \
             WHERE data_class='xattr_file' AND metadata->'xattrs'->>'user.codedb_class' = 'gate-fixture'",
            &[],
        )
        .expect("xattr query")
        .get(0);
    assert_eq!(xattrs, 1, "the xattr value is queryable");
    let sparse: i64 = client
        .query_one(
            "SELECT count(*) FROM host_import_entries WHERE data_class='sparse' \
             AND (metadata->'sparse'->>'size')::bigint = 1048576",
            &[],
        )
        .expect("sparse query")
        .get(0);
    assert_eq!(sparse, 1, "sparseness metadata is queryable");

    // The encrypted secret imported its exact ciphertext.
    let entries = session_entries(&conn, receipt.session_id).expect("entries");
    let secret = entries
        .iter()
        .find(|e| e.data_class == "protected_encrypted_secret")
        .expect("secret entry");
    assert_eq!(
        secret.byte_sha256.as_deref(),
        Some(sha256_hex(&[0xA5u8, 0x5A, 0x42, 0x42, 0x99]).as_str()),
        "ciphertext bytes import exactly, never decrypted"
    );

    std::fs::remove_dir_all(&corpus).ok();
}

#[test]
fn reconstruction_proves_byte_structure_metadata_semantic_provenance_equality() {
    let _guard = PG_LOCK.lock().expect("pg lock");
    let conn = disposable_conn();
    reset(&conn);
    ensure_schema(&conn).expect("schema");
    let corpus = temp_root("reconstruct");
    build_corpus(&corpus);
    let receipt = import_corpus(&conn, &corpus).expect("import");

    let target = temp_root("reconstructed");
    std::fs::remove_dir_all(&target).ok();
    let proof = reconstruct_and_verify(&conn, receipt.session_id, &corpus, &target)
        .expect("reconstruct");
    assert_eq!(proof.schema_version, RECONSTRUCTION_RECEIPT_SCHEMA_VERSION);
    assert!(proof.byte_equality, "bytes must be equal: {:?}", proof.mismatches);
    assert!(proof.structure_equality, "structure must be equal: {:?}", proof.mismatches);
    assert!(proof.metadata_equality, "metadata must be equal: {:?}", proof.mismatches);
    assert!(proof.semantic_equality, "classes must re-derive: {:?}", proof.mismatches);
    assert!(proof.provenance_recorded, "provenance must be recorded");
    assert!(proof.mismatches.is_empty());
    assert!(proof.entries_verified >= 15);

    // Spot-check the physically reconstructed tree.
    assert_eq!(
        std::fs::read(target.join("src/main.rs")).expect("bytes"),
        std::fs::read(corpus.join("src/main.rs")).expect("bytes"),
    );
    assert_eq!(
        std::fs::read_link(target.join("dangling.link")).expect("link").to_string_lossy(),
        "missing-target"
    );
    assert_eq!(
        std::fs::metadata(target.join("src/main.rs")).expect("meta").permissions().mode() & 0o7777,
        0o750,
        "permissions restore exactly"
    );
    assert_eq!(
        xattr::get(target.join("tagged.conf"), "user.codedb_class")
            .expect("xattr")
            .expect("value"),
        b"gate-fixture".to_vec(),
        "xattrs restore exactly"
    );

    std::fs::remove_dir_all(&corpus).ok();
    std::fs::remove_dir_all(&target).ok();
}

#[test]
fn unclassifiable_objects_fail_the_import_closed() {
    let _guard = PG_LOCK.lock().expect("pg lock");
    let conn = disposable_conn();
    reset(&conn);
    ensure_schema(&conn).expect("schema");
    let corpus = temp_root("fifo");
    std::fs::write(corpus.join("ok.txt"), b"fine\n").expect("file");
    let fifo = corpus.join("stream.fifo");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");
    assert!(status.success());

    let result = import_corpus(&conn, &corpus);
    assert!(result.is_err(), "a fifo is not a declared data class and must abort");
    let error = result.err().expect("error").to_string();
    assert!(
        error.contains("stream.fifo"),
        "the refusal names the unclassifiable object: {error}"
    );

    // Fail-closed means fail-closed: nothing from the aborted session is
    // acknowledged as a completed import.
    let mut client = Client::connect(&conn, NoTls).expect("connect");
    let completed: i64 = client
        .query_one(
            "SELECT count(*) FROM host_import_sessions WHERE completed_at IS NOT NULL",
            &[],
        )
        .expect("sessions")
        .get(0);
    assert_eq!(completed, 0, "no completed session may exist after the refusal");

    std::fs::remove_dir_all(&corpus).ok();
}

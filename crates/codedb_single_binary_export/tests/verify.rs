//! ARCHBP-024 red tests: snapshot verify, list, license, corruption
//! rejection, and deterministic generation of the embedded assets.

use codedb_single_binary_export::{
    EMBEDDED_CHECKSUMS, EMBEDDED_LICENSE_MANIFEST, EMBEDDED_MANIFEST, EMBEDDED_PACK,
    generate_assets, license_report, list_entries, schema_info, summary, verify_embedded,
    verify_pack,
};

#[test]
fn embedded_snapshot_verifies_end_to_end() {
    let report = verify_embedded().expect("embedded snapshot verifies");
    assert!(report.pack_sha256_ok, "pack digest must match the manifest");
    assert!(report.per_file_checksums_ok, "every embedded file digest must match");
    assert!(report.file_count >= 5, "the bounded snapshot carries a real corpus");
}

#[test]
fn list_enumerates_every_embedded_entry_with_digests() {
    let entries = list_entries().expect("list");
    assert!(entries.len() >= 5);
    for entry in &entries {
        assert!(!entry.path.is_empty());
        assert_eq!(entry.sha256.len(), 64);
        assert!(!entry.path.starts_with('/'), "entries are relative");
    }
    // Deterministic order: byte-sorted by path.
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        entries.iter().map(|e| &e.path).collect::<Vec<_>>(),
        sorted.iter().map(|e| &e.path).collect::<Vec<_>>()
    );
}

#[test]
fn schema_and_summary_are_bounded_and_versioned() {
    let schema = schema_info().expect("schema");
    assert_eq!(schema.schema_version, "codedb.single-binary-snapshot.v0");
    let summary = summary().expect("summary");
    assert!(summary.file_count >= 5);
    assert!(summary.total_bytes > 0);
    assert!(!summary.snapshot_source.is_empty());
}

#[test]
fn license_report_names_every_embedded_component() {
    let report = license_report().expect("license report");
    assert!(!report.components.is_empty());
    for component in &report.components {
        assert!(!component.name.is_empty());
        assert!(!component.license.is_empty(), "{} lacks a license", component.name);
    }
    // The snapshot content itself must be covered.
    assert!(report.components.iter().any(|c| c.name == "codedb-snapshot-content"));
}

#[test]
fn corrupted_pack_bytes_are_rejected_fail_closed() {
    let mut corrupted = EMBEDDED_PACK.to_vec();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xff;
    let result = verify_pack(&corrupted, EMBEDDED_MANIFEST, EMBEDDED_CHECKSUMS);
    assert!(result.is_err(), "a flipped byte must fail verification");

    // A manifest that lies about the pack digest is equally rejected.
    let mut manifest: serde_json::Value = serde_json::from_str(EMBEDDED_MANIFEST).unwrap();
    manifest["pack_sha256"] = serde_json::json!("0".repeat(64));
    let lying = manifest.to_string();
    assert!(verify_pack(EMBEDDED_PACK, &lying, EMBEDDED_CHECKSUMS).is_err());
}

#[test]
fn asset_generation_is_deterministic_and_matches_the_committed_assets() {
    let dir = tempfile::tempdir().expect("tempdir");
    generate_assets(dir.path()).expect("generate");
    let regenerated_pack = std::fs::read(dir.path().join("codedb-pack.zst")).expect("pack");
    let regenerated_manifest =
        std::fs::read_to_string(dir.path().join("manifest.json")).expect("manifest");
    let regenerated_checksums =
        std::fs::read_to_string(dir.path().join("checksums.sha256")).expect("checksums");
    let regenerated_licenses =
        std::fs::read_to_string(dir.path().join("license-manifest.json")).expect("licenses");
    assert_eq!(regenerated_pack, EMBEDDED_PACK, "pack bytes must regenerate identically");
    assert_eq!(regenerated_manifest, EMBEDDED_MANIFEST);
    assert_eq!(regenerated_checksums, EMBEDDED_CHECKSUMS);
    assert_eq!(regenerated_licenses, EMBEDDED_LICENSE_MANIFEST);

    // Twice in a row: byte-identical again.
    let dir2 = tempfile::tempdir().expect("tempdir");
    generate_assets(dir2.path()).expect("generate again");
    assert_eq!(
        std::fs::read(dir2.path().join("codedb-pack.zst")).expect("pack"),
        regenerated_pack
    );
}

#[test]
fn no_secret_shaped_content_is_embedded() {
    let entries = list_entries().expect("list");
    for entry in &entries {
        for marker in [".env", "id_rsa", ".pem", ".kdbx", "credentials"] {
            assert!(
                !entry.path.contains(marker),
                "secret-shaped path {} must never be embedded",
                entry.path
            );
        }
    }
}

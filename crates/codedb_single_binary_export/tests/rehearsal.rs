//! ARCHBP-024 red test: the all-peer consolidation rehearsal accounts for
//! every capability and preserves each independent repository and lockfile.
//! Requires the lifeos planning spine on disk; set CODEDB_SPINE_ROOT to run
//! (skips with a notice otherwise, e.g. in a bare CI checkout).

use codedb_single_binary_export::rehearsal;

#[test]
fn rehearsal_accounts_for_every_capability_and_preserves_every_peer() {
    let Ok(spine_root) = std::env::var("CODEDB_SPINE_ROOT") else {
        eprintln!("CODEDB_SPINE_ROOT unset: rehearsal test requires the lifeos spine; skipping");
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("rehearsal-receipt.json");
    let receipt = rehearsal::run(std::path::Path::new(&spine_root), &out)
        .expect("rehearsal runs read-only");
    assert_eq!(receipt.schema_version, rehearsal::REHEARSAL_RECEIPT_SCHEMA_VERSION);
    assert_eq!(receipt.units_checked, 3, "all three retirement units");
    assert_eq!(receipt.capabilities_accounted, 19, "all nineteen adopted capabilities");
    assert!(receipt.peers_checked >= 42, "all provenance peers accounted");
    assert!(receipt.all_preserved, "every repo+lockfile preserved: {:?}", receipt.findings);
    assert!(receipt.bounded_claim.contains("bounded"),
        "the receipt itself must bound its claim");
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).expect("receipt file"))
            .expect("receipt json");
    assert_eq!(written["schema_version"], rehearsal::REHEARSAL_RECEIPT_SCHEMA_VERSION);
}

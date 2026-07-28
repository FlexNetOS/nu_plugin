//! ARCHBP-024 red tests: materialization, unsafe-overwrite refusal, and
//! rollback (a failed materialization leaves the target untouched).

use codedb_single_binary_export::{
    EMBEDDED_CHECKSUMS, EMBEDDED_MANIFEST, list_entries, materialize_embedded,
    materialize_pack,
};
use sha2::{Digest, Sha256};

#[test]
fn materialize_recreates_the_exact_tree_with_modes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("tree");
    let receipt = materialize_embedded(&target, false).expect("materialize");
    assert!(receipt.files_written >= 5);
    let entries = list_entries().expect("list");
    for entry in &entries {
        let restored = std::fs::read(target.join(&entry.path)).expect("restored bytes");
        assert_eq!(
            format!("{:x}", Sha256::digest(&restored)),
            entry.sha256,
            "{} must restore byte-exact",
            entry.path
        );
    }
}

#[test]
fn materialize_refuses_unsafe_overwrite_by_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("tree");
    std::fs::create_dir_all(&target).expect("pre-existing target");
    std::fs::write(target.join("sentinel.txt"), b"precious").expect("sentinel");
    let refused = materialize_embedded(&target, false);
    assert!(refused.is_err(), "a non-empty target must be refused by default");
    assert_eq!(
        std::fs::read(target.join("sentinel.txt")).expect("sentinel intact"),
        b"precious",
        "the refusal must not touch existing content"
    );
    // Explicit opt-in overwrites.
    materialize_embedded(&target, true).expect("explicit overwrite succeeds");
}

#[test]
fn failed_materialization_rolls_back_to_an_untouched_target() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("tree");
    // Corrupt pack: materialization must fail closed BEFORE the target
    // appears — verification precedes any write, and staging guarantees no
    // partial tree ever lands.
    let mut corrupted = codedb_single_binary_export::EMBEDDED_PACK.to_vec();
    corrupted[0] ^= 0xff;
    let result = materialize_pack(
        &corrupted,
        EMBEDDED_MANIFEST,
        EMBEDDED_CHECKSUMS,
        &target,
        false,
    );
    assert!(result.is_err());
    assert!(
        !target.exists(),
        "rollback: the target must not exist after a failed materialization"
    );
}

use std::path::Path;

use codedb_core::store::{
    atomic_materialize_file, materialize_symlink, platform_symlink_materialization_status,
    prepare_materialization_path, rollback_materialized_file, safe_materialization_path,
    take_materialized_file_rollback,
};
use codedb_core::{FilesystemEntryKind, SymlinkMaterializationStatus, scan_filesystem};
use sha2::{Digest, Sha256};

#[test]
fn accepts_only_normal_portable_relative_paths() {
    let root = Path::new("/tmp/codedb-output");
    assert_eq!(
        safe_materialization_path(root, "src/lib.rs").unwrap(),
        root.join("src/lib.rs")
    );

    for unsafe_path in [
        "",
        ".",
        "./src/lib.rs",
        "../escape",
        "src/../../escape",
        "/tmp/escape",
        "src//lib.rs",
        "src/../lib.rs",
        r"src\..\escape",
        r"C:\temp\escape",
        "nul\0byte",
    ] {
        assert!(
            safe_materialization_path(root, unsafe_path).is_err(),
            "unsafe path was accepted: {unsafe_path:?}"
        );
    }
}

#[test]
fn platform_symlink_status_matches_the_available_publication_primitive() {
    let expected = if cfg!(target_os = "linux") {
        SymlinkMaterializationStatus::Supported
    } else {
        SymlinkMaterializationStatus::MetadataOnlyFallback
    };
    assert_eq!(platform_symlink_materialization_status(), expected);
}

#[test]
fn unsupported_platform_symlink_materialization_is_deterministic_metadata_only() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let output_root = temporary_directory.path().join("output-root");

    let first = materialize_symlink(
        &output_root,
        "links/current",
        "../targets/current",
        SymlinkMaterializationStatus::MetadataOnlyFallback,
    )
    .expect("metadata-only fallback");
    let second = materialize_symlink(
        &output_root,
        "links/current",
        "../targets/current",
        SymlinkMaterializationStatus::MetadataOnlyFallback,
    )
    .expect("repeat metadata-only fallback");

    assert_eq!(first, second, "fallback reports must be deterministic");
    assert_eq!(
        first.status,
        SymlinkMaterializationStatus::MetadataOnlyFallback
    );
    assert_eq!(first.path, output_root.join("links/current"));
    assert_eq!(first.target, "../targets/current");
    assert!(!first.link_created);
    assert!(
        !output_root.exists(),
        "metadata-only fallback must not mutate the output tree"
    );

    let error = materialize_symlink(
        &output_root,
        "links/current",
        "../../outside",
        SymlinkMaterializationStatus::MetadataOnlyFallback,
    )
    .expect_err("escaping fallback target must be rejected");
    assert!(error.message().contains("escapes output root"));
    assert!(!output_root.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_supported_symlink_materialization_restores_link_metadata_and_target() {
    assert_eq!(
        platform_symlink_materialization_status(),
        SymlinkMaterializationStatus::Supported
    );
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let source_root = temporary_directory.path().join("source-root");
    std::fs::create_dir_all(source_root.join("links")).expect("source link parent");
    std::fs::create_dir_all(source_root.join("targets")).expect("source target parent");
    std::fs::write(source_root.join("targets/current"), b"restored target")
        .expect("source target bytes");
    std::os::unix::fs::symlink("../targets/current", source_root.join("links/current"))
        .expect("source symlink");
    let entries = scan_filesystem(&source_root).expect("scan source symlink metadata");
    let captured = entries
        .iter()
        .find(|entry| entry.relative_path == "links/current")
        .expect("captured symlink row");
    assert_eq!(captured.kind, FilesystemEntryKind::Symlink);
    assert!(captured.is_symlink);
    assert_eq!(
        captured.symlink_target.as_deref(),
        Some("../targets/current")
    );

    let output_root = temporary_directory.path().join("output-root");
    std::fs::create_dir_all(output_root.join("targets")).expect("target parent");
    std::fs::write(output_root.join("targets/current"), b"restored target").expect("target bytes");

    let report = materialize_symlink(
        &output_root,
        &captured.relative_path,
        captured.symlink_target.as_deref().expect("captured target"),
        SymlinkMaterializationStatus::Supported,
    )
    .expect("native symlink materialization");

    assert_eq!(report.status, SymlinkMaterializationStatus::Supported);
    assert_eq!(report.path, output_root.join("links/current"));
    assert_eq!(report.target, "../targets/current");
    assert!(report.link_created);
    assert!(
        std::fs::symlink_metadata(&report.path)
            .expect("symlink metadata")
            .file_type()
            .is_symlink(),
        "supported materialization must publish a symlink, not a regular file"
    );
    assert_eq!(
        std::fs::read_link(&report.path).expect("read link target"),
        Path::new("../targets/current")
    );
    assert_eq!(
        std::fs::read(&report.path).expect("follow restored link"),
        b"restored target"
    );

    let error = materialize_symlink(
        &output_root,
        &captured.relative_path,
        "../targets/replacement",
        SymlinkMaterializationStatus::Supported,
    )
    .expect_err("native symlink publication must not replace an existing entry");
    assert!(error.message().contains("no-replace"));
    assert_eq!(
        std::fs::read_link(&report.path).expect("original link remains"),
        Path::new("../targets/current")
    );
}

#[cfg(unix)]
#[test]
fn rejects_an_output_root_that_is_a_symlink() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let outside = temporary_directory.path().join("outside");
    let output_root = temporary_directory.path().join("output-root");
    std::fs::create_dir(&outside).expect("outside directory");
    std::os::unix::fs::symlink(&outside, &output_root).expect("output root symlink");

    let error =
        prepare_materialization_path(&output_root, "src/lib.rs").expect_err("symlink rejected");

    assert!(
        error.message().contains("symlink"),
        "unexpected error: {error}"
    );
    assert!(!outside.join("src/lib.rs").exists());
}

#[cfg(unix)]
#[test]
fn atomic_materialization_is_checksum_bound_durable_and_no_replace() {
    use std::os::unix::fs::PermissionsExt;

    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let output = temporary_directory.path().join("nested/bin/tool");
    let bytes = b"#!/bin/sh\nexit 0\n";
    let sha256 = format!("{:x}", Sha256::digest(bytes));

    let report = atomic_materialize_file(&output, bytes, &sha256, Some(0o755))
        .expect("first publication succeeds");
    assert_eq!(report.path, output);
    assert_eq!(report.sha256, sha256);
    assert_eq!(report.bytes, bytes.len() as u64);
    assert_eq!(
        std::fs::read(&output).expect("read materialized file"),
        bytes
    );
    assert_eq!(
        std::fs::metadata(&output)
            .expect("materialized metadata")
            .permissions()
            .mode()
            & 0o777,
        0o755
    );

    let replacement_sha256 = format!("{:x}", Sha256::digest(b"replacement"));
    let error = atomic_materialize_file(&output, b"replacement", &replacement_sha256, Some(0o600))
        .expect_err("existing destination must never be replaced");
    assert!(
        error.message().contains("exists") || error.message().contains("replace"),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read(&output).expect("original remains"),
        bytes,
        "failed no-replace publication must preserve the original"
    );
}

#[test]
fn checksum_mismatch_publishes_nothing_and_cleans_temporary_files() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let output = temporary_directory.path().join("nested/data.bin");

    let error = atomic_materialize_file(&output, b"actual", &"0".repeat(64), None)
        .expect_err("checksum mismatch must fail closed");
    assert!(
        error.message().contains("checksum"),
        "unexpected error: {error}"
    );
    assert!(!output.exists(), "corrupt bytes must never be published");
    let parent = output.parent().expect("output parent");
    if parent.exists() {
        let entries = std::fs::read_dir(parent)
            .expect("read output parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect output parent");
        assert!(entries.is_empty(), "temporary publication file leaked");
    }
}

#[cfg(unix)]
#[test]
fn atomic_materialization_rejects_symlink_components_without_escape() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let inside = temporary_directory.path().join("inside");
    let outside = temporary_directory.path().join("outside");
    std::fs::create_dir(&inside).expect("inside");
    std::fs::create_dir(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, inside.join("redirect")).expect("redirect symlink");
    let output = inside.join("redirect/escaped.txt");
    let bytes = b"must remain contained";
    let sha256 = format!("{:x}", Sha256::digest(bytes));

    let error = atomic_materialize_file(&output, bytes, &sha256, None)
        .expect_err("symlink path must fail closed");
    assert!(
        error.message().contains("symlink")
            || error.message().contains("descriptor")
            || error.message().contains("resolve"),
        "unexpected error: {error}"
    );
    assert!(
        !outside.join("escaped.txt").exists(),
        "materialization escaped through a symlink"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn identity_bound_rollback_removes_only_the_published_file() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let output = temporary_directory.path().join("nested/published.txt");
    let sibling = temporary_directory.path().join("nested/preserve.txt");
    let bytes = b"published";
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    atomic_materialize_file(&output, bytes, &sha256, None).expect("publish file");
    let rollback =
        take_materialized_file_rollback(&output).expect("retain exact publication identity");
    std::fs::write(&sibling, b"preserve").expect("write sibling");

    rollback_materialized_file(rollback).expect("remove exact published file");

    assert!(!output.exists());
    assert_eq!(std::fs::read(&sibling).expect("read sibling"), b"preserve");
}

#[cfg(target_os = "linux")]
#[test]
fn identity_bound_rollback_preserves_a_concurrent_replacement() {
    let temporary_directory = tempfile::tempdir().expect("temporary directory");
    let output = temporary_directory.path().join("nested/published.txt");
    let published = b"published by this attempt";
    let replacement = b"concurrent replacement";
    let sha256 = format!("{:x}", Sha256::digest(published));
    atomic_materialize_file(&output, published, &sha256, None).expect("publish file");
    let rollback =
        take_materialized_file_rollback(&output).expect("retain exact publication identity");

    std::fs::remove_file(&output).expect("remove original before deterministic replacement");
    std::fs::write(&output, replacement).expect("install concurrent replacement");

    let error = rollback_materialized_file(rollback)
        .expect_err("rollback must refuse a replacement with a different identity");

    assert!(
        error.message().contains("rollback identity conflict")
            && error.message().contains("residual"),
        "unexpected conflict error: {error}"
    );
    assert_eq!(
        std::fs::read(&output).expect("read preserved replacement"),
        replacement
    );
}

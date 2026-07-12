#![cfg(target_os = "linux")]

use codedb_core::store::ContainedDirectory;

#[test]
fn reads_regular_file_bytes_and_mode_from_one_contained_handle() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().expect("temporary root");
    let root = temporary.path().join("repo");
    std::fs::create_dir(&root).expect("repo root");
    let source = root.join("src/lib.rs");
    std::fs::create_dir(source.parent().expect("source parent")).expect("source parent");
    std::fs::write(&source, b"pub fn contained() {}\n").expect("source bytes");
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640)).expect("source mode");

    let contained = ContainedDirectory::open_existing(&root).expect("open root");
    let file = contained
        .read_regular_file("src/lib.rs")
        .expect("read contained file");

    assert_eq!(file.bytes, b"pub fn contained() {}\n");
    assert_eq!(file.unix_mode, Some(0o640));
}

#[test]
fn root_path_replacement_cannot_redirect_an_opened_containment_handle() {
    let temporary = tempfile::tempdir().expect("temporary root");
    let root = temporary.path().join("repo");
    let held = temporary.path().join("held-original");
    let outside = temporary.path().join("outside");
    std::fs::create_dir(&root).expect("repo root");
    std::fs::create_dir(&outside).expect("outside root");
    std::fs::write(root.join("value.txt"), b"inside").expect("inside value");
    std::fs::write(outside.join("value.txt"), b"outside-secret").expect("outside value");

    let contained = ContainedDirectory::open_existing(&root).expect("open root");
    std::fs::rename(&root, &held).expect("move original root");
    std::os::unix::fs::symlink(&outside, &root).expect("replace path with symlink");

    let file = contained
        .read_regular_file("value.txt")
        .expect("descriptor remains bound to original root");
    assert_eq!(file.bytes, b"inside");
    assert_ne!(file.bytes, b"outside-secret");
}

#[test]
fn final_and_ancestor_symlinks_are_rejected_without_reading_outside_bytes() {
    let temporary = tempfile::tempdir().expect("temporary root");
    let root = temporary.path().join("repo");
    let outside = temporary.path().join("outside");
    std::fs::create_dir(&root).expect("repo root");
    std::fs::create_dir(&outside).expect("outside root");
    std::fs::write(outside.join("secret.txt"), b"outside-secret").expect("outside secret");
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("final-link"))
        .expect("final link");
    std::os::unix::fs::symlink(&outside, root.join("ancestor-link")).expect("ancestor link");
    let contained = ContainedDirectory::open_existing(&root).expect("open root");

    for path in ["final-link", "ancestor-link/secret.txt"] {
        let error = contained
            .read_regular_file(path)
            .expect_err("symlink read must fail closed");
        assert!(
            error.message().contains("refused"),
            "unexpected error for {path}: {error}"
        );
    }
}

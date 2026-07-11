use std::path::Path;

use codedb_core::store::{prepare_materialization_path, safe_materialization_path};

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

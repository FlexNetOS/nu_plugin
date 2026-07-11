use std::path::Path;

use codedb_core::store::safe_materialization_path;

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

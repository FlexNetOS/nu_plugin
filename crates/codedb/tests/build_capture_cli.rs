use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn temp_root() -> PathBuf {
    static NEXT_TEMP_ROOT: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT_TEMP_ROOT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "codedb-build-cli-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("reserve unique build CLI fixture root");
    root
}

fn source_snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, current: &Path, rows: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(current)
            .expect("read source directory")
            .map(|entry| entry.expect("read source entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).expect("source metadata");
            if metadata.is_dir() {
                walk(root, &path, rows);
            } else if metadata.is_file() {
                rows.insert(
                    path.strip_prefix(root)
                        .expect("relative source path")
                        .to_path_buf(),
                    fs::read(path).expect("source bytes"),
                );
            }
        }
    }

    let mut rows = BTreeMap::new();
    walk(root, root, &mut rows);
    rows
}

#[test]
fn build_cli_temp_roots_are_atomically_unique() {
    let roots = (0..64)
        .map(|_| std::thread::spawn(temp_root))
        .map(|thread| thread.join().expect("create fixture root"))
        .collect::<Vec<_>>();
    let unique = roots.iter().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), roots.len());
    for root in roots {
        fs::remove_dir(root).expect("remove fixture root");
    }
}

#[test]
fn approved_cli_capture_persists_receipt_and_never_mutates_source() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let raw_log = evidence.join("capture.log");
    let store = evidence.join("capture.redb");
    fs::create_dir_all(repo.join("src")).expect("create fixture source");
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"codedb-cli-build-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n",
    )
    .expect("write manifest");
    fs::write(repo.join("src/lib.rs"), "pub fn value() -> u8 { 7 }\n").expect("write source");
    fs::write(
        repo.join("build.rs"),
        r#"fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out_dir.join("generated.rs"), b"pub const GENERATED: u8 = 9;\n")
        .expect("write generated source");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-link-lib=static=codedb_cli_fixture");
    println!("cargo:rustc-link-search=native=vendor/native");
    println!("cargo:rustc-link-arg=-Wl,--as-needed");
}
"#,
    )
    .expect("write build script");
    let before = source_snapshot(&repo);

    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB079",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("run approved capture build");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("capture JSON rows");
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("build_script_runs")
            && row.get("status").map(String::as_str) == Some("captured")
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("out_dir_artifacts")
            && row.get("relative_path").map(String::as_str) == Some("generated.rs")
            && row.get("sha256").is_some_and(|sha256| sha256.len() == 64)
    }));
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("native_link_facts")
            && row.get("fact_kind").map(String::as_str) == Some("linked_lib")
            && row.get("value").map(String::as_str) == Some("static=codedb_cli_fixture")
    }));
    let receipt = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("build_capture_receipts"))
        .expect("persisted build receipt row");
    assert_eq!(receipt.get("status").map(String::as_str), Some("persisted"));
    assert!(raw_log.is_file());
    assert!(store.is_file());
    assert_eq!(source_snapshot(&repo), before);

    let report = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "store-report",
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("read build receipt store");
    assert!(report.status.success());
    let report_rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&report.stdout).expect("store report JSON rows");
    assert!(report_rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("source_files")
            && row
                .get("key")
                .is_some_and(|key| key.starts_with("dynamic-build-captures/"))
            && row.get("value") == receipt.get("blob_ref")
    }));

    let artifact_dir = evidence.join("reproduced-out-dir");
    let reproduced = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "reproduce",
            "--approval-id",
            receipt.get("approval_id").expect("receipt approval id"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--artifact-dir",
            artifact_dir.to_str().expect("UTF-8 artifact path"),
            "--format",
            "json",
        ])
        .output()
        .expect("reproduce captured OUT_DIR");
    assert!(
        reproduced.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&reproduced.stderr)
    );
    let reproduction_rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&reproduced.stdout).expect("reproduction JSON rows");
    assert!(!reproduction_rows.is_empty());
    assert!(reproduction_rows.iter().all(|row| {
        row.get("table").map(String::as_str) == Some("out_dir_reproduction_proofs")
            && row.get("status").map(String::as_str) == Some("verified")
    }));
    assert_eq!(
        fs::read(artifact_dir.join("generated.rs")).expect("reproduced generated source"),
        b"pub const GENERATED: u8 = 9;\n"
    );

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn duplicate_out_dir_paths_require_an_unambiguous_package_selector() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let raw_log = evidence.join("capture.log");
    let store = evidence.join("capture.redb");
    fs::create_dir_all(&repo).expect("create workspace root");
    fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"3\"\n",
    )
    .expect("write workspace manifest");
    for (name, generated) in [("alpha", 11_u8), ("beta", 22_u8)] {
        let package = repo.join(name);
        fs::create_dir_all(package.join("src")).expect("create package source");
        fs::write(
            package.join("Cargo.toml"),
            format!(
                "[package]\nname = \"codedb-{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n"
            ),
        )
        .expect("write package manifest");
        fs::write(package.join("src/lib.rs"), "pub fn value() -> u8 { 1 }\n")
            .expect("write package source");
        fs::write(
            package.join("build.rs"),
            format!(
                "fn main() {{\n    let out = std::path::PathBuf::from(std::env::var_os(\"OUT_DIR\").unwrap());\n    std::fs::write(out.join(\"generated.rs\"), b\"pub const GENERATED: u8 = {generated};\\n\").unwrap();\n}}\n"
            ),
        )
        .expect("write package build script");
    }

    let captured = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB080",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("capture duplicate OUT_DIR fixture");
    assert!(
        captured.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&captured.stdout).expect("capture JSON rows");
    let receipt = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("build_capture_receipts"))
        .expect("capture receipt");
    let approval_id = receipt.get("approval_id").expect("approval id");
    let mut packages = rows
        .iter()
        .filter(|row| {
            row.get("table").map(String::as_str) == Some("out_dir_artifacts")
                && row.get("relative_path").map(String::as_str) == Some("generated.rs")
        })
        .map(|row| row.get("package_id").expect("artifact package id").clone())
        .collect::<Vec<_>>();
    packages.sort();
    packages.dedup();
    assert_eq!(packages.len(), 2, "both packages must emit generated.rs");

    let ambiguous_dir = evidence.join("ambiguous");
    let ambiguous = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "reproduce",
            "--approval-id",
            approval_id,
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--artifact-dir",
            ambiguous_dir.to_str().expect("UTF-8 artifact path"),
            "--format",
            "json",
        ])
        .output()
        .expect("refuse ambiguous reproduction");
    assert!(!ambiguous.status.success());
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr).contains("--package-id"),
        "stderr: {}",
        String::from_utf8_lossy(&ambiguous.stderr)
    );
    assert!(!ambiguous_dir.exists());

    for (index, package_id) in packages.iter().enumerate() {
        let artifact_dir = evidence.join(format!("selected-{index}"));
        let reproduced = Command::new(env!("CARGO_BIN_EXE_codedb"))
            .args([
                "reproduce",
                "--approval-id",
                approval_id,
                "--package-id",
                package_id,
                "--store",
                store.to_str().expect("UTF-8 store path"),
                "--artifact-dir",
                artifact_dir.to_str().expect("UTF-8 artifact path"),
                "--format",
                "json",
            ])
            .output()
            .expect("reproduce selected package");
        assert!(
            reproduced.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&reproduced.stderr)
        );
        let generated = fs::read_to_string(artifact_dir.join("generated.rs"))
            .expect("read selected generated artifact");
        if package_id.contains("codedb-alpha") {
            assert!(generated.contains("= 11;"));
        } else if package_id.contains("codedb-beta") {
            assert!(generated.contains("= 22;"));
        } else {
            panic!("unexpected package id: {package_id}");
        }
    }

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn one_package_with_duplicate_paths_requires_an_artifact_group_selector() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let raw_log = evidence.join("capture.log");
    let store = evidence.join("capture.redb");
    fs::create_dir_all(repo.join("shared/src")).expect("create shared source");
    fs::create_dir_all(repo.join("consumer/src")).expect("create consumer source");
    fs::write(
        repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"shared\", \"consumer\"]\ndefault-members = [\"consumer\"]\nresolver = \"3\"\n",
    )
    .expect("write workspace manifest");
    fs::write(
        repo.join("shared/Cargo.toml"),
        "[package]\nname = \"codedb-shared\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[features]\ndefault = []\nhost-unit = []\n",
    )
    .expect("write shared manifest");
    fs::write(
        repo.join("shared/build.rs"),
        r#"fn main() {
    let generated = if std::env::var_os("CARGO_FEATURE_HOST_UNIT").is_some() { 22 } else { 11 };
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("generated.rs"), format!("pub const GENERATED: u8 = {generated};\n")).unwrap();
}
"#,
    )
    .expect("write shared build script");
    fs::write(
        repo.join("shared/src/lib.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
    )
    .expect("write shared source");
    fs::write(
        repo.join("consumer/Cargo.toml"),
        "[package]\nname = \"codedb-consumer\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[dependencies]\ncodedb-shared = { path = \"../shared\" }\n\n[build-dependencies]\ncodedb-shared = { path = \"../shared\", features = [\"host-unit\"] }\n",
    )
    .expect("write consumer manifest");
    fs::write(
        repo.join("consumer/build.rs"),
        "fn main() { assert_eq!(codedb_shared::GENERATED, 22); }\n",
    )
    .expect("write consumer build script");
    fs::write(
        repo.join("consumer/src/lib.rs"),
        "pub fn generated() -> u8 { codedb_shared::GENERATED }\n",
    )
    .expect("write consumer source");

    let captured = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB080",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--format",
            "json",
        ])
        .output()
        .expect("capture dual-unit package fixture");
    assert!(
        captured.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&captured.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&captured.stdout).expect("capture JSON rows");
    let receipt = rows
        .iter()
        .find(|row| row.get("table").map(String::as_str) == Some("build_capture_receipts"))
        .expect("capture receipt");
    let approval_id = receipt.get("approval_id").expect("approval id");
    let shared_rows = rows
        .iter()
        .filter(|row| {
            row.get("table").map(String::as_str) == Some("out_dir_artifacts")
                && row.get("relative_path").map(String::as_str) == Some("generated.rs")
                && row
                    .get("package_id")
                    .is_some_and(|package_id| package_id.contains("codedb-shared"))
        })
        .collect::<Vec<_>>();
    let package_id = shared_rows
        .first()
        .and_then(|row| row.get("package_id"))
        .expect("shared package id")
        .clone();
    let out_dirs = shared_rows
        .iter()
        .map(|row| row.get("out_dir").expect("artifact OUT_DIR").clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        out_dirs.len(),
        2,
        "shared package must be compiled into two distinct OUT_DIR execution units"
    );

    let ambiguous_dir = evidence.join("ambiguous-shared");
    let ambiguous = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "reproduce",
            "--approval-id",
            approval_id,
            "--package-id",
            &package_id,
            "--store",
            store.to_str().expect("UTF-8 store path"),
            "--artifact-dir",
            ambiguous_dir.to_str().expect("UTF-8 artifact path"),
            "--format",
            "json",
        ])
        .output()
        .expect("refuse ambiguous same-package reproduction");
    assert!(!ambiguous.status.success());
    assert!(
        String::from_utf8_lossy(&ambiguous.stderr).contains("--artifact-group"),
        "stderr: {}",
        String::from_utf8_lossy(&ambiguous.stderr)
    );
    assert!(!ambiguous_dir.exists());

    let mut groups = shared_rows
        .iter()
        .map(|row| {
            row.get("artifact_group_id")
                .expect("artifact group id")
                .clone()
        })
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    assert_eq!(groups.len(), 2);
    let mut generated_values = std::collections::BTreeSet::new();
    for (index, group) in groups.iter().enumerate() {
        let artifact_dir = evidence.join(format!("selected-group-{index}"));
        let reproduced = Command::new(env!("CARGO_BIN_EXE_codedb"))
            .args([
                "reproduce",
                "--approval-id",
                approval_id,
                "--package-id",
                &package_id,
                "--artifact-group",
                group,
                "--store",
                store.to_str().expect("UTF-8 store path"),
                "--artifact-dir",
                artifact_dir.to_str().expect("UTF-8 artifact path"),
                "--format",
                "json",
            ])
            .output()
            .expect("reproduce selected artifact group");
        assert!(
            reproduced.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&reproduced.stderr)
        );
        generated_values.insert(
            fs::read_to_string(artifact_dir.join("generated.rs"))
                .expect("read selected generated artifact"),
        );
    }
    assert_eq!(generated_values.len(), 2);
    assert!(generated_values.iter().any(|value| value.contains("= 11;")));
    assert!(generated_values.iter().any(|value| value.contains("= 22;")));

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn approved_cli_capture_records_instrumented_proc_macro_tokens() {
    let root = temp_root();
    let repo = root.join("repo");
    let evidence = root.join("evidence");
    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/proc_macro_consumer");
    for relative in [
        "Cargo.toml",
        "crates/demo_macro/Cargo.toml",
        "crates/demo_macro/src/lib.rs",
        "crates/consumer/Cargo.toml",
        "crates/consumer/src/lib.rs",
    ] {
        let destination = repo.join(relative);
        fs::create_dir_all(destination.parent().expect("fixture parent"))
            .expect("create fixture parent");
        fs::copy(fixture_root.join(relative), destination).expect("copy proc-macro fixture");
    }
    let before = source_snapshot(&repo);
    let raw_log = evidence.join("capture.log");
    let output = Command::new(env!("CARGO_BIN_EXE_codedb"))
        .args([
            "capture",
            "build",
            repo.to_str().expect("UTF-8 repo path"),
            "--unsafe-execute-build",
            "--approver",
            "integration-test",
            "--task-id",
            "CDB078",
            "--before-state",
            "source-snapshot-recorded",
            "--cleanup-plan",
            "remove-isolated-sandbox",
            "--raw-log",
            raw_log.to_str().expect("UTF-8 raw log path"),
            "--format",
            "json",
        ])
        .output()
        .expect("run approved proc-macro capture");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: Vec<BTreeMap<String, String>> =
        serde_json::from_slice(&output.stdout).expect("capture JSON rows");
    assert!(rows.iter().any(|row| {
        row.get("table").map(String::as_str) == Some("proc_macro_invocations")
            && row.get("status").map(String::as_str) == Some("observed")
            && row.get("macro_name").map(String::as_str) == Some("demo_attr")
    }));
    for table in [
        "proc_macro_input_token_streams",
        "proc_macro_output_token_streams",
    ] {
        assert!(rows.iter().any(|row| {
            row.get("table").map(String::as_str) == Some(table)
                && row.get("sha256").is_some_and(|sha256| sha256.len() == 64)
        }));
    }
    assert_eq!(source_snapshot(&repo), before);
    assert!(raw_log.is_file());
    fs::remove_dir_all(root).expect("remove fixture");
}

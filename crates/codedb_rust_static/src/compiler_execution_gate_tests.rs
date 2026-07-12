use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CompilerEvidenceCollectionStatus, CompilerEvidenceOptions, CompilerExecutionApprovalAuthority,
    CompilerSandboxOptions, capture_compiler_evidence, capture_compiler_evidence_with_capability,
};

#[test]
fn boolean_and_request_shaped_data_cannot_execute_a_compiler() {
    let fixture = FixtureDirectory::new("boolean_refusal");
    let source = fixture.write("lib.rs", "pub fn visible() -> u32 { 42 }\n");
    let marker = fixture.path.join("compiler-executed");
    let fake_compiler = fake_compiler(&fixture.path, &marker);
    let options = CompilerEvidenceOptions {
        enabled: true,
        rustc: fake_compiler.clone(),
        rustdoc: fake_compiler,
        ..CompilerEvidenceOptions::default()
    };

    let report = capture_compiler_evidence(&source, options);

    assert_eq!(
        report.collection_status,
        CompilerEvidenceCollectionStatus::EvidenceUnavailable
    );
    assert!(report.artifacts.is_empty());
    assert!(report.semantic_hash.is_none());
    assert!(report.public_api_hash.is_none());
    assert!(!marker.exists(), "boolean-only request executed a compiler");
    assert!(gap_contains(&report, "opaque request-bound capability"));
}

#[test]
fn capability_is_bound_to_exact_options_and_source_bytes() {
    let fixture = FixtureDirectory::new("request_binding");
    let source = fixture.write("lib.rs", "pub fn visible() -> u32 { 1 }\n");
    let marker = fixture.path.join("compiler-executed");
    let fake_compiler = fake_compiler(&fixture.path, &marker);
    let options = CompilerEvidenceOptions {
        enabled: true,
        rustc: fake_compiler.clone(),
        rustdoc: fake_compiler,
        ..CompilerEvidenceOptions::default()
    };
    let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");

    let wrong_options_capability = authority
        .approve(&source, &options)
        .expect("request-bound capability");
    let mut changed_options = options.clone();
    changed_options.edition = "2021".to_string();
    let wrong_options = capture_compiler_evidence_with_capability(
        &authority,
        wrong_options_capability,
        &source,
        changed_options,
    );
    assert!(gap_contains(&wrong_options, "exact compiler request"));
    assert!(!marker.exists(), "wrong capability executed a compiler");

    let source_shift_capability = authority
        .approve(&source, &options)
        .expect("source-bound capability");
    fs::write(&source, "pub fn visible() -> u32 { 2 }\n").expect("shift source");
    let source_shift = capture_compiler_evidence_with_capability(
        &authority,
        source_shift_capability,
        &source,
        options,
    );
    assert!(gap_contains(&source_shift, "exact compiler request"));
    assert!(
        !marker.exists(),
        "source-shifted capability executed a compiler"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn missing_mandatory_sandbox_fails_closed_before_compiler_execution() {
    let fixture = FixtureDirectory::new("missing_sandbox");
    let source = fixture.write("lib.rs", "pub fn visible() -> u32 { 42 }\n");
    let marker = fixture.path.join("compiler-executed");
    let fake_compiler = fake_compiler(&fixture.path, &marker);
    let options = CompilerEvidenceOptions {
        enabled: true,
        rustc: fake_compiler.clone(),
        rustdoc: fake_compiler,
        sandbox: CompilerSandboxOptions {
            executable: PathBuf::from("/definitely/missing/codedb-bwrap"),
        },
        ..CompilerEvidenceOptions::default()
    };
    let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
    let capability = authority
        .approve(&source, &options)
        .expect("request-bound capability");

    let report =
        capture_compiler_evidence_with_capability(&authority, capability, &source, options);

    assert_eq!(
        report.collection_status,
        CompilerEvidenceCollectionStatus::EvidenceUnavailable
    );
    assert!(gap_contains(&report, "mandatory Linux sandbox"));
    assert!(
        !marker.exists(),
        "missing sandbox fell back to direct execution"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn capability_is_single_use_and_replay_fails_closed() {
    let fixture = FixtureDirectory::new("capability_replay");
    let source = fixture.write("lib.rs", "pub fn visible() -> u32 { 42 }\n");
    let options = CompilerEvidenceOptions {
        enabled: true,
        ..CompilerEvidenceOptions::default()
    };
    let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
    let capability = authority
        .approve(&source, &options)
        .expect("request-bound capability");
    let replay = capability.clone();

    let first =
        capture_compiler_evidence_with_capability(&authority, capability, &source, options.clone());
    assert_eq!(
        first.collection_status,
        CompilerEvidenceCollectionStatus::CompilerObserved,
        "authorized sandboxed compiler evidence unexpectedly failed: {:#?}",
        first.gaps
    );

    let second = capture_compiler_evidence_with_capability(&authority, replay, &source, options);
    assert_eq!(
        second.collection_status,
        CompilerEvidenceCollectionStatus::EvidenceUnavailable
    );
    assert!(gap_contains(&second, "already been used"));
    assert!(second.artifacts.is_empty());
    assert!(second.semantic_hash.is_none());
    assert!(second.public_api_hash.is_none());
}

fn gap_contains(report: &crate::CompilerEvidenceReport, needle: &str) -> bool {
    report.gaps.iter().any(|gap| gap.reason.contains(needle))
}

fn fake_compiler(root: &Path, marker: &Path) -> PathBuf {
    let script = root.join("fake-compiler");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf executed > '{}'\nexit 99\n",
            marker.display()
        ),
    )
    .expect("write fake compiler");
    let mut permissions = fs::metadata(&script)
        .expect("fake compiler metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("make fake compiler executable");
    script
}

struct FixtureDirectory {
    path: PathBuf,
}

impl FixtureDirectory {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codedb_rust_static_{label}_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir(&path).expect("create fixture directory");
        Self { path }
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::write(&path, contents).expect("write fixture");
        path
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

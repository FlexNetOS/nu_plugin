use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CompilerArtifactStatus, CompilerEvidenceArtifactKind, CompilerEvidenceCollectionStatus,
    CompilerEvidenceOptions, CompilerExecutionApprovalAuthority, CompilerExtern,
    CompilerExternKind, capture_compiler_evidence_with_capability,
};

#[test]
fn observes_real_proc_macro_and_pins_every_compiler_artifact() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/compiler_observed");
    let proc_macro_source = fixture_root.join("proc_macro/src/lib.rs");
    let consumer_template = fixture_root.join("consumer/src/lib.rs");
    let scratch = ScratchDirectory::new();
    let proc_macro_path = scratch.path.join(format!(
        "{}codedb_observed_proc_macro.{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_EXTENSION
    ));

    let rustc = CompilerEvidenceOptions::default().rustc;
    let compile = Command::new(&rustc)
        .args([
            "--crate-name",
            "codedb_observed_proc_macro",
            "--crate-type",
            "proc-macro",
            "--edition",
            "2021",
        ])
        .arg(&proc_macro_source)
        .arg("-o")
        .arg(&proc_macro_path)
        .output()
        .expect("configured rustc must be executable for the positive fixture");
    assert!(
        compile.status.success(),
        "proc-macro fixture compilation failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let consumer_source = scratch.path.join("consumer.rs");
    let consumer = fs::read_to_string(&consumer_template)
        .expect("read compiler-observed consumer template")
        .replace("__CODEDB_SOURCE__", &consumer_source.display().to_string())
        .replace("__CODEDB_EXTERN__", &proc_macro_path.display().to_string());
    fs::write(&consumer_source, consumer).expect("write bound compiler-observed consumer");

    let target = rustc_host(&rustc);
    let options = CompilerEvidenceOptions {
        enabled: true,
        edition: "2021".to_string(),
        crate_name: Some("codedb_observed_consumer".to_string()),
        target: Some(target.clone()),
        cfgs: vec!["codedb_compiler_observed".to_string()],
        features: vec!["fixture-feature".to_string()],
        externs: vec![CompilerExtern {
            name: "codedb_observed_proc_macro".to_string(),
            path: proc_macro_path,
            kind: CompilerExternKind::ProcMacro,
        }],
        ..CompilerEvidenceOptions::default()
    };
    let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
    let capability = authority
        .approve(&consumer_source, &options)
        .expect("request-bound compiler approval");
    let report = capture_compiler_evidence_with_capability(
        &authority,
        capability,
        &consumer_source,
        options.clone(),
    );

    assert_eq!(
        report.collection_status,
        CompilerEvidenceCollectionStatus::CompilerObserved,
        "positive fixture must not silently degrade: {:#?}",
        report.gaps
    );
    let toolchain = report.toolchain.as_ref().expect("toolchain provenance");
    assert!(toolchain.rustc_version.contains("commit-hash:"));
    assert!(toolchain.rustdoc_version.contains("commit-hash:"));
    assert!(!toolchain.toolchain_sha256.is_empty());

    let context = report
        .context
        .as_ref()
        .expect("compiler context provenance");
    assert_eq!(context.crate_name, "codedb_observed_consumer");
    assert_eq!(context.target, target);
    assert_eq!(context.cfgs, ["codedb_compiler_observed"]);
    assert_eq!(context.features, ["fixture-feature"]);
    assert!(
        context
            .compiler_cfg
            .iter()
            .any(|cfg| cfg == "codedb_compiler_observed")
    );
    assert!(
        context
            .compiler_cfg
            .iter()
            .any(|cfg| cfg == "feature=\"fixture-feature\"")
    );
    assert_eq!(context.externs.len(), 1);
    assert_eq!(context.externs[0].kind, CompilerExternKind::ProcMacro);
    assert!(context.externs[0].artifact_bytes > 0);
    assert_eq!(context.externs[0].artifact_sha256.len(), 64);
    assert_eq!(
        context.environment,
        [
            ("HOME".to_string(), "/homeless".to_string()),
            ("LANG".to_string(), "C".to_string()),
            ("LC_ALL".to_string(), "C".to_string()),
            ("SOURCE_DATE_EPOCH".to_string(), "1".to_string()),
            ("TMPDIR".to_string(), "/tmp".to_string()),
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(context.context_sha256.len(), 64);

    let expansion = report
        .artifact(CompilerEvidenceArtifactKind::MacroExpansion)
        .expect("macro expansion");
    assert_eq!(expansion.status, CompilerArtifactStatus::CompilerObserved);
    assert!(expansion.output.as_deref().is_some_and(|output| {
        output.contains("generated_by_macro_rules")
            && output.contains("generated_by_observed_proc_macro")
    }));
    let expansion_output = expansion.output.as_deref().expect("expansion output");
    for proof in [
        "CODEDB_SANDBOX_NETWORK_DENIED",
        "CODEDB_SANDBOX_HOME_HIDDEN",
        "CODEDB_SANDBOX_SOURCE_READ_ONLY",
        "CODEDB_SANDBOX_EXTERN_READ_ONLY",
    ] {
        assert!(
            expansion_output.lines().any(|line| {
                line.contains(&format!("pub const {proof}: bool")) && line.contains("true")
            }),
            "sandbox proof {proof} was not compiler observed: {expansion_output}"
        );
    }
    assert!(expansion
        .command
        .windows(2)
        .any(|args| args[0] == "--extern"
            && args[1].starts_with("codedb_observed_proc_macro=")));

    for kind in [
        CompilerEvidenceArtifactKind::MacroExpansion,
        CompilerEvidenceArtifactKind::MacroResolution,
        CompilerEvidenceArtifactKind::MacroHygiene,
        CompilerEvidenceArtifactKind::Hir,
        CompilerEvidenceArtifactKind::Mir,
        CompilerEvidenceArtifactKind::RustdocPublicApi,
    ] {
        let artifact = report.artifact(kind).expect("required artifact");
        assert_eq!(artifact.status, CompilerArtifactStatus::CompilerObserved);
        assert_eq!(
            artifact.context_sha256.as_deref(),
            Some(context.context_sha256.as_str())
        );
        assert_eq!(
            artifact.toolchain_sha256.as_deref(),
            Some(toolchain.toolchain_sha256.as_str())
        );
        assert_eq!(artifact.pin_sha256.as_deref().map(str::len), Some(64));
    }

    for kind in [
        CompilerEvidenceArtifactKind::Hir,
        CompilerEvidenceArtifactKind::Mir,
        CompilerEvidenceArtifactKind::RustdocPublicApi,
    ] {
        let output = report
            .artifact(kind)
            .and_then(|artifact| artifact.output.as_deref())
            .expect("pinned text artifact");
        assert!(
            output.contains("generated_by_observed_proc_macro"),
            "{kind:?} must contain compiler-observed proc-macro output"
        );
    }

    let repeated_capability = authority
        .approve(&consumer_source, &options)
        .expect("repeat request compiler approval");
    let repeated = capture_compiler_evidence_with_capability(
        &authority,
        repeated_capability,
        &consumer_source,
        options,
    );
    assert_eq!(
        repeated.collection_status,
        CompilerEvidenceCollectionStatus::CompilerObserved
    );
    assert_eq!(
        repeated
            .context
            .as_ref()
            .map(|context| context.context_sha256.as_str()),
        Some(context.context_sha256.as_str())
    );
    for kind in [
        CompilerEvidenceArtifactKind::Hir,
        CompilerEvidenceArtifactKind::Mir,
        CompilerEvidenceArtifactKind::RustdocPublicApi,
    ] {
        assert_eq!(
            repeated
                .artifact(kind)
                .and_then(|artifact| artifact.pin_sha256.as_deref()),
            report
                .artifact(kind)
                .and_then(|artifact| artifact.pin_sha256.as_deref()),
            "{kind:?} pin must be stable for an unchanged toolchain and context"
        );
    }
}

#[test]
fn unavailable_toolchain_fails_closed_with_an_operable_positive_path() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/compiler_observed");
    let source = fixture_root.join("consumer/src/lib.rs");
    let options = CompilerEvidenceOptions {
        enabled: true,
        rustc: PathBuf::from("/definitely/missing/codedb-rustc"),
        rustdoc: PathBuf::from("/definitely/missing/codedb-rustdoc"),
        ..CompilerEvidenceOptions::default()
    };
    let authority = CompilerExecutionApprovalAuthority::new().expect("approval authority");
    let capability = authority
        .approve(&source, &options)
        .expect("request-bound compiler approval");
    let report =
        capture_compiler_evidence_with_capability(&authority, capability, &source, options);

    assert_eq!(
        report.collection_status,
        CompilerEvidenceCollectionStatus::EvidenceUnavailable
    );
    assert!(report.semantic_hash.is_none());
    assert!(report.public_api_hash.is_none());
    assert!(report.artifacts.is_empty());
    assert!(report.context.is_none());
    assert!(
        report
            .operator_instructions
            .iter()
            .any(|line| line.contains("RUSTC=/absolute/path/to/nightly-rustc"))
    );
    assert!(
        report
            .operator_instructions
            .iter()
            .any(|line| line.contains("cargo test -p codedb-rust-static"))
    );
}

struct ScratchDirectory {
    path: PathBuf,
}

impl ScratchDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codedb_compiler_observed_test_{}_{}",
            std::process::id(),
            nonce
        ));
        fs::create_dir_all(&path).expect("create compiler-observed scratch directory");
        Self { path }
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn rustc_host(rustc: &PathBuf) -> String {
    let output = Command::new(rustc)
        .args(["--version", "--verbose"])
        .output()
        .expect("configured rustc must report its host");
    assert!(
        output.status.success(),
        "configured rustc --version --verbose failed"
    );
    String::from_utf8(output.stdout)
        .expect("rustc version output must be UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .expect("rustc verbose version must contain a host triple")
}

//! Trusted internal compiler/build broker.
//!
//! This broker is the sanctioned production entrypoint that turns
//! [`CompilerEvidenceOptions`] into positive, compiler-observed proof. It lives
//! *inside* `codedb-rust-static` on purpose: it must construct the crate-private
//! [`CompilerExecutionApprovalAuthority`] and call the crate-private
//! [`capture_compiler_evidence_with_capability`] executor. External crates
//! provably cannot do either — the `compile_fail` doctests on
//! [`capture_compiler_evidence`](crate::capture_compiler_evidence) enforce it.
//! The broker therefore adds no public API surface and does not weaken the
//! fail-closed library gate; it is a privileged internal consumer of the same
//! authorization + sandbox path the unit tests already exercise.
//!
//! It runs the real pinned `rustc`/`rustdoc` over each fixture through the
//! mandatory bubblewrap sandbox and writes a deterministic, re-runnable evidence
//! tree (HIR, MIR, rustdoc public-API JSON, macro expansion/resolution/hygiene,
//! each with its SHA-256 and full toolchain/context provenance). Fixtures whose
//! source cannot compile hermetically (build-script env injection, generated
//! `OUT_DIR` include) are recorded as explicit capability boundaries
//! (`capture_gap`) rather than faked or silently omitted.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    CompilerArtifactStatus, CompilerEvidenceArtifactKind, CompilerEvidenceCollectionStatus,
    CompilerEvidenceOptions, CompilerEvidenceReport, CompilerExecutionApprovalAuthority,
    CompilerExtern, CompilerExternKind, capture_compiler_evidence_with_capability,
};

/// One captured compiler artifact and its full pin provenance.
#[derive(Debug, Clone)]
pub(crate) struct BrokerArtifactRecord {
    pub kind: CompilerEvidenceArtifactKind,
    pub status: CompilerArtifactStatus,
    pub evidence_sha256: Option<String>,
    pub evidence_bytes: Option<usize>,
    pub context_sha256: Option<String>,
    pub pin_sha256: Option<String>,
    /// Relative path (under the fixture directory) of the written evidence file.
    pub file: Option<String>,
}

/// One fixture run through the broker.
#[derive(Debug, Clone)]
pub(crate) struct BrokerFixtureOutcome {
    pub name: String,
    pub source_relpath: String,
    pub edition: String,
    pub crate_name: String,
    pub collection_status: CompilerEvidenceCollectionStatus,
    pub context_sha256: Option<String>,
    pub artifacts: Vec<BrokerArtifactRecord>,
    /// Precise capability-boundary notes for fixtures that cannot compile
    /// hermetically. Empty for compiler-observed fixtures.
    pub capture_gaps: Vec<String>,
}

impl BrokerFixtureOutcome {
    pub(crate) fn artifact(
        &self,
        kind: CompilerEvidenceArtifactKind,
    ) -> Option<&BrokerArtifactRecord> {
        self.artifacts.iter().find(|artifact| artifact.kind == kind)
    }
}

/// Full broker run: toolchain provenance plus every fixture outcome.
#[derive(Debug, Clone)]
pub(crate) struct BrokerReport {
    pub output_dir: PathBuf,
    pub rustc_version: String,
    pub rustdoc_version: String,
    pub host: String,
    pub sysroot: String,
    pub toolchain_sha256: String,
    pub fixtures: Vec<BrokerFixtureOutcome>,
}

impl BrokerReport {
    pub(crate) fn fixture(&self, name: &str) -> Option<&BrokerFixtureOutcome> {
        self.fixtures.iter().find(|fixture| fixture.name == name)
    }
}

/// Repository root, derived from this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate manifest dir must have a grandparent repo root")
        .to_path_buf()
}

/// Declarative description of one broker fixture.
struct FixtureSpec {
    name: &'static str,
    /// Source file, relative to the repository root.
    source_relpath: &'static str,
    edition: &'static str,
    crate_name: &'static str,
    /// When set, a proc-macro whose source (relative to repo root) is compiled to
    /// a real dynamic library and supplied as `--extern <name>=...`.
    proc_macro: Option<ProcMacroSpec>,
    /// Human-readable reason the fixture is expected to hit a hermetic-compile
    /// capability boundary, if any. When present and capture fails closed, the
    /// broker records a precise `capture_gap` instead of treating it as an error.
    expected_boundary: Option<&'static str>,
}

struct ProcMacroSpec {
    extern_name: &'static str,
    source_relpath: &'static str,
    /// Placeholders in the consumer source, substituted with the canonical source
    /// and extern paths so the sandbox proofs bind to real inputs.
    source_placeholder: &'static str,
    extern_placeholder: &'static str,
}

/// Runs the broker over every fixture and writes the deterministic evidence tree
/// under `output_dir`. Returns the structured outcome for gating.
pub(crate) fn run_compiler_broker(output_dir: &Path) -> BrokerReport {
    let repo = repo_root();
    // Regenerate the evidence tree from scratch so runs are deterministic and
    // never carry stale files from an earlier toolchain.
    let _ = fs::remove_dir_all(output_dir);
    fs::create_dir_all(output_dir).expect("create broker output dir");
    // Scratch tree for compiled proc-macro externs and substituted consumers.
    // Kept at a fixed path (outside the evidence tree) so the evidence directory
    // stays clean and the request-bound context hashes remain deterministic.
    let build_root = std::env::temp_dir().join("codedb-compiler-broker");
    let _ = fs::remove_dir_all(&build_root);
    fs::create_dir_all(&build_root).expect("create broker build scratch");

    let specs = [
        FixtureSpec {
            name: "macro_rules",
            source_relpath: "fixtures/macro_rules/src/lib.rs",
            edition: "2021",
            crate_name: "codedb_fixture_macro_rules",
            proc_macro: None,
            expected_boundary: None,
        },
        FixtureSpec {
            name: "proc_macro",
            source_relpath: "crates/codedb_rust_static/fixtures/compiler_observed/consumer/src/lib.rs",
            edition: "2021",
            crate_name: "codedb_observed_consumer",
            proc_macro: Some(ProcMacroSpec {
                extern_name: "codedb_observed_proc_macro",
                source_relpath: "crates/codedb_rust_static/fixtures/compiler_observed/proc_macro/src/lib.rs",
                source_placeholder: "__CODEDB_SOURCE__",
                extern_placeholder: "__CODEDB_EXTERN__",
            }),
            expected_boundary: None,
        },
        FixtureSpec {
            name: "build_script",
            source_relpath: "fixtures/build_script/src/lib.rs",
            edition: "2021",
            crate_name: "codedb_fixture_build_script",
            proc_macro: None,
            expected_boundary: Some(
                "source reads `env!(\"CODEDB_FIXTURE_BUILD_SCRIPT\")`, which is injected only by \
                 Cargo build-script execution; the fail-closed hermetic rustc sandbox intentionally \
                 exposes no build-script environment, so pure compiler-observed HIR/MIR/rustdoc \
                 capture is out of scope here (build-script execution proof is owned by \
                 codedb-build-capture, CDB079).",
            ),
        },
        FixtureSpec {
            name: "out_dir",
            source_relpath: "fixtures/out_dir_generator/src/lib.rs",
            edition: "2021",
            crate_name: "codedb_fixture_out_dir_generator",
            proc_macro: None,
            expected_boundary: Some(
                "source does `include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"))`, which \
                 requires a Cargo build-script-generated OUT_DIR; the hermetic rustc sandbox \
                 provides no OUT_DIR and no generated file, so pure compiler-observed capture is \
                 out of scope here (OUT_DIR reproduction proof is owned by codedb-build-capture, \
                 CDB080).",
            ),
        },
    ];

    let authority = CompilerExecutionApprovalAuthority::new().expect("broker approval authority");

    let mut outcomes = Vec::new();
    let mut toolchain: Option<(String, String, String, String, String)> = None;
    for spec in &specs {
        let (outcome, report) = run_fixture(&repo, output_dir, &build_root, &authority, spec);
        if toolchain.is_none()
            && let Some(tc) = report.toolchain.as_ref()
        {
            toolchain = Some((
                tc.rustc_version.clone(),
                tc.rustdoc_version.clone(),
                tc.host.clone(),
                tc.sysroot.display().to_string(),
                tc.toolchain_sha256.clone(),
            ));
        }
        outcomes.push(outcome);
    }

    let (rustc_version, rustdoc_version, host, sysroot, toolchain_sha256) =
        toolchain.expect("at least one fixture must observe the toolchain");

    write_toolchain_json(
        output_dir,
        &rustc_version,
        &rustdoc_version,
        &host,
        &sysroot,
        &toolchain_sha256,
    );
    write_summary_json(output_dir, &host, &toolchain_sha256, &outcomes);

    BrokerReport {
        output_dir: output_dir.to_path_buf(),
        rustc_version,
        rustdoc_version,
        host,
        sysroot,
        toolchain_sha256,
        fixtures: outcomes,
    }
}

/// Runs a single fixture through the authorized sandbox lane and writes its
/// evidence directory. Returns the structured outcome and the raw report.
fn run_fixture(
    repo: &Path,
    output_dir: &Path,
    build_root: &Path,
    authority: &CompilerExecutionApprovalAuthority,
    spec: &FixtureSpec,
) -> (BrokerFixtureOutcome, CompilerEvidenceReport) {
    let fixture_dir = output_dir.join(spec.name);
    fs::create_dir_all(&fixture_dir).expect("create fixture evidence dir");

    // Resolve the exact source compiled for this fixture. Proc-macro fixtures
    // first build the macro dynamic library, then bind its real path + SHA-256
    // into the consumer source so the sandbox proofs are genuine.
    let (source_path, externs) = match spec.proc_macro.as_ref() {
        None => (repo.join(spec.source_relpath), Vec::new()),
        Some(pm) => {
            let scratch = build_root.join(spec.name);
            fs::create_dir_all(&scratch).expect("create proc-macro build scratch");
            let so_path = scratch.join(format!(
                "{}{}.{}",
                std::env::consts::DLL_PREFIX,
                pm.extern_name,
                std::env::consts::DLL_EXTENSION
            ));
            compile_proc_macro_extern(&repo.join(pm.source_relpath), pm.extern_name, &so_path);

            let consumer_out = scratch.join("consumer.rs");
            let template = fs::read_to_string(repo.join(spec.source_relpath))
                .expect("read proc-macro consumer template");
            let bound = template
                .replace(pm.source_placeholder, &consumer_out.display().to_string())
                .replace(pm.extern_placeholder, &so_path.display().to_string());
            fs::write(&consumer_out, bound).expect("write bound consumer source");
            (
                consumer_out,
                vec![CompilerExtern {
                    name: pm.extern_name.to_string(),
                    path: so_path,
                    kind: CompilerExternKind::ProcMacro,
                }],
            )
        }
    };

    let options = CompilerEvidenceOptions {
        enabled: true,
        edition: spec.edition.to_string(),
        crate_name: Some(spec.crate_name.to_string()),
        externs,
        ..CompilerEvidenceOptions::default()
    };

    let capability = authority
        .approve(&source_path, &options)
        .expect("broker request-bound approval");
    let report =
        capture_compiler_evidence_with_capability(authority, capability, &source_path, options);

    let outcome = write_fixture_evidence(&fixture_dir, spec, &source_path, &report);
    (outcome, report)
}

/// Compiles a proc-macro source file to a real dynamic library using the pinned
/// `rustc`. This trusted extern is prepared by the broker, not the sandboxed
/// observation lane; its bytes are hashed before the observed compile runs.
fn compile_proc_macro_extern(source: &Path, crate_name: &str, out_path: &Path) {
    let rustc = CompilerEvidenceOptions::default().rustc;
    let output = Command::new(&rustc)
        .args([
            "--crate-name",
            crate_name,
            "--crate-type",
            "proc-macro",
            "--edition",
            "2021",
        ])
        .arg(source)
        .arg("-o")
        .arg(out_path)
        .output()
        .expect("pinned rustc must be executable to build the proc-macro extern");
    assert!(
        output.status.success(),
        "proc-macro extern build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Writes every produced artifact plus a per-fixture `manifest.json`, returning
/// the structured outcome.
fn write_fixture_evidence(
    fixture_dir: &Path,
    spec: &FixtureSpec,
    source_path: &Path,
    report: &CompilerEvidenceReport,
) -> BrokerFixtureOutcome {
    let mut records = Vec::new();
    for artifact in &report.artifacts {
        let file = if let Some(payload) = artifact.payload.as_ref() {
            let name = artifact_file_name(artifact.kind);
            fs::write(fixture_dir.join(&name), payload.as_bytes())
                .expect("write artifact evidence");
            if let Some(sha) = artifact.evidence_sha256.as_ref() {
                fs::write(
                    fixture_dir.join(format!("{name}.sha256")),
                    format!("{sha}\n"),
                )
                .expect("write artifact sha sidecar");
            }
            Some(name)
        } else {
            None
        };
        records.push(BrokerArtifactRecord {
            kind: artifact.kind,
            status: artifact.status,
            evidence_sha256: artifact.evidence_sha256.clone(),
            evidence_bytes: artifact.evidence_bytes,
            context_sha256: artifact.context_sha256.clone(),
            pin_sha256: artifact.pin_sha256.clone(),
            file,
        });
    }

    // Capability boundaries: record the broker's precise reason plus every gap
    // the executor reported. Never fake artifacts for a failed-closed fixture.
    let mut capture_gaps = Vec::new();
    if report.collection_status != CompilerEvidenceCollectionStatus::CompilerObserved {
        if let Some(boundary) = spec.expected_boundary {
            capture_gaps.push(boundary.to_string());
        }
        for gap in &report.gaps {
            let scope = gap
                .artifact
                .map(|kind| format!("{}: ", kind.as_str()))
                .unwrap_or_default();
            capture_gaps.push(format!("{scope}{}", gap.reason));
        }
        if capture_gaps.is_empty() {
            capture_gaps.push("evidence unavailable; see manifest.json".to_string());
        }
    }

    let outcome = BrokerFixtureOutcome {
        name: spec.name.to_string(),
        source_relpath: spec.source_relpath.to_string(),
        edition: spec.edition.to_string(),
        crate_name: spec.crate_name.to_string(),
        collection_status: report.collection_status,
        context_sha256: report.context.as_ref().map(|c| c.context_sha256.clone()),
        artifacts: records,
        capture_gaps,
    };

    write_fixture_manifest(fixture_dir, &outcome, source_path, report);
    outcome
}

fn artifact_file_name(kind: CompilerEvidenceArtifactKind) -> String {
    let ext = match kind {
        CompilerEvidenceArtifactKind::MacroResolution => "rmeta",
        CompilerEvidenceArtifactKind::RustdocPublicApi => "json",
        _ => "txt",
    };
    format!("{}.{ext}", kind.as_str())
}

// ----- deterministic JSON emission (no serde dependency) -----

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn json_str(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn json_str_array(values: &[String]) -> String {
    if values.is_empty() {
        return "[]".to_string();
    }
    let items = values
        .iter()
        .map(|v| format!("    {}", json_str(v)))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("[\n{items}\n  ]")
}

fn write_fixture_manifest(
    fixture_dir: &Path,
    outcome: &BrokerFixtureOutcome,
    source_path: &Path,
    report: &CompilerEvidenceReport,
) {
    let mut lines = Vec::new();
    lines.push("{".to_string());
    lines.push(format!("  \"fixture\": {},", json_str(&outcome.name)));
    lines.push(format!(
        "  \"source_relpath\": {},",
        json_str(&outcome.source_relpath)
    ));
    lines.push(format!(
        "  \"observed_source_path\": {},",
        json_str(&source_path.display().to_string())
    ));
    lines.push(format!(
        "  \"source_sha256\": {},",
        json_str(report.source_sha256.as_deref().unwrap_or(""))
    ));
    lines.push(format!("  \"edition\": {},", json_str(&outcome.edition)));
    lines.push(format!(
        "  \"crate_name\": {},",
        json_str(&outcome.crate_name)
    ));
    lines.push(format!(
        "  \"collection_status\": {},",
        json_str(collection_status_str(outcome.collection_status))
    ));
    if let Some(context) = report.context.as_ref() {
        lines.push(format!("  \"target\": {},", json_str(&context.target)));
        lines.push(format!(
            "  \"context_sha256\": {},",
            json_str(&context.context_sha256)
        ));
        lines.push(format!("  \"cfgs\": {},", json_str_array(&context.cfgs)));
        lines.push(format!(
            "  \"features\": {},",
            json_str_array(&context.features)
        ));
        lines.push(format!(
            "  \"compiler_cfg\": {},",
            json_str_array(&context.compiler_cfg)
        ));
        let env_lines = context
            .environment
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>();
        lines.push(format!(
            "  \"environment\": {},",
            json_str_array(&env_lines)
        ));
        let externs = context
            .externs
            .iter()
            .map(|e| {
                format!(
                    "{} kind={} sha256={} bytes={}",
                    e.name,
                    e.kind.as_str(),
                    e.artifact_sha256,
                    e.artifact_bytes
                )
            })
            .collect::<Vec<_>>();
        lines.push(format!("  \"externs\": {},", json_str_array(&externs)));
    }
    lines.push(format!(
        "  \"semantic_hash\": {},",
        json_str(report.semantic_hash.as_deref().unwrap_or(""))
    ));
    lines.push(format!(
        "  \"public_api_hash\": {},",
        json_str(report.public_api_hash.as_deref().unwrap_or(""))
    ));

    // Artifacts array.
    let mut artifact_objs = Vec::new();
    for artifact in &report.artifacts {
        let record = outcome
            .artifact(artifact.kind)
            .expect("every reported artifact has a record");
        let obj = format!(
            "    {{\n      \"kind\": {},\n      \"status\": {},\n      \"evidence_sha256\": {},\n      \"evidence_bytes\": {},\n      \"context_sha256\": {},\n      \"pin_sha256\": {},\n      \"file\": {},\n      \"command\": {}\n    }}",
            json_str(artifact.kind.as_str()),
            json_str(artifact_status_str(artifact.status)),
            json_str(record.evidence_sha256.as_deref().unwrap_or("")),
            record
                .evidence_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "null".to_string()),
            json_str(record.context_sha256.as_deref().unwrap_or("")),
            json_str(record.pin_sha256.as_deref().unwrap_or("")),
            json_str(record.file.as_deref().unwrap_or("")),
            json_str(&artifact.command.join(" ")),
        );
        artifact_objs.push(obj);
    }
    let artifacts = if artifact_objs.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{}\n  ]", artifact_objs.join(",\n"))
    };
    lines.push(format!("  \"artifacts\": {artifacts},"));
    lines.push(format!(
        "  \"capture_gaps\": {}",
        json_str_array(&outcome.capture_gaps)
    ));
    lines.push("}".to_string());

    fs::write(
        fixture_dir.join("manifest.json"),
        format!("{}\n", lines.join("\n")),
    )
    .expect("write fixture manifest");

    if !outcome.capture_gaps.is_empty() {
        fs::write(
            fixture_dir.join("capture_gap.json"),
            format!(
                "{{\n  \"fixture\": {},\n  \"collection_status\": {},\n  \"capture_gaps\": {}\n}}\n",
                json_str(&outcome.name),
                json_str(collection_status_str(outcome.collection_status)),
                json_str_array(&outcome.capture_gaps),
            ),
        )
        .expect("write capture gap");
    }
}

fn write_toolchain_json(
    output_dir: &Path,
    rustc_version: &str,
    rustdoc_version: &str,
    host: &str,
    sysroot: &str,
    toolchain_sha256: &str,
) {
    let body = format!(
        "{{\n  \"rustc_version\": {},\n  \"rustdoc_version\": {},\n  \"host\": {},\n  \"sysroot\": {},\n  \"toolchain_sha256\": {}\n}}\n",
        json_str(rustc_version),
        json_str(rustdoc_version),
        json_str(host),
        json_str(sysroot),
        json_str(toolchain_sha256),
    );
    fs::write(output_dir.join("toolchain.json"), body).expect("write toolchain.json");
}

fn write_summary_json(
    output_dir: &Path,
    host: &str,
    toolchain_sha256: &str,
    outcomes: &[BrokerFixtureOutcome],
) {
    let generated = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut fixtures = Vec::new();
    for outcome in outcomes {
        let observed = outcome
            .artifacts
            .iter()
            .filter(|a| a.status == CompilerArtifactStatus::CompilerObserved)
            .map(|a| a.kind.as_str().to_string())
            .collect::<Vec<_>>();
        let obj = format!(
            "    {{\n      \"fixture\": {},\n      \"collection_status\": {},\n      \"observed_artifacts\": {},\n      \"capture_gaps\": {}\n    }}",
            json_str(&outcome.name),
            json_str(collection_status_str(outcome.collection_status)),
            json_str_array(&observed),
            json_str_array(&outcome.capture_gaps),
        );
        fixtures.push(obj);
    }
    let fixtures = if fixtures.is_empty() {
        "[]".to_string()
    } else {
        format!("[\n{}\n  ]", fixtures.join(",\n"))
    };
    let body = format!(
        "{{\n  \"generated_unix\": {generated},\n  \"host\": {},\n  \"toolchain_sha256\": {},\n  \"fixtures\": {fixtures}\n}}\n",
        json_str(host),
        json_str(toolchain_sha256),
    );
    fs::write(output_dir.join("SUMMARY.json"), body).expect("write SUMMARY.json");
}

fn collection_status_str(status: CompilerEvidenceCollectionStatus) -> &'static str {
    match status {
        CompilerEvidenceCollectionStatus::CompilerObserved => "compiler_observed",
        CompilerEvidenceCollectionStatus::EvidenceUnavailable => "evidence_unavailable",
    }
}

fn artifact_status_str(status: CompilerArtifactStatus) -> &'static str {
    match status {
        CompilerArtifactStatus::CompilerObserved => "compiler_observed",
        CompilerArtifactStatus::EvidenceUnavailable => "evidence_unavailable",
    }
}

fn evidence_tree_snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(
        root: &Path,
        directory: &Path,
        snapshot: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
    ) {
        if !directory.exists() {
            return;
        }
        let mut entries = fs::read_dir(directory)
            .expect("read evidence snapshot directory")
            .map(|entry| entry.expect("read evidence snapshot entry"))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("evidence path must remain beneath snapshot root")
                .to_path_buf();
            if entry
                .file_type()
                .expect("inspect evidence snapshot entry")
                .is_dir()
            {
                visit(root, &path, snapshot);
            } else {
                snapshot.insert(
                    relative,
                    fs::read(path).expect("read evidence snapshot file"),
                );
            }
        }
    }

    let mut snapshot = std::collections::BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

struct TemporaryBrokerOutput(PathBuf);

impl TemporaryBrokerOutput {
    fn new() -> Self {
        let sequence = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("broker test clock")
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "codedb-compiler-broker-test-{}-{sequence}",
            std::process::id()
        )))
    }
}

impl Drop for TemporaryBrokerOutput {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn broker_captures_pinned_hir_mir_rustdoc_for_macro_rules_and_proc_macro() {
    let tracked_output_dir = repo_root().join("logs/compiler-observed");
    let tracked_before = evidence_tree_snapshot(&tracked_output_dir);
    let temporary_output = TemporaryBrokerOutput::new();
    let output_dir = temporary_output.0.clone();
    let report = run_compiler_broker(&output_dir);

    assert!(
        report.rustc_version.contains("commit-hash:"),
        "broker must record exact rustc identity: {:?}",
        report.rustc_version
    );
    assert!(
        report.rustdoc_version.contains("commit-hash:"),
        "broker must record exact rustdoc identity"
    );
    assert_eq!(report.toolchain_sha256.len(), 64);
    assert_eq!(report.output_dir, output_dir);
    assert!(
        !report.host.is_empty(),
        "broker must record the host triple"
    );
    assert!(!report.sysroot.is_empty(), "broker must record the sysroot");
    assert!(output_dir.join("toolchain.json").is_file());
    assert!(output_dir.join("SUMMARY.json").is_file());
    assert_eq!(
        evidence_tree_snapshot(&tracked_output_dir),
        tracked_before,
        "ordinary broker tests must never regenerate tracked evidence"
    );

    println!("codedb compiler/build broker");
    println!(
        "  rustc:     {}",
        report.rustc_version.lines().next().unwrap_or("")
    );
    println!(
        "  rustdoc:   {}",
        report.rustdoc_version.lines().next().unwrap_or("")
    );
    println!("  host:      {}", report.host);
    println!("  sysroot:   {}", report.sysroot);
    println!("  toolchain: sha256:{}", report.toolchain_sha256);
    println!("  evidence:  {}", report.output_dir.display());
    for fixture in &report.fixtures {
        println!(
            "  fixture {:<12} status={}",
            fixture.name,
            collection_status_str(fixture.collection_status)
        );
        for artifact in &fixture.artifacts {
            println!(
                "    {:<20} status={:<20} bytes={:<8} pin={}",
                artifact.kind.as_str(),
                artifact_status_str(artifact.status),
                artifact
                    .evidence_bytes
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                artifact.pin_sha256.as_deref().unwrap_or("-")
            );
        }
        for gap in &fixture.capture_gaps {
            println!("    capture_gap: {gap}");
        }
    }

    for name in ["macro_rules", "proc_macro"] {
        let fixture = report
            .fixture(name)
            .unwrap_or_else(|| panic!("broker must run the {name} fixture"));
        assert_eq!(
            fixture.collection_status,
            CompilerEvidenceCollectionStatus::CompilerObserved,
            "{name} fixture must be compiler-observed, not degraded"
        );
        assert_eq!(
            fixture.context_sha256.as_deref().map(str::len),
            Some(64),
            "{name} fixture must carry a bound context pin"
        );
        for kind in [
            CompilerEvidenceArtifactKind::Hir,
            CompilerEvidenceArtifactKind::Mir,
            CompilerEvidenceArtifactKind::RustdocPublicApi,
        ] {
            let artifact = fixture
                .artifact(kind)
                .unwrap_or_else(|| panic!("{name}/{kind:?} artifact must be present"));
            assert_eq!(
                artifact.status,
                CompilerArtifactStatus::CompilerObserved,
                "{name}/{kind:?} must be compiler-observed"
            );
            assert!(
                artifact.evidence_bytes.unwrap_or(0) > 0,
                "{name}/{kind:?} evidence must be non-empty"
            );
            assert_eq!(
                artifact.evidence_sha256.as_deref().map(str::len),
                Some(64),
                "{name}/{kind:?} must carry a SHA-256"
            );
            assert_eq!(
                artifact.pin_sha256.as_deref().map(str::len),
                Some(64),
                "{name}/{kind:?} must carry a pin"
            );
            let relpath = artifact
                .file
                .as_deref()
                .unwrap_or_else(|| panic!("{name}/{kind:?} must be written to disk"));
            let written = output_dir.join(name).join(relpath);
            assert!(
                written.is_file() && fs::metadata(&written).map(|m| m.len()).unwrap_or(0) > 0,
                "{name}/{kind:?} evidence file must exist and be non-empty: {}",
                written.display()
            );
        }
        let resolution = fixture
            .artifact(CompilerEvidenceArtifactKind::MacroResolution)
            .expect("macro resolution artifact");
        let resolution_file = resolution
            .file
            .as_deref()
            .expect("macro resolution artifact must be written");
        assert!(
            resolution_file.ends_with(".rmeta")
                && output_dir.join(name).join(resolution_file).is_file(),
            "macro resolution must be retained as a binary rmeta artifact"
        );
    }

    // The build-script and OUT_DIR fixtures cannot compile hermetically through
    // the fail-closed rustc sandbox. The broker must fail closed and record a
    // precise capture_gap rather than fabricate compiler-observed artifacts.
    for name in ["build_script", "out_dir"] {
        let fixture = report
            .fixture(name)
            .unwrap_or_else(|| panic!("broker must run the {name} fixture"));
        assert_eq!(
            fixture.collection_status,
            CompilerEvidenceCollectionStatus::EvidenceUnavailable,
            "{name} fixture must fail closed at the hermetic-compile boundary"
        );
        assert!(
            !fixture.capture_gaps.is_empty(),
            "{name} boundary must be recorded as a capture_gap"
        );
        assert!(
            fixture.artifacts.iter().all(|a| a.status
                == CompilerArtifactStatus::EvidenceUnavailable
                && a.file.is_none()),
            "{name} must not fake any compiler-observed artifact"
        );
        assert!(
            output_dir.join(name).join("capture_gap.json").is_file(),
            "{name} capture_gap.json must be written"
        );
    }
}

/// Explicit maintenance entrypoint for the checked compiler evidence tree.
/// Ordinary `cargo test` skips this mutating operation.
#[test]
#[ignore = "explicit tracked-evidence regeneration; run only during final proof sealing"]
fn regenerate_tracked_compiler_evidence() {
    let output_dir = repo_root().join("logs/compiler-observed");
    let report = run_compiler_broker(&output_dir);
    assert_eq!(report.output_dir, output_dir);
    assert!(report.output_dir.join("SUMMARY.json").is_file());
}

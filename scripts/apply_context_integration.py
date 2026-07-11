#!/usr/bin/env python3
"""One-way, idempotent migration for the runtime-context integration slice.

The host command launcher is broken before repository processes start. CI applies
these exact asserted edits, tests them, and commits the resulting product source.
The assertions intentionally fail closed if any source anchor drifts.
"""

from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected exactly one integration anchor")
    target.write_text(text.replace(old, new, 1))


def remove_once(path: str, old: str) -> None:
    target = Path(path)
    text = target.read_text()
    if old not in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected exactly one removal anchor")
    target.write_text(text.replace(old, "", 1))


def cut_between(path: str, start: str, end: str) -> None:
    target = Path(path)
    text = target.read_text()
    if start not in text:
        return
    if text.count(start) != 1 or text.count(end) != 1:
        raise SystemExit(f"{path}: deletion anchors drifted")
    before, rest = text.split(start, 1)
    _removed, after = rest.split(end, 1)
    target.write_text(before + end + after)


def append_host_detector() -> None:
    path = Path("crates/codedb_context/src/lib.rs")
    text = path.read_text()
    if "pub fn detect_host_triple_with_runner" in text:
        return
    anchor = "fn sha256_hex(bytes: &[u8]) -> String {\n    format!(\"{:x}\", Sha256::digest(bytes))\n}\n"
    if text.count(anchor) != 1:
        raise SystemExit("codedb_context anchor drifted")
    path.write_text(
        text
        + r'''

/// Detect the active rustc host triple for callers that want a host-target
/// context without inventing or hard-coding a target.
pub fn detect_host_triple() -> Result<String, ContextError> {
    detect_host_triple_with_runner(&SystemCommandRunner, Path::new("."))
}

/// Testable host-triple detector using the same command boundary as full
/// context capture.
pub fn detect_host_triple_with_runner<R: CommandRunner + ?Sized>(
    runner: &R,
    current_dir: &Path,
) -> Result<String, ContextError> {
    let rustc_verbose = checked_output(runner, "rustc", &["-vV".to_string()], current_dir)?;
    rustc_verbose
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(ContextError::MissingHostTriple)
}
'''
    )


def add_manifest_dependencies() -> None:
    for path in [
        "crates/codedb/Cargo.toml",
        "crates/codedb_cargo/Cargo.toml",
        "crates/codedb_mcp/Cargo.toml",
        "crates/nu_plugin_codedb/Cargo.toml",
    ]:
        replace_once(path, "[dependencies]\n", "[dependencies]\ncodedb-context.workspace = true\n")


def migrate_cargo_projector() -> None:
    path = "crates/codedb_cargo/src/lib.rs"
    remove_once(path, "use std::io;\nuse std::path::{Path, PathBuf};\nuse std::process::Command;\n\n")
    remove_once(path, "use sha2::{Digest, Sha256};\n")
    cut_between(
        path,
        "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct CargoContextCapture",
        "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct CargoWorkspaceRow",
    )
    replace_once(
        path,
        """#[derive(Debug)]
pub enum CargoMetadataError {
    NonUtf8Path { path: PathBuf },
    Spawn { source: io::Error },
    Failed { status: i32, stderr: String },
    Parse { source: serde_json::Error },
}

impl Display for CargoMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonUtf8Path { path } => {
                write!(f, "path is not valid UTF-8: {}", path.display())
            }
            Self::Spawn { source } => write!(f, "failed to run cargo metadata: {source}"),
            Self::Failed { status, stderr } => {
                write!(f, "cargo metadata exited with status {status}: {stderr}")
            }
            Self::Parse { source } => write!(f, "failed to parse cargo metadata JSON: {source}"),
        }
    }
}
""",
        """#[derive(Debug)]
pub enum CargoMetadataError {
    Parse { source: serde_json::Error },
}

impl Display for CargoMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse { source } => write!(f, "failed to parse cargo metadata JSON: {source}"),
        }
    }
}
""",
    )
    start = "pub fn capture_cargo_metadata(\n"
    end = "#[derive(Debug, Deserialize)]\nstruct Metadata"
    target = Path(path)
    text = target.read_text()
    if start in text:
        if text.count(start) != 1 or text.count(end) != 1:
            raise SystemExit("codedb_cargo capture anchors drifted")
        before, rest = text.split(start, 1)
        _old, after = rest.split(end, 1)
        projector = """pub fn capture_cargo_metadata_json(
    metadata_json: &str,
) -> Result<CargoMetadataCapture, CargoMetadataError> {
    let metadata: Metadata = serde_json::from_str(metadata_json)
        .map_err(|source| CargoMetadataError::Parse { source })?;
    Ok(metadata.into_capture())
}

"""
        target.write_text(before + projector + end + after)
    cut_between(path, "pub fn build_context_rows", "fn sorted")
    cut_between(
        path,
        "    // Defends: CDB021 context rows must be keyed and deterministic across input ordering.\n",
        "    struct FixtureWorkspace",
    )
    replace_once(path, "    use std::fs;\n", "    use std::fs;\n    use std::path::PathBuf;\n")
    target = Path(path)
    text = target.read_text()
    test_start = "    #[test]\n    fn cargo_metadata_fixture_capture_is_stable() {"
    classifier = "    // Defends: CDB020 must classify registry, git, and path provenance without network mutation."
    if test_start in text:
        before, rest = text.split(test_start, 1)
        _old, after = rest.split(classifier, 1)
        new_test = r'''    #[test]
    fn captured_metadata_json_projection_is_stable() {
        let json = r#"{
          "packages": [{
            "id": "path+file:///fixture#codedb_fixture@0.1.0",
            "name": "codedb_fixture",
            "version": "0.1.0",
            "source": null,
            "manifest_path": "/fixture/Cargo.toml",
            "edition": "2024",
            "targets": [{
              "name": "codedb_fixture",
              "kind": ["lib"],
              "crate_types": ["lib"],
              "src_path": "/fixture/src/lib.rs",
              "edition": "2024"
            }],
            "dependencies": [],
            "features": {"default": ["serde"], "serde": []}
          }],
          "workspace_members": ["path+file:///fixture#codedb_fixture@0.1.0"],
          "workspace_root": "/fixture",
          "target_directory": "/fixture/target",
          "resolve": {"nodes": [{
            "id": "path+file:///fixture#codedb_fixture@0.1.0",
            "dependencies": [],
            "features": ["default", "serde"]
          }]}
        }"#;
        let first = capture_cargo_metadata_json(json).expect("first projection");
        let second = capture_cargo_metadata_json(json).expect("second projection");
        assert_eq!(first, second);
        assert_eq!(first.packages.len(), 1);
        assert_eq!(first.packages[0].name, "codedb_fixture");
        assert_eq!(first.targets[0].kind, ["lib"]);
        assert_eq!(first.features.len(), 2);
        assert_eq!(first.resolve_nodes.len(), 1);
    }

'''
        target.write_text(before + new_test + classifier + after)


def migrate_cli() -> None:
    path = "crates/codedb/src/main.rs"
    replace_once(
        path,
        "use codedb_cargo::capture_cargo_metadata;\n",
        "use codedb_cargo::{CargoMetadataCapture, capture_cargo_metadata_json};\nuse codedb_context::{CargoContextRequest, CapturedCargoContext, capture_context, detect_host_triple};\n",
    )
    replace_once(
        path,
        "impl StdError for CliError {}\n\n",
        """impl StdError for CliError {}

fn capture_repo_cargo(repo_path: &Path) -> Result<(CapturedCargoContext, CargoMetadataCapture), CliError> {
    let target_triple = detect_host_triple().map_err(|source| CliError::Core(Box::new(source)))?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(|source| CliError::Core(Box::new(source)))?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json)
        .map_err(|source| CliError::Core(Box::new(source)))?;
    Ok((context, metadata))
}

""",
    )
    replace_once(
        path,
        """    let cargo_metadata = if manifest_path.exists() {
        Some(
            capture_cargo_metadata(&manifest_path)
                .map_err(|source| CliError::Core(Box::new(source)))?,
        )
    } else {
        None
    };
""",
        """    let cargo_capture = if manifest_path.exists() {
        Some(capture_repo_cargo(repo_path)?)
    } else {
        None
    };
""",
    )
    replace_once(
        path,
        "    if let Some(cargo_metadata) = cargo_metadata {\n        rows.push(summary_row(\n",
        """    if let Some((context, cargo_metadata)) = cargo_capture {
        rows.push(row([
            ("table", "codedb_contexts".to_string()),
            ("context_id", context.context_id),
            ("cargo_version", context.cargo_version),
            ("rustc_version", context.rustc_version),
            ("host_triple", context.host_triple),
            ("target_triple", context.target_triple),
            ("target_cfgs", context.target_cfgs.join(";")),
            ("requested_features", context.requested_features.join(";")),
            ("all_features", context.all_features.to_string()),
            ("no_default_features", context.no_default_features.to_string()),
            ("profile", context.profile),
            ("cargo_lock_sha256", context.cargo_lock_sha256),
            ("resolved_package_count", context.resolved_features.len().to_string()),
            ("status", "available".to_string()),
        ]));
        rows.push(summary_row(
""",
    )


def migrate_nu() -> None:
    path = "crates/nu_plugin_codedb/src/main.rs"
    replace_once(
        path,
        "use codedb_cargo::{CargoContextInput, build_context_rows, capture_cargo_metadata};\n",
        "use codedb_cargo::{CargoMetadataCapture, capture_cargo_metadata_json};\nuse codedb_context::{CargoContextRequest, CapturedCargoContext, capture_context, detect_host_triple};\n",
    )
    anchor = "fn page_rows(rows: Vec<Row>, call: &EvaluatedCall) -> Result<Vec<Row>, LabeledError> {"
    helper = """fn capture_repo_cargo(
    repo_path: &Path,
    span: Span,
) -> Result<(CapturedCargoContext, CargoMetadataCapture), LabeledError> {
    let target_triple = detect_host_triple().map_err(|source| {
        LabeledError::new("rustc host detection failed").with_label(source.to_string(), span)
    })?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(|source| {
        LabeledError::new("locked Cargo context capture failed").with_label(source.to_string(), span)
    })?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json).map_err(|source| {
        LabeledError::new("captured Cargo metadata projection failed").with_label(source.to_string(), span)
    })?;
    Ok((context, metadata))
}

"""
    replace_once(path, anchor, helper + anchor)
    replace_once(
        path,
        "let cargo_metadata = capture_cargo_metadata(&manifest_path).map_err(cargo_error)?;",
        "let (_context, cargo_metadata) = capture_repo_cargo(repo_path, span)?;",
    )
    target = Path(path)
    text = target.read_text()
    old = "let metadata = capture_cargo_metadata(repo_path.join(\"Cargo.toml\")).map_err(cargo_error)?;"
    if old in text:
        target.write_text(text.replace(old, "let (_context, metadata) = capture_repo_cargo(repo_path, span)?;"))
    target = Path(path)
    text = target.read_text()
    start = "fn rust_cfg_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {"
    end = "fn build_script_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {"
    if "CargoContextInput" in text:
        before, rest = text.split(start, 1)
        _old, after = rest.split(end, 1)
        new_fn = r'''fn rust_cfg_rows(repo_path: &Path, span: Span) -> Result<Vec<Row>, LabeledError> {
    let (context, metadata) = capture_repo_cargo(repo_path, span)?;
    let edition = metadata.packages.first().map(|package| package.edition.clone()).unwrap_or_default();
    Ok(vec![
        vec![
            ("table", string("codedb_contexts", span)),
            ("context_id", string(context.context_id.clone(), span)),
            ("cargo_version", string(context.cargo_version, span)),
            ("rustc_version", string(context.rustc_version, span)),
            ("host_triple", string(context.host_triple, span)),
            ("target_triple", string(context.target_triple, span)),
            ("target_cfgs", string(context.target_cfgs.join(";"), span)),
            ("requested_features", string(context.requested_features.join(";"), span)),
            ("all_features", bool_value(context.all_features, span)),
            ("no_default_features", bool_value(context.no_default_features, span)),
            ("profile", string(context.profile, span)),
            ("edition", string(edition, span)),
            ("cargo_lock_sha256", string(context.cargo_lock_sha256, span)),
        ],
        vec![
            ("table", string("feature_sets", span)),
            ("context_id", string(context.context_id, span)),
            ("requested_features", string(context.requested_features.join(";"), span)),
            ("resolved_package_count", int(context.resolved_features.len(), span)?),
        ],
    ])
}

'''
        target.write_text(before + new_fn + end + after)


def migrate_mcp() -> None:
    path = "crates/codedb_mcp/src/lib.rs"
    replace_once(
        path,
        "use codedb_cargo::capture_cargo_metadata;\n",
        "use codedb_cargo::capture_cargo_metadata_json;\nuse codedb_context::{CargoContextRequest, capture_context, detect_host_triple};\n",
    )
    replace_once(
        path,
        """fn cargo_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml")).map_err(core_error)?;
""",
        """fn cargo_summary_rows(repo_path: &Path) -> Result<Vec<Row>, McpError> {
    let target_triple = detect_host_triple().map_err(core_error)?;
    let context = capture_context(&CargoContextRequest {
        manifest_path: repo_path.join("Cargo.toml"),
        target_triple,
        features: Vec::new(),
        all_features: false,
        no_default_features: false,
        profile: "dev".to_string(),
    })
    .map_err(core_error)?;
    let metadata = capture_cargo_metadata_json(&context.cargo_metadata_json).map_err(core_error)?;
""",
    )


def verify() -> None:
    cargo = Path("crates/codedb_cargo/src/lib.rs").read_text()
    if 'Command::new("cargo")' in cargo or "pub fn capture_cargo_metadata(" in cargo:
        raise SystemExit("unlocked cargo execution remains")
    if "pub fn build_context_rows(" in cargo:
        raise SystemExit("duplicate context identity remains")
    for path in ["crates/codedb/src/main.rs", "crates/nu_plugin_codedb/src/main.rs", "crates/codedb_mcp/src/lib.rs"]:
        if "capture_cargo_metadata(" in Path(path).read_text():
            raise SystemExit(f"{path}: unlocked metadata call remains")


def main() -> None:
    append_host_detector()
    add_manifest_dependencies()
    migrate_cargo_projector()
    migrate_cli()
    migrate_nu()
    migrate_mcp()
    verify()


if __name__ == "__main__":
    main()

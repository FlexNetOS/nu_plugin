#!/usr/bin/env python3
"""Run deterministic remote migrations while the local command launcher is unavailable."""

from pathlib import Path

FRONTDOORS = [
    Path("crates/codedb/src/main.rs"),
    Path("crates/nu_plugin_codedb/src/main.rs"),
    Path("crates/codedb_mcp/src/lib.rs"),
]


def complete() -> bool:
    cargo = Path("crates/codedb_cargo/src/lib.rs").read_text()
    context = Path("crates/codedb_context/src/lib.rs").read_text()
    nu = Path("crates/nu_plugin_codedb/src/main.rs").read_text()
    manifests = [Path("crates/codedb/Cargo.toml"), Path("crates/codedb_cargo/Cargo.toml"), Path("crates/codedb_mcp/Cargo.toml"), Path("crates/nu_plugin_codedb/Cargo.toml")]
    return (
        "pub fn capture_cargo_metadata_json" in cargo
        and "pub fn capture_cargo_metadata(" not in cargo
        and "pub fn build_context_rows(" not in cargo
        and "pub fn detect_host_triple_with_runner" in context
        and "CargoContextInput" not in nu
        and "cargo_lock_sha256" in nu
        and all("capture_cargo_metadata(" not in path.read_text() for path in FRONTDOORS)
        and all("codedb-context.workspace = true" in path.read_text() for path in manifests)
    )


def feature_evidence_complete() -> bool:
    nu = Path("crates/nu_plugin_codedb/src/main.rs").read_text()
    rust_cfg = nu.split("fn rust_cfg_rows", 1)[1].split("fn build_script_rows", 1)[0]
    return all(marker in rust_cfg for marker in ['"declared_features"', '"resolved_features"', "metadata.features", "context.resolved_features"])


def patch_feature_evidence() -> None:
    if feature_evidence_complete():
        return
    path = Path("crates/nu_plugin_codedb/src/main.rs")
    text = path.read_text()
    old = """    let edition = metadata
        .packages
        .first()
        .map(|package| package.edition.clone())
        .unwrap_or_default();
    Ok(vec![
"""
    new = """    let edition = metadata
        .packages
        .first()
        .map(|package| package.edition.clone())
        .unwrap_or_default();
    let mut declared_features = Vec::new();
    for feature in &metadata.features {
        declared_features.push(format!("{}={}", feature.package_id, feature.feature));
    }
    declared_features.sort();
    declared_features.dedup();
    let mut resolved_features = Vec::new();
    for (package_id, features) in &context.resolved_features {
        for feature in features {
            resolved_features.push(format!("{package_id}={feature}"));
        }
    }
    Ok(vec![
"""
    if text.count(old) != 1:
        raise SystemExit("Nu feature evidence prelude anchor drifted")
    text = text.replace(old, new, 1)
    old_row = """            (
                "resolved_package_count",
                int(context.resolved_features.len(), span)?,
            ),
"""
    new_row = """            ("declared_features", string(declared_features.join(";"), span)),
            ("resolved_features", string(resolved_features.join(";"), span)),
            (
                "resolved_package_count",
                int(context.resolved_features.len(), span)?,
            ),
"""
    if text.count(old_row) != 1:
        raise SystemExit("Nu feature evidence row anchor drifted")
    path.write_text(text.replace(old_row, new_row, 1))


def materialization_complete() -> bool:
    source = Path("crates/codedb/src/main.rs").read_text()
    materialize = source.split("fn materialize_rows", 1)[1].split("fn scan_rows", 1)[0]
    return "prepare_materialization_path" in materialize and "out_dir.join(&file.relative_path)" not in materialize


def patch_materialization() -> None:
    if materialization_complete():
        return
    path = Path("crates/codedb/src/main.rs")
    text = path.read_text()
    old_import = "use codedb_core::store::BlobStore;\n"
    new_import = "use codedb_core::store::{BlobStore, prepare_materialization_path};\n"
    if text.count(old_import) != 1:
        raise SystemExit("CLI store import anchor drifted")
    text = text.replace(old_import, new_import, 1)
    old_join = "        let out_path = out_dir.join(&file.relative_path);\n"
    new_join = """        let out_path = prepare_materialization_path(out_dir, &file.relative_path)
            .map_err(|error| CliError::Message(format!("unsafe materialization path {}: {error}", file.relative_path)))?;
"""
    if text.count(old_join) != 1:
        raise SystemExit("CLI materialization join anchor drifted")
    path.write_text(text.replace(old_join, new_join, 1))


def migrate_context() -> None:
    migration_path = Path("scripts/apply_context_integration.py")
    source = migration_path.read_text()
    old = 'end = "#[derive(Debug, Deserialize)]\\nstruct Metadata"'
    new = 'end = "#[derive(Debug, Deserialize)]\\nstruct Metadata {"'
    if source.count(old) != 1:
        raise SystemExit("context migration metadata anchor drifted")
    source = source.replace(old, new, 1)
    delayed = "    migrate_mcp()\n    verify()\n"
    if source.count(delayed) != 1:
        raise SystemExit("context migration verification anchor drifted")
    source = source.replace(delayed, "    migrate_mcp()\n", 1)
    namespace = {"__name__": "context_migration_runtime", "__file__": str(migration_path)}
    exec(compile(source, str(migration_path), "exec"), namespace)
    namespace["main"]()
    cli = Path("crates/codedb/src/main.rs")
    text = cli.read_text()
    old_capture = """    let metadata = capture_cargo_metadata(repo_path.join("Cargo.toml"))
        .map_err(|source| CliError::Core(Box::new(source)))?;
"""
    count = text.count(old_capture)
    if count not in (0, 3):
        raise SystemExit(f"CLI locked-export anchor count drifted: {count}")
    if count:
        cli.write_text(text.replace(old_capture, "    let (_context, metadata) = capture_repo_cargo(repo_path)?;\n"))
    namespace["verify"]()


def main() -> None:
    if not complete():
        migrate_context()
    patch_feature_evidence()
    patch_materialization()
    if not complete() or not feature_evidence_complete() or not materialization_complete():
        raise SystemExit("remote migration postconditions are incomplete")


if __name__ == "__main__":
    main()

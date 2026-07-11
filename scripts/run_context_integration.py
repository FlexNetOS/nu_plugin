#!/usr/bin/env python3
"""Run the context migration with corrected exact anchors and final-state idempotence."""

from pathlib import Path


def complete() -> bool:
    cargo = Path("crates/codedb_cargo/src/lib.rs").read_text()
    context = Path("crates/codedb_context/src/lib.rs").read_text()
    nu = Path("crates/nu_plugin_codedb/src/main.rs").read_text()
    manifests = [
        Path("crates/codedb/Cargo.toml"),
        Path("crates/codedb_cargo/Cargo.toml"),
        Path("crates/codedb_mcp/Cargo.toml"),
        Path("crates/nu_plugin_codedb/Cargo.toml"),
    ]
    return (
        "pub fn capture_cargo_metadata_json" in cargo
        and "pub fn capture_cargo_metadata(" not in cargo
        and "pub fn build_context_rows(" not in cargo
        and "pub fn detect_host_triple_with_runner" in context
        and "CargoContextInput" not in nu
        and "cargo_lock_sha256" in nu
        and all("codedb-context.workspace = true" in path.read_text() for path in manifests)
    )


def main() -> None:
    if complete():
        return
    migration_path = Path("scripts/apply_context_integration.py")
    source = migration_path.read_text()
    old = 'end = "#[derive(Debug, Deserialize)]\\nstruct Metadata"'
    new = 'end = "#[derive(Debug, Deserialize)]\\nstruct Metadata {"'
    if source.count(old) != 1:
        raise SystemExit("context migration metadata anchor drifted")
    source = source.replace(old, new, 1)
    namespace = {"__name__": "context_migration_runtime", "__file__": str(migration_path)}
    exec(compile(source, str(migration_path), "exec"), namespace)
    namespace["main"]()


if __name__ == "__main__":
    main()

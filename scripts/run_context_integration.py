#!/usr/bin/env python3
"""Run the context migration with corrected exact anchors and final-state idempotence."""

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
        and all("capture_cargo_metadata(" not in path.read_text() for path in FRONTDOORS)
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
        text = text.replace(
            old_capture,
            "    let (_context, metadata) = capture_repo_cargo(repo_path)?;\n",
        )
        cli.write_text(text)
    namespace["verify"]()
    if not complete():
        raise SystemExit("context integration postconditions are incomplete")


if __name__ == "__main__":
    main()

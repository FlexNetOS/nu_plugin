# Polyglot Schema Extension

This schema plan extends the existing Rust-first CodeDB layout without replacing
the current Rust-specialized tables.

## Principles

- Existing Rust-specific tables remain valid specialized tables.
- New polyglot rows must link back to source files, source blobs, parser/tool
  versions, context hashes, and proof rows.
- Unsupported or unsafe observations become polyglot_capture_gaps or
  polyglot_validation_errors.
- The database remains a fact store; it does not become source truth without
  repeated round-trip proof.

## Planned Table Groups

| Group | Tables | Purpose |
|---|---|---|
| Detection | language_kinds, language_detectors, parser_backends, parser_versions | identify how a file or module was classified and by which detector/parser version |
| Package and lockfile | repo_package_managers, repo_packages, repo_lockfiles, repo_dependency_edges | package identity, manager provenance, and lockfile dependency facts |
| Module graph | polyglot_modules, polyglot_symbols, polyglot_imports, polyglot_references, polyglot_call_edges | shared cross-language structural graph |
| Config and build | polyglot_config_files, polyglot_build_files, polyglot_runtime_scripts, polyglot_generated_files | config/build/runtime surfaces and generated artifact markers |
| Validation | polyglot_capture_gaps, polyglot_validation_errors | explicit unsupported, unsafe, or incomplete facts |
| Single-binary export | single_binary_export_runs, single_binary_embedded_blobs, single_binary_materialization_proofs | generated crate/binary proof surfaces |

## Linking Rules

Every planned polyglot row family should reference:

- source_files and source_blobs
- the context key already used by CodeDB where applicable
- parser or tool identity via parser_backends and parser_versions
- the originating scan or generation run
- validation or capture-gap rows when inference or unsafe escalation is involved

## Compatibility With Rust-first Tables

Rust-specific rows stay the authoritative specialized shape for V1.1 and the
near-term V1.2 planning baseline:

- rust_modules
- rust_items
- rust_imports
- rust_impls
- rust_traits
- rust_symbol_refs
- rust_type_edges
- rust_call_edges

Polyglot tables complement these rows by describing non-Rust repository
surfaces and by offering a shared cross-language abstraction when it is proven
useful.

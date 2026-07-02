# CodeDB Schema

Source: PRD sections 10, 11, and 12.

## Identity context

Every captured fact must be keyed by enough context to avoid lying about Rust reality:

```text
toolchain_id + target_triple + feature_set_hash + cfg_hash + cargo_lock_hash + profile + edition
```

## Table groups

| Group | Tables |
|---|---|
| Core identity | `codedb_contexts`, `toolchains`, `rustc_versions`, `cargo_versions`, `target_triples`, `feature_sets`, `semantic_hashes` |
| Store/blob | `store_metadata`, `schema_versions`, `source_blobs`, `artifact_blobs`, `blob_refs`, `blob_policies` |
| Filesystem/source | `source_roots`, `source_files`, `source_snapshots`, `source_spans`, `source_attributes`, `source_comments`, `source_doc_comments` |
| Cargo | `cargo_workspaces`, `cargo_packages`, `cargo_targets`, `cargo_dependencies`, `cargo_resolve_nodes`, `cargo_features`, `cargo_profiles`, `cargo_configs` |
| Rust static | `rust_modules`, `rust_items`, `rust_imports`, `rust_impls`, `rust_traits`, `rust_symbol_refs`, `rust_type_edges`, `rust_call_edges` |
| Macro/build/native | `macro_definitions`, `macro_invocations`, `macro_expansion_gaps`, `proc_macro_crates`, `build_scripts`, `build_script_gaps`, `native_libraries`, `link_args`, `link_search_paths` |
| Proof/artifact | `scan_runs`, `generation_runs`, `compile_runs`, `test_runs`, `rustdoc_runs`, `validation_errors`, `capture_gaps`, `no_mutation_proofs` |
| Agent/export | `export_runs`, `export_files`, `mcp_views`, `bounded_output_events`, `source_leak_denials` |

## Row rule

A row is canonical only for the observation it proves. Missing compiler-observable facts must be represented as `capture_gaps` or `validation_errors`, never silently omitted.

## Blob rule

Raw source bytes, token streams, generated outputs, and raw proof logs are blobs with hashes and policy rows. Queryable rows point to blobs; rows do not replace exact bytes.

## Stable hash rule

Hash identities must include the context key and source/proof checksum. Public API and semantic hashes are proof aids, not a substitute for cargo/rustc gates.

## CDB085 Static Hash Inputs

Static semantic hashes are built from normalized Rust item rows: relative path,
module path, item kind, item name, visibility, identity kind, and identity note.
Public API hashes use the same normalized inputs but include only public rows.
They intentionally exclude function bodies, type layout, macro expansion, and
rustc semantic checks.

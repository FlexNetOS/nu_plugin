# CodeDB Fixture Matrix

These fixtures are intentionally small source workspaces for CodeDB scan, export,
security, no-mutation, and reproduction tests.

CodeDB is expected to preserve richer store semantics than envctl. The CodeDB
store owns source blobs, table rows, checksums, crate/build facts, capture gaps,
and validation evidence. Envctl is an edge integration that can receive selected
CodeDB exports and materialize files again when needed.

## Matrix

| Fixture id | Path | Expected CodeDB observations |
|---|---|---|
| single_simple_crate | single_simple_crate | source file rows, Cargo package rows, module/function/struct items |
| workspace_two_crates | workspace_two_crates | workspace members, package ids, path dependency edge |
| feature_cfg | feature_cfg | features, cfg-gated functions, target-specific dependency metadata |
| macro_rules | macro_rules | macro_rules definition, invocation, static expansion gap |
| proc_macro_consumer | proc_macro_consumer | proc-macro crate metadata, consumer dependency, dynamic macro gap |
| build_script | build_script | build.rs source row, cargo directive rows, unsafe execution gap |
| out_dir_generator | out_dir_generator | OUT_DIR generated artifact when unsafe execution is approved |
| include_edges | include_edges | include_str/include_bytes path edges and blob/source references |
| non_rust_assets | non_rust_assets | non-Rust asset envelope rows and blob hashes |
| native_link | native_link | native link directive rows or capture gap |
| secret_like | secret_like | secret-looking placeholder rows with refusal/hash-only policy |
| clean_repo | clean_repo | clean no-mutation proof fixture |
| dirty_repo | dirty_repo | documented pre-existing dirty-state proof fixture |
| symlink | symlink | symlink manifest rows or platform limitation gap |

## No-Mutation Boundary

Fixtures do not require mutation of real source repositories. Later tests that
need dirty Git state should copy `dirty_repo` into a temporary directory, create
the dirty state there, and prove CodeDB leaves that state unchanged.

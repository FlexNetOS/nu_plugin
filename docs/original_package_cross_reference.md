# Original package cross-reference

Audit date: 2026-07-02

## Compared roots

- Original execution package: `/home/flexnetos/Downloads/nu_plugin`
- Built Git repository: `/home/flexnetos/FlexNetOS/src/nu_plugin`
- Built remote: `https://github.com/FlexNetOS/nu_plugin`
- Built revision audited: `55b5ff0 plugin: add structured envctl inventory rows (#1)`

The comparison excluded `.git/` and `target/` build caches. With those excluded,
the Downloads package has 211 files and the built repository has 212 tracked
files. The only built-only file is `.gitignore`.

Among common files, only `crates/nu_plugin_codedb/src/main.rs` differs between
the Downloads package and the built repository. The built repository therefore
preserves the original package almost byte-for-byte while adding the final
plugin-side envctl inventory upgrade in one Rust source file.

## What was built

- The loose execution package was turned into a durable Git repository under
  `FlexNetOS/nu_plugin`
- The original package documents, task graph, logs, manifests, fixtures, tests,
  PRD, Nix packaging, and Rust workspace were imported into that repository
- The repo contains workspace crates for:
  - `codedb`
  - `codedb_core`
  - `codedb_cargo`
  - `codedb_rust_static`
  - `codedb_build_capture`
  - `codedb_store_redb`
  - `codedb_mcp`
  - `codedb_fixtures`
  - `nu_plugin_codedb`
- The Nu plugin exposes native table commands including:
  - `codedb scan`
  - `codedb export`
  - `codedb envctl import inventory`
  - `codedb fs entries`
  - `codedb source files`
  - `codedb cargo packages`
  - `codedb cargo deps`
  - `codedb cargo sources`
  - `codedb rust items`
  - `codedb rust macros`
  - `codedb rust cfg`
  - `codedb build scripts`
  - `codedb tables`
  - `codedb gaps`
  - `codedb validation errors`
  - `codedb schema`
  - `codedb doctor`
- `codedb envctl import inventory` now converts the Yazelix generated file
  inventory into `envctl_yazelix_file_import` rows, with content hashes,
  blob refs, structured status, structured row counts, and embedded
  `envctl_yazelix_file_structured_rows`
- The built plugin adds structured conversion for JSON/JSONC and line-oriented
  text-like configuration formats such as TOML, Nix, KDL, Nu, Lua, YAML,
  Markdown, desktop files, service files, shell, conf, terminal conf, and plain
  config
- The envctl side was integrated separately through `FlexNetOS/envctl` PR #410,
  so envctl can import the generated Yazelix inventory catalog

## What was upgraded

- The package moved from a local Downloads artifact to a published GitHub repo
- The built plugin adds 434 lines of implementation over the Downloads copy in
  `crates/nu_plugin_codedb/src/main.rs`
- `last_observed` changed from the static
  `inventory_artifact_current_run` sentinel to a `unix:<seconds>` timestamp
- Blob rows were upgraded from metadata-only rows to hash-backed blob metadata
  rows when the inventory target is a regular file and `import_mode` is
  `content_blob`
- The envctl import command now emits nested structured rows instead of only
  file-level import metadata
- Regression coverage was added for structured datatable payloads and
  no-mutation behavior in the plugin source

## What was downgraded

- The original execution package was imported into Git without generated build
  caches. That is an intentional repository hygiene downgrade, not a runtime
  capability downgrade
- The package checksum manifest is not authoritative in either compared root.
  `sha256sum -c manifests/CHECKSUMS.sha256` currently fails for `Cargo.lock`,
  `crates/nu_plugin_codedb/Cargo.toml`, and
  `crates/nu_plugin_codedb/src/main.rs` in both the Downloads package and the
  built repository
- The task graph still marks most implementation tasks as `planned`, even though
  implementation files exist in the repository. This downgrades the task graph
  from an execution source of truth to a stale package-planning artifact
- GitHub publication currently proves repository presence and CodeQL, but not a
  full CI gate for Cargo tests, Nu smoke tests, Nix package checks, and the
  package checksum manifest

## What was overlooked

- The final structured envctl import upgrade was not copied back to
  `/home/flexnetos/Downloads/nu_plugin`; the Downloads target remains behind the
  built repository by one Rust source file
- `manifests/CHECKSUMS.sha256` was inherited stale from the Downloads package
  and was not regenerated during repo publication
- `manifests/PACK_MANIFEST.json`, `PACKAGE_VALIDATION.json`, and related final
  package evidence still describe the sealed execution package, not the
  post-import Git repository state
- The original task graph reports only 18 of 69 tasks as complete. The remaining
  51 tasks are still planned on paper, including the Rust workspace skeleton,
  store, scanner, CLI, Nu plugin, MCP, envctl export, tests, release, and
  packaging tasks
- The repo has a Nix package/check surface, but no `devShells.ci`; earlier
  attempts to enter `.#ci` are not represented by a supported flake output
- Plain `cargo` is not available on the current host PATH, so workspace Rust
  verification requires the intended Nix/dev environment rather than direct
  shell execution

## Current gaps

- The envctl bridge produces datatable rows from the inventory artifact, but it
  does not yet persist the full file/blob model into a redb-backed CodeDB store
- Blob semantics are currently hash/ref metadata in the envctl import path, not
  a complete persisted source blob table with restore/materialization ownership
- The structured conversion is deterministic and useful, but it is not yet a
  full native Nushell parser bridge for every file type. It performs Rust-side
  JSON/JSONC flattening and line-oriented parsing for text formats
- envctl remains a downstream consumer. The CodeDB-to-envctl export contract is
  documented, but the full `codedb export envctl` surface and envctl
  file-materialization round trip are not complete in this repository
- The redb crate initializes metadata tables and backup/restore proof rows, but
  the status is still `planned`; it is not yet the authoritative live store for
  every generated table
- The MCP crate exposes bounded in-process request handling, but a complete
  externally packaged MCP server lifecycle is not proven here
- The published repo should get a real CI workflow for `cargo test --workspace`,
  Nu syntax/smoke tests, `nix flake check`, package checksum validation, and the
  envctl inventory smoke

## Bottom line

The built repository is not a rewrite or a reduced copy of the original
Downloads package. It is the original package imported nearly intact, plus the
final `nu_plugin_codedb` envctl structured-table upgrade and repository hygiene.

The largest remaining gap is not missing files. The gap is that the original
package promises a complete CodeDB store/export/runtime ecosystem, while the
built repo currently proves an early but useful slice: read-only scans,
bounded table exports, static Rust/Cargo summaries, a bounded MCP library, redb
metadata initialization, and a concrete Yazelix inventory-to-envctl table import.

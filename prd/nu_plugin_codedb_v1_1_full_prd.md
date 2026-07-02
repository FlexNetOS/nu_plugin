# PRD V1.1 — `nu_plugin_codedb`: Nushell-Native Rust Crate Capture Plugin

**Version:** V1.1 full PRD  
**Date:** 2026-07-01  
**Owner:** FlexNetOS / David  
**Status:** Clean canonical PRD; includes integrated Yazelix/Nushell runtime bridge update; supersedes V1.0 and all prior non-canonical documents  
**Primary executable deliverables:** `nu_plugin_codedb`, `codedb` CLI, read-only `codedb mcp serve`, redb-backed local object/index store  
**Package stance:** future changes must update this PRD directly and regenerate task/checksum artifacts.

---

## 1. Executive decision

Build a Rust-native Nushell plugin named `nu_plugin_codedb` that captures complete Rust crate reality into Nushell-native tables backed by one local embedded Rust database target.

The plugin must not reinvent Nushell. Nushell remains the table cockpit: structured tables, pipelines, joins, filters, exports, review, operator visibility, and agent-safe command composition. The plugin supplies the Rust/Cargo/compiler-observable facts that Nushell does not natively know.

### Core product statement

```text
Nushell = table cockpit and plugin host
Rust extractor = compiler/crate reality capture
redb = first durable embedded Rust object/index store
Cargo/rustc/runner = external proof gate
Codex/meta/envctl/Yazelix = orchestration and consumption surfaces
```

### V1.1 doctrine

```text
crate filesystem
+ exact source blobs
+ Cargo reality
+ cfg/feature/target/toolchain context
+ macro/build/proc-macro evidence
+ native/linker evidence
+ generated artifacts
+ proof runs
= reproducible CodeDB capture envelope
```

### Non-negotiable guardrail

No table, plugin command, MCP response, generated artifact, README, or task row may claim complete crate reproduction if any compiler-observable input is missing.

If a component cannot be observed in V1.1, it must become a row in `capture_gaps` or `validation_errors`. Silent omission is failure.

---

## 2. Product doctrine

### 2.1 Source doctrine

```text
Rust crates/files -> compiler-observed Nushell tables -> CodeDB objects/blobs -> generated crate artifacts -> cargo/rustc proof
```

### 2.2 Authority model

V1.1 is **read-only capture and reproduction from raw snapshots**. It does not claim that the database owns Rust source truth yet.

```text
Git/source files = authoritative input for V1.1
CodeDB = complete capture envelope + queryable/provenance projection
Generated crates/files = artifacts
cargo/rustc/runner = proof gates
```

Promotion to “database-owned source truth” is blocked until lossless round-trip, stable object identity, semantic hashing, macro/build capture, `cargo check`, tests, rustdoc/API proof where enabled, and provenance gates pass repeatedly.

### 2.3 Relationship to FlexNetOS doctrine

The existing FlexNetOS doctrine already treats env/config as table-first:

```text
files -> Nushell tables -> validated envctl tables -> generated files
```

`nu_plugin_codedb` applies the same discipline to code, but does not overload envctl. CodeDB is a sibling capability. envctl consumes CodeDB exports and checksums but remains environment/config truth.

---

## 3. Problem statement

Rust code is hard for agents to safely merge, reproduce, and unify when the only working substrate is file text. A Rust crate is not just `.rs` files. It includes Cargo metadata, feature resolution, target-specific `cfg`, `Cargo.lock`, build scripts, macro expansion behavior, procedural macro execution, generated `OUT_DIR` artifacts, native/linker inputs, package metadata, tests, examples, benches, docs, and hidden/non-Rust assets.

Text-based indexing loses too much. A partial AST table also loses too much. A comprehensive system must capture the full compiler-observable crate envelope and expose that envelope as reviewable, queryable, deterministic Nushell tables.

V1.1 must therefore build the first executable bridge:

```text
crate/source/cargo/compiler observations -> redb-backed object store -> Nu-native tables -> proof artifacts
```

---

## 4. Users and use cases

### 4.1 Primary users

| User | Need |
|---|---|
| David / operator | Inspect and reason about Rust projects as table data without losing compiler reality. |
| Codex CLI | Query bounded repo facts through CLI/MCP instead of reading entire repos into context. |
| envctl | Consume stable CodeDB exports/checksums as environment/tool/database facts. |
| meta | Use CodeDB as a per-repo code intelligence surface while meta remains repo graph owner. |
| Yazelix | Host CodeDB commands/plugins inside Nushell/Zellij operator flow. |
| runner/fxrun | Prove capture, no-mutation, reproduction, and release readiness. |

### 4.2 Core use cases

1. Scan a Rust repo or workspace into redb without mutating files.
2. Return crate/file/Cargo/Rust/macro/build/cfg facts as Nushell tables.
3. Identify merge candidates and conflicts by object identity rather than line text.
4. Preserve exact source bytes and non-Rust crate inputs for reproducible artifact generation.
5. Capture build-script/proc-macro risk honestly, requiring explicit unsafe execution before dynamic capture.
6. Emit `capture_gaps` for unobserved reality instead of pretending completeness.
7. Provide Codex with a bounded read-only MCP/CLI tool surface.
8. Let envctl, meta, Yazelix, GitKB, and runner consume outputs without ownership conflicts.

---

## 5. Session-output coverage review

This full PRD incorporates the complete session direction:

| Session output / decision | PRD inclusion |
|---|---|
| Nushell is the best table/operator example | Nushell is the cockpit and plugin host, not replaced. |
| envctl is env-centered | CodeDB is a sibling domain; envctl consumes exports only. |
| Rust files/crates may eventually become DB-produced artifacts | V1.1 blocks DB-owned source truth until proof gates pass. |
| A database-backed system must include macros | Macro definitions, invocations, expansion evidence, hygiene gaps, and proc-macro tables are P0. |
| Tables alone are not enough | V1.1 requires tables + blobs + traces + hashes + proof rows. |
| Need one database target first | redb only for V1.1; other stores deferred. |
| Need Codex integration | CLI and bounded read-only MCP are included. |
| Need agent-tool compatibility | Stable JSON/NUON/CSV outputs, schema introspection, pagination, errors, and exit codes are included. |
| Need Yazelix placement | Hosted inside Yazelix Nu/Zellij flow; not a Yazelix plugin. |
| Need meta placement | meta supplies repo graph/project selection; CodeDB does not replace meta. |
| Need envctl leverage path | envctl consumes CodeDB checksums/tool/db/capture-status exports. |
| Need no external addenda | This document is the standalone V1.1 product truth. |
| Need professional execution package next | See companion checklist and task graph CSV. |

---

## 6. Scope

### 6.1 In scope for V1.1

- Rust Nushell plugin executable: `nu_plugin_codedb`.
- Rust CLI executable: `codedb`.
- Read-only MCP shim: `codedb mcp serve`.
- redb durable store with schema/version/lock/backup metadata.
- Read-only filesystem and source capture.
- Exact source blob storage with content addressing.
- Cargo metadata capture using `cargo metadata --format-version 1`.
- Cargo source provenance capture: registry/git/path/source/patch/replace/config facts where observable.
- `cfg`, feature, target, profile, edition, toolchain context capture.
- Static Rust item/module/import/macro/build-script inventory.
- Explicit unsafe execution gate for dynamic build-script and proc-macro capture.
- `capture_gaps` for unsupported/unobserved facts.
- No-mutation proof: before/after Git status and file-hash checks.
- Nu-native table outputs and examples.
- Bounded agent/MCP output policy.
- Integration contracts for Codex, Yazelix, meta, envctl, runner, GitKB, RTK, Kache, wild, Fenix.
- Fixture matrix and acceptance gates.

### 6.2 Out of scope for V1.1

- Database-owned Rust source truth.
- Automatic code rewriting or source overwrites.
- Full semantic/HIR/MIR truth as mandatory V1.1 success.
- Cargo-expand as canonical macro truth.
- Always-on build-script/proc-macro execution.
- Unbounded MCP source reads.
- DataFusion/DuckDB/Qdrant/Tantivy/Postgres/libSQL server as primary V1 store.
- Meta plugin first or Yazelix-native plugin first.
- envctl absorbing CodeDB internals.

---

## 7. Non-goals and downgrade exclusions

These are excluded because they would weaken correctness, safety, or ownership clarity.

| Excluded item | Why it is a downgrade now |
|---|---|
| DB-owned Rust source truth | False authority until complete capture and proof gates exist. |
| Automatic rewrite/merge engine | Violates no-bulk-rewrite discipline and risks semantic corruption. |
| Direct source overwrite command | Breaks source/artifact separation. |
| `cargo expand` as canonical truth | Useful debugging artifact, but expansion to text is not canonical source. |
| Always executing `build.rs` and proc macros | These execute code and require explicit unsafe capture gates. |
| Vector-first search | Does not solve reproducibility. |
| DataFusion/DuckDB first | Useful later; too much query surface before capture correctness. |
| Qdrant/Postgres/service DB first | Adds daemons and operational surface. |
| Meta plugin first | Blurs repo-graph ownership. |
| Yazelix plugin first | Wrong layer; Yazelix should host the tool. |
| envctl owns CodeDB | envctl remains env/config truth. |
| Unbounded MCP tool responses | Context blast and source-leak risk. |

---

## 8. Database target decision

### 8.1 Decision

Use **redb only** for V1.1 durable storage.

### 8.2 Rationale

- Pure Rust embedded database.
- ACID key-value store.
- No daemon, server, SQL engine, network service, or external runtime.
- Suitable for local content-addressed blobs and deterministic indexes.
- Lower dependency and operational surface than DuckDB, DataFusion, Qdrant, Postgres, RocksDB, or libSQL server mode.

### 8.3 Constraint

redb is a key-value/object-index store, not a SQL database. V1.1 must not imply SQL-like querying inside redb.

Querying happens by:

1. deterministic table/index scans inside `codedb-core`,
2. Nu-native tables returned by `nu_plugin_codedb`,
3. JSON/NUON/CSV exports,
4. optional MVP2 analytical engines.

### 8.4 V1.1 redb store requirements

The store must include:

- schema version record,
- store creation metadata,
- toolchain/codedb version metadata,
- redb file checksum after clean close,
- single-writer lock policy,
- reader concurrency policy,
- backup/export command,
- restore/smoke command,
- migration plan record with explicit unsupported-state behavior,
- corruption-detection validation row.

---

## 9. System architecture

### 9.1 Component layout

```text
nu_plugin_codedb           # Nushell plugin executable
codedb                     # CLI executable; same core engine as plugin
codedb-core                # schema, capture model, identity, validation, exports
codedb-store-redb          # redb-backed durable store
codedb-cargo               # cargo metadata/source provenance capture
codedb-rust-static         # source, CST/AST-ish static inventory, item/macro/build-script discovery
codedb-build-capture       # optional unsafe dynamic build/proc-macro/build.rs observation layer
codedb-mcp                 # read-only bounded MCP server
codedb-fixtures            # fixture workspaces and expected outputs
codedb-runner              # smoke/proof helpers or runner integration shim
```

### 9.2 Data flow

```text
selected repo/workspace
  -> filesystem scanner
  -> source blob hasher
  -> Cargo metadata/provenance capture
  -> static Rust source inventory
  -> cfg/feature/target/toolchain context capture
  -> optional unsafe dynamic capture
  -> redb object/index store
  -> Nu-native tables / JSON / NUON / CSV
  -> generated crate artifacts, only when explicitly requested
  -> cargo/rustc/runner proof
```

### 9.3 Runtime modes

| Mode | Mutation allowed? | Executes build/proc macros? | Purpose |
|---|---:|---:|---|
| `scan` | No | No | Safe static capture. |
| `capture build --unsafe-execute-build` | No source mutation; build artifacts only | Yes | Dynamic compiler/build observation. |
| `export` | No | No | Emit tables/artifacts. |
| `reproduce --artifact-dir` | No source mutation | Optional proof command after artifact generation | Generate artifact tree from captured data. |
| `verify` | No source mutation | Depends on proof profile | Run no-mutation, checksums, cargo/rustc gates. |

---

## 10. Full crate input envelope

V1.1 must define “crate” as the full reproducible input envelope, not only `.rs` files.

### 10.1 Filesystem reality

Capture:

- all files under selected crate/workspace roots,
- file paths,
- normalized paths,
- symlink status and targets where available,
- file mode/permissions where available,
- file size,
- modified time if policy allows,
- binary/text classification,
- ignored/generated/vendor/cache classification,
- inclusion/exclusion reason.

Tables:

```text
filesystem_entries
crate_input_files
non_rust_assets
ignored_files
symlink_edges
file_classification_rules
```

### 10.2 Exact source bytes

Capture:

- raw source bytes,
- content hash,
- encoding guess/status,
- newline style,
- BOM status,
- file mode,
- source spans where static parser can identify them,
- comments/doc comments/attributes where observable.

Tables/blobs:

```text
source_blobs
source_files
source_snapshots
source_spans
source_comments
source_doc_comments
source_attributes
source_byte_facts
```

### 10.3 Cargo reality

Capture:

- `Cargo.toml`,
- `Cargo.lock`,
- workspace members/default members,
- packages,
- targets,
- dependencies,
- features,
- dependency kinds,
- target-specific dependencies,
- profiles where observable,
- `.cargo/config.toml`,
- path/git/registry/source facts,
- patch/replace/source overrides,
- package metadata needed for reproduction.

Tables:

```text
cargo_workspaces
cargo_packages
cargo_targets
cargo_dependencies
cargo_resolve_nodes
cargo_features
cargo_locks
cargo_profiles
cargo_configs
cargo_sources
cargo_patch_overrides
cargo_replace_overrides
cargo_registry_sources
cargo_git_sources
cargo_path_sources
```

### 10.4 Conditional compilation context

Capture:

- toolchain,
- rustc version,
- cargo version,
- edition,
- target triple,
- host triple,
- target cfg values,
- feature set,
- profile,
- environment facts used by cargo/rustc where observable.

Tables:

```text
codedb_contexts
toolchains
rustc_versions
cargo_versions
target_triples
host_triples
target_cfgs
feature_sets
cargo_profiles
cfg_predicates
cfg_eval_results
```

### 10.5 Rust source structure

Capture static facts first:

```text
rust_modules
rust_items
rust_functions
rust_structs
rust_enums
rust_traits
rust_impls
rust_imports
rust_attributes
rust_visibility
rust_generics
rust_where_clauses
rust_types
rust_symbol_refs_static
```

V1.1 static extraction may be conservative. Parser uncertainty must emit `validation_errors` or `capture_gaps`.

### 10.6 Declarative macros

Capture:

```text
macro_definitions
macro_rules
macro_matchers
macro_transcribers
macro_fragments
macro_invocations
macro_resolution_static
macro_expansion_events
macro_expansion_edges
macro_hygiene_contexts
```

Static capture must include definitions and invocations. True compiler-level expansion/hygiene is not assumed unless a verified backend captures it. Any missing expansion truth becomes a `capture_gaps` row.

### 10.7 Procedural macros

Capture, when safely observable:

```text
proc_macro_crates
proc_macro_artifacts
proc_macro_invocations
proc_macro_input_token_streams
proc_macro_output_token_streams
proc_macro_panics
proc_macro_env
proc_macro_file_access
```

Proc-macro execution is not read-only in the ordinary sense. It requires `--unsafe-execute-build` or equivalent operator approval and must preserve raw logs.

### 10.8 Build scripts and OUT_DIR

Capture:

```text
build_scripts
build_script_runs
build_script_env
build_script_stdout
build_script_stderr
build_script_cargo_instructions
out_dir_artifacts
generated_rust_files
rerun_if_changed
rerun_if_env_changed
```

Build-script execution requires explicit unsafe approval. Static detection of `build.rs` is safe; execution capture is gated.

### 10.9 Native/linker behavior

Capture:

```text
native_libraries
linker_tools
link_args
link_search_paths
pkg_config_results
cc_invocations
system_library_facts
```

This layer is required because build scripts can emit linker and native-library instructions.

### 10.10 Static include/path edges

Capture static evidence of:

- `include!`,
- `include_str!`,
- `include_bytes!`,
- paths used in build scripts where parseable,
- test fixtures and example assets where discovered.

Tables:

```text
static_include_edges
static_path_references
fixture_assets
example_assets
bench_assets
doc_assets
```

Dynamic file access tracing is MVP2 unless a safe local backend is approved. Missing dynamic tracing must emit `capture_gaps`.

### 10.11 License/package metadata

Capture:

```text
license_files
readme_files
package_metadata
publish_metadata
workspace_metadata
compliance_flags
```

This is needed for artifact packaging and code movement decisions.

---

## 11. Schema summary

### 11.1 Core identity and provenance

```text
codedb_stores
codedb_schema_versions
codedb_contexts
capture_runs
capture_inputs
capture_gaps
validation_errors
object_identities
object_hashes
git_provenance
no_mutation_proofs
```

### 11.2 Store and blob tables

```text
source_blobs
artifact_blobs
blob_indexes
blob_ref_counts
redb_store_facts
redb_backups
redb_restore_tests
```

### 11.3 Filesystem/source tables

```text
filesystem_entries
crate_input_files
non_rust_assets
ignored_files
symlink_edges
source_files
source_snapshots
source_spans
source_comments
source_doc_comments
source_attributes
source_byte_facts
```

### 11.4 Cargo tables

```text
cargo_workspaces
cargo_packages
cargo_targets
cargo_dependencies
cargo_resolve_nodes
cargo_features
cargo_locks
cargo_profiles
cargo_configs
cargo_sources
cargo_patch_overrides
cargo_replace_overrides
cargo_registry_sources
cargo_git_sources
cargo_path_sources
```

### 11.5 Rust static tables

```text
rust_modules
rust_items
rust_functions
rust_structs
rust_enums
rust_traits
rust_impls
rust_imports
rust_attributes
rust_visibility
rust_generics
rust_where_clauses
rust_types
rust_symbol_refs_static
```

### 11.6 Macro/build/native tables

```text
macro_definitions
macro_rules
macro_matchers
macro_transcribers
macro_fragments
macro_invocations
macro_resolution_static
macro_expansion_events
macro_expansion_edges
macro_hygiene_contexts
proc_macro_crates
proc_macro_artifacts
proc_macro_invocations
proc_macro_input_token_streams
proc_macro_output_token_streams
proc_macro_panics
proc_macro_env
proc_macro_file_access
build_scripts
build_script_runs
build_script_env
build_script_stdout
build_script_stderr
build_script_cargo_instructions
out_dir_artifacts
generated_rust_files
native_libraries
linker_tools
link_args
link_search_paths
pkg_config_results
cc_invocations
system_library_facts
```

### 11.7 Proof/artifact tables

```text
generated_crates
generated_files
artifact_files
generation_runs
compile_runs
test_runs
rustdoc_runs
reproduction_proofs
public_api_deltas
semantic_hashes
```

### 11.8 Agent/export tables

```text
export_manifests
table_checksums
mcp_tool_calls
mcp_response_limits
pagination_cursors
schema_introspection
source_leak_policy
```

---

## 12. Stable identity policy

Object identity must be deterministic and context-aware.

A stable object ID must include:

```text
store schema version
workspace/repo identity
crate/package identity
module path
object kind
stable name when available
source span or token anchor
context hash
source blob hash
```

Identity is not the same as semantic equality. V1.1 must distinguish:

- source identity,
- token identity,
- syntax identity,
- public API identity,
- context identity,
- proof identity.

Required fields:

```text
object_id
object_kind
stable_name
module_path
crate_id
package_id
edition
visibility
source_span
source_blob_hash
token_hash
syntax_hash
context_hash
public_api_hash_optional
semantic_hash_optional
identity_status
```

If the extractor cannot derive a stable identity, it must mark the object as `unstable_identity` and add a `capture_gaps` row.

---

## 13. Command surface

Every command must support machine-readable output and bounded results.

### 13.1 Nushell plugin commands

```nu
codedb scan <repo_path> [--store <path>] [--profile static]
codedb fs entries [--store <path>] [--limit <n>] [--cursor <cursor>]
codedb source files [--store <path>] [--limit <n>] [--cursor <cursor>]
codedb cargo packages [--store <path>]
codedb cargo deps [--store <path>]
codedb cargo sources [--store <path>]
codedb rust items [--store <path>] [--limit <n>] [--cursor <cursor>]
codedb rust macros [--store <path>] [--limit <n>] [--cursor <cursor>]
codedb rust cfg [--store <path>]
codedb build scripts [--store <path>]
codedb capture build <repo_path> --unsafe-execute-build [--store <path>]
codedb gaps [--store <path>]
codedb validation errors [--store <path>]
codedb schema [--store <path>]
codedb export <table> --format nuon|json|csv [--store <path>]
codedb backup --store <path> --out <path>
codedb restore --backup <path> --store <path>
codedb prove no-mutation <repo_path> --store <path>
codedb verify <repo_path> --store <path>
codedb doctor [--nu] [--yazelix] [--codex] [--meta] [--envctl]
```

### 13.2 CLI parity

The `codedb` CLI must expose equivalent commands for Codex, runner, scripts, and non-interactive usage.

Examples:

```bash
codedb scan /path/to/repo --store .codedb/repo.redb --format json
codedb export rust_items --store .codedb/repo.redb --format nuon
codedb mcp serve --store .codedb/repo.redb --readonly --max-rows 200 --max-bytes 65536
codedb verify /path/to/repo --store .codedb/repo.redb --format json
```

### 13.3 MCP tool surface

V1.1 MCP is read-only and bounded.

Allowed tools:

```text
codedb_schema
codedb_list_tables
codedb_get_table_page
codedb_get_capture_gaps
codedb_get_validation_errors
codedb_get_repo_summary
codedb_get_cargo_summary
codedb_get_rust_item_summary
codedb_get_macro_summary
codedb_get_build_script_summary
codedb_get_no_mutation_proof
```

Blocked by default:

```text
raw_source_blob_read
full_file_dump
unsafe_build_capture
source_overwrite
patch_apply
git_mutation
unbounded_table_dump
```

---

## 14. Nushell plugin compatibility strategy

### 14.1 Plugin protocol boundary

`nu_plugin_codedb` is a Nushell plugin executable. It must use the matching `nu-plugin` and `nu-protocol` crate versions for the target Nushell runtime.

### 14.2 Host vs Yazelix runtime Nu

There may be two Nushell runtimes:

1. host/user Nushell,
2. Yazelix-bundled/runtime Nushell.

The plugin must not assume one registry covers both.

Required doctor checks:

```bash
codedb doctor --nu
codedb doctor --yazelix
```

The checks must report:

- host `nu` path,
- host `nu --version`,
- Yazelix runtime `nu` path if present,
- Yazelix runtime `nu --version` if present,
- plugin binary path,
- plugin protocol compatibility status,
- plugin registration status for each runtime,
- recommended registration command.

### 14.3 Failure behavior

If plugin protocol versions mismatch, `codedb doctor` must fail clearly and recommend rebuild/reinstall. It must not silently register an incompatible plugin.

---

## 15. Safety, secrets, and no-mutation policy

### 15.1 Read-only default

Default commands must not mutate source repos.

Mutation check:

```text
before scan: git status + file hash sample/full manifest
run scan/export
after scan: git status + file hash sample/full manifest
emit no_mutation_proof
```

If Git is unavailable, emit `no_mutation_proof.status = degraded` and explain why.

### 15.2 Unsafe execution gate

Build scripts and proc macros execute code. Dynamic build capture must require explicit opt-in:

```bash
codedb capture build /repo --unsafe-execute-build --store .codedb/repo.redb
```

Required output rows:

```text
unsafe_execution_approval
build_script_runs
proc_macro_invocations
raw_log_paths
capture_gaps
validation_errors
```

No dynamic execution may run through MCP in V1.1.

### 15.3 Source blob secret policy

Exact source capture can conflict with secret policy. V1.1 must support source blob policy modes:

| Mode | Behavior |
|---|---|
| `refuse` | Stop if secret-looking content is detected. |
| `hash-only` | Store metadata/hash, not raw bytes. |
| `local-only` | Store raw blob only in untracked local redb; export hash only. |
| `allow` | Operator-approved mode for controlled fixtures only. |

Default: `refuse` for tracked/exported artifacts, `local-only` for operator-owned private store only if explicitly selected.

### 15.4 Agent source-leak guard

The MCP server must avoid returning raw source by default. It should return summaries, hashes, spans, counts, and paginated rows. Any command exposing source content must require explicit local CLI use, not default MCP.

---

## 16. Integration contracts

### 16.1 Codex CLI integration

Codex has two supported integration modes:

1. **Direct CLI use** through shell commands.
2. **Read-only MCP use** through `codedb mcp serve`.

#### Codex-specific conflict bridge with Nushell

Codex does not need to load the Nushell plugin directly. That avoids plugin registry/version ambiguity. The bridge is:

```text
Codex -> codedb CLI or codedb MCP -> codedb-core -> redb store -> Nu-compatible outputs
Nushell -> nu_plugin_codedb -> codedb-core -> same redb store
```

This solves these conflicts:

| Conflict | Bridge |
|---|---|
| Codex shell may not run inside Nushell | Codex calls `codedb` CLI or MCP directly. |
| Nu plugin registry may differ between host/Yazelix Nu | `codedb doctor --nu --yazelix` validates registrations; CLI remains fallback. |
| Codex context can be blasted by large tables | MCP pagination/byte limits; CLI supports `--limit`, `--format`, `--summary`. |
| Codex may mutate files by accident | CodeDB default commands are read-only and emit no-mutation proof. |
| Unsafe build capture needs approval | Dynamic capture is blocked in MCP and requires explicit CLI flag. |
| Codex config and envctl config ownership may conflict | envctl renders Codex config/MCP fragments; CodeDB only supplies tool command targets. |

Codex must use official auth and manual gates. The plugin must not require browser/session-token hacks or hidden mutation scripts.

### 16.2 Compatible agent tool contract

For an AI agent, the plugin must provide:

- stable command names,
- deterministic output schemas,
- JSON output for all CLI commands,
- NUON output for Nushell-native workflows,
- CSV output for task tables and spreadsheet-like review,
- bounded output with pagination,
- machine-readable errors,
- exit codes,
- no hidden prompts in non-interactive mode,
- dry-run/proof modes,
- explicit unsafe flags,
- raw log path reporting,
- schema introspection.

### 16.3 Yazelix integration

CodeDB is **not** a Yazelix plugin. It fits as:

- a Nu plugin registered inside Yazelix runtime Nushell,
- a CLI executable reachable in Yazelix shells/popups,
- a Codex sidecar tool launched inside Yazelix terminal workflow,
- a status/report source if a later widget consumes capture status.

Yazelix owns operator flow: Nix runtime closure, Nushell shell surface, Zellij workspace, Yazi/Helix, popups/status, and Codex terminal workflow. CodeDB must not bypass Yazelix pane/session ownership.

### 16.4 meta integration

CodeDB is **not** a meta plugin in V1.1.

meta owns:

- repo graph,
- peer repo ownership,
- project IDs,
- tags/capabilities,
- task/release graph.

CodeDB accepts meta-selected repo paths and emits CodeDB facts. Later, a `meta-codedb` plugin can plan scans, but V1.1 must avoid making meta execute broad cross-repo mutations.

Required CLI support:

```bash
codedb scan --repo-id <meta_project_id> --repo-path <path> --store <path>
```

### 16.5 envctl integration

CodeDB does not replace envctl. envctl can leverage CodeDB by consuming exports:

```text
codedb_tool_versions
codedb_database_endpoints
codedb_capture_status
codedb_table_checksums
codedb_validation_errors
codedb_cache_dirs
codedb_log_dirs
codedb_release_artifacts
```

envctl must not read redb internals directly in V1.1. It consumes stable JSON/NUON/CSV exports and checksums.

### 16.6 runner/fxrun integration

runner owns proof and release readiness.

Required runner gates:

- `codedb scan` succeeds.
- no-mutation proof succeeds.
- schema introspection succeeds.
- redb backup/restore succeeds.
- export checksums recorded.
- fixture matrix passes.
- unsafe capture is skipped unless explicitly approved.
- generated artifact trees compile if reproduction mode is enabled.

### 16.7 GitKB integration

GitKB stores durable explanations, decisions, and handoffs. It should store:

- CodeDB doctrine summary,
- known capture gaps,
- repo-specific scan notes,
- fixture outcomes,
- command-ledger references,
- runner proof links.

It should not store raw source blobs.

### 16.8 RTK integration

RTK may summarize long outputs but must preserve raw failure logs. CodeDB commands must emit raw log paths. RTK compression must not replace root-cause evidence.

### 16.9 Kache/wild/Fenix integration

CodeDB must capture facts about these as environment/toolchain context where relevant:

```text
kache status and wrapper path
wild linker status and opt-in feature state
Fenix toolchain/channel/component/target facts
rustc/cargo/rustfmt/clippy/rust-analyzer paths and versions
```

These are facts for CodeDB context and envctl exports, not CodeDB-owned installs.

---

## 17. Import/export/archive/restore

### 17.1 Export formats

Required:

- JSON,
- JSON Lines for large events,
- NUON,
- CSV for flat review tables,
- manifest JSON with table checksums.

### 17.2 Archive contents

A CodeDB archive must contain:

```text
store manifest
schema version
table checksums
redb file checksum
exported table snapshots
capture gaps
validation errors
no-mutation proof
raw log references
fixture/proof status
```

### 17.3 Restore gate

`codedb restore` must prove:

- backup opens,
- schema matches or migration is explicitly supported,
- table checksums match,
- sample query works,
- no raw secret export policy was violated.

---

## 18. Fixture matrix

Minimum fixtures:

| Fixture | Required evidence |
|---|---|
| Single simple crate | source/files/Cargo/items export. |
| Workspace with two crates | workspace members, package IDs, dependencies. |
| Feature-gated code | feature context and cfg rows. |
| Target-gated dependency | target-specific dependency rows. |
| `macro_rules!` crate | macro definition/invocation rows. |
| Proc-macro consumer | static proc-macro identification and dynamic gap unless unsafe run. |
| Build script crate | `build.rs` static detection; dynamic gap unless unsafe run. |
| OUT_DIR generator fixture | generated artifact rows when unsafe run approved. |
| Native/link fixture | linker/native rows or capture gap. |
| include_str/include_bytes/include fixture | static include edges. |
| Non-Rust asset crate | non-Rust assets included in crate envelope. |
| Symlink fixture | symlink rows or platform limitation gap. |
| Secret-looking fixture | refusal/hash-only/local-only policy tests. |
| Dirty Git repo fixture | no-mutation proof records pre-existing dirty state. |
| Generated artifact reproduction fixture | artifact tree plus cargo check proof. |

---

## 19. Acceptance criteria

V1.1 is acceptable only when:

1. `nu_plugin_codedb` builds as a Rust Nushell plugin.
2. `codedb` CLI builds and shares the same core engine.
3. `codedb doctor` validates host Nu, Yazelix Nu where present, Codex CLI path where present, store path, and plugin compatibility.
4. redb is the only durable DB target enabled by default.
5. redb schema versioning, lock policy, backup, and restore test exist.
6. `codedb scan` captures filesystem entries, exact source metadata, Cargo metadata, and static Rust facts without mutating repo files.
7. `capture_gaps` exists and is populated for unsupported macro/proc-macro/build/hygiene/dynamic-file-access facts.
8. `validation_errors` exists and is populated for malformed/unreadable/ambiguous facts.
9. Source blob secret policy is implemented and tested.
10. `cargo metadata --format-version 1` is used where Cargo metadata is captured.
11. `cfg`, target, feature, profile, edition, toolchain context tables exist.
12. `macro_rules!` definitions/invocations are captured statically.
13. Proc-macro and build-script dynamic capture is gated behind explicit unsafe execution.
14. No dynamic execution is exposed through MCP in V1.1.
15. MCP outputs are bounded, paginated, and source-leak guarded.
16. Nu output tables load and can be filtered/joined in Nushell.
17. CLI JSON output is valid and deterministic for repeated scans of unchanged fixtures.
18. No-mutation proof passes for clean fixture repos.
19. Runner proof records table checksums and raw log paths.
20. envctl export contract exists and does not require reading redb internals.
21. meta integration accepts project ID/path but does not mutate meta graph.
22. Yazelix integration docs explain host/runtime Nu registration.
23. Codex integration docs explain CLI/MCP bridge and conflict handling.
24. Fixture matrix passes.
25. Package manifest records binary, schema, table, and artifact checksums.

---

## 20. MVP2 expansion backlog

After V1.1 proves capture correctness:

- DataFusion/Arrow analytical projection.
- Tantivy full-text search index.
- SQLite/libSQL export bridge.
- DuckDB read-only analytical export.
- Dynamic file access tracing backend.
- Rust-analyzer/HIR semantic backend.
- rustdoc JSON API-delta backend with pinned nightly where approved.
- Change-plan generator without auto-apply.
- Generated crate artifact tree and equivalence gate expansion.
- Meta plugin wrapper.
- Yazelix status widget.
- Envctl native CodeDB export importer.
- GitKB summarizer for capture gaps.

---

## 21. Professional package deliverables required next

This PRD is one input to an execution package. The package must also contain:

```text
CODEDB_START_HERE.md
GOAL.md
SUBGOALS.md
ACCEPTANCE.md
NAVIGATION.md
NAVIGATION.json
DOC_GRAPH.md
READINESS_GATE.md
DRIFT_GUARD.md
STOP_CONDITIONS.md
FIRST_RUN_PROMPT.md
nu_plugin_codedb_v1_1_full_prd.md
ARCHITECTURE.md
SCHEMA.md
COMMANDS.md
INTEGRATION_CONTRACTS.md
SECURITY_AND_SECRET_POLICY.md
TEST_PLAN.md
FIXTURE_MATRIX.md
RELEASE_GATE.md
TASK_GRAPH.csv
TASK_FILE_MAP.csv
COMMAND_LEDGER.csv
WORKLOG.md
PACK_MANIFEST.json
LINK_CHECK_REPORT.md
```

See companion checklist: `nu_plugin_codedb_execution_package_checklist.md`.

---

## 22. Codex implementation prompt

```text
You are Codex working inside the FlexNetOS execution doctrine.

Task:
Implement the first executable V1.1 slice of nu_plugin_codedb.

Doctrine:
- Nushell is the table cockpit and plugin host.
- CodeDB captures compiler-observable Rust crate reality into tables/blobs/proof rows.
- redb is the only V1.1 durable database target.
- Git/source files remain authoritative input for V1.1.
- Generated files/crates are artifacts.
- cargo/rustc/runner are proof gates.
- No table may claim complete crate reproduction if any compiler-observable component is missing.
- Missing observations must become capture_gaps or validation_errors rows.
- Do not mutate source repos.
- Do not run bulk rewrite scripts.
- Do not expose raw secrets.
- Preserve raw logs.
- Record manual/state-changing commands in the command ledger.

Required first implementation slice:
1. Create a Rust workspace with crates:
   - nu_plugin_codedb
   - codedb
   - codedb-core
   - codedb-store-redb
   - codedb-cargo
   - codedb-rust-static
   - codedb-mcp
   - codedb-fixtures
2. Implement redb store initialization with schema version and metadata rows.
3. Implement codedb scan <repo> as read-only static capture.
4. Capture filesystem_entries, crate_input_files, source_blobs metadata, source_files, cargo metadata tables, target/toolchain context, and initial rust_items/macro/build_script static rows.
5. Implement capture_gaps and validation_errors.
6. Implement no-mutation proof for Git repos.
7. Implement CLI JSON output and Nushell plugin table output for at least schema, filesystem entries, cargo packages/deps, rust items, macros, gaps, and validation errors.
8. Implement codedb doctor --nu --codex --yazelix with clear degraded statuses when tools are absent.
9. Implement read-only codedb mcp serve with bounded/paginated tools; do not expose raw source by default.
10. Add fixtures and tests for simple crate, workspace crate, feature-gated code, macro_rules, build.rs static detection, proc-macro static detection/gap, include_str/static include edge, non-Rust asset, and secret-looking source policy.
11. Add docs for Codex, Yazelix, meta, envctl, runner integration.

Acceptance:
- cargo test passes.
- repeated scans of unchanged fixtures produce stable table checksums.
- no source files are mutated.
- capture_gaps are emitted for unsupported compiler-observed dynamic facts.
- MCP output is bounded and read-only.
- envctl can consume exported JSON/NUON/CSV checksums without reading redb internals.
```

---

## 23. Source grounding

This PRD is grounded by current public documentation and local FlexNetOS doctrine:

- Nushell plugin model: executable plugins communicate with Nu through the plugin protocol stream; plugin compatibility must be version-aware.
- redb: pure Rust embedded ACID key-value store suitable for local object/index storage.
- Cargo metadata: `cargo metadata --format-version` is the supported structured workspace/dependency metadata source.
- Cargo build scripts: `build.rs` can emit Cargo instructions, linker args, generated files, and `OUT_DIR` artifacts.
- Rust macro systems: `macro_rules!` and procedural macros require explicit capture strategies; proc macros execute compile-time Rust code.
- Rust conditional compilation: `cfg` depends on target/toolchain/options and must be context-keyed.
- Codex CLI/MCP: Codex can run local shell commands and connect to MCP tools through configuration; CodeDB must bound this interface.
- FlexNetOS doctrine: envctl remains env/config truth; Codex must avoid hidden mutation/bulk rewrite; Yazelix hosts operator flow; meta owns repo graph; runner owns release proof.

---

## 24. Integrated Yazelix/Nushell runtime bridge update

This section records the current Yazelix/Nushell runtime bridge requirements after cross-referencing Nushell behavior with the FlexNetOS/yazelix runtime tree. It is part of the canonical V1.1 PRD.

### 24.1 Verified integration stance

`nu_plugin_codedb` must integrate with Yazelix through the existing runtime/tool/initializer pattern, not by editing the tracked Yazelix `nushell/config/config.nu` file.

The correct placement is:

```text
runtime package:
  libexec/nu_plugin_codedb
  libexec/codedb

generated user/runtime state:
  ~/.local/share/yazelix/initializers/nushell/codedb_init.nu
  ~/.local/share/yazelix/initializers/nushell/codedb_extern.nu

optional aggregate bridge:
  ~/.local/share/yazelix/initializers/nushell/yazelix_extern.nu
```

### 24.2 Design consequence

CodeDB must treat Yazelix as a runtime host. It is not a Yazelix Zellij plugin, not a replacement for Yazelix's generated shell initializers, and not a second owner for Nushell startup configuration.

Yazelix owns:

```text
Nix runtime closure
YAZELIX_RUNTIME_DIR
YAZELIX_CONFIG_DIR
YAZELIX_STATE_DIR
YAZELIX_LOGS_DIR
YAZELIX_NU_BIN resolution
Zellij/Yazi/Helix workspace ownership
generated shell initializer lifecycle
```

CodeDB owns:

```text
nu_plugin_codedb executable
codedb CLI executable
redb-backed local store
crate capture/export commands
bounded MCP surface
generated CodeDB-specific Nu init/extern artifacts
```

### 24.3 Required package additions

The execution package must include these documents or generated sections before implementation begins:

```text
YAZELIX_NUSHELL_RUNTIME.md
CODEDB_NU_PLUGIN_REGISTRATION.md
CODEDB_YAZELIX_RUNTIME_TOOL.md
CODEDB_NUSHELL_SYNTAX_GATE.md
CODEDB_YAZELIX_INIT_CONTRACT.md
```

These files may be generated as standalone docs or folded into `ARCHITECTURE.md`, `COMMANDS.md`, `YAZELIX_PLACEMENT.md`, and `TEST_PLAN.md`, but the topics must exist explicitly.

### 24.4 Required task additions

The task graph must include a Yazelix/Nushell bridge block after the original release task range:

```text
CDB049 Inspect Yazelix Nushell runtime bridge
CDB050 Package nu_plugin_codedb as runtime tool
CDB051 Validate host Nu vs Yazelix runtime Nu protocol
CDB052 Implement transient nu --plugins smoke test
CDB053 Implement temp-HOME plugin registry smoke test
CDB054 Generate CodeDB extern/init bridge artifact
CDB055 Verify generated initializer checksums/provenance
CDB056 Extend syntax validator path for CodeDB fixtures
CDB057 Add no-real-HOME plugin registration test
CDB058 Add Yazelix launch smoke with CodeDB disabled
CDB059 Add Yazelix launch smoke with CodeDB enabled
CDB060 Add plugin stderr/trace secret-leak guard
CDB061 Add redb lock/plugin-GC behavior test
CDB062 Add Codex bounded CLI/MCP invocation smoke
CDB063 Add envctl table rows for CodeDB runtime integration
```

### 24.5 Runtime validation gates

V1.1 is not accepted until these gates are represented in the task graph and package checklist:

| Gate | Required proof |
|---|---|
| Host Nu compatibility | `codedb doctor --nu` reports protocol/runtime status. |
| Yazelix Nu compatibility | `codedb doctor --yazelix` resolves `YAZELIX_NU_BIN` or reports degraded status. |
| Transient plugin loading | `nu --plugins '[...]'` smoke test passes without mutating real HOME. |
| Registry plugin loading | temp-HOME `plugin add` / `plugin use` smoke test passes. |
| Generated initializer | CodeDB init/extern files are generated under state, with checksums and provenance. |
| No tracked config mutation | tracked `nushell/config/config.nu` is unchanged by CodeDB tasks. |
| Startup safety | Yazelix launches with CodeDB disabled and enabled. |
| Secret safety | plugin stderr/log/MCP outputs do not leak source secrets by default. |
| redb safety | redb lock, backup, and plugin lifecycle tests pass. |

### 24.6 Hard rejection

Do not implement CodeDB by:

- directly modifying Yazelix's tracked `nushell/config/config.nu`;
- loading heavy CodeDB implementation into every Nushell startup;
- assuming host `nu` and Yazelix runtime `nu` use the same plugin protocol;
- using the real operator HOME for plugin registry tests;
- exposing raw source through Codex MCP by default;
- treating a Yazelix Zellij plugin surface as the CodeDB integration layer.


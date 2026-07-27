# CodeDB V1.1 Architecture

This document specifies the architecture required by sections 8 and 9 of
`prd/nu_plugin_codedb_v1_1_full_prd.md`. The PRD remains product truth; this
document makes its component, data-flow, and runtime-mode boundaries explicit.

## Architectural stance

CodeDB V1.1 is a local, Rust-native capture and query system for the complete
compiler-observable envelope of a Rust crate or workspace.

```text
Git and source files       authoritative input
CodeDB                     captured, queryable provenance projection
redb                       primary local V1.1 object and index store
Nushell                    table cockpit and plugin host
Cargo, rustc, and runner   external proof gates
generated files            disposable, verified artifacts
```

The database does not own Rust source truth in V1.1. A scan must not change its
input repository. CodeDB may report incomplete observations, but it must report
them as `capture_gaps` or `validation_errors`; it may never silently present an
incomplete capture as reproducible.

## System context

```text
                           selected repository/workspace
                                       |
                                       v
                 +-------------------------------------------+
                 | safe capture                              |
                 | filesystem | source | Cargo | Rust static |
                 | context: cfg | features | target | tools  |
                 +----------------------+--------------------+
                                        |
                         explicit approval, only when needed
                                        |
                                        v
                 +-------------------------------------------+
                 | unsafe dynamic capture                    |
                 | build.rs | proc macros | OUT_DIR | linker |
                 +----------------------+--------------------+
                                        |
                                        v
                 +-------------------------------------------+
                 | codedb-core normalization and validation  |
                 +----------------------+--------------------+
                                        |
                                        v
                 +-------------------------------------------+
                 | redb tables, indexes, blobs, gaps, proofs |
                 +----------+---------------+----------------+
                            |               |
              +-------------+----+     +----+----------------+
              | operator surfaces |     | external proof      |
              | Nu | CLI | MCP     |     | Cargo/rustc/runner |
              +------+------+------+     +---------------------+
                     |      |
              bounded views | explicit artifact generation
                     v      v
              Codex/meta/envctl/Yazelix
```

All normal capture and query paths are local and require no service database or
network daemon. The read-only MCP server is a bounded presentation surface, not
an independent source of truth.

## Workspace components

The executable surfaces share the same capture model and storage contracts.
They must not grow separate interpretations of identity, schema, validation, or
capture completeness.

| Component | Kind | Responsibility | Must not own |
|---|---|---|---|
| `codedb-core` | library crate | Schema types, stable identity, capture rows, gaps, validation, deterministic indexes, export models, and backend-neutral contracts. | Nushell protocol, CLI parsing, or database-specific IO. |
| `codedb-context` | library crate | Toolchain, Cargo/rustc version, host/target, feature, profile, environment, and `cfg` observation context. | Source parsing or persistence policy. |
| `codedb-cargo` | library crate | `cargo metadata`, workspace/package/target/dependency resolution, lock/config facts, and source provenance. | Regex-only Cargo truth or implicit network mutation. |
| `codedb-rust-static` | library crate | Read-only Rust structure, items, modules, imports, attributes, macro/build-script discovery, include/path edges, and compiler-facing static evidence. | Executing `build.rs` or procedural macros. |
| `codedb-build-capture` | library crate | Approved dynamic observation of build scripts, procedural macros, generated `OUT_DIR` files, native/linker behavior, and raw logs. | Default scan behavior or approval decisions. |
| `codedb-store-redb` | library crate | Primary V1.1 embedded persistence: initialization, schema metadata, content-addressed blobs, deterministic indexes, locking, clean-close checksum, backup, restore, and corruption checks. | Source authority, SQL semantics, or external services. |
| `codedb` | binary crate | Agent- and shell-friendly scan, capture, export, schema, doctor, verify, reproduce, archive, and restore entrypoint. | A second capture engine distinct from the plugin. |
| `nu_plugin_codedb` | binary crate | Nushell plugin protocol adapter and Nu-native table commands. | Compiler truth, global plugin registration, or tracked Nu configuration. |
| `codedb-mcp` | binary/library crate | Bounded, paginated, read-only MCP views over the shared query surface. | Raw-source disclosure by default, writes, capture execution, or unbounded responses. |
| `codedb-fixtures` | support crate/data | Deterministic fixtures and expected rows for capture, security, no-mutation, compatibility, backup, and reproduction tests. | Production capture state. |
| runner integration | external adapter | Executes smoke, Cargo/rustc, no-mutation, checksum, and release-proof gates and preserves evidence. | CodeDB schema or source mutation. |

The PRD selects redb as the sole primary V1.1 durable store. Any experimental
or later backend code is outside this V1.1 architecture and cannot weaken redb
compatibility, deterministic exports, or the acceptance gates.

## Dependency direction

Dependencies point inward toward shared models and outward only through narrow
adapters:

```text
codedb-context -----+
codedb-cargo -------+
codedb-rust-static -+--> codedb-core contracts --> codedb-store-redb
codedb-build-capture+
                                            |
                        +-------------------+------------------+
                        v                   v                  v
                      codedb       nu_plugin_codedb       codedb-mcp
```

`codedb-core` defines data meaning. Capture crates produce typed observations.
The redb adapter persists them. The CLI, Nu plugin, and MCP adapter translate
the same query results for different consumers.

## Capture data flow

### 1. Select and freeze the observation context

The caller selects a repository or Cargo workspace, capture policy, source-blob
mode, toolchain, target, feature set, and profile. CodeDB records the canonical
root and before-state needed for no-mutation proof. Paths are normalized, but
the original path and byte facts remain available where policy permits.

### 2. Perform safe capture

The safe scanner walks the complete selected root and records Rust and non-Rust
inputs, symlinks, classifications, hashes, permissions, and exclusion reasons.
It then combines:

- exact-source metadata and policy-controlled content-addressed blobs;
- Cargo metadata, lock/config information, resolution, and provenance;
- static Rust structure, macro definitions/invocations, include/path edges,
  build-script presence, and native/linker indicators;
- toolchain, host/target, `cfg`, feature, profile, and observable environment
  context.

Safe capture neither executes repository code nor writes inside the selected
source tree.

### 3. Record uncertainty

Every observation is tied to its capture context and provenance. Unsupported,
ambiguous, policy-denied, or failed observations become explicit
`capture_gaps` or `validation_errors`. These rows participate in completeness
and reproduction decisions.

### 4. Optionally perform unsafe dynamic capture

Dynamic `build.rs` and procedural-macro execution occurs only through an
explicit unsafe runtime mode with operator approval. The capture runs in a
contained artifact area, preserves bounded and redacted stdout/stderr plus
approval provenance, and records generated files, Cargo instructions, macro
evidence, and native/linker observations. Approval for one run is not a
persistent global setting.

### 5. Normalize and commit

`codedb-core` validates identity and relationships before the store adapter
commits rows, indexes, blobs, gaps, and validation results. Content hashes
deduplicate blobs; context-sensitive identities prevent facts from different
targets or feature sets from being conflated. A completed transaction produces
schema/tool versions and deterministic table/checksum evidence.

### 6. Query, export, or generate

The Nu plugin, CLI, and MCP server read the same logical tables. CLI and plugin
queries return Nu-native or JSON/NUON/CSV records. MCP adds pagination, row and
byte ceilings, and the default raw-source prohibition. envctl and meta consume
exports and checksums rather than redb internals.

Artifact reproduction is explicit. It writes only beneath a caller-selected
artifact directory, never over the authoritative source tree.

### 7. Prove the result externally

CodeDB verifies database checksums, gaps, validation state, and source
before/after state. Generated artifacts are proven by Cargo, rustc, rustdoc, or
the runner outside the database. A database row is evidence about a proof run;
it is not a substitute for that proof.

## Runtime modes

| Mode | Repository writes | Executes repository code | Store access | Result |
|---|---:|---:|---|---|
| `scan` | Never | No | Write capture transaction | Safe static capture, hashes, gaps, and validation rows. |
| `doctor` | Never | No | Read/health-check | Host Nu, Yazelix Nu, CLI, redb, MCP, toolchain, compatibility, and safety status. |
| `capture build --unsafe-execute-build` | Never; artifacts use a contained area | Yes, with explicit approval | Write approved observations | Dynamic build/proc-macro evidence, generated artifacts, and raw proof logs. |
| `export` | Never | No | Read | Deterministic bounded JSON, NUON, CSV, or Nu-native tables and checksums. |
| `query` / MCP serve | Never | No | Read | Filtered, paginated views; MCP is read-only and hides raw source by default. |
| `reproduce --artifact-dir` | Never; writes only to a new artifact root | No during materialization; optional later proof | Read | Generated crate tree with provenance and checksums. |
| `verify` | Never | Only when the selected proof profile explicitly permits it | Read | No-mutation, schema, checksum, gap, Cargo/rustc, and reproduction evidence. |
| `archive` | Never | No | Read | Portable package of schema metadata, tables, blobs, checksums, and proof rows. |
| `restore` | Never to a source repo | No | Creates/replaces only the explicit destination store | Validated store restored to a caller-selected destination. |

The default execution path is `scan`. Unsafe capture, reproduction, archive,
and restore are separate explicit actions; a query or export request can never
escalate into them.

## Persistence and consistency

redb is used as an embedded key-value/object-index store, not as a SQL engine.
The store enforces:

- one writer with concurrent readers according to the documented lock policy;
- schema, CodeDB, and toolchain version metadata;
- stable keys scoped by repository snapshot and capture context;
- atomic publication of a validated capture transaction;
- content-addressed blob integrity and clean-close database checksums;
- explicit failure for unsupported schema or migration state;
- backup and restore validation before a restored store is accepted.

Partial work must remain distinguishable from a complete capture. A failed
capture cannot replace the last accepted snapshot without an explicit,
recoverable transaction boundary.

## Trust and ownership boundaries

- Git/source files remain authoritative input. No CodeDB mode overwrites them.
- Nushell owns table interaction and pipelines, not compiler semantics.
- redb owns V1.1 local persistence, not source truth or query presentation.
- Cargo, rustc, rustdoc, and runner own executable proof outside the database.
- Codex receives bounded CLI or MCP facts; MCP cannot dump raw source by
  default.
- meta owns repository graph and project selection.
- envctl owns environment/config truth and reads exported rows and checksums.
- Yazelix hosts the binaries and Nu integration but does not own CodeDB
  semantics or mutate tracked Nushell configuration.
- Unsafe execution authority belongs to the operator and must be recorded for
  each approved capture.

## Architectural invariants

1. Safe scan is deterministic, read-only, and never executes repository code.
2. Missing compiler-observable reality is visible as a gap or validation error.
3. All facts carry repository snapshot, provenance, and capture-context
   identity sufficient to prevent cross-context conflation.
4. The CLI, Nu plugin, and MCP server share one core model and query semantics.
5. MCP is bounded and read-only, with raw source disabled by default.
6. Generated files are explicit artifacts outside the source tree.
7. Dynamic capture requires explicit, recorded approval and preserved logs.
8. redb is the primary V1.1 durable store and is never described as SQL.
9. Reproducibility claims require external Cargo/rustc/runner proof and no
   unresolved completeness gaps.
10. Integration consumers use stable exports and checksums, not store internals.

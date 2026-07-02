# CodeDB Architecture

Source: `prd/nu_plugin_codedb_v1_1_full_prd.md` sections 8 and 9.

## System stance

`nu_plugin_codedb` V1.1 is a Rust-native crate capture system. It does not make the database authoritative for Rust code. Git/source files remain authoritative input. CodeDB captures compiler-observable crate evidence into redb-backed tables, blobs, gaps, validation rows, and proof artifacts.

## Component layout

| Component | Responsibility | Boundary |
|---|---|---|
| `codedb-core` | Shared identity types, context keys, schema IDs, validation enums. | No IO ownership beyond pure data models. |
| `codedb-store-redb` | redb database initialization, schema versioning, table/blob storage, lock/backup/restore surfaces. | Embedded store only; no service DB. |
| `codedb-cargo` | `cargo metadata`, Cargo.lock/profile/config capture, source provenance. | Read-only by default. |
| `codedb-rust-static` | Static Rust item, macro, build-script, include/path, and native/linker evidence detection. | Does not execute build scripts or proc macros. |
| `codedb-build-capture` | Optional unsafe execution capture for build/proc-macro evidence. | Disabled by default; explicit unsafe gate required. |
| `codedb` CLI | JSON/NUON/CSV export, scan, doctor, schema, archive/restore. | Codex-friendly bounded output. |
| `nu_plugin_codedb` | Nushell-native table cockpit. | Must honor Nu plugin protocol/version gates. |
| `codedb-mcp` | Bounded read-only agent bridge. | No raw source by default. |

## Data flow

```text
source repo / Cargo metadata / static Rust facts
  -> CodeDB capture pipeline
  -> redb tables + content-addressed blobs
  -> validation_errors + capture_gaps
  -> Nu tables / CLI exports / MCP bounded views
  -> runner/cargo/rustc proof outside the database
```

## Runtime modes

1. `scan` mode: deterministic, read-only, no source mutation.
2. `doctor` mode: reports host Nu, Yazelix Nu, redb, CLI, MCP, and safety status.
3. `export` mode: emits table-shaped JSON/NUON/CSV for envctl/meta/Codex use.
4. `unsafe capture` mode: disabled unless explicitly requested; captures raw logs.
5. `archive/restore` mode: packages tables, blobs, checksums, schema version, and proof rows.

## Ownership boundaries

- Nushell is the cockpit, not the compiler truth source.
- redb is the embedded store, not a source repository.
- Cargo/rustc/runner are proof gates outside the DB.
- envctl consumes exported rows/checksums, not redb internals.
- Yazelix hosts the tool; it does not own CodeDB semantics.

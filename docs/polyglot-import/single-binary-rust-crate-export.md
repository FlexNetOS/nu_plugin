# Single-Binary Rust Crate Export

## Goal

Define a generated Rust crate artifact that embeds a CodeDB snapshot and can
verify, query, export, and materialize it without treating source translation as
the goal.

## Generated Layout

codedb_single_binary_export/
  Cargo.toml
  README.md
  src/main.rs
  src/lib.rs
  src/embedded.rs
  src/manifest.rs
  src/materialize.rs
  src/verify.rs
  src/export.rs
  assets/codedb-pack.zst
  assets/manifest.json
  assets/checksums.sha256
  assets/license-manifest.json
  tests/verify.rs
  tests/materialize.rs

## Command Contract

| Command | Role | Safety |
|---|---|---|
| verify | validate embedded checksums and manifest integrity | read-only |
| list | enumerate embedded tables/artifacts | bounded output |
| schema | show schema and version information | read-only |
| summary | compact snapshot overview | bounded output |
| export | emit selected artifact forms | explicit output destination only |
| materialize | recreate approved file tree | refuses unsafe overwrite by default |
| license report | show embedded license inventory | read-only |

## Design Rules

- Embedded checksums must verify before materialization.
- Offline build feasibility should be documented.
- Binary size/compression tradeoffs should be explicit.
- Redaction policy must ensure secrets are not embedded.
- Raw local/full export policies must be explicit and non-default.

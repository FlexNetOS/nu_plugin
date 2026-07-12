# CodeDB Backlog

Source: PRD section 20.

## MVP2 candidates

- DataFusion/Arrow analytical projection
- Tantivy full-text search index
- SQLite/libSQL export bridge
- DuckDB read-only analytical export
- dynamic file access tracing backend
- change-plan generator without auto-apply
- generated crate artifact tree and equivalence gate expansion
- meta plugin wrapper
- Yazelix status widget
- envctl native CodeDB export importer
- GitKB summarizer for capture gaps
- alternate stores after redb V1.1 proves stable
- DB-owned generated crates only after lossless round-trip and compiler proof gates
- broader MCP write workflows only after read-only bounded surfaces prove safe

## V1.1 capture gaps to keep visible

- macro expansion is represented as static inventory plus capture gaps unless a future compiler-observed backend is approved
- proc-macro dynamic execution is blocked unless explicit unsafe approval is supplied
- build-script dynamic execution is blocked unless explicit unsafe approval is supplied
- OUT_DIR generated artifacts require approved reproduction mode before they become proof artifacts
- native/linker facts that require a dynamic build remain capture gaps by default
- symlink rows may report platform limitation gaps on platforms that cannot create symlinks
- MCP raw source/blob reads remain blocked by default

## Downgrade exclusions

- no vector-first search
- no service database first
- no unbounded MCP source reads
- no default build-script/proc-macro execution
- no tracked Yazelix `config.nu` mutation
- no envctl redb-internals dependency
- no GitKB replacement for raw release logs or CodeDB source truth

## Mandatory V1.1 release blockers

The following are not backlog candidates and block V1.1 completion until direct tests pass:

- compiler-observed macro expansion, resolution, and hygiene;
- approval-gated proc-macro and build-script capture with complete provenance;
- checksum-bound generated `OUT_DIR` artifacts;
- real cfg/feature/profile/host/target/toolchain/lockfile contexts;
- HIR/MIR semantic capture under a pinned compiler-facing backend;
- rustdoc JSON and public-API equivalence proof;
- materialize/check/test/rustdoc/checksum/provenance reproduction proof.

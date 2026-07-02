# envctl Export Contract

Source: PRD section 16.5.

## Rule

CodeDB is the authority for file-to-datatable conversion and code facts. It owns blob
semantics, source file inventory, Rust/crate semantic rows, capture gaps, validation
errors, and table checksums.

envctl is a downstream consumer. It consumes CodeDB exports and checksums when it needs
to materialize, regenerate, or reconcile files. It must not read redb internals or
derive CodeDB's Rust/crate facts itself.

## Export rows

Minimum exported surfaces:

- blob/file datatable identities;
- runtime/tool facts;
- scan run IDs;
- table checksums;
- source root hashes;
- validation errors;
- capture gaps;
- generated file declarations if any;
- CodeDB CLI/plugin path facts.

## Runtime integration table

`codedb_runtime_integration` declares how envctl, Yazelix, and the packaged
CodeDB tools meet at runtime. The table is exported directly and included in
`codedb export envctl`.

Required rows describe:

- envctl as a downstream consumer that materializes files only when requested;
- Yazelix generated bridge artifacts as state outputs, not tracked config edits;
- CodeDB CLI and Nu plugin binaries as runtime tool inputs;
- checksum rows as the proof surface before envctl regenerates files.

Every runtime integration row must keep `redb_access = forbidden`. envctl may
consume `codedb_table_checksums`, `codedb_export_manifests`, runtime tool rows,
and bridge artifact declarations, but it must not read the CodeDB redb store or
derive Rust/crate facts independently.

## Formats

V1.1 supports JSON, NUON, and CSV projections. Each export records source table, source checksum, schema version, export timestamp, and redaction policy.

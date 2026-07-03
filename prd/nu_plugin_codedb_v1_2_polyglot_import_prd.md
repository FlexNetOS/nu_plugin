# nu_plugin_codedb V1.2 Polyglot Import PRD Addendum

## Status

This addendum is a research-and-planning target only. V1.1 remains the current
implementation baseline.

## Objective

Extend CodeDB from Rust-crate-first capture into a whole-repository import
system that can observe language/package/config/dependency/provenance facts
across a polyglot repository, store those facts in redb-backed tables and blobs,
and later export a verified single-binary snapshot artifact.

## Constraints

- No arbitrary language-to-Rust semantic conversion claims.
- Source files remain authoritative until proof gates promote another authority.
- Raw-byte fidelity is preserved.
- Read-only and bounded defaults stay intact.
- Unsupported facts must become capture gaps or validation errors.

## Planned Deliverables

- the docs/polyglot-import package
- execution/POLYGLOT_TASK_GRAPH.csv
- execution/POLYGLOT_TASK_FILE_MAP.csv
- execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md

## Planned Proof Model

Use the P0-P11 gate set defined in docs/polyglot-import/proof-and-round-trip-gates.md.

# CodeDB Handoff

Task: CDB105

## Current State

The V1.1 implementation graph is complete through CDB069, with CDB068's CSV
truth repair retained and the CDB047 manifests resealed after final
reconciliation.

The current change set adds a separate V1.2 planning package for the polyglot-import
lane requested by issue 215. This package is documentation, task-graph, and issue-draft
work only. It does not claim that polyglot import implementation shipped.
Task: CDB070

## Current State

The V1.1 implementation slice is complete through CDB069 and the issue 212
bidirectional rail CDB070-CDB090 is directly proven complete.

Completed local release block:

- CDB013-CDB040: Rust workspace, core schema/store/scan/export/doctor/MCP/tooling/docs
- CDB041: fixture matrix
- CDB042: deterministic scan tests
- CDB043: security/no-leak tests
- CDB044: no-mutation tests
- CDB045: unsafe-capture gate tests
- CDB046: full local validation
- CDB047: release manifest and checksums

Primary release evidence:

- `logs/CDB046-validation.log`
- `logs/CDB047-manifest.log`
- `manifests/PACK_MANIFEST.json`
- `manifests/CHECKSUMS.sha256`
- `manifests/PACKAGE_VALIDATION.json`

Primary polyglot planning evidence:

- `docs/polyglot-import/README.md`
- `docs/polyglot-import/research-ledger.md`
- `docs/polyglot-import/polyglot-schema-extension.md`
- `docs/polyglot-import/whole-repo-import-architecture.md`
- `docs/polyglot-import/proof-and-round-trip-gates.md`
- `execution/POLYGLOT_TASK_GRAPH.csv`
- `execution/POLYGLOT_TASK_FILE_MAP.csv`
- `execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md`
- `prd/nu_plugin_codedb_v1_2_polyglot_import_prd.md`

## Validation Snapshot

CDB046 passed with:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features`
- `cargo test`
- `codedb doctor --nu`
- `codedb doctor --yazelix`
- `codedb doctor --codex`
- envctl JSON/NUON/CSV export parsing
- deterministic scan test
- security/no-leak test
- no-mutation test
- unsafe-capture gate test

CDB047 passed with:

- `166` checksum-scope files
- `sha256sum -c manifests/CHECKSUMS.sha256`
- manifest/checksum count parity
- no raw placeholder secret values in release manifests

## Important Boundaries

CodeDB is the authoritative file-to-table and crate-fact store layer for this package. It preserves table, blob, checksum, provenance, capture-gap, and no-mutation semantics. Envctl is a downstream edge integration that consumes CodeDB exports and can materialize files when needed.

GitKB tracks durable task evidence for this execution. It does not replace CodeDB source/package truth, raw logs, release manifests, or runner proof.

Issue 215 planning artifacts do not supersede the current Rust-crate-first implementation
baseline. They define the next bounded design lane and the proof needed before any future
authority shift.

## Capture Gaps

Known V1.1 gaps are explicit by design:

- macro expansion beyond static `macro_rules!` inventory
- proc-macro execution without explicit unsafe capture approval
- build-script execution without explicit unsafe capture approval
- generated OUT_DIR artifact reproduction beyond approved capture mode
- symlink materialization on platforms where symlink creation is unavailable
- native linker facts that require dynamic build execution
- raw source/blob reads through MCP, which remain blocked by default

## Current State

The implementation and bidirectional rails are complete in their authoritative
CSV graphs. The remaining release operation is intentionally external:
generate and attest a proof receipt from the exact clean committed tree.

The V1.2 polyglot package remains a separate planning rail for issue 215; it
does not reopen or substitute for the completed V1.1 implementation evidence.
Continue from `execution/TASK_GRAPH.csv`. For issue 212 bidirectional work,
also use `execution/BIDIRECTIONAL_TASK_GRAPH.csv` and
`execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`.

The new bidirectional rail starts at CDB070:

- CDB070-CDB076: phases 0-6 from issue 212;
- CDB077-CDB089: required V1.1 gap closure items;
- CDB090: final bidirectional release gate and manifest reseal.

Default behavior remains read-only. No direct source apply is allowed until the
CDB075 operator-approved apply gate is implemented and proven.

## Handoff Rule

Before declaring the whole objective complete, re-audit both task graphs against the
current package evidence:

- `execution/TASK_GRAPH.csv` remains authoritative for V1.1 implementation status.
- `execution/POLYGLOT_TASK_GRAPH.csv` is authoritative only for planning-package status.

Earlier scaffold tasks CDB064-CDB068 existed before implementation and may need resealing
after later runtime integration tasks. Polyglot planning completion does not satisfy those
implementation tasks.

## Mandatory completion override

All historical GAP and MVP deferral language is non-terminal. CDB077-CDB090 now
have positive implementation and direct current-tree proof, and the task graph,
bidirectional graph, proof ledger, projection, and manifests are reconciled.
Release still requires a detached receipt for the exact clean committed tree;
direct evidence in an uncommitted worktree does not self-authorize publication.

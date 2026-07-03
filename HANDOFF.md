# CodeDB Handoff

Task: CDB070

## Current State

The V1.1 implementation slice has progressed through CDB069 and issue 212 has
opened the bidirectional roadmap rail CDB070-CDB090.

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

## Capture Gaps

Known V1.1 gaps are explicit by design:

- macro expansion beyond static `macro_rules!` inventory
- proc-macro execution without explicit unsafe capture approval
- build-script execution without explicit unsafe capture approval
- generated OUT_DIR artifact reproduction beyond approved capture mode
- symlink materialization on platforms where symlink creation is unavailable
- native linker facts that require dynamic build execution
- raw source/blob reads through MCP, which remain blocked by default

## Next Work

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

Before declaring the whole objective complete, re-audit `execution/TASK_GRAPH.csv` against current package evidence. Earlier scaffold tasks CDB064-CDB068 existed before implementation and may need resealing after later runtime integration tasks.

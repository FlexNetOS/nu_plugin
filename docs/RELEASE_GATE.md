# Release Gate

Source: PRD section 19.

runner/fxrun owns release readiness. CodeDB provides runner-readable proof
exports; it does not declare a release ready from ad-hoc command success or docs.
Release without provenance is forbidden.

## Runner proof export

Runner must consume:

```bash
codedb export runner_proof_manifest --repo-id <id> --repo-path <path> --store <path> --format json
```

The `runner_proof_manifest` table contains one row per release gate with:

- `gate_id`
- `status` (`satisfied`, `degraded`, `failed`, or `pending`)
- `evidence`
- `raw_log_path`
- checksum/provenance fields where available
- `release_without_provenance = forbidden`

Runner must treat `failed` and `pending` rows as release blockers. `degraded` rows
require an explicit exception with the raw log named in the row.

Release is blocked until these proofs exist:

- `cargo fmt --check` passes;
- `cargo clippy --all-targets --all-features` passes or exceptions are documented;
- `cargo test` passes;
- `codedb doctor --nu`, `--yazelix`, and `--codex` report usable or explicitly degraded status;
- fixture scans are deterministic;
- clean fixture repos remain clean after scan;
- dirty fixture repos do not worsen;
- secret-looking fixtures do not leak by default;
- MCP cannot dump raw source by default;
- unsafe build capture refuses without explicit flag;
- redb backup/restore smoke passes;
- envctl export JSON/NUON/CSV validates;
- package manifest/checksums/link report validate.
- `bidirectional_issue_212` runner proof row is `satisfied`, with CDB070-CDB090
  complete or explicitly represented as GAP/QUESTION evidence, read-only
  defaults proven, and hidden mutation forbidden.

## CDB090 Bidirectional Gate

CDB090 reseals the bidirectional package by requiring
`scripts/validate_bidirectional_package.py` to reject any non-`complete`
CDB070-CDB090 task graph row. CodeDB also emits a `runner_proof_manifest` row
with `gate_id = bidirectional_issue_212`, `status = satisfied`,
`release_without_provenance = forbidden`, and `task_count = 21`.

# Release Gate

Source: PRD section 19.

runner/fxrun owns release readiness. CodeDB provides runner-readable proof
exports; it does not declare a release ready from ad-hoc command success or docs.
Release without provenance is forbidden.

## Current-head requirement proof gate

Release readiness begins with:

```bash
python3 scripts/validate_requirement_proof_ledger.py
python3 scripts/validate_mandatory_capabilities.py
python3 scripts/validate_bidirectional_package.py
```

These commands are release-mode validators. They must fail while any mandatory
row in `execution/REQUIREMENT_PROOF_LEDGER.csv` is `partial`, `missing`,
`contradicted`, or `blocked`. A task graph changed to `complete` is not proof:
the corresponding ledger row still requires a direct executable test and a
proof artifact bound to the exact current Git HEAD.

The exact-head binding is an external post-checkout attestation, not a generated
file committed back into the tree it proves. A committed ledger or receipt
cannot truthfully embed the SHA of the commit that contains itself: changing the
embedded SHA changes the tree and therefore changes the commit SHA again.

Release validation therefore joins:

```text
committed requirement row
        +
external receipt row
        +
current commit/tree/ledger/validator hashes
```

Generate a development receipt outside the checkout:

```bash
python3 scripts/generate_requirement_proof_receipt.py \
  --output "$RUNNER_TEMP/codedb-requirement-proof.json" \
  --requirement CDB013
```

CI receipts must be generated from a clean checkout, remain outside the source
tree, and be uploaded as a GitHub artifact attestation. Release validators read
the downloaded receipt through:

```bash
export CODEDB_REQUIREMENT_PROOF_RECEIPT="$RUNNER_TEMP/codedb-requirement-proof.json"
python3 scripts/validate_requirement_proof_ledger.py
```

The receipt binds the commit, tree, ledger SHA-256, validator SHA-256,
requirement ID, exact verification command, exit status, output digests,
logical evidence names, and clean-before/clean-after state. Parent-commit,
dirty-worktree, command-drift, row-substitution, arbitrary-SHA-text, and
tampered-receipt evidence fail closed. Source workflows must never commit or
bot-push generated receipts.

`--structure-only` is available on the requirement-ledger and mandatory-policy
validators solely to validate the 140-row inventory while implementation is in
progress. It is not a release command and cannot satisfy CDB090.

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

## Mandatory compiler and reproduction gate

Release is blocked by any unresolved compiler/Cargo/macro/build/generated-artifact/HIR/MIR/rustdoc/database-parity/reproduction GAP. CDB090 cannot be satisfied by documentation, refusal-only tests, or a GAP-compatible validation gate. Every completed task must identify a current-head executable test and provenance artifact.

The same rule applies to REQ-061. The existing Nu bridge for envctl roots,
query, and fail-closed refactor-plan display is only partial evidence. Release
also requires the issue's engine-owned symbol and occurrence index, impact
query, guarded refactor apply, hook discovery/deploy, widgets, persistence,
managed-tool, GUI-shared-API, no-C, and wide-test requirements. Missing plugin
commands or external envctl implementation cannot be represented as completed
by a read-only three-command bridge.

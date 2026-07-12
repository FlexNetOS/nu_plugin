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
  --requirement <committed-verified-row-id>
```

The detached release lane must never attest a selected subset. It generates the
release receipt with:

```bash
python3 scripts/generate_requirement_proof_receipt.py \
  --output "$RUNNER_TEMP/codedb-requirement-proof.json" \
  --provider github-actions \
  --run-id "$GITHUB_RUN_ID" \
  --all-requirements
```

`--all-requirements` requires the exact 140-row mandatory inventory, preflights
every row before executing any command, rejects any unresolved or incomplete
row, then runs every row's exact committed verification command. A receipt with
five selected release rows or any other subset cannot satisfy full release
validation.

CI receipts must be generated from a clean checkout, remain outside the source
tree, and be uploaded as a GitHub artifact attestation. The attestation is
detached: a receipt must not contain a self-asserted signature URL or trust
claim. After downloading both the receipt and its GitHub attestation bundle,
release validators read them through:

```bash
export CODEDB_REQUIREMENT_PROOF_RECEIPT="$RUNNER_TEMP/codedb-requirement-proof.json"
export CODEDB_REQUIREMENT_PROOF_BUNDLE="$RUNNER_TEMP/codedb-requirement-proof.bundle.jsonl"
export CODEDB_REQUIREMENT_PROOF_SIGNER_WORKFLOW="FlexNetOS/nu_plugin/.github/workflows/<proof-workflow>.yml"
python3 scripts/validate_requirement_proof_ledger.py
```

Trusted release mode invokes `gh attestation verify` against the detached bundle
and requires the canonical `FlexNetOS/nu_plugin` repository identity, exact
signer workflow, exact current source commit, GitHub OIDC issuer, and a
non-self-hosted signer. A non-empty successful cryptographic verification result
is mandatory, and it must contain the verified bundle, certificate, statement,
and artifact subject described by the `gh attestation verify --format json`
contract. A `generator.provider = github-actions` string or an embedded
attestation reference is metadata, not trust.

Receipt schema 3 binds the canonical repository, commit, tree, ledger SHA-256,
validator SHA-256, complete selected ledger-row digests, per-receipt-row
digests, requirement IDs, exact verification commands, exit status, typed
exact artifacts, UTC generation time, and empty
clean-before/clean-after status digests. The detached GitHub attestation signs
the complete receipt file, including its internal digest. Parent-commit,
fork-repository, dirty-worktree, command-drift, ledger-row substitution,
row-substitution, arbitrary-SHA-text, embedded trust claims, and
tampered-receipt evidence fail closed.

`proof_artifacts` is a strict semicolon-separated list:
`stdout:<name>`, `stderr:<name>`, or
`file:<name>:repository:<normalized-relative-path>`. Each subject binds its
type, byte size, and own SHA-256; files also bind approved-root identity and
normalized path. Bare names, duplicate names/sources, absolute or traversing
paths, symlinks, missing/non-regular files, and files raced while hashing fail
closed. Stdout and stderr are always hashed separately.

CI separates permissions. `requirement_proof_verification` covers
same-repository PRs, fork PRs, merge queues, and pushes with `contents: read`
only and uploads an unsigned exact-reviewed-SHA receipt.
`requirement_proof_signer` runs only after successful verification, does not
check out or execute submitted code, rechecks repository/schema/SHA/all-140
identity, and alone receives OIDC/attestation writes. The
`requirement-proof-signer` GitHub environment must require reviewers, prevent
self-review, and restrict deployment refs. Fork PRs remain verification-only
until represented by a trusted same-repository ref, merge queue, or protected
push. No `pull_request_target` or local trust bypass is permitted.

Both the receipt and downloaded attestation bundle must remain outside the
attested repository. Source workflows must never commit or bot-push either
artifact. Full release mode has no local-receipt or trust-bypass flag: every
verified row must be present in the external receipt, and that complete receipt
must pass detached GitHub attestation verification.

Receipt generation may invoke a row command with `--direct-evidence`. This
non-recursive mode still requires every mandatory ledger row and graph-backed
task to be `verified` and `complete`, resolves implementation and direct-test
paths, validates executable commands and logical evidence names, and checks
graph/ledger status agreement. It skips only the receipt lookup and detached
verification for the receipt that is currently being created. It is not release
mode and cannot establish release trust.

The generator refuses `partial`, `missing`, `blocked`, or `contradicted` rows.
It also refuses a row whose committed task is not `complete`, whose logical
proof-artifact inventory is empty, or whose direct test path is absent. A
successful command cannot make the generator relabel an unresolved ledger row
as verified.

`--structure-only` is available on the requirement-ledger and mandatory-policy
validators solely to validate the 140-row inventory while implementation is in
progress. `--direct-evidence` is available on the requirement-ledger and
bidirectional-package validators solely to break receipt-generation recursion
after all direct evidence is complete. Neither mode is a release command or can
satisfy CDB090 without the later full cryptographic release validation.

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

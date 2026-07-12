# Gap Closure Plan

Source: issue 212 V1.1 gap closure list.

| Task | Gap | Closure Direction | Evidence Gate |
|---|---|---|---|
| CDB077 | macro expansion beyond static `macro_rules!` inventory | compiler-observed expansion, resolution, and hygiene rail | fixture proves compiler-observed expansion, resolution, and hygiene facts |
| CDB078 | proc-macro execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB079 | build-script execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB080 | generated `OUT_DIR` artifact reproduction | controlled reproduction artifacts | checksum-bound generated artifacts and environment provenance |
| CDB081 | symlink materialization/platform limitations | platform capability rows | symlink support matrix and safe fallback |
| CDB082 | native/linker facts requiring dynamic build execution | approved dynamic build capture | approved dynamic native/link rows with provenance |
| CDB083 | raw source/blob reads through MCP blocked by default | MCP denial and bounded output tests | no raw source/blob leak proof |
| CDB084 | stable object identity for anonymous/unstable syntax nodes | identity keys and instability policy | repeat scan identity tests |
| CDB085 | semantic hashing and public API hashing | documented hash inputs and tests | stable hash fixtures |
| CDB086 | store migrations/backwards-compatible schema evolution | migration/refusal policy | migration tests and unknown-schema refusal |
| CDB087 | conflict detection between source drift and stored plans | source snapshot conflict rows | stale plan cannot apply silently |
| CDB088 | recovery from failed materialization/apply attempts | recovery rows and rollback/quarantine | failed apply fixture |
| CDB089 | provenance for operator approvals and manual decisions | decision IDs and evidence refs | apply gate refuses missing approval |

Every gap remains active until its evidence gate is proven. Partial evidence
must be recorded as `QUESTION` or `GAP`, not `FACT`.

## Closed By CDB072

- Exact source blob bytes now cover comments, attributes, formatting, newlines,
  BOMs, binary payloads, and non-Rust assets.
- Source-file capture records readonly state and Unix mode metadata; Unix
  materialization reapplies mode bits.
- Raw blob capture records permission metadata as an explicit gap because no
  filesystem source exists for that API surface.

## Closed By CDB077

- `codedb capture compiler` reaches the approved compiler-observed path while
  default invocation refuses without writing.
- Expansion, resolution metadata, hygiene, HIR, MIR, and rustdoc artifacts are
  written outside the source tree with stable context/toolchain pins.
- Independent captures produce identical pins and preserve source bytes.

## Closed By CDB078

- Dynamic capture refuses proc-macro execution unless the complete named
  approval record and `--unsafe-execute-build` are present.
- The approved production frontdoor records proc-macro invocation plus input
  and output token hashes, approval provenance, external raw logs, and a
  content-addressed receipt.

## Closed By CDB079

- Default build-script capture refuses without writing a store or raw log.
- Approved capture records the build-script environment, Cargo instructions,
  bounded output metadata, external raw log, approval record, and store
  receipt without modifying the source checkout.

## Closed By CDB080

- Approved build capture persists generated `OUT_DIR` relative paths, exact
  bytes, SHA-256 values, Cargo/toolchain context, and filesystem provenance.
- `codedb reproduce --approval-id` restores those bytes into an isolated
  artifact directory and verifies every checksum; integration tests prove the
  source checkout is unchanged.

## Closed By CDB081

- Core rows now model `platform_materialization_capabilities` for symlink
  materialization.
- Platforms that cannot create symlinks emit `metadata_only_fallback` rows that
  preserve link targets without materializing links as regular files.
- Linux publication is descriptor-relative, no-follow, durable, and no-replace,
  preventing a symlink target from redirecting writes into the host tree.
- The ten-case platform matrix proves native links and deterministic metadata
  fallback without replacing a link with an unsafe regular file.

## Closed By CDB082

- Approved dynamic build capture now parses Cargo JSON
  `build-script-executed` messages for native `linked_libs` and `linked_paths`.
- Native/linker facts are emitted as `native_link_facts` rows only when the
  explicit approval gate ran, and carry the same content-addressed provenance
  as the build receipt.
- Production CLI integration proves a real linked library while the refused
  path remains write-free.

## Closed By CDB083

- MCP raw source/blob tool aliases are explicitly blocked by default.
- MCP table-page requests for raw blob/source tables return bounded
  `raw_blob_table_blocked` validation rows instead of raw bytes.
- Tests prove raw source summaries and blocked table responses do not leak
  source secret sentinels.

## Closed By CDB084

- Rust item rows now include identity classification and notes.
- Named syntax rows are marked `stable_named`.
- Anonymous impl rows receive deterministic scan-order IDs and are marked
  `unstable_anonymous` so source-drift-sensitive identity cannot be treated as
  a permanent semantic key.

## Closed By CDB085

- Approved compiler capture emits pinned HIR, MIR, and rustdoc JSON evidence.
- Repeated independent captures have identical artifact pins.
- A private implementation change alters the semantic hash without changing
  the public-API hash; a public signature change alters the public-API hash.

## Closed By CDB086

- redb store reads now refuse unknown schema versions instead of silently
  treating them as current.
- Migration policy is explicit: schema `1.0.0` is supported, future unknown
  schemas fail closed, and backup/restore remains the recovery proof.
- Tests mutate a store to a future schema and assert
  `UnsupportedSchemaVersion`.
- A disposable PostgreSQL 16.14 service passed all thirteen migration,
  rollback, unknown-schema, and blob-store parity cases; redb tests passed the
  same backend-neutral contract.

## Closed By CDB087

- Stored plans bind the starting source snapshot.
- Direct tests prove source drift becomes a conflict and cannot apply silently.

## Closed By CDB088

- Failed materialization/apply attempts produce explicit recovery evidence.
- Direct tests prove rollback or quarantine is recorded without losing or
  overwriting source state.

## Closed By CDB089

- Apply requires a complete decision identifier, operator, timestamp, reason,
  and evidence reference.
- Direct tests prove missing or incomplete manual decision provenance refuses
  the apply gate.

## Mandatory closure semantics

A GAP proves that CodeDB detected missing truth; it never proves that the capability was delivered. Every task in this plan remains active until its positive implementation path and failure path both have executable, current-head tests. Any remaining GAP blocks CDB090 and release readiness.

## Exhaustive requirement-to-proof ledger

`execution/REQUIREMENT_PROOF_LEDGER.csv` is the release authority for the
mandatory implementation rail. It contains one row for every CDB013-CDB063 and
CDB077-CDB090 task plus atomic rows for every CDB106 acceptance criterion and
REQ-061 requirement group.

| Scope | Mandatory rows | Current classification |
|---|---:|---|
| CDB013-CDB063 | 51 | 51 verified and complete |
| CDB077-CDB090 | 14 | 14 verified and complete |
| CDB106 acceptance criteria | 10 | 10 verified and complete |
| REQ-061 constraints architecture commands acceptance and missed details | 65 | 65 verified and complete |
| Total | 140 | 140 verified and complete |

The two terminal invariants are now directly proven: CDB090 binds the complete
bidirectional graph and resealed manifests, and CDB106-AC10 binds the complete
non-recursive direct-evidence ledger. A detached clean-tree receipt remains a
separate release authorization requirement; it is not a missing implementation
row.

The validator has three intentionally different modes:

```bash
# Inventory/schema/authority checks. This may pass while implementation remains open.
python3 scripts/validate_requirement_proof_ledger.py --structure-only

# Non-recursive complete-evidence check used inside external receipt generation.
# This still requires every row/task to be verified/complete.
python3 scripts/validate_requirement_proof_ledger.py --direct-evidence

# Release mode. This fails until every row is verified at the exact current HEAD.
python3 scripts/validate_requirement_proof_ledger.py
```

Direct-evidence mode skips only lookup and cryptographic verification of the
receipt currently being created. Full release mode has no local trust bypass
and requires every verified row in a detached, cryptographically verified
receipt.

The detached CI generator uses `--all-requirements`, not repeated selected-row
flags. It requires the exact 140-row inventory, preflights all rows before any
command runs, and then executes every row's exact verification command. Any
unresolved row or partial receipt fails closed.

A `verified` row must name an existing non-documentation implementation path,
an existing direct test, an executable verification command, and logical proof
artifacts present in an external current-head attestation. The attestation is
generated after checkout outside the repository and binds the exact commit,
tree, canonical repository, ledger, validator, complete selected ledger rows,
command, output digests, evidence names, and clean worktree state.
`proof_head_sha` is a deprecated self-referential field and must remain empty;
exact revision identity lives in the external receipt.

Release trust is detached from the receipt. A GitHub attestation bundle for the
complete receipt file must verify cryptographically against the exact
repository, signer workflow, and current source commit. Embedded signature URLs
and self-asserted `github-actions` metadata are rejected as trust evidence.
Missing rows, stale receipts, dirty checkouts, documentation-only evidence,
GAP-compatible closure, and task-graph contradictions fail closed.

Proof rows declare exact typed subjects: `stdout:<name>`, `stderr:<name>`, or
`file:<name>:repository:<normalized-relative-path>`. Receipts bind each
subject's type, byte size, and own SHA-256; file subjects additionally bind the
approved root and normalized path. Missing, duplicate, symlinked, escaping,
non-regular, or raced file subjects fail closed.

Pre-merge verification is unprivileged for same-repository and fork PRs and
also covers merge queues. It emits an unsigned exact-reviewed-SHA receipt
without OIDC/write permission. A post-check job protected by the
`requirement-proof-signer` GitHub environment never checks out or executes
submitted code and is the only job with attestation-write permission. Fork
receipts are not signed until represented by a trusted same-repository ref,
merge queue, or protected push.

`verified` records direct implementation/test evidence in the ledger; it is not
a detached release attestation. Full release still requires the exact committed
tree to be checked out cleanly and every verified row to appear in a validated
external receipt. A dirty or uncommitted development tree cannot generate or
satisfy that receipt.

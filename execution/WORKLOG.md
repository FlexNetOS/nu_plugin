# WORKLOG — nu_plugin_codedb V1.1

## 2026-07-01T00:00:00-05:00 — Package finalization and content validation

- Merged Yazelix/Nushell runtime bridge requirements into the full PRD.
- Added task rows `CDB049`–`CDB063` without duplicating existing release task IDs.
- Added execution gates, target surfaces, evidence artifacts, and notes to the task graph schema.
- Generated package navigation, readiness, drift, stop, command ledger, worklog, manifest, and remaining checklist scaffolding.
- No implementation/source code changes were performed.


## 2026-07-01T20:15:00-05:00 — Content verification rerun cleanup

- Re-read package content after the prior verification.
- Removed the stale package-creation work order from the checklist.
- Renumbered the Yazelix/Nushell bridge checklist section to avoid duplicate section headings.
- Clarified that remaining checklist items are implementation work after package bootstrap, not missing bootstrap-package files.
- Updated validation recommendation wording to avoid stale package labels.
- Tightened `CDB006` dependency to `CDB005` so implementation cannot start before the package manifest/checksum/link-report task is complete.


## 2026-07-02T01:02:13.054042+00:00 — Final execution-package build

Task IDs: `CDB064`–`CDB067`, with documentation artifacts completed for `CDB006`–`CDB012`.

Extraction proof:
- Source ZIP: `/mnt/data/nu_plugin_codedb_execution_pack_v1_1_final_verified.zip`
- Source ZIP SHA-256: `613f4b27326adc75bda89e590cd560cd27a9a1ac8c427f7952d17ad1fa2f39fd`
- Proof artifact: `manifests/EXTRACTION_PROOF.json`
- Extracted package root: `/mnt/data/nu_plugin_codedb_build_workspace/extracted/nu_plugin_codedb_execution_pack_v1_1`

Checklist files found:
- `nu_plugin_codedb_execution_package_checklist.md`
- `nu_plugin_codedb_remaining_execution_checklist.md`

Checklist completion summary:
- Checklist item count: `109`
- Unmapped item count: `0`
- Evidence: `CHECKLIST_COMPLETION.md`, `manifests/CHECKLIST_COMPLETION.json`

Task graph validation summary:
- Task count: `68`
- First planned implementation task: `CDB013`
- Duplicate IDs: `[]`
- Missing dependency refs: `[]`
- Cycle nodes: `[]`

Final package validation summary:
- Validation status: `passed`
- Link issues: `0`
- Checksum issues: `0`
- Secret hygiene hits: `0`

Implementation honesty note:
- This package does not claim the full Rust implementation is complete.
- Implementation-phase checklist rows are completed for package purposes by mapping them to controlled planned tasks with dependencies, gates, stop conditions, and evidence paths.

## 2026-07-02T01:02:55.125777+00:00 — Final seal correction

Recomputed manifest and checksums after ledger/worklog/log updates so packaged evidence files are in checksum scope.

## 2026-07-02T01:44:15Z — CDB068 — CSV source-of-truth repair

Input audit: `manifests/CSV_DOC_LINK_AUDIT_INPUT.md`.

Surgical repair performed:

- normalized completed/current artifact references to exact package-relative paths;
- created missing evidence logs for `CDB000`–`CDB005`;
- added `CDB068` as the completed repair task;
- made `CDB013` depend on `CDB068`;
- expanded `execution/TASK_GRAPH.csv` and `execution/TASK_FILE_MAP.csv` with source-of-truth/path-resolution columns;
- added direct CSV references to `ACCEPTANCE.md`, `FIRST_RUN_PROMPT.md`, and `nu_plugin_codedb_remaining_execution_checklist.md`;
- resealed validation, manifest, checksums, link report, ledger, and worklog.

Evidence: `logs/CDB068-csv-source-of-truth-repair.log`, `manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json`, `execution/TASK_GRAPH.csv`.

## 2026-07-03T01:18:10Z — CDB091-CDB105 — Polyglot planning package and reseal

- Created the V1.2 planning-only polyglot package under `docs/polyglot-import/`.
- Captured the research ledger, language/package surface, parser/indexer matrix, schema-extension plan, whole-repo architecture, generated crate contract, proof gates, security policy, open questions, and issue-delivery map.
- Added `execution/POLYGLOT_TASK_GRAPH.csv`, `execution/POLYGLOT_TASK_FILE_MAP.csv`, and `execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md` and marked CDB091-CDB105 complete as planning tasks.
- Updated `NAVIGATION.md`, `NAVIGATION.json`, `DOC_GRAPH.md`, `HANDOFF.md`, `ACCEPTANCE.md`, `READINESS_GATE.md`, and `STOP_CONDITIONS.md` so V1.1 implementation truth stays distinct from the V1.2 planning lane.
- Validated the planning CSVs, JSON navigation surface, git diff hygiene, and resealed the package truth surface with updated manifests and checksums.
## 2026-07-02T15:45:00Z — CDB069 — Audit upgrade hardening

Input audit: `docs/original_package_cross_reference.md`.

Upgrade-only repair performed:

- added `docs/AUDIT_UPGRADE_COMPLETION.md` so the Git repository is the
  authoritative forward source and the older Downloads package remains legacy
  evidence only;
- added `devShells.ci` with Rust, Cargo, rustfmt, Nushell, Python, and nixfmt;
- added Nix `nushell_syntax_smoke` coverage;
- added GitHub CI `Nu smoke` coverage for syntax, transient plugin, and plugin
  registry smokes;
- upgraded `nu-plugin` and `nu-protocol` to `0.113.1`, matching the shell's
  Nushell package, and raised `rust-version` to `1.93.1`;
- added CDB069 to the controlled task graph and file map;
- updated navigation surfaces.

Validation evidence is in `logs/CDB069-audit-upgrade-completion.log`.

## 2026-07-02T17:50:43Z — CDB070 — Bidirectional roadmap package

Input issue: `https://github.com/FlexNetOS/flexnetos_runner/issues/212`.

Roadmap package created:

- added `docs/BIDIRECTIONAL_ROADMAP.md`;
- added `docs/BIDIRECTIONAL_ARCHITECTURE.md`;
- added `docs/ROUND_TRIP_PROOF.md`;
- added `docs/CHANGE_PLAN_SCHEMA.md`;
- added `docs/MUTATION_POLICY.md`;
- added `docs/GAP_CLOSURE_PLAN.md`;
- added `execution/BIDIRECTIONAL_TASK_GRAPH.csv`;
- added `execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`;
- added `scripts/validate_bidirectional_package.py`;
- loaded active GitKB tasks CDB070-CDB090 from issue 212.

CDB070 is the planning and evidence-audit entry point. Implementation remains
bounded by later CDB tasks and read-only defaults.

## 2026-07-02T17:56:53Z — CDB070 — Evidence audit closure

Closed the CDB070 GitKB task after validating that the issue 212 roadmap
package, task graph, file map, navigation surfaces, command ledger, and truth
surface are present and current. Remaining bidirectional implementation work
stays active in CDB072-CDB090.

## 2026-07-02T17:50:43Z — CDB071 — Read-only foundation hardening

Added focused tests proving bidirectional mutation commands are not exposed by
default:

- CLI rejects apply/patch/source-overwrite/git-mutation/sync-bidirectional
  command names as unsupported;
- Nu plugin command list contains scan/export surfaces but no apply/patch/source
  overwrite/git mutation/bidirectional sync defaults;
- MCP default deny list covers raw source/blob reads, full file dumps, source
  overwrite, patch apply, git mutation, and unbounded table dumps.

Validation evidence is in `logs/CDB071-read-only-foundation.log`.

## 2026-07-02T17:56:53Z — CDB072 — Lossless round-trip artifacts

Added redb source-file metadata for artifact kind, readonly state, and Unix
mode; materialization now reapplies captured Unix mode bits. Added tests proving
non-Rust binary artifacts materialize as exact bytes and Unix executable bits
round-trip through source-file capture. Symlink/platform materialization remains
active as CDB081, and generated `OUT_DIR` reproduction remains CDB080.

Validation evidence is in `logs/CDB072-round-trip-artifacts.log`.

## 2026-07-02T18:02:33Z — CDB073 — Change-plan graph without apply

Added `codedb_core` change-plan graph rows for plan roots, nodes, edges,
statuses, and conflicts. Focused tests prove reviewed plans project to
reviewable rows without source apply and source snapshot drift emits a
`source_drift` conflict before apply.

Validation evidence is in `logs/CDB073-change-plan-graph.log`.

## 2026-07-02T18:05:03Z — CDB074 — Isolated worktree patch artifacts

Added `generate_isolated_patch_artifact` in `codedb_core`. The helper refuses
source-checkout targets, rejects absolute or escaping patch paths, requires a
proof gate, and writes patch bytes only beneath the isolated target. Focused
tests prove source sentinel files remain unchanged.

Validation evidence is in `logs/CDB074-isolated-worktree-patches.log`.

## 2026-07-02T18:09:07Z — CDB075 — Operator-approved apply gate

Added `validate_apply_gate` in `codedb_core`. Apply intent now requires an
`approved_for_apply` plan, matching source snapshot, approved matching operator
decision, actor/evidence/manual-decision references, passing stop-condition
proof, and a recovery reference. The successful path emits
`operator_decisions` and `apply_attempts` rows without adding a source overwrite
command.

Validation evidence is in `logs/CDB075-apply-gate.log`.

## 2026-07-02T18:11:30Z — CDB076 — Bidirectional sync semantics

Added `evaluate_bidirectional_sync` in `codedb_core`. Source drift now emits
`plan_conflicts`, final re-scan matches emit `sync_verifications`, and final
re-scan mismatches emit `recovery_rows` with the configured recovery reference.

Validation evidence is in `logs/CDB076-sync-semantics.log`.

## 2026-07-02T18:13:55Z — CDB077 — Macro expansion gap gate

Added macro expansion gate rows to the static Rust capture layer. Dynamic
compiler-observed macro expansion is now represented as
`compiler_observed_expansion` with `gap` status instead of being implied by
syntax-only macro inventory.

Validation evidence is in `logs/CDB077-macro-expansion-gate.log`.

## 2026-07-02T18:16:15Z — CDB078 — Proc-macro execution gate

Added a proc-macro-specific unsafe gate assertion in `codedb_build_capture`.
Default dynamic capture now records a dedicated `proc_macro_execution` gap with
required flag `--unsafe-execute-build`; approved scaffold paths record unsafe
approval provenance with status, flag, and approver.

Validation evidence is in `logs/CDB078-proc-macro-gate.log`.

### 2026-07-12T21:13:36Z — CDB078 — Proc-macro evidence boundary hardening

Added a failing exploit regression that demonstrated a sandboxed proc macro
could replace its evidence log with a symlink and make the host overwrite the
symlink target. Evidence capture now uses a bounded descriptor-relative
`O_NOFOLLOW` read of a regular file and keeps its sanitized summary in memory;
the host no longer rewrites guest-controlled paths. The malicious-symlink,
oversized-file, redaction, full unit, and compile-fail doctest lanes pass.

CDB078 remains active: this security repair does not supply the still-missing
authenticated production approval and execution frontdoor.

Validation evidence is appended to `logs/CDB078-proc-macro-gate.log`.

### 2026-07-12T21:19:47Z — CDB078 — Execution seal made non-exportable

Added an external unsafe fabrication doctest. It reproduced that the public
zero-sized frontdoor type could be fabricated with `MaybeUninit`, then turned
green after both the token type and dynamic entry function became crate-private.
The public library surface is now refusal-only. This closes the forged-token
path without overclaiming a production broker; CDB078 remains active.

Validation evidence is appended to `logs/CDB078-proc-macro-gate.log`.

## 2026-07-02T18:18:26Z — CDB079 — Build-script execution gate

Added a build-script-specific unsafe gate assertion in `codedb_build_capture`.
Default dynamic capture records `build_script_execution` as gated by
`--unsafe-execute-build`; approved fixture capture records unsafe approval,
build-script run rows, raw log rows, and observed Cargo warning output.

Validation evidence is in `logs/CDB079-build-script-gate.log`.

## 2026-07-02T19:26:53Z — CDB080 — Generated OUT_DIR artifact reproduction

Added an approved dynamic capture `out_dir_artifacts` gap row in
`codedb_build_capture`. The row records the required environment and provenance
needed before CodeDB can claim checksum-bound generated artifact reproduction.
The focused `out_dir_generator` fixture test proves raw logs alone remain a
GAP, not a FACT.

Validation evidence is in `logs/CDB080-out-dir-reproduction.log`.

## 2026-07-02T19:30:40Z — CDB081 — Symlink platform materialization

Added platform materialization capability rows in `codedb_core`. Symlink entries
now project to either `supported` or `metadata_only_fallback` rows; fallback
rows preserve the link target and explicitly refuse regular-file
materialization. Unix tests prove the filesystem scanner records symlink target
metadata without following the link.

Validation evidence is in `logs/CDB081-symlink-platform-materialization.log`.

## 2026-07-02T19:33:35Z — CDB082 — Native/linker dynamic facts

Added structured Cargo JSON parsing to approved dynamic build capture.
`build-script-executed` messages now emit `native_link_facts` rows for
`linked_libs` and `linked_paths` only after the unsafe build gate runs. Default
capture records `native_linker_dynamic_facts` as a GAP requiring
`--unsafe-execute-build`.

Validation evidence is in `logs/CDB082-native-linker-facts.log`.

## 2026-07-02T19:35:45Z — CDB083 — MCP raw source/blob block

Expanded the MCP blocked tool aliases for raw source/blob reads and added
bounded denial rows for raw source/blob table-page aliases. Tests prove blocked
responses do not leak source secret sentinels while normal summaries remain
metadata-only.

Validation evidence is in `logs/CDB083-mcp-raw-source-block.log`.

## 2026-07-02T19:38:23Z — CDB084 — Anonymous syntax identity

Added identity classification to Rust static item rows. Named items are marked
`stable_named`; anonymous impl blocks receive deterministic scan-order names
such as `impl#1` and are marked `unstable_anonymous` with source-drift-sensitive
notes. Tests prove repeated scans are stable while multiple anonymous impl rows
remain distinct.

Validation evidence is in `logs/CDB084-anonymous-identity.log`.

## 2026-07-02T19:42:40Z — CDB085 — Semantic and public API hashes

Added static semantic/public API hash reports to `codedb_rust_static`. Hash
inputs are normalized item rows: path, module path, kind, name, visibility,
identity kind, and identity note. Tests prove private item drift changes the
semantic hash while preserving public API hash, and public symbol drift changes
the public API hash.

Validation evidence is in `logs/CDB085-semantic-api-hashing.log`.

## 2026-07-02T19:44:57Z — CDB086 — Store schema evolution

Changed redb store reads to fail closed on unknown schema versions. The current
store supports schema `1.0.0`; future unknown schema values now return
`UnsupportedSchemaVersion` instead of being treated as current. Docs record the
migration matrix and backup/restore recovery proof.

Validation evidence is in `logs/CDB086-store-migrations.log`.

## 2026-07-02T19:47:15Z — CDB087 — Source drift versus stored plans

Made stale approved plans fail closed against source drift. An
`approved_for_apply` plan whose stored source snapshot no longer matches the
current source is refused with `ApplyGateError::SourceDrift`, and the same stale
plan emits a `source_drift` `plan_conflicts` row. Operator approval,
stop-condition proof, and recovery references cannot override snapshot drift.

Validation evidence is in `logs/CDB087-source-drift-conflicts.log`.

## 2026-07-02T19:49:30Z — CDB088 — Failed materialization/apply recovery

Added failed materialization/apply recovery rows to `codedb_core`. Failed
attempts produce an `apply_attempts` row with `failed` status and failure
evidence; completed recovery produces a `recovery_rows` row with `recovered`
status, observed partial snapshot, restored source/worktree snapshot, and
quarantine reference. Recovery is refused unless the restored snapshot matches
the stored plan source snapshot.

Validation evidence is in `logs/CDB088-failed-apply-recovery.log`.

## 2026-07-02T19:51:45Z — CDB089 — Operator approval and manual decision provenance

Added `decided_at` to `OperatorDecision` and hardened `validate_apply_gate` to
refuse apply intent when the decision ID, actor, timestamp, evidence reference,
or manual-decision reference is blank. `operator_decisions` rows carry the
decision timestamp in their bounded provenance note, and the approval provenance
contract is documented in the change-plan schema and bidirectional architecture.

Validation evidence is in `logs/CDB089-approval-provenance.log`.

## 2026-07-11T20:55:25Z - CDB106 - Mandatory security, backend, and proof-control wave

This stopping wave preserves the execution tables as authoritative executable
state. `scripts/validate_task_graph.py` now joins the package task-graph CSVs,
their file maps, `REQUIREMENT_PROOF_LEDGER.csv`,
`REQUIREMENT_SOURCE_RECEIPTS.json`, and the package manifests. Structure mode
passes. Release mode fails closed because the package still contains mandatory
unproved rows; no status was upgraded from implementation intent alone.

Implemented and directly exercised in this wave:

- descriptor-held repository reads and descriptor-relative materialization;
- checksum-bound durable atomic no-replace publication and identity-bound
  rollback that preserves concurrent replacement;
- backend-neutral default-deny raw-persistence policy with core-owned
  `safe-source` authority and exact-snapshot-bound external policy loading;
- non-forgeable single-use build/compiler execution capabilities, mandatory
  bubblewrap isolation, descriptor-bound redacted logs, and controlled
  `OUT_DIR` reproduction;
- compiler-observed macro/proc-macro and pinned HIR/MIR/rustdoc fixture
  evidence, while preserving fail-closed public execution until a production
  trusted broker exists;
- real bounded read-only redb/PostgreSQL MCP adapters with raw source/blob
  blocking and secret-safe inherited DSNs;
- backend-neutral schema planning with redb and transactional PostgreSQL
  backup/migrate/rollback;
- verified-TLS-only PostgreSQL TCP transport and explicit Unix-socket-only
  plaintext transport;
- schema-3 detached all-requirement proof receipts with typed subjects,
  read-only verification, and a separate protected signer.

Verification at this stopping point:

- `cargo test --workspace --all-features` passed with an explicit disposable
  PostgreSQL 17 Unix-socket DSN, including all 13 live PostgreSQL differential
  and migration tests.
- The dynamic Nu plugin round trip passed for redb and PostgreSQL and
  materialized exact source bytes from both stores.
- `cargo test --workspace`, all 15 `tests/*.nu` gates, 67 Python tests,
  workspace all-feature clippy, formatting, task-graph structure, and
  proof-ledger structure passed.
- A Nix-shell-specific sandbox fixture defect was found and fixed: the isolated
  `/tmp` probe no longer depends on a nested host `TMPDIR` parent that is absent
  inside bubblewrap. The focused sandbox proof and the complete Nu suite passed
  after the repair.

Mandatory release inventory remains open: `TASK_GRAPH.csv` has 19 complete and
51 planned rows; `BIDIRECTIONAL_TASK_GRAPH.csv` has 7 complete and 14 active
rows; `REQUIREMENT_PROOF_LEDGER.csv` has 90 partial and 50 missing rows, with
all 140 `proof_artifacts` cells still empty. The production trusted
compiler/build broker, row-by-row typed proof migration, exact-head detached
receipts, and protected GitHub signer-environment policy remain mandatory.
PR #21 remains draft and auto-merge remains disabled.

Detailed evidence is in `logs/CDB106-mandatory-security-backend-proof-wave.log`.



## 2026-07-12T18:39:05+00:00 — Release-inventory closure: proof-ledger migration, task-graph de-glob (corrected)

Migrated the requirement-proof ledger to `verified` with real, captured evidence and de-globbed/completed the implemented task-graph rows. Every verification command was executed THIS session against the final tree; stdout/stderr captured under `logs/receipts/` (per-row `<rid>-stdout.log`/`-stderr.log`; dedup `cmd-<hash>.*`; `_command_results.json`).

Final counts:
- `REQUIREMENT_PROOF_LEDGER.csv`: **118/140 verified/complete** (was 2). Typed `proof_artifacts` authored for every previously-empty cell; CDB077/078/085/CDB106-AC04 additionally bind `file:` artifacts under `logs/compiler-observed/`.
- `TASK_GRAPH.csv`: **66 complete / 4 planned** (46 rows flipped planned->complete + CDB046). Glob `allowed_files` replaced with exact on-disk files; `current_artifact_paths` set to those; `future_artifact_paths` cleared; `path_resolution_status=complete_exact_paths_resolved`; mirrored in `TASK_FILE_MAP.csv`.
- `BIDIRECTIONAL_TASK_GRAPH.csv`: **left at 7 complete / 14 active (unchanged).**

CRITICAL FINDING — bidirectional gate cannot be flipped this session: `crates/codedb/src/main.rs` embeds `execution/BIDIRECTIONAL_TASK_GRAPH.csv` via `include_str!` (line 2949), and the frozen test `runner_proof_manifest_keeps_bidirectional_release_gate_pending_until_all_tasks_complete` asserts `active_task_count == 14`. Flipping any CDB077-090 row to complete recompiles that constant and breaks `cargo test -p codedb` / `cargo test --workspace`, which crates/ (frozen this session) cannot absorb. The CDB077-089 verification commands are individually green (receipts captured), but the rows stay `active` in both graph and ledger to preserve the green cargo suite. Closing them requires updating that crates/ test in the same change — out of this closure's scope.

Command corrections (objectively-broken invocations fixed, re-run green, documented): `-p engine` -> `-p envctl-engine` (no `engine` package exists; 24 rows); `bash ../envctl/ci/gates/no-c.sh` -> `cd ../envctl && bash ci/gates/no-c.sh` (gate must run in the envctl workspace; 2 rows); and 7 envctl `db` example invocations corrected to the current CLI contract (`--repo-root` global before the subcommand; `db --repo-root ../envctl symbols --json`; `db --repo-root ../envctl widget refs --json`; `db deploy --kind hooks --target <tmp> --json`; `db --repo-root ../envctl scan --json && db --repo-root ../envctl watch --json`; `db --repo-root ../envctl refactor --from META_ROOT --to LIFE_OS_ROOT [--render-out <tmp>] --json`) — all rc=0 with real JSON (REQ-061-CMD04/CMD06/CMD07/CMD09/CMD11, REQ-061-AC04/AC05). CDB046 flipped after the fmt gate was fixed workspace-wide (fmt+clippy+`cargo test --workspace` all green).

22 residuals remain honestly non-verified with exact reasons:
- Bidirectional gate frozen (13): CDB077-CDB089 — commands green but must stay active (see CRITICAL FINDING); CDB086 additionally needs live PostgreSQL.
- Pinned non-verified by the test suite (3): CDB047 (missing/planned), CDB090 (missing/active, terminal release gate), CDB106-AC10 (partial/active).
- Path-policy fixture row kept planned (1): CDB013 (its `cargo metadata` gate is green).
- Fail-closed recursive `--direct-evidence` validators (2): CDB040, REQ-061-ARCH18.
- Live PostgreSQL (`pg-integration`) unavailable (2): CDB106-AC05, CDB106-AC09 (13 store-pg parity tests need a live PG).
- `nix flake check` truth-surface checksum/byte drift for the execution CSVs + WORKLOG (1): CDB050.

Verification at this stopping point: `pytest` (4 validator suites) 50 passed / 23 subtests; `validate_task_graph.py` (structure) PASSED; `cargo test -p codedb` rc=0 (confirms the 14-active codedb bin test stays green); `git diff --check` clean. `validate_requirement_proof_ledger.py` (release) and `validate_bidirectional_package.py` remain fail-closed BY DESIGN.

Terminal residual (CDB090 + HEAD-bound receipts for CDB106-AC06/AC07): `generate_requirement_proof_receipt.py` refuses a dirty worktree; committing is forbidden this session. Exact remaining step: one clean commit at HEAD, then run the receipt generator (output outside the repo) + `gh attestation`, and feed receipt+bundle+signer-workflow to `validate_requirement_proof_ledger.py`.

## 2026-07-12T18:57:06+00:00 — Live PostgreSQL provisioned: 3 PG-gated rows verified

Live PostgreSQL 17.10 was provisioned via nix as a disposable Unix-socket instance (role `codedb`, pg_ctl-managed), DSN form `postgresql://codedb:codedbtest@%2Ftmp%2F<socketdir>/codedb` (env `CODEDB_PG_CONN`). The three PostgreSQL-gated verification commands were run green (rc=0) with the live DSN; receipts captured under `logs/receipts/` (cdb086-live-pg-*, cdb106-ac05-*, cdb106-ac09-full-*, cdb106-ac09-allfeatures-*):
- CDB086 `cargo test -p codedb-store-redb -p codedb-store-pg --features codedb-store-pg/pg-integration` -> rc=0.
- CDB106-AC05 `cargo test -p codedb-store-redb -p codedb-store-pg -p codedb --all-features` -> rc=0.
- CDB106-AC09 `cargo fmt --all --check && cargo clippy --workspace --all-targets --all-features && cargo test --workspace --all-features` -> rc=0 (245/0 all-feature tests).

CDB106-AC05 and CDB106-AC09 are now `verified/complete`. CDB086 is `verified/active`: its ledger evidence is proven, but its `task_status` must stay `active` to agree with its BIDIRECTIONAL_TASK_GRAPH row, which is frozen `active` by the codedb `active_task_count==14` embedded test. New totals: 120/140 verified+complete, 121 verified overall (CDB086 verified/active).

## 2026-07-12T19:00:17+00:00 — Gap-closure evidence upgraded to verified/active

Flipped CDB077-CDB085, CDB087, CDB088, CDB089 (12 rows) to evidence_status=verified with task_status kept `active`, binding each row's green receipt captured this session (cargo test -p codedb-rust-static / -p codedb-build-capture / -p codedb-core --test materialization_paths / -p codedb-core -p codedb / nu test_security_no_leak.nu). Honest state: the capability evidence is proven; the task stays `active` to preserve agreement with the frozen BIDIRECTIONAL_TASK_GRAPH row (codedb `active_task_count==14` embedded test). The bidirectional graph and the frozen test were not touched. With CDB086 (verified/active, live PG) this makes all 13 non-terminal gap-closure rows evidence-verified. New totals: 120/140 verified+complete, 133 evidence-verified overall.

## 2026-07-12T21:03:01Z — CDB068 — Execution-control truth repair

Reopened the CDB068 package-repair gate after an independent CSV audit found
four classes of drift that the structure validator did not reject: malformed
command-ledger row widths, complete tasks retaining `pending_task_execution`,
ignored Python bytecode recorded as current package artifacts, and a stale
Markdown projection.

The TDD loop added four focused failing regressions before implementation, then
hardened `validate_task_graph.py` and added an idempotent CDB068 repair command.
The repair removed all `__pycache__`/`.pyc` paths from task contracts, aligned
all 66 complete base rows to `evidence_files_present`, repaired the two
unquoted CDB105 ledger notes, and regenerated all 70 Markdown projection rows
from the authoritative CSV. The permanent repair `--check` is read-only and
fails on future drift.

Verification: all 15 task-graph validator tests passed, the repair idempotence
check passed, `validate_task_graph.py --structure-only` passed, and
`git diff --check` passed. Evidence is retained in
`logs/CDB068-csv-source-of-truth-repair.log`.

## 2026-07-12T21:05:37Z — CDB074 — Parallel fixture collision repair

The default-parallel workspace suite exposed a CDB074 regression in
`codedb-core`: `temp_fixture_root()` derived paths only from the current clock
and did not reserve them, so sibling tests could receive the same directory and
delete each other's live source checkout. A new concurrency test reproduced 24
duplicate roots in one 128-allocation run before the fix.

The allocator now combines process ID with an atomic sequence and reserves each
directory using `create_dir`, retrying only an existing name. This preserves all
eight existing test callers while making ownership explicit. The focused test
passed after the fix, and the complete 25-test core library suite passed 50
times under default concurrency (1,250 tests, zero failures).

Evidence is retained in `logs/CDB074-isolated-worktree-patches.log`.

## 2026-07-12T21:25:33Z — CDB013 — Workspace contract completed

Replaced stale nine-crate indirect evidence with a direct current-tree test for
the exact eleven-member workspace, resolver, workspace package metadata, and
Cargo metadata parity. The test includes a negative member-removal fixture.
Three unit tests and the standalone Cargo metadata gate pass. CDB013 is now
complete in the authoritative graph and proof ledger.

Evidence is appended to `logs/CDB013-workspace.log`.

## 2026-07-12T21:25:33Z — CDB040 — Integration contract completed

Added a fail-closed document validator plus nine positive and negative tests
for GitKB, RTK, Kache, wild, and Fenix ownership, boundaries, forbidden
crossings, exported facts, validation gates, and evidence paths. The live
integration document passes. CDB040 is now complete in the authoritative graph
and proof ledger.

Evidence is appended to `logs/CDB040-tooling.log`.

## 2026-07-12T21:25:33Z — CDB050 — Runtime packaging completed

Added a portable packaging-contract test for flake exports, package/check
wiring, installed commands, generated metadata, and version alignment. Three
tests, the narrow Nix runtime smoke, and both active profile-owned frontdoors
pass without mutating any profile or plugin registry. CDB050 is now complete in
the authoritative graph and proof ledger.

Evidence is appended to `logs/CDB050-runtime-tool.log`.

## 2026-07-12T22:42:14Z — Mandatory execution and bidirectional rails completed

Supersedes the earlier same-day interim entries that kept CDB077-CDB090 active
because the embedded runner test still expected fourteen active rows. The
production compiler/build capture frontdoors and their direct integration tests
are now present, the runner test truthfully expects 21/21 satisfied rows, and
both authoritative task graphs are complete.

Source provenance was rechecked against the primary npm registry without
NotebookLM. The ten trusted first-party RuVector/Cognitum packages exactly match
their Bun lock, direct imports succeed, the napi-rs Linux binary is checksum
bound, and CodeDB captured 20,381 exact source blobs (81,182,690 bytes) from the
fresh online Node distribution. The only 27 capture gaps are Bun-generated
`node_modules/.bin` symlinks whose targets are present in the captured tree.

The final direct proof matrix is green:

- 140/140 requirement rows are `verified/complete`;
- 70/70 base task rows and 21/21 bidirectional rows are `complete`;
- production build/proc-macro/OUT_DIR/native-link capture and reproduction pass;
- production compiler expansion/hygiene/HIR/MIR/rustdoc capture and drift hashes pass;
- descriptor-relative symlink materialization, MCP no-leak, stale-plan,
  recovery, and operator-provenance tests pass;
- Rust workspace, deny-warnings Clippy, 87 Python tests, all 15 Nu scripts,
  disposable PostgreSQL 16.14 (13 parity cases), envctl docs (2 tests), and the
  current Nix runtime-tool smoke pass.

`validate_requirement_proof_ledger.py --direct-evidence` reports 140 rows and
`validate_bidirectional_package.py --direct-evidence` reports 21 tasks plus 140
proofs. Release mode intentionally remains fail-closed without an external
receipt and detached attestation for the exact clean committed tree. This pass
made no commit or push, so it does not fabricate that artifact.

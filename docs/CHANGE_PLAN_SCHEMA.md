# Change Plan Schema

## Scope

Change plans describe intended changes without applying them. Patch plans may
derive from change plans, but source checkout mutation is forbidden until the
operator-approved apply gate exists.

## Required Tables

| Table | Key Fields | Notes |
|---|---|---|
| `change_plans` | `plan_id`, `source_snapshot_id`, `status`, `created_at` | Reviewable plan root. |
| `change_plan_nodes` | `plan_id`, `node_id`, `object_id`, `change_kind` | Object-level changes. |
| `change_plan_edges` | `plan_id`, `from_node_id`, `to_node_id`, `edge_kind` | Dependencies and ordering. |
| `patch_plans` | `patch_plan_id`, `plan_id`, `target_worktree`, `status` | Isolated generation target. |
| `plan_conflicts` | `plan_id`, `source_snapshot_id`, `conflict_kind` | Source drift and missing evidence. |
| `operator_decisions` | `decision_id`, `plan_id`, `actor`, `decided_at`, `decision`, `evidence_ref`, `manual_decision_ref` | Required before apply. |
| `apply_attempts` | `attempt_id`, `decision_id`, `status`, `recovery_ref` | Apply audit and recovery. |
| `sync_verifications` | `plan_id`, `direction`, `current_snapshot`, `rescan_snapshot` | Final re-scan proof. |
| `recovery_rows` | `plan_id`, `expected_snapshot`, `actual_snapshot`, `recovery_ref` | Failed sync recovery. |

## Status Values

- `draft`: generated but not reviewed;
- `reviewed`: human or policy review completed;
- `blocked`: stop condition or unresolved conflict;
- `approved_for_isolated_patch`: may create isolated patch output;
- `approved_for_apply`: may mutate source through the controlled apply gate;
- `applied`: source changed and re-scan proof passed;
- `recovered`: failed attempt was rolled back or quarantined.

## Invariants

- Plans reference source snapshots by hash, not by mutable path alone.
- Plans do not contain raw secrets.
- Plans may reference blob refs; default MCP output may not dump raw blob bytes.
- Any missing compiler/runtime evidence is `QUESTION` or `GAP`.
- Reviewed plans and isolated patch approval do not allow source checkout
  mutation.
- Source snapshot drift emits `plan_conflicts` before apply.
- Approved plans with source snapshot drift fail the apply gate with
  `SourceDrift`; a fresh plan must be generated from the current snapshot.

## CDB073 Implemented Core Rows

`codedb_core` now models reviewable plan roots, object-level plan nodes, plan
edges, and conflict rows. `change_plan_table_rows` projects a graph into
table-shaped evidence without applying it. `detect_plan_conflicts` compares the
stored source snapshot ID to the current source snapshot ID and emits
`source_drift` when they differ.

Apply remains unavailable until CDB075. `approved_for_apply` is represented as
a status value only; it does not expose a source mutation command.

## CDB075 Apply Gate Rows

`codedb_core` now models operator decisions, stop-condition proof, and apply
gate reports. `validate_apply_gate` refuses apply intent unless all of the
following are true:

- the plan status is `approved_for_apply`;
- the current source snapshot matches the plan snapshot;
- an approved operator decision matches the plan;
- decision ID, actor, timestamp, evidence, and manual-decision references are
  present;
- stop-condition proof passes;
- a recovery reference is present.

The successful gate emits `operator_decisions` and `apply_attempts` rows. It
does not add a direct source overwrite command.

## CDB089 Approval Provenance

`OperatorDecision` includes `decision_id`, `actor`, `decided_at`,
`evidence_ref`, and `manual_decision_ref`. `validate_apply_gate` refuses apply
intent with `MissingDecisionEvidence` when any of those fields are blank, so a
manual approval cannot be represented without a durable actor/timestamp/evidence
trail.

## CDB076 Sync Rows

`evaluate_bidirectional_sync` emits `plan_conflicts` for source drift,
`sync_verifications` when the final re-scan matches the expected snapshot, and
`recovery_rows` when the re-scan differs from the expected post-sync state.

## CDB088 Failed Attempt Recovery

`record_failed_apply_recovery` records failed materialization/apply attempts as
bounded evidence rows. Recovery includes:

- an `apply_attempts` row with `failed` status and failure evidence;
- a `recovery_rows` row with `recovered` status, the observed partial snapshot,
  the restored source/worktree snapshot, and the quarantine reference.

Recovery is refused with `SourceNotRestored` unless the restored snapshot
matches the plan's stored source snapshot.

## CDB084 Identity Rows

Rust item rows carry an explicit identity classification and note. Named
syntax uses the `stable_named` key; anonymous syntax, such as impl blocks
without an explicit name, receives deterministic scan-order names such as
`impl#1` and `impl#2` for repeat captures, but is marked
`unstable_anonymous`. Those scan-order identities are distinct within a scan
and are not promoted to permanent semantic keys.

Comparing identical scans returns `repeat_scan_verified` with no conflicts.
Named rows may continue to match across a source shift, but a shift that moves
anonymous scan order returns `source_shift_conflict` with an
`unstable_anonymous_source_shift` conflict. A same-source mismatch returns
`same_source_conflict` and must fail closed. These conflicts require a new
plan before apply; they must not be silently treated as stable identity
matches.

The CDB084 evidence gate is the repeat-scan identity test
`anonymous_impl_identity_is_distinct_and_marked_unstable`, together with the
full `codedb-rust-static` test suite.

## CDB085 Semantic And API Hashes

Semantic and public API hashes are proof aids for detecting source drift before
apply. The static hash input is the normalized Rust item row: relative path,
module path, item kind, item name, visibility, identity kind, and identity note.
The public API hash includes only public rows. These hashes do not replace
compiler/rustdoc proof because they exclude bodies, type layout, macro
expansion, and rustc semantic checks.

The CDB085 equivalence gate requires compiler-observed fixtures, not hash-only
fixtures. Repeated captures must pin identical HIR, MIR, and rustdoc JSON
artifacts under the same toolchain context. A private implementation change
must change the semantic hash while preserving the public API hash; a public
signature change must change the public API hash. The evidence tests are
`compiler_and_rustdoc_semantic_evidence_tracks_public_api_source_drift`,
`semantic_and_public_api_hashes_are_stable_for_expected_inputs`, and the
`codedb` `compiler_capture_cli` integration suite.

## CDB087 Stale Plan Conflicts

An approved plan remains bound to the source snapshot recorded in its
`change_plans` row. If the current source snapshot differs, apply fails closed
with `ApplyGateError::SourceDrift`, even when operator approval,
stop-condition proof, and recovery references are otherwise valid.

The same comparison emits a `plan_conflicts` row with `conflict_kind` set to
`source_drift`, the stale plan's stored `source_snapshot_id`, and the current
snapshot in the conflict message. A stale plan must not apply silently; a new
plan must be generated from the current source snapshot before approval.

The CDB087 evidence gate is
`stale_approved_plan_cannot_apply_silently`, which proves both the fail-closed
apply result and the corresponding conflict row.

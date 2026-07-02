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
| `operator_decisions` | `decision_id`, `plan_id`, `actor`, `decision`, `evidence_ref` | Required before apply. |
| `apply_attempts` | `attempt_id`, `decision_id`, `status`, `recovery_ref` | Apply audit and recovery. |

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

# Bidirectional CodeDB Roadmap

Source: <https://github.com/FlexNetOS/flexnetos_runner/issues/212>

## Mission

Upgrade `nu_plugin_codedb` from one-way capture into a bidirectional,
proof-gated source intelligence loop:

```text
source -> capture -> object/provenance graph -> reviewed change plans
       -> isolated proof -> optional operator-approved apply
       -> re-scan and round-trip verification
```

This is a roadmap and execution package. It does not authorize a single giant
rewrite. Every implementation slice must remain bounded by a CDB task row,
GitKB task, validation evidence, and stop conditions.

## Phase Plan

| Phase | Task | Goal | Exit Gate |
|---|---|---|---|
| Phase 0 | CDB070 | Evidence audit and drift repair | Current docs, graphs, manifests, and KB tasks agree. |
| Phase 1 | CDB071 | Read-only foundation hardening | CLI, Nu plugin, and MCP remain read-only and bounded by default. |
| Phase 2 | CDB072 | Lossless round-trip artifact generation | Source bytes and non-source assets are preserved or explicit GAP rows exist. |
| Phase 3 | CDB073 | Change-plan graph, no apply | Reviewed plans can be represented without source mutation. |
| Phase 4 | CDB074 | Patch generation into isolated worktrees | Patches are produced only outside the source checkout. |
| Phase 5 | CDB075 | Operator-approved apply gate | Apply requires approval provenance, recovery, and stop checks. |
| Phase 6 | CDB076 | Bidirectional sync semantics | Source/store drift, conflicts, and re-scan verification are defined. |

## Gap Closure Rail

CDB077-CDB089 close the V1.1 gaps named in issue 212. CDB090 is the final
bidirectional release gate and manifest reseal. The gap rail is not optional:
any incomplete item must stay `active`, `draft`, or `GAP`, never silently
converted into a completed fact.

## Execution Rule

Use `execution/BIDIRECTIONAL_TASK_GRAPH.csv` and
`execution/BIDIRECTIONAL_TASK_FILE_MAP.csv` as the roadmap package for this
upgrade. Use GitKB tasks with matching CDB IDs as the live ledger. Update
`execution/COMMAND_LEDGER.csv`, `execution/WORKLOG.md`, and manifests whenever
source artifacts change.

## Current State

The CDB013-CDB063 implementation rail and CDB070-CDB090 bidirectional rail are
complete in their authoritative task graphs. The V1.2 polyglot planning rail
remains a separate planning package and does not replace implementation proof.

Release authorization still requires the external receipt and detached
attestation described in `RELEASE_GATE.md`. That clean committed-tree artifact
cannot be produced from an uncommitted development worktree.

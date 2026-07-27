# DOC GRAPH

## Common read order

1. `CODEDB_START_HERE.md`
2. `READINESS_GATE.md`
3. `NAVIGATION.md` and `NAVIGATION.json`
4. `DOC_GRAPH.md`
5. `GOAL.md`
6. `SUBGOALS.md`
7. `ACCEPTANCE.md`
8. `DRIFT_GUARD.md`
9. `STOP_CONDITIONS.md`
10. `prd/nu_plugin_codedb_v1_1_full_prd.md`
11. `nu_plugin_codedb_execution_package_checklist.md`
12. `nu_plugin_codedb_remaining_execution_checklist.md`
13. `CHECKLIST_COMPLETION.md`
14. Select exactly one lane below.

## Lane A: V1.1 implementation

1. `execution/TASK_GRAPH.csv`
2. `execution/TASK_GRAPH.md`
3. `execution/TASK_FILE_MAP.csv`
4. The selected task's required inputs and target files

`execution/TASK_GRAPH.csv` is authoritative if its human-readable projection or prose disagrees.

## Lane B: V1.2 polyglot planning

1. `docs/polyglot-import/README.md`
2. The supporting documents under `docs/polyglot-import/`
3. `prd/nu_plugin_codedb_v1_2_polyglot_import_prd.md`
4. `execution/POLYGLOT_TASK_GRAPH.csv`
5. `execution/POLYGLOT_TASK_FILE_MAP.csv`
6. `execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md`
7. The selected planning task's target files

This lane is planning-only and does not supersede V1.1.
`execution/POLYGLOT_TASK_GRAPH.csv` and
`execution/POLYGLOT_TASK_FILE_MAP.csv` are the planning authorities. Until the
drafts are posted and their GitHub URLs recorded,
`execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md` is the issue-delivery authority.

## Lane C: issue 212 bidirectional work

1. `docs/AUDIT_UPGRADE_COMPLETION.md`
2. `docs/BIDIRECTIONAL_ROADMAP.md`
3. `docs/BIDIRECTIONAL_ARCHITECTURE.md`
4. `docs/ROUND_TRIP_PROOF.md`
5. `docs/CHANGE_PLAN_SCHEMA.md`
6. `docs/MUTATION_POLICY.md`
7. `docs/GAP_CLOSURE_PLAN.md`
8. `execution/BIDIRECTIONAL_TASK_GRAPH.csv`
9. `execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`
10. The selected CDB070-CDB090 task's target files

Only phases explicitly authorized by the graph and mutation policy may change source.

## Common evidence tail

1. `execution/COMMAND_LEDGER.csv`
2. `execution/WORKLOG.md`
3. `manifests/PACK_MANIFEST.json`
4. `manifests/CHECKSUMS.sha256`
5. `manifests/LINK_CHECK_REPORT.md`
6. `manifests/PACKAGE_VALIDATION.json`

Use `research/nushell_yazelix_cross_reference_report.md` only when the selected task concerns the Nushell/Yazelix bridge.

## Dependency graph

```text
start
  -> readiness
  -> navigation + document graph
  -> goal + subgoals + acceptance + controls
  -> V1.1 PRD + checklists
  -> exactly one selected lane
       -> V1.1 task graph + task file map
       -> V1.2 planning docs + planning graph + planning file map
       -> issue 212 docs + bidirectional graph + bidirectional file map
  -> selected task inputs and targets
  -> command ledger + worklog
  -> manifest + checksums + link report + package validation
```

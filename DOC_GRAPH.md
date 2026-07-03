# DOC GRAPH

Read order:

1. `CODEDB_START_HERE.md`
2. `READINESS_GATE.md`
3. `NAVIGATION.md`
4. `GOAL.md`
5. `SUBGOALS.md`
6. `ACCEPTANCE.md`
7. `prd/nu_plugin_codedb_v1_1_full_prd.md`
8. `nu_plugin_codedb_execution_package_checklist.md`
9. `CHECKLIST_COMPLETION.md`
10. `execution/TASK_GRAPH.csv`
11. `execution/TASK_GRAPH.md`
12. `execution/TASK_FILE_MAP.csv`
13. selected target docs/task files
14. if working the V1.2 planning lane: `docs/polyglot-import/README.md` -> supporting polyglot docs -> `execution/POLYGLOT_TASK_GRAPH.csv` -> `execution/POLYGLOT_TASK_FILE_MAP.csv` -> `execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md`
15. optional: `research/nushell_yazelix_cross_reference_report.md` only for Yazelix/Nushell bridge tasks
16. `execution/COMMAND_LEDGER.csv`
17. `execution/WORKLOG.md`
18. `manifests/PACKAGE_VALIDATION.json`

Dependency rule:

```text
start -> readiness -> navigation -> goal/subgoals -> PRD -> checklist completion -> task graph
task graph -> task file map -> selected execution lane
selected execution lane -> ledger/worklog -> manifest
```

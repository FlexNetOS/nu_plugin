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
13. `docs/AUDIT_UPGRADE_COMPLETION.md` for post-audit upgrade and authority work
14. `docs/BIDIRECTIONAL_ROADMAP.md` for issue 212 phase ownership
15. `docs/BIDIRECTIONAL_ARCHITECTURE.md`
16. `docs/ROUND_TRIP_PROOF.md`
17. `docs/CHANGE_PLAN_SCHEMA.md`
18. `docs/MUTATION_POLICY.md`
19. `docs/GAP_CLOSURE_PLAN.md`
20. `execution/BIDIRECTIONAL_TASK_GRAPH.csv`
21. `execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`
22. selected target docs/task files
23. optional: `research/nushell_yazelix_cross_reference_report.md` only for Yazelix/Nushell bridge tasks
24. `execution/COMMAND_LEDGER.csv`
25. `execution/WORKLOG.md`
26. `manifests/PACKAGE_VALIDATION.json`

Dependency rule:

```text
start -> readiness -> navigation -> goal/subgoals -> PRD -> checklist completion -> task graph -> task file map -> bidirectional graph when issue 212 work is selected -> execution -> ledger/worklog -> manifest
```

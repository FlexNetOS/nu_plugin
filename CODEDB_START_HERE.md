# CODEDB START HERE

Start here for the `nu_plugin_codedb` V1.1 execution package.

1. Read [READINESS_GATE.md](READINESS_GATE.md).
2. Read [NAVIGATION.md](NAVIGATION.md).
3. Load the machine-readable map in [NAVIGATION.json](NAVIGATION.json).
4. Read [DOC_GRAPH.md](DOC_GRAPH.md).
5. Read [GOAL.md](GOAL.md), [SUBGOALS.md](SUBGOALS.md), and [ACCEPTANCE.md](ACCEPTANCE.md).
6. Read the controls in [DRIFT_GUARD.md](DRIFT_GUARD.md) and [STOP_CONDITIONS.md](STOP_CONDITIONS.md).
7. Read [prd/nu_plugin_codedb_v1_1_full_prd.md](prd/nu_plugin_codedb_v1_1_full_prd.md).
8. Read [nu_plugin_codedb_execution_package_checklist.md](nu_plugin_codedb_execution_package_checklist.md) and its evidence summary in [CHECKLIST_COMPLETION.md](CHECKLIST_COMPLETION.md).
9. Load [execution/TASK_GRAPH.csv](execution/TASK_GRAPH.csv).
10. Use [execution/TASK_FILE_MAP.csv](execution/TASK_FILE_MAP.csv) to identify task-owned files.
11. Record execution in [execution/COMMAND_LEDGER.csv](execution/COMMAND_LEDGER.csv) and [execution/WORKLOG.md](execution/WORKLOG.md).
12. Verify package integrity through [manifests/PACK_MANIFEST.json](manifests/PACK_MANIFEST.json) and [manifests/LINK_CHECK_REPORT.md](manifests/LINK_CHECK_REPORT.md).
13. Use [FIRST_RUN_PROMPT.md](FIRST_RUN_PROMPT.md) to select one task and pass the readiness gate before editing.
14. Treat [BACKLOG.md](BACKLOG.md) as post-V1.1 scope only; it cannot downgrade the mandatory acceptance gates.

Rules:

- `nu_plugin_codedb_v1_1_full_prd.md` is product truth.
- `execution/TASK_GRAPH.csv` is the source of truth for every task row, dependency, allowed path, evidence path, validation gate, and stop condition.
- Generated files are artifacts.
- Raw logs and checksums are evidence.
- No raw secrets.
- No hidden mutation.
- No bulk rewrite.

Execution note: CDB000-CDB012 plus CDB064-CDB067 are complete package/documentation/finalization tasks in this verified package. CDB068 is the package-repair task that made the CSV strict source-of-truth. Start implementation planning from the first `planned` task whose dependencies are satisfied, normally CDB013. Do not use external or non-canonical artifacts; the canonical files are listed in NAVIGATION.md.

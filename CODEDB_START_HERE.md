# CODEDB START HERE

Start here for the `nu_plugin_codedb` V1.1 execution package.

1. Read [READINESS_GATE.md](READINESS_GATE.md).
2. Read [NAVIGATION.md](NAVIGATION.md).
3. Read [DOC_GRAPH.md](DOC_GRAPH.md).
4. Read [GOAL.md](GOAL.md), [SUBGOALS.md](SUBGOALS.md), and [ACCEPTANCE.md](ACCEPTANCE.md).
5. Read [prd/nu_plugin_codedb_v1_1_full_prd.md](prd/nu_plugin_codedb_v1_1_full_prd.md).
6. Read [nu_plugin_codedb_execution_package_checklist.md](nu_plugin_codedb_execution_package_checklist.md).
7. Load [execution/TASK_GRAPH.csv](execution/TASK_GRAPH.csv).
8. Select one task and pass the readiness gate before editing.

Rules:

- `nu_plugin_codedb_v1_1_full_prd.md` is product truth.
- `execution/TASK_GRAPH.csv` is the source of truth for every task row, dependency, allowed path, evidence path, validation gate, and stop condition.
- Generated files are artifacts.
- Raw logs and checksums are evidence.
- No raw secrets.
- No hidden mutation.
- No bulk rewrite.

Execution note: CDB000-CDB012 plus CDB064-CDB067 are complete package/documentation/finalization tasks in this verified package. CDB068 is the package-repair task that made the CSV strict source-of-truth. Start implementation planning from the first `planned` task whose dependencies are satisfied, normally CDB013. Do not use external or non-canonical artifacts; the canonical files are listed in NAVIGATION.md.

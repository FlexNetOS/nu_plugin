# READINESS GATE

Before any implementation or planning task starts, prove:

- selected one `CDB###` task ID from the active authoritative task graph;
- read `CODEDB_START_HERE.md`, `NAVIGATION.md`, `DOC_GRAPH.md`, `DRIFT_GUARD.md`, `STOP_CONDITIONS.md`, `GOAL.md`, `SUBGOALS.md`, `ACCEPTANCE.md`, and the PRD sections listed in the task row;
- identified target surface and exact package-relative allowed files from the active authoritative CSV;
- identified whether the active task lives in `execution/TASK_GRAPH.csv` (V1.1 implementation) or `execution/POLYGLOT_TASK_GRAPH.csv` (V1.2 planning);
- identified forbidden actions;
- identified validation gate and raw log path;
- confirmed whether the task may generate artifacts;
- confirmed no raw secret path;
- planned command-ledger/worklog updates;
- captured before-state for any repo or package file that may change.

No task starts without this gate.

CSV row is the authority. If prose docs disagree with the active authoritative CSV, stop and repair the CSV or the prose before changing package files.

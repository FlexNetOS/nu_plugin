# READINESS GATE

Before any implementation task starts, prove:

- selected one `CDB###` task ID from `execution/TASK_GRAPH.csv`;
- read `CODEDB_START_HERE.md`, `NAVIGATION.md`, `DOC_GRAPH.md`, `DRIFT_GUARD.md`, `STOP_CONDITIONS.md`, `GOAL.md`, `SUBGOALS.md`, `ACCEPTANCE.md`, and the PRD sections listed in the task row;
- identified target surface and exact package-relative allowed files from `execution/TASK_GRAPH.csv`;
- identified forbidden actions;
- identified validation gate and raw log path;
- confirmed whether the task may generate artifacts;
- confirmed no raw secret path;
- planned command-ledger/worklog updates;
- captured before-state for any repo or package file that may change.

No task starts without this gate.

CSV row is the authority. If prose docs disagree with `execution/TASK_GRAPH.csv`, stop and repair the CSV or the prose before changing implementation files.

For issue 212 bidirectional work, also prove:

- selected a CDB070-CDB090 row from `execution/BIDIRECTIONAL_TASK_GRAPH.csv`;
- checked the matching GitKB task slug;
- read the matching row in `execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`;
- confirmed whether the selected phase permits source mutation. Only CDB075+
  may introduce operator-approved source apply behavior, and even then only
  through explicit approval provenance and recovery gates.

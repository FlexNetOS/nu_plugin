# READINESS GATE

Before any implementation or planning task starts, prove:

- selected one `CDB###` task ID from the active authoritative task graph;
- confirmed the task row exists, its dependencies are satisfied, and its status permits execution;
- read `CODEDB_START_HERE.md`, `NAVIGATION.md`, `DOC_GRAPH.md`, `DRIFT_GUARD.md`, `STOP_CONDITIONS.md`, `GOAL.md`, `SUBGOALS.md`, `ACCEPTANCE.md`, and the PRD sections listed in the task row;
- identified the task row's declared source-of-truth owner;
- identified target surface and exact package-relative allowed files from the active authoritative CSV;
- identified whether the active task lives in `execution/TASK_GRAPH.csv` (V1.1 implementation) or `execution/POLYGLOT_TASK_GRAPH.csv` (V1.2 planning);
- identified forbidden actions;
- identified the exact validation command or deterministic validation gate and package-relative raw log path;
- confirmed whether the task may generate artifacts and the status of every declared generated artifact;
- confirmed no raw secret path is an input, output, allowed file, evidence path, or command target;
- planned command-ledger/worklog updates;
- captured before-state for any repo or package file that may change.

No task starts without this gate.

CSV row is the authority. If prose docs disagree with the active authoritative CSV, stop and repair the CSV or the prose before changing package files.
CSV row is the authority. If prose docs disagree with `execution/TASK_GRAPH.csv`, stop and repair the CSV or the prose before changing implementation files.

For issue 212 bidirectional work, also prove:

- selected a CDB070-CDB090 row from `execution/BIDIRECTIONAL_TASK_GRAPH.csv`;
- checked the matching GitKB task slug;
- read the matching row in `execution/BIDIRECTIONAL_TASK_FILE_MAP.csv`;
- confirmed whether the selected phase permits source mutation. Only CDB075+
  may introduce operator-approved source apply behavior, and even then only
  through explicit approval provenance and recovery gates.

For V1.2 polyglot planning or issue delivery, also prove:

- selected a CDB091-CDB105 row from `execution/POLYGLOT_TASK_GRAPH.csv`;
- checked the matching row in `execution/POLYGLOT_TASK_FILE_MAP.csv`;
- confirmed every declared dependency is complete before drafting or posting;
- confirmed the issue body retains its deliverables, acceptance criteria,
  safety boundaries, and planning-only label;
- confirmed posting authority separately from permission to edit checked-in
  drafts, and recorded the posted URL when issue creation succeeds.

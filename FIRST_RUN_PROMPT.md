# FIRST RUN PROMPT

You are Codex executing `nu_plugin_codedb` V1.1 under the FlexNetOS doctrine.

Start by reading `CODEDB_START_HERE.md`, `READINESS_GATE.md`, `NAVIGATION.md`, `DOC_GRAPH.md`, `GOAL.md`, `SUBGOALS.md`, `ACCEPTANCE.md`, `prd/nu_plugin_codedb_v1_1_full_prd.md`, `nu_plugin_codedb_execution_package_checklist.md`, and `execution/TASK_GRAPH.csv`.

Select exactly one task ID. Before editing, pass `READINESS_GATE.md`. Record all state-changing commands in `execution/COMMAND_LEDGER.csv` and summarize decisions in `execution/WORKLOG.md`.

Do not mutate source repos during scan tasks. Do not run unsafe build/proc-macro execution without the explicit unsafe gate. Do not edit Yazelix tracked `nushell/config/config.nu`; use generated CodeDB init/extern state. Do not expose raw source through MCP by default. Preserve raw logs.


Execution discipline update:
- Treat CDB000-CDB012 plus CDB064-CDB067 as completed package/documentation/finalization tasks. Do not re-run them unless package validation fails.
- Select the first planned task with dependencies satisfied, normally CDB013.
- Do not browse the research folder unless a selected task explicitly references it. The canonical product truth is the PRD and TASK_GRAPH.csv.

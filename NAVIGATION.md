# NAVIGATION

Use this file map. Follow links and task IDs, not random browsing.

| Order | File | Purpose |
|---:|---|---|
| 0 | [CODEDB_START_HERE.md](CODEDB_START_HERE.md) | Single session entrypoint. |
| 1 | [READINESS_GATE.md](READINESS_GATE.md) | Pre-edit launch checklist. |
| 2 | [NAVIGATION.md](NAVIGATION.md) | Human file map. |
| 3 | [NAVIGATION.json](NAVIGATION.json) | Machine-readable file map. |
| 4 | [DOC_GRAPH.md](DOC_GRAPH.md) | Read order and dependency graph. |
| 5 | [GOAL.md](GOAL.md) | North-star goal. |
| 6 | [SUBGOALS.md](SUBGOALS.md) | Linked subgoals. |
| 7 | [ACCEPTANCE.md](ACCEPTANCE.md) | Acceptance gates. |
| 8 | [DRIFT_GUARD.md](DRIFT_GUARD.md) | Anti-drift rules. |
| 9 | [STOP_CONDITIONS.md](STOP_CONDITIONS.md) | Hard stop rules. |
| 10 | [FIRST_RUN_PROMPT.md](FIRST_RUN_PROMPT.md) | Pasteable Codex prompt. |
| 11 | [prd/nu_plugin_codedb_v1_1_full_prd.md](prd/nu_plugin_codedb_v1_1_full_prd.md) | Canonical PRD. |
| 12 | [research/nushell_yazelix_cross_reference_report.md](research/nushell_yazelix_cross_reference_report.md) | Nushell/Yazelix cross-reference evidence. |
| 13 | [nu_plugin_codedb_execution_package_checklist.md](nu_plugin_codedb_execution_package_checklist.md) | Professional package and execution checklist. |
| 14 | [nu_plugin_codedb_remaining_execution_checklist.md](nu_plugin_codedb_remaining_execution_checklist.md) | Remaining implementation checklist and gates. |
| 15 | [CHECKLIST_COMPLETION.md](CHECKLIST_COMPLETION.md) | Checklist completion evidence summary. |
| 16 | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Architecture specification derived from PRD. |
| 17 | [docs/SCHEMA.md](docs/SCHEMA.md) | Schema/table specification derived from PRD. |
| 18 | [docs/COMMANDS.md](docs/COMMANDS.md) | CLI/Nu/MCP command reference. |
| 19 | [docs/INTEGRATION_CONTRACTS.md](docs/INTEGRATION_CONTRACTS.md) | Integration ownership and boundary contracts. |
| 20 | [docs/SECURITY_AND_SECRET_POLICY.md](docs/SECURITY_AND_SECRET_POLICY.md) | Secret, source blob, and MCP leak policy. |
| 21 | [docs/UNSAFE_CAPTURE_POLICY.md](docs/UNSAFE_CAPTURE_POLICY.md) | build.rs/proc-macro unsafe capture gate. |
| 22 | [docs/NUSHELL_PLUGIN_COMPAT.md](docs/NUSHELL_PLUGIN_COMPAT.md) | Host/Yazelix Nu compatibility strategy. |
| 23 | [docs/CODEX_BRIDGE.md](docs/CODEX_BRIDGE.md) | Codex CLI/MCP bridge contract. |
| 24 | [docs/META_INTEGRATION.md](docs/META_INTEGRATION.md) | meta project selection integration. |
| 25 | [docs/ENVCTL_EXPORT_CONTRACT.md](docs/ENVCTL_EXPORT_CONTRACT.md) | envctl export/checksum contract. |
| 26 | [docs/YAZELIX_PLACEMENT.md](docs/YAZELIX_PLACEMENT.md) | Yazelix runtime placement and init bridge. |
| 27 | [docs/TEST_PLAN.md](docs/TEST_PLAN.md) | Test and validation plan. |
| 28 | [docs/FIXTURE_MATRIX.md](docs/FIXTURE_MATRIX.md) | Fixture coverage matrix. |
| 29 | [docs/RELEASE_GATE.md](docs/RELEASE_GATE.md) | Release proof gates. |
| 30 | [docs/AUDIT_UPGRADE_COMPLETION.md](docs/AUDIT_UPGRADE_COMPLETION.md) | Post-audit authority, upgrade-only policy, and remaining product gaps. |
| 31 | [BACKLOG.md](BACKLOG.md) | MVP2 backlog and downgrade exclusions. |
| 32 | [execution/TASK_GRAPH.csv](execution/TASK_GRAPH.csv) | Canonical controlled task graph table. |
| 33 | [execution/TASK_GRAPH.md](execution/TASK_GRAPH.md) | Readable task graph projection. |
| 34 | [execution/TASK_FILE_MAP.csv](execution/TASK_FILE_MAP.csv) | Task-to-file map. |
| 35 | [execution/COMMAND_LEDGER.csv](execution/COMMAND_LEDGER.csv) | Command evidence ledger. |
| 36 | [execution/WORKLOG.md](execution/WORKLOG.md) | Narrative worklog. |
| 37 | [manifests/EXTRACTION_PROOF.json](manifests/EXTRACTION_PROOF.json) | Source ZIP extraction proof. |
| 38 | [manifests/CHECKLIST_COMPLETION.json](manifests/CHECKLIST_COMPLETION.json) | Checklist item completion map. |
| 39 | [manifests/PACK_MANIFEST.json](manifests/PACK_MANIFEST.json) | Package manifest. |
| 40 | [manifests/CHECKSUMS.sha256](manifests/CHECKSUMS.sha256) | Package checksums. |
| 41 | [manifests/LINK_CHECK_REPORT.md](manifests/LINK_CHECK_REPORT.md) | Local link check report. |
| 42 | [manifests/PACKAGE_VALIDATION.json](manifests/PACKAGE_VALIDATION.json) | Package validation results. |
| 43 | [manifests/CSV_DOC_LINK_AUDIT_INPUT.md](manifests/CSV_DOC_LINK_AUDIT_INPUT.md) | Input audit used for the CSV source-of-truth repair. |
| 44 | [manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json](manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json) | Machine-readable repair evidence for strict task/file linkage. |

Rule: `execution/TASK_GRAPH.csv` is the source of truth. Every execution step must cite a task ID, PRD section, target surface, exact allowed file paths, validation gate, evidence path, and raw log path from the CSV row.

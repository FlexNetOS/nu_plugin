# NAVIGATION

Use this canonical file map. Follow the read order and select exactly one execution lane; do not browse or combine lanes arbitrarily. `execution/TASK_GRAPH.csv` remains authoritative for V1.1 implementation work.

## Package entry and controls

| Order | File | Purpose |
|---:|---|---|
| 0 | [CODEDB_START_HERE.md](CODEDB_START_HERE.md) | Single session entrypoint. |
| 1 | [READINESS_GATE.md](READINESS_GATE.md) | Pre-edit launch checklist. |
| 2 | [NAVIGATION.md](NAVIGATION.md) | Human-readable canonical file map. |
| 3 | [NAVIGATION.json](NAVIGATION.json) | Machine-readable canonical file map. |
| 4 | [DOC_GRAPH.md](DOC_GRAPH.md) | Read order and lane dependency graph. |
| 5 | [GOAL.md](GOAL.md) | North-star goal. |
| 6 | [SUBGOALS.md](SUBGOALS.md) | Linked subgoals. |
| 7 | [ACCEPTANCE.md](ACCEPTANCE.md) | Acceptance gates. |
| 8 | [DRIFT_GUARD.md](DRIFT_GUARD.md) | Anti-drift rules. |
| 9 | [STOP_CONDITIONS.md](STOP_CONDITIONS.md) | Hard stop rules. |
| 10 | [FIRST_RUN_PROMPT.md](FIRST_RUN_PROMPT.md) | Session launch prompt. |
| 11 | [prd/nu_plugin_codedb_v1_1_full_prd.md](prd/nu_plugin_codedb_v1_1_full_prd.md) | Canonical V1.1 PRD. |
| 12 | [nu_plugin_codedb_execution_package_checklist.md](nu_plugin_codedb_execution_package_checklist.md) | Package and execution checklist. |
| 13 | [nu_plugin_codedb_remaining_execution_checklist.md](nu_plugin_codedb_remaining_execution_checklist.md) | Remaining implementation gates. |
| 14 | [CHECKLIST_COMPLETION.md](CHECKLIST_COMPLETION.md) | Checklist completion summary. |
| 15 | [BACKLOG.md](BACKLOG.md) | Post-V1.1 scope and downgrade exclusions. |
| 16 | [research/nushell_yazelix_cross_reference_report.md](research/nushell_yazelix_cross_reference_report.md) | Optional Nushell/Yazelix bridge evidence. |

## V1.1 product and implementation lane

| Order | File | Purpose |
|---:|---|---|
| 17 | [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Architecture specification. |
| 18 | [docs/SCHEMA.md](docs/SCHEMA.md) | Schema and table specification. |
| 19 | [docs/COMMANDS.md](docs/COMMANDS.md) | CLI, Nu, and MCP command reference. |
| 20 | [docs/INTEGRATION_CONTRACTS.md](docs/INTEGRATION_CONTRACTS.md) | Integration ownership and boundary contracts. |
| 21 | [docs/SECURITY_AND_SECRET_POLICY.md](docs/SECURITY_AND_SECRET_POLICY.md) | Secret, source-blob, and MCP leak policy. |
| 22 | [docs/UNSAFE_CAPTURE_POLICY.md](docs/UNSAFE_CAPTURE_POLICY.md) | Unsafe build and proc-macro capture gate. |
| 23 | [docs/NUSHELL_PLUGIN_COMPAT.md](docs/NUSHELL_PLUGIN_COMPAT.md) | Host and Yazelix Nu compatibility strategy. |
| 24 | [docs/CODEX_BRIDGE.md](docs/CODEX_BRIDGE.md) | Codex CLI and MCP bridge contract. |
| 25 | [docs/META_INTEGRATION.md](docs/META_INTEGRATION.md) | Meta project-selection integration. |
| 26 | [docs/ENVCTL_EXPORT_CONTRACT.md](docs/ENVCTL_EXPORT_CONTRACT.md) | envctl export and checksum contract. |
| 27 | [docs/YAZELIX_PLACEMENT.md](docs/YAZELIX_PLACEMENT.md) | Yazelix runtime placement and init bridge. |
| 28 | [docs/TEST_PLAN.md](docs/TEST_PLAN.md) | Test and validation plan. |
| 29 | [docs/FIXTURE_MATRIX.md](docs/FIXTURE_MATRIX.md) | Fixture coverage matrix. |
| 30 | [docs/RELEASE_GATE.md](docs/RELEASE_GATE.md) | Release proof gates. |
| 31 | [docs/AUDIT_UPGRADE_COMPLETION.md](docs/AUDIT_UPGRADE_COMPLETION.md) | Post-audit authority and upgrade policy. |
| 32 | [execution/TASK_GRAPH.csv](execution/TASK_GRAPH.csv) | Authoritative V1.1 implementation task graph. |
| 33 | [execution/TASK_GRAPH.md](execution/TASK_GRAPH.md) | Human-readable V1.1 task projection. |
| 34 | [execution/TASK_FILE_MAP.csv](execution/TASK_FILE_MAP.csv) | V1.1 task-to-file ownership map. |

## V1.2 polyglot planning lane

This lane is planning-only and does not supersede the V1.1 implementation graph.

| Order | File | Purpose |
|---:|---|---|
| 35 | [docs/polyglot-import/README.md](docs/polyglot-import/README.md) | Polyglot planning entrypoint. |
| 36 | [docs/polyglot-import/research-ledger.md](docs/polyglot-import/research-ledger.md) | Official-source research ledger. |
| 37 | [docs/polyglot-import/language-import-surface.md](docs/polyglot-import/language-import-surface.md) | Language and package capture surface. |
| 38 | [docs/polyglot-import/polyglot-schema-extension.md](docs/polyglot-import/polyglot-schema-extension.md) | Planned schema extension. |
| 39 | [docs/polyglot-import/parser-and-indexer-tooling-matrix.md](docs/polyglot-import/parser-and-indexer-tooling-matrix.md) | Parser and indexer comparison. |
| 40 | [docs/polyglot-import/package-manager-and-lockfile-matrix.md](docs/polyglot-import/package-manager-and-lockfile-matrix.md) | Package-manager and lockfile coverage. |
| 41 | [docs/polyglot-import/whole-repo-import-architecture.md](docs/polyglot-import/whole-repo-import-architecture.md) | Whole-repository import/export architecture. |
| 42 | [docs/polyglot-import/single-binary-rust-crate-export.md](docs/polyglot-import/single-binary-rust-crate-export.md) | Generated Rust crate and binary contract. |
| 43 | [docs/polyglot-import/proof-and-round-trip-gates.md](docs/polyglot-import/proof-and-round-trip-gates.md) | Polyglot proof and round-trip gates. |
| 44 | [docs/polyglot-import/security-and-execution-policy.md](docs/polyglot-import/security-and-execution-policy.md) | Polyglot safety policy. |
| 45 | [docs/polyglot-import/github-issue-delivery-plan.md](docs/polyglot-import/github-issue-delivery-plan.md) | Issue delivery dependency map. |
| 46 | [docs/polyglot-import/open-questions.md](docs/polyglot-import/open-questions.md) | Remaining gaps and blockers. |
| 47 | [prd/nu_plugin_codedb_v1_2_polyglot_import_prd.md](prd/nu_plugin_codedb_v1_2_polyglot_import_prd.md) | V1.2 planning addendum. |
| 48 | [execution/POLYGLOT_TASK_GRAPH.csv](execution/POLYGLOT_TASK_GRAPH.csv) | Polyglot planning task graph. |
| 49 | [execution/POLYGLOT_TASK_FILE_MAP.csv](execution/POLYGLOT_TASK_FILE_MAP.csv) | Polyglot planning task-to-file map. |
| 50 | [execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md](execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md) | Authoritative ready-to-post planning issue drafts for CDB091-CDB105. |

## Issue 212 bidirectional lane

| Order | File | Purpose |
|---:|---|---|
| 51 | [docs/BIDIRECTIONAL_ROADMAP.md](docs/BIDIRECTIONAL_ROADMAP.md) | Phase plan and ownership. |
| 52 | [docs/BIDIRECTIONAL_ARCHITECTURE.md](docs/BIDIRECTIONAL_ARCHITECTURE.md) | Source-to-plan-to-apply architecture. |
| 53 | [docs/ROUND_TRIP_PROOF.md](docs/ROUND_TRIP_PROOF.md) | Round-trip and re-scan proof chain. |
| 54 | [docs/CHANGE_PLAN_SCHEMA.md](docs/CHANGE_PLAN_SCHEMA.md) | Change-plan and apply-row schema. |
| 55 | [docs/MUTATION_POLICY.md](docs/MUTATION_POLICY.md) | Mutation approval gates and stop rules. |
| 56 | [docs/GAP_CLOSURE_PLAN.md](docs/GAP_CLOSURE_PLAN.md) | Issue 212 V1.1 gap-closure rail. |
| 57 | [execution/BIDIRECTIONAL_TASK_GRAPH.csv](execution/BIDIRECTIONAL_TASK_GRAPH.csv) | CDB070-CDB090 task graph. |
| 58 | [execution/BIDIRECTIONAL_TASK_FILE_MAP.csv](execution/BIDIRECTIONAL_TASK_FILE_MAP.csv) | CDB070-CDB090 task ownership map. |
| 59 | [scripts/validate_bidirectional_package.py](scripts/validate_bidirectional_package.py) | Bidirectional package validation gate. |

## Evidence and package integrity

| Order | File | Purpose |
|---:|---|---|
| 60 | [execution/COMMAND_LEDGER.csv](execution/COMMAND_LEDGER.csv) | Command evidence ledger. |
| 61 | [execution/WORKLOG.md](execution/WORKLOG.md) | Narrative execution worklog. |
| 62 | [manifests/EXTRACTION_PROOF.json](manifests/EXTRACTION_PROOF.json) | Source extraction proof. |
| 63 | [manifests/CHECKLIST_COMPLETION.json](manifests/CHECKLIST_COMPLETION.json) | Machine-readable checklist completion. |
| 64 | [manifests/PACK_MANIFEST.json](manifests/PACK_MANIFEST.json) | Package manifest. |
| 65 | [manifests/CHECKSUMS.sha256](manifests/CHECKSUMS.sha256) | Package checksums. |
| 66 | [manifests/LINK_CHECK_REPORT.md](manifests/LINK_CHECK_REPORT.md) | Local link-check report. |
| 67 | [manifests/PACKAGE_VALIDATION.json](manifests/PACKAGE_VALIDATION.json) | Package validation results. |
| 68 | [manifests/CSV_DOC_LINK_AUDIT_INPUT.md](manifests/CSV_DOC_LINK_AUDIT_INPUT.md) | CSV/document linkage audit input. |
| 69 | [manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json](manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json) | Strict task/file linkage repair evidence. |

Rules:

- Use `execution/TASK_GRAPH.csv` for V1.1 implementation work.
- Use `execution/POLYGLOT_TASK_GRAPH.csv` only for V1.2 planning work.
- Treat checked-in CDB091-CDB105 issue drafts as the delivery surface until their posted GitHub URLs are recorded.
- Use `execution/BIDIRECTIONAL_TASK_GRAPH.csv` for issue 212 work.
- After the selected lane, record evidence in the command ledger and worklog, then validate package integrity.

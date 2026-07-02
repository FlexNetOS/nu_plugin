# CSV Doc-Link Audit

Generated: `2026-07-02T01:30:28Z`
Final ZIP: `nu_plugin_codedb_final_execution_package_verified.zip`
ZIP SHA-256: `a5f1362272140e48615f09aebfa9c8857bd562cce791464d98779dce13604ecc`

## Verdict

The task graph CSV does reference the documentation set, but the references are not strict enough for a hard-runner interpretation. Several completed documentation tasks use basename-only artifact/evidence references such as `ARCHITECTURE.md` while the real file path is `docs/ARCHITECTURE.md`. These resolve uniquely by basename, but they are not exact links.

## Counts

- Task graph rows: `68`
- Task status counts: `{'complete': 17, 'planned': 51}`
- Reference resolution counts: `{'exact': 869, 'missing_or_future_path': 181, 'unique_basename_only': 47, 'planned_glob_or_future_path': 53}`
- Markdown/docs checked: `30`
- Docs with zero direct TASK_GRAPH.csv references: `3`

## Docs with zero direct TASK_GRAPH.csv references

- `ACCEPTANCE.md`
- `FIRST_RUN_PROMPT.md`
- `nu_plugin_codedb_remaining_execution_checklist.md`

## Completed documentation rows

| Task | Name | Output artifacts | Allowed files | Evidence path |
|---|---|---|---|---|
| `CDB006` | Write architecture document | `ARCHITECTURE.md` | `docs/ARCHITECTURE.md` | `logs/CDB006-architecture.log;ARCHITECTURE.md` |
| `CDB007` | Write schema document | `SCHEMA.md` | `docs/SCHEMA.md` | `logs/CDB007-schema.log;SCHEMA.md` |
| `CDB008` | Write command reference | `COMMANDS.md` | `docs/COMMANDS.md` | `logs/CDB008-commands.log;COMMANDS.md` |
| `CDB009` | Write integration contracts | `INTEGRATION_CONTRACTS.md` | `docs/INTEGRATION_CONTRACTS.md` | `logs/CDB009-integration.log;INTEGRATION_CONTRACTS.md` |
| `CDB010` | Write security and unsafe capture policies | `SECURITY_AND_SECRET_POLICY.md` | `docs/SECURITY_AND_SECRET_POLICY.md;docs/UNSAFE_CAPTURE_POLICY.md` | `logs/CDB010-security.log;SECURITY_AND_SECRET_POLICY.md` |
| `CDB011` | Write compatibility bridge docs | `CODEX_BRIDGE.md` | `docs/CODEX_BRIDGE.md;docs/NUSHELL_PLUGIN_COMPAT.md;docs/YAZELIX_PLACEMENT.md` | `logs/CDB011-bridge.log;CODEX_BRIDGE.md` |
| `CDB012` | Write test and fixture matrix | `TEST_PLAN.md` | `docs/TEST_PLAN.md;docs/FIXTURE_MATRIX.md` | `logs/CDB012-tests-docs.log;TEST_PLAN.md` |

## Planned rows that reference docs

| Task | Name | Output artifacts | Allowed files | Evidence path |
|---|---|---|---|---|
| `CDB035` | Implement envctl export contract | `export manifests` | `crates/codedb/**;docs/ENVCTL_EXPORT_CONTRACT.md` | `logs/CDB035-envctl-export.log;export manifests` |
| `CDB036` | Implement meta repo selection inputs | `--repo-id/--repo-path` | `codedb CLI;docs/META_INTEGRATION.md` | `logs/CDB036-meta.log;--repo-id/--repo-path` |
| `CDB037` | Implement Codex bridge docs and sample MCP config | `Codex bridge docs` | `docs/CODEX_BRIDGE.md;examples/codex/**` | `logs/CDB037-codex-bridge.log;Codex bridge docs` |
| `CDB038` | Implement Yazelix placement docs | `Yazelix docs` | `docs/YAZELIX_PLACEMENT.md` | `logs/CDB038-yazelix.log;Yazelix docs` |
| `CDB039` | Implement runner proof contract | `proof export` | `docs/RELEASE_GATE.md;crates/codedb/**` | `logs/CDB039-runner.log;proof export` |
| `CDB040` | Implement GitKB/RTK/Kache/wild/Fenix docs | `integration docs` | `docs/INTEGRATION_CONTRACTS.md` | `logs/CDB040-tooling.log;integration docs` |
| `CDB048` | Prepare handoff and backlog | `handoff docs` | `HANDOFF.md;BACKLOG.md` | `logs/CDB048-handoff.log;handoff docs` |
| `CDB049` | Inspect Yazelix Nushell runtime bridge | `YAZELIX_NUSHELL_RUNTIME.md` | `research/nushell_yazelix_cross_reference_report.md;docs/YAZELIX_NUSHELL_RUNTIME.md` | `cross-reference report;YAZELIX_NUSHELL_RUNTIME.md` |
| `CDB050` | Package nu_plugin_codedb as runtime tool | `nu_plugin_codedb runtime package` | `flake.nix;packaging/**;docs/CODEDB_YAZELIX_RUNTIME_TOOL.md` | `runtime package metadata;plugin/CLI smoke output` |
| `CDB051` | Validate host Nu vs Yazelix runtime Nu protocol | `codedb doctor --nu --yazelix` | `crates/codedb/**;docs/NUSHELL_PLUGIN_COMPAT.md` | `doctor output;protocol status row` |
| `CDB054` | Generate CodeDB extern/init bridge artifact | `codedb_init.nu/codedb_extern.nu` | `crates/codedb/**;templates/nushell/**;docs/CODEDB_YAZELIX_INIT_CONTRACT.md` | `generated init/extern checksums` |
| `CDB056` | Extend syntax validator path for CodeDB fixtures | `nu --no-config-file --ide-check` | `tests/**;fixtures/**;docs/CODEDB_NUSHELL_SYNTAX_GATE.md` | `syntax report` |
| `CDB058` | Add Yazelix launch smoke with CodeDB disabled | `disabled smoke` | `tests/**;docs/YAZELIX_PLACEMENT.md` | `launch smoke log` |
| `CDB059` | Add Yazelix launch smoke with CodeDB enabled | `enabled smoke` | `tests/**;docs/YAZELIX_PLACEMENT.md` | `launch smoke log` |
| `CDB060` | Add plugin stderr/trace secret-leak guard | `stderr/log/MCP leak tests` | `tests/**;docs/SECURITY_AND_SECRET_POLICY.md` | `redaction report;test log` |
| `CDB062` | Add Codex bounded CLI/MCP invocation smoke | `Codex bridge smoke` | `tests/**;docs/CODEX_BRIDGE.md;examples/codex/**` | `MCP tool report;CLI output sample` |
| `CDB063` | Add envctl table rows for CodeDB runtime integration | `CodeDB envctl export rows` | `docs/ENVCTL_EXPORT_CONTRACT.md;crates/codedb/**` | `export sample;checksum rows` |

## Complete-task basename-only references

| Task | Column | CSV ref | Real file |
|---|---|---|---|
| `CDB003` | `output_artifacts` | `TASK_GRAPH.csv` | `execution/TASK_GRAPH.csv` |
| `CDB003` | `evidence_path` | `TASK_GRAPH.csv` | `execution/TASK_GRAPH.csv` |
| `CDB003` | `evidence_artifacts` | `TASK_GRAPH.csv` | `execution/TASK_GRAPH.csv` |
| `CDB003` | `allowed_files` | `TASK_GRAPH.csv` | `execution/TASK_GRAPH.csv` |
| `CDB003` | `allowed_files` | `TASK_FILE_MAP.csv` | `execution/TASK_FILE_MAP.csv` |
| `CDB003` | `primary_artifact` | `TASK_GRAPH.csv` | `execution/TASK_GRAPH.csv` |
| `CDB004` | `output_artifacts` | `COMMAND_LEDGER.csv` | `execution/COMMAND_LEDGER.csv` |
| `CDB004` | `evidence_path` | `COMMAND_LEDGER.csv` | `execution/COMMAND_LEDGER.csv` |
| `CDB004` | `evidence_artifacts` | `COMMAND_LEDGER.csv` | `execution/COMMAND_LEDGER.csv` |
| `CDB004` | `allowed_files` | `COMMAND_LEDGER.csv` | `execution/COMMAND_LEDGER.csv` |
| `CDB004` | `allowed_files` | `WORKLOG.md` | `execution/WORKLOG.md` |
| `CDB004` | `primary_artifact` | `COMMAND_LEDGER.csv` | `execution/COMMAND_LEDGER.csv` |
| `CDB005` | `output_artifacts` | `PACK_MANIFEST.json` | `manifests/PACK_MANIFEST.json` |
| `CDB005` | `evidence_path` | `PACK_MANIFEST.json` | `manifests/PACK_MANIFEST.json` |
| `CDB005` | `evidence_artifacts` | `PACK_MANIFEST.json` | `manifests/PACK_MANIFEST.json` |
| `CDB005` | `allowed_files` | `PACK_MANIFEST.json` | `manifests/PACK_MANIFEST.json` |
| `CDB005` | `allowed_files` | `CHECKSUMS.sha256` | `manifests/CHECKSUMS.sha256` |
| `CDB005` | `allowed_files` | `LINK_CHECK_REPORT.md` | `manifests/LINK_CHECK_REPORT.md` |
| `CDB005` | `primary_artifact` | `PACK_MANIFEST.json` | `manifests/PACK_MANIFEST.json` |
| `CDB006` | `output_artifacts` | `ARCHITECTURE.md` | `docs/ARCHITECTURE.md` |
| `CDB006` | `evidence_path` | `ARCHITECTURE.md` | `docs/ARCHITECTURE.md` |
| `CDB006` | `evidence_artifacts` | `ARCHITECTURE.md` | `docs/ARCHITECTURE.md` |
| `CDB006` | `primary_artifact` | `ARCHITECTURE.md` | `docs/ARCHITECTURE.md` |
| `CDB007` | `output_artifacts` | `SCHEMA.md` | `docs/SCHEMA.md` |
| `CDB007` | `evidence_path` | `SCHEMA.md` | `docs/SCHEMA.md` |
| `CDB007` | `evidence_artifacts` | `SCHEMA.md` | `docs/SCHEMA.md` |
| `CDB007` | `primary_artifact` | `SCHEMA.md` | `docs/SCHEMA.md` |
| `CDB008` | `output_artifacts` | `COMMANDS.md` | `docs/COMMANDS.md` |
| `CDB008` | `evidence_path` | `COMMANDS.md` | `docs/COMMANDS.md` |
| `CDB008` | `evidence_artifacts` | `COMMANDS.md` | `docs/COMMANDS.md` |
| `CDB008` | `primary_artifact` | `COMMANDS.md` | `docs/COMMANDS.md` |
| `CDB009` | `output_artifacts` | `INTEGRATION_CONTRACTS.md` | `docs/INTEGRATION_CONTRACTS.md` |
| `CDB009` | `evidence_path` | `INTEGRATION_CONTRACTS.md` | `docs/INTEGRATION_CONTRACTS.md` |
| `CDB009` | `evidence_artifacts` | `INTEGRATION_CONTRACTS.md` | `docs/INTEGRATION_CONTRACTS.md` |
| `CDB009` | `primary_artifact` | `INTEGRATION_CONTRACTS.md` | `docs/INTEGRATION_CONTRACTS.md` |
| `CDB010` | `output_artifacts` | `SECURITY_AND_SECRET_POLICY.md` | `docs/SECURITY_AND_SECRET_POLICY.md` |
| `CDB010` | `evidence_path` | `SECURITY_AND_SECRET_POLICY.md` | `docs/SECURITY_AND_SECRET_POLICY.md` |
| `CDB010` | `evidence_artifacts` | `SECURITY_AND_SECRET_POLICY.md` | `docs/SECURITY_AND_SECRET_POLICY.md` |
| `CDB010` | `primary_artifact` | `SECURITY_AND_SECRET_POLICY.md` | `docs/SECURITY_AND_SECRET_POLICY.md` |
| `CDB011` | `output_artifacts` | `CODEX_BRIDGE.md` | `docs/CODEX_BRIDGE.md` |
| `CDB011` | `evidence_path` | `CODEX_BRIDGE.md` | `docs/CODEX_BRIDGE.md` |
| `CDB011` | `evidence_artifacts` | `CODEX_BRIDGE.md` | `docs/CODEX_BRIDGE.md` |
| `CDB011` | `primary_artifact` | `CODEX_BRIDGE.md` | `docs/CODEX_BRIDGE.md` |
| `CDB012` | `output_artifacts` | `TEST_PLAN.md` | `docs/TEST_PLAN.md` |
| `CDB012` | `evidence_path` | `TEST_PLAN.md` | `docs/TEST_PLAN.md` |
| `CDB012` | `evidence_artifacts` | `TEST_PLAN.md` | `docs/TEST_PLAN.md` |
| `CDB012` | `primary_artifact` | `TEST_PLAN.md` | `docs/TEST_PLAN.md` |

## Complete-task missing evidence refs

| Task | Column | Missing ref |
|---|---|---|
| `CDB000` | `evidence_path` | `logs/CDB000-package-init.log` |
| `CDB000` | `raw_log_path` | `logs/CDB000-package-init.log` |
| `CDB000` | `evidence_artifacts` | `logs/CDB000-package-init.log` |
| `CDB001` | `evidence_path` | `logs/CDB001-navigation.log` |
| `CDB001` | `raw_log_path` | `logs/CDB001-navigation.log` |
| `CDB001` | `evidence_artifacts` | `logs/CDB001-navigation.log` |
| `CDB002` | `evidence_path` | `logs/CDB002-gates.log` |
| `CDB002` | `raw_log_path` | `logs/CDB002-gates.log` |
| `CDB002` | `evidence_artifacts` | `logs/CDB002-gates.log` |
| `CDB003` | `evidence_path` | `logs/CDB003-task-graph.log` |
| `CDB003` | `raw_log_path` | `logs/CDB003-task-graph.log` |
| `CDB003` | `evidence_artifacts` | `logs/CDB003-task-graph.log` |
| `CDB004` | `evidence_path` | `logs/CDB004-ledger.log` |
| `CDB004` | `raw_log_path` | `logs/CDB004-ledger.log` |
| `CDB004` | `evidence_artifacts` | `logs/CDB004-ledger.log` |
| `CDB005` | `evidence_path` | `logs/CDB005-manifest.log` |
| `CDB005` | `raw_log_path` | `logs/CDB005-manifest.log` |
| `CDB005` | `evidence_artifacts` | `logs/CDB005-manifest.log` |

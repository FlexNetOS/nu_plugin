# Checklist Completion Evidence

Created: 2026-07-02T01:02:12.782019+00:00
Updated: 2026-07-02T01:44:15Z

Status: **PASSED**

Checklist items mapped/addressed: **109**
Unmapped items: **0**

## Source-of-truth repair

- Repair task: `CDB068`
- Evidence: [manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json](manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json)
- Rule: `execution/TASK_GRAPH.csv` is the task source of truth; current package artifacts use exact package-relative paths.

## Completion semantics

- `complete_artifact`: Artifact exists in final package and is validated.
- `mapped_to_controlled_planned_task`: Implementation work is not falsely marked done; it is assigned to a stable task with dependencies, gates, evidence path, and status planned.
- `complete_doc_artifact_future_task_still_planned`: Documentation artifact exists now; executable proof remains controlled by future task.

## Status counts

- `complete_artifact`: 47
- `complete_doc_artifact_future_task_still_planned`: 11
- `mapped_to_controlled_planned_task`: 36
- `mapped_to_controlled_planned_gate`: 15

## Checklist map

| Line | Status | Mapped task(s) | Evidence | Item |
| --- | --- | --- | --- | --- |
| 57 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `CODEDB_START_HERE.md` with one mandatory path: read gate first, then navigation, then PRD, then task graph. |
| 58 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `NAVIGATION.md` with a numbered file index. |
| 59 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `NAVIGATION.json` with the same file order and purposes. |
| 60 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md | - [ ] Create `DOC_GRAPH.md` with dependency edges: |
| 64 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md | - [ ] Create `DRIFT_GUARD.md` with explicit context-size and ownership limits. |
| 65 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `STOP_CONDITIONS.md` with hard stops for source overwrite, hidden mutation, plugin version mismatch, unbounded MCP output, unsafe build execution, secret capture, and missing raw logs. |
| 66 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `READINESS_GATE.md` requiring task ID, PRD section, source-of-truth owner, generated artifact status, validation command, raw log path, and no-secret path. |
| 67 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Link every file from `CODEDB_START_HERE.md` and `NAVIGATION.md`. |
| 68 | complete_artifact | CDB000, CDB001, CDB002, CDB064, CDB067, CDB068 | CODEDB_START_HERE.md; NAVIGATION.md; NAVIGATION.json; DOC_GRAPH.md; DRIFT_GUARD.md; STOP_CONDITIONS.md; READINESS_GATE.md; manifests/LINK_CHECK_REPORT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Run a local markdown link check and save `LINK_CHECK_REPORT.md`. |
| 74 | complete_artifact | CDB003, CDB065, CDB068 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Create `TASK_GRAPH.csv`. |
| 75 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Use stable task IDs: `CDB000`–`CDB999`. |
| 76 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Include columns: |
| 90 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Ensure no duplicate task IDs. |
| 91 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Ensure every P0 PRD requirement has at least one task. |
| 92 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Ensure every task maps back to one PRD section. |
| 93 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add dependency edges for storage before scan, scan before export, export before MCP, fixtures before release. |
| 94 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add tasks for no-mutation proof and secret-policy tests before any reproduction task. |
| 95 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add tasks for host Nu and Yazelix Nu compatibility before declaring plugin install complete. |
| 96 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add tasks for Codex CLI and MCP bridge before Codex is allowed to use CodeDB. |
| 97 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add tasks for envctl export contract before envctl integration. |
| 98 | complete_artifact | CDB003, CDB065 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Add tasks for meta project selection before multi-repo scanning. |
| 122 | complete_artifact | CDB006 | docs/ARCHITECTURE.md | - [ ] `ARCHITECTURE.md`: system diagram, crates, data flow, runtime modes. |
| 123 | complete_artifact | CDB007, CDB068 | docs/SCHEMA.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] `SCHEMA.md`: table groups, field-level schema, identity rules, checksums. |
| 124 | complete_artifact | CDB008 | docs/COMMANDS.md | - [ ] `COMMANDS.md`: CLI, Nu plugin commands, MCP tools, flags, examples. |
| 125 | complete_artifact | CDB009 | docs/INTEGRATION_CONTRACTS.md | - [ ] `INTEGRATION_CONTRACTS.md`: Codex, Yazelix, meta, envctl, runner, GitKB, RTK, Kache/wild/Fenix. |
| 126 | complete_artifact | CDB010 | docs/SECURITY_AND_SECRET_POLICY.md | - [ ] `SECURITY_AND_SECRET_POLICY.md`: source blob modes, secret detection, MCP leak guard. |
| 127 | complete_artifact | CDB010 | docs/UNSAFE_CAPTURE_POLICY.md | - [ ] `UNSAFE_CAPTURE_POLICY.md`: build.rs/proc-macro execution gates, raw logs, approval flow. |
| 128 | complete_artifact | CDB011 | docs/NUSHELL_PLUGIN_COMPAT.md | - [ ] `NUSHELL_PLUGIN_COMPAT.md`: host/Yazelix Nu plugin registration and version strategy. |
| 129 | complete_artifact | CDB011 | docs/CODEX_BRIDGE.md | - [ ] `CODEX_BRIDGE.md`: CLI/MCP bridge, config fragments, output bounds, no-mutation rules. |
| 130 | complete_doc_artifact_future_task_still_planned | CDB036 | docs/META_INTEGRATION.md | - [ ] `META_INTEGRATION.md`: repo selection, project IDs, no meta replacement. |
| 131 | complete_doc_artifact_future_task_still_planned | CDB035, CDB068 | docs/ENVCTL_EXPORT_CONTRACT.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] `ENVCTL_EXPORT_CONTRACT.md`: exported tables/checksums/env facts. |
| 132 | complete_artifact | CDB011 | docs/YAZELIX_PLACEMENT.md | - [ ] `YAZELIX_PLACEMENT.md`: where plugin/CLI appears in Yazelix runtime. |
| 133 | complete_artifact | CDB012 | docs/TEST_PLAN.md | - [ ] `TEST_PLAN.md`: unit, fixture, integration, security, no-mutation, MCP, CLI, Nu plugin. |
| 134 | complete_artifact | CDB012 | docs/FIXTURE_MATRIX.md | - [ ] `FIXTURE_MATRIX.md`: fixture definitions and expected rows. |
| 135 | complete_doc_artifact_future_task_still_planned | CDB039 | docs/RELEASE_GATE.md | - [ ] `RELEASE_GATE.md`: release proof requirements. |
| 136 | complete_doc_artifact_future_task_still_planned | CDB048 | BACKLOG.md | - [ ] `BACKLOG.md`: MVP2 features and downgrade exclusions. |
| 185 | mapped_to_controlled_planned_task | CDB013 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Create Rust workspace skeleton. |
| 186 | mapped_to_controlled_planned_task | CDB030 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `nu_plugin_codedb` crate. |
| 187 | mapped_to_controlled_planned_task | CDB029 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb` CLI crate. |
| 188 | mapped_to_controlled_planned_task | CDB014 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-core` crate. |
| 189 | mapped_to_controlled_planned_task | CDB015 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-store-redb` crate. |
| 190 | mapped_to_controlled_planned_task | CDB019 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-cargo` crate. |
| 191 | mapped_to_controlled_planned_task | CDB022 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-rust-static` crate. |
| 192 | mapped_to_controlled_planned_task | CDB033 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-build-capture` crate, gated and disabled by default. |
| 193 | mapped_to_controlled_planned_task | CDB032 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-mcp` crate. |
| 194 | mapped_to_controlled_planned_task | CDB041 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add `codedb-fixtures` crate/folder. |
| 195 | mapped_to_controlled_planned_task | CDB014 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Add schema definitions and version metadata. |
| 196 | mapped_to_controlled_planned_task | CDB015 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement redb initialization and metadata record. |
| 197 | mapped_to_controlled_planned_task | CDB017 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement filesystem scanner. |
| 198 | mapped_to_controlled_planned_task | CDB018 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement exact source metadata capture. |
| 199 | mapped_to_controlled_planned_task | CDB018 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement source blob policy modes. |
| 200 | mapped_to_controlled_planned_task | CDB019 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement Cargo metadata capture. |
| 201 | mapped_to_controlled_planned_task | CDB020 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement Cargo source provenance capture. |
| 202 | mapped_to_controlled_planned_task | CDB022 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement static Rust item/macro/build-script detection. |
| 203 | mapped_to_controlled_planned_task | CDB021 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement `cfg`/feature/target/toolchain context capture. |
| 204 | mapped_to_controlled_planned_task | CDB027 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement capture gaps and validation errors. |
| 205 | mapped_to_controlled_planned_task | CDB028 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement no-mutation proof. |
| 206 | mapped_to_controlled_planned_task | CDB030 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement Nu plugin command outputs. |
| 207 | mapped_to_controlled_planned_task | CDB029 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement CLI JSON/NUON/CSV outputs. |
| 208 | mapped_to_controlled_planned_task | CDB032 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement read-only MCP server. |
| 209 | mapped_to_controlled_planned_task | CDB032 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement output pagination and byte limits. |
| 210 | mapped_to_controlled_planned_task | CDB016 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement backup/restore smoke. |
| 211 | mapped_to_controlled_planned_task | CDB041 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement fixtures and tests. |
| 212 | mapped_to_controlled_planned_task | CDB006 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md | - [ ] Implement docs and examples. |
| 218 | mapped_to_controlled_planned_gate | CDB046 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `cargo fmt --check` passes. |
| 219 | mapped_to_controlled_planned_gate | CDB046 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `cargo clippy --all-targets --all-features` passes or documented exceptions exist. |
| 220 | mapped_to_controlled_planned_gate | CDB046 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `cargo test` passes. |
| 221 | mapped_to_controlled_planned_gate | CDB031 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `codedb doctor --nu` reports usable or degraded status clearly. |
| 222 | mapped_to_controlled_planned_gate | CDB031 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `codedb doctor --yazelix` reports usable or degraded status clearly. |
| 223 | mapped_to_controlled_planned_gate | CDB031 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `codedb doctor --codex` reports usable or degraded status clearly. |
| 224 | mapped_to_controlled_planned_gate | CDB042, CDB068 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Repeated scan of unchanged fixture produces same table checksums. |
| 225 | mapped_to_controlled_planned_gate | CDB044 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Clean fixture repo remains clean after scan. |
| 226 | mapped_to_controlled_planned_gate | CDB044 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Dirty fixture repo records pre-existing dirty state and does not worsen it. |
| 227 | mapped_to_controlled_planned_gate | CDB043 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] Secret-looking fixture follows configured policy. |
| 228 | mapped_to_controlled_planned_gate | CDB062 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] MCP cannot dump raw source by default. |
| 229 | mapped_to_controlled_planned_gate | CDB045 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `capture build` refuses without unsafe flag. |
| 230 | mapped_to_controlled_planned_gate | CDB034 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] `capture build --unsafe-execute-build` preserves raw logs if implemented. |
| 231 | mapped_to_controlled_planned_gate | CDB016 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] redb backup and restore smoke passes. |
| 232 | mapped_to_controlled_planned_gate | CDB035 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] envctl export JSON/NUON/CSV validates. |
| 233 | complete_artifact | CDB065, CDB068 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] task graph has no duplicate IDs. |
| 234 | complete_artifact | CDB067 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json | - [ ] all local markdown links resolve. |
| 235 | complete_artifact | CDB067, CDB068 | execution/TASK_GRAPH.csv; manifests/PACKAGE_VALIDATION.json; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] package manifest checksums match actual files. |
| 245 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] `CODEDB_START_HERE.md` exists and points to `READINESS_GATE.md`. |
| 246 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256 | - [x] PRD is standalone V1.1 full. |
| 247 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] `TASK_GRAPH.csv` exists and has unique task IDs. |
| 248 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] `TASK_FILE_MAP.csv` maps every task to docs/files. |
| 249 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] `FIRST_RUN_PROMPT.md` includes no-mutation and unsafe-capture rules. |
| 250 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] `STOP_CONDITIONS.md` includes plugin version mismatch and unsafe execution stop rules. |
| 251 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] Link check passes. |
| 252 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256 | - [x] Secret-pattern scan over package passes. |
| 253 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067, CDB068 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [x] Manifest and checksums exist. |
| 254 | complete_artifact | CDB000, CDB001, CDB002, CDB003, CDB004, CDB005, CDB067 | manifests/PACKAGE_VALIDATION.json; manifests/PACK_MANIFEST.json; manifests/CHECKSUMS.sha256 | - [x] Scaffold tasks `CDB000`–`CDB005` are marked complete. |
| 260 | complete_doc_artifact_future_task_still_planned | CDB037 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv | - [ ] Codex bridge: CLI/MCP path, absolute binary paths, output bounds, no auth hacks. |
| 261 | complete_doc_artifact_future_task_still_planned | CDB051 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv | - [ ] Nushell plugin compatibility: host/runtime Nu protocol checks and registry isolation. |
| 262 | complete_doc_artifact_future_task_still_planned | CDB035, CDB068 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] envctl export contract: envctl reads exported rows/checksums, not redb internals. |
| 263 | complete_doc_artifact_future_task_still_planned | CDB036 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv | - [ ] meta integration: selected repo graph input, no meta replacement. |
| 264 | complete_doc_artifact_future_task_still_planned | CDB038 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv | - [ ] Yazelix placement: runtime tool + generated init/extern bridge, no tracked `config.nu` edit. |
| 265 | complete_doc_artifact_future_task_still_planned | CDB010 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv | - [ ] Security and secret policy: source blob modes, MCP leak guard, log redaction. |
| 266 | complete_doc_artifact_future_task_still_planned | CDB039, CDB068 | docs/CODEX_BRIDGE.md; docs/NUSHELL_PLUGIN_COMPAT.md; docs/ENVCTL_EXPORT_CONTRACT.md; docs/META_INTEGRATION.md; docs/YAZELIX_PLACEMENT.md; docs/SECURITY_AND_SECRET_POLICY.md; docs/RELEASE_GATE.md; execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Release gate: no-mutation proof, table checksums, raw logs, manifest. |
| 356 | complete_artifact | CDB006, CDB068 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] During implementation, generate the standalone docs listed in section 6 or explicitly fold each topic into existing docs with a `covered_by` entry in `TASK_FILE_MAP.csv`. |
| 357 | mapped_to_controlled_planned_task | CDB013 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement the Rust workspace and crates from the PRD. |
| 358 | mapped_to_controlled_planned_task | CDB016 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement redb store init, schema version, locking, backup, and restore smoke. |
| 359 | mapped_to_controlled_planned_task | CDB028 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement read-only scan and no-mutation proof before any dynamic capture. |
| 360 | mapped_to_controlled_planned_task | CDB030 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement Nu plugin command surface and host/Yazelix runtime compatibility checks. |
| 361 | mapped_to_controlled_planned_task | CDB054 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement generated CodeDB init/extern bridge under Yazelix state paths only. |
| 362 | mapped_to_controlled_planned_task | CDB062 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement bounded CLI/MCP surfaces for Codex. |
| 363 | mapped_to_controlled_planned_task | CDB035 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement envctl export contract and sample exported tables. |
| 364 | mapped_to_controlled_planned_task | CDB041 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json | - [ ] Implement fixture matrix, deterministic scan tests, no-leak tests, no-mutation tests, and unsafe-capture refusal tests. |
| 365 | complete_artifact | CDB067, CDB068 | execution/TASK_GRAPH.csv; execution/TASK_GRAPH.md; docs/RELEASE_GATE.md; manifests/PACKAGE_VALIDATION.json; execution/TASK_FILE_MAP.csv; manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json | - [ ] Run final release proof and regenerate package manifest/checksums. |

# TASK GRAPH

`execution/TASK_GRAPH.csv` is the source of truth for task execution. This Markdown file is a readable projection only.

## Source-of-truth contract

- Every task row must cite exact package-relative file paths for current package artifacts.
- Future implementation paths may use declared globs only when the task status is `planned`.
- Completed tasks must have an existing evidence path and raw log path.
- Any execution starts by selecting one row from `execution/TASK_GRAPH.csv`, then passing `READINESS_GATE.md`.

## Summary

- Generated: `2026-07-02T01:44:15Z`
- Task rows: `69`
- Status counts: `{'complete': 18, 'planned': 51}`
- First implementation task after package repair: `CDB013`
- Package repair task: `CDB068`

## Tasks

| Task | Status | Phase | Name | Depends on | Primary artifact | Validation gate | Evidence | Path status |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| CDB000 | complete | package | Initialize execution package |  | CODEDB_START_HERE.md | all P0 docs listed | logs/CDB000-package-init.log;CODEDB_START_HERE.md | complete_current_paths_exact |
| CDB001 | complete | package | Create AI navigation graph | CDB000 | NAVIGATION.md | links validate | logs/CDB001-navigation.log;NAVIGATION.md | complete_current_paths_exact |
| CDB002 | complete | package | Create readiness and stop gates | CDB000 | READINESS_GATE.md | gate checklist covers task/prd/log/secret | logs/CDB002-gates.log;READINESS_GATE.md | complete_current_paths_exact |
| CDB003 | complete | package | Create task graph and task-file map | CDB000 | execution/TASK_GRAPH.csv | CSV parses and task IDs unique | logs/CDB003-task-graph.log;execution/TASK_GRAPH.csv | complete_current_paths_exact |
| CDB004 | complete | package | Create command ledger and worklog | CDB000 | execution/COMMAND_LEDGER.csv | CSV parses with expected header | logs/CDB004-ledger.log;execution/COMMAND_LEDGER.csv | complete_current_paths_exact |
| CDB005 | complete | package | Generate manifest, checksums, link report | CDB001;CDB003;CDB004 | manifests/PACK_MANIFEST.json | checksums match files and links pass | logs/CDB005-manifest.log;manifests/PACK_MANIFEST.json | complete_current_paths_exact |
| CDB006 | complete | docs | Write architecture document | CDB005 | docs/ARCHITECTURE.md | covers crates/data flow/runtime modes | logs/CDB006-architecture.log;docs/ARCHITECTURE.md | complete_current_paths_exact |
| CDB007 | complete | docs | Write schema document | CDB006 | docs/SCHEMA.md | table groups and IDs defined | logs/CDB007-schema.log;docs/SCHEMA.md | complete_current_paths_exact |
| CDB008 | complete | docs | Write command reference | CDB006 | docs/COMMANDS.md | CLI/Nu/MCP commands documented | logs/CDB008-commands.log;docs/COMMANDS.md | complete_current_paths_exact |
| CDB009 | complete | docs | Write integration contracts | CDB006 | docs/INTEGRATION_CONTRACTS.md | Codex/Yazelix/meta/envctl/runner covered | logs/CDB009-integration.log;docs/INTEGRATION_CONTRACTS.md | complete_current_paths_exact |
| CDB010 | complete | docs | Write security and unsafe capture policies | CDB006 | docs/SECURITY_AND_SECRET_POLICY.md | source blob and unsafe gates covered | logs/CDB010-security.log;docs/SECURITY_AND_SECRET_POLICY.md | complete_current_paths_exact |
| CDB011 | complete | docs | Write compatibility bridge docs | CDB009 | docs/CODEX_BRIDGE.md | Codex/Nu/Yazelix conflicts bridged | logs/CDB011-bridge.log;docs/CODEX_BRIDGE.md | complete_current_paths_exact |
| CDB012 | complete | docs | Write test and fixture matrix | CDB007 | docs/TEST_PLAN.md | all required fixtures listed | logs/CDB012-tests-docs.log;docs/TEST_PLAN.md | complete_current_paths_exact |
| CDB013 | planned | workspace | Create Rust workspace skeleton | CDB006;CDB068 | Cargo.toml | cargo metadata succeeds | logs/CDB013-workspace.log;Cargo.toml | planned_future_paths_declared |
| CDB014 | planned | core | Implement codedb-core schemas | CDB013;CDB007 | codedb-core | unit tests pass | logs/CDB014-core.log;codedb-core | planned_future_paths_declared |
| CDB015 | planned | store | Implement redb store init | CDB014 | codedb-store-redb | store init/metadata tests pass | logs/CDB015-redb-init.log;codedb-store-redb | planned_future_paths_declared |
| CDB016 | planned | store | Implement redb schema version, locks, backup, restore | CDB015 | backup/restore API | backup restore smoke passes | logs/CDB016-redb-restore.log;backup/restore API | planned_future_paths_declared |
| CDB017 | planned | scan | Implement filesystem scanner | CDB014;CDB015 | filesystem_entries | fixture scan rows stable | logs/CDB017-fs.log;filesystem_entries | planned_future_paths_declared |
| CDB018 | planned | scan | Implement exact source metadata and blob policy | CDB017 | source_blobs metadata | secret policy tests pass | logs/CDB018-source.log;source_blobs metadata | planned_future_paths_declared |
| CDB019 | planned | cargo | Implement cargo metadata capture | CDB014;CDB015 | cargo tables | cargo metadata fixture passes | logs/CDB019-cargo.log;cargo tables | planned_future_paths_declared |
| CDB020 | planned | cargo | Implement Cargo source provenance capture | CDB019 | cargo_sources tables | registry/git/path facts captured | logs/CDB020-cargo-sources.log;cargo_sources tables | planned_future_paths_declared |
| CDB021 | planned | context | Implement cfg/feature/target/toolchain context | CDB019 | codedb_contexts | context rows deterministic | logs/CDB021-context.log;codedb_contexts | planned_future_paths_declared |
| CDB022 | planned | rust-static | Implement static Rust item inventory | CDB018;CDB021 | rust_items | simple item fixture passes | logs/CDB022-rust-items.log;rust_items | planned_future_paths_declared |
| CDB023 | planned | rust-static | Implement macro_rules static inventory | CDB022 | macro tables | macro fixture passes with gaps where needed | logs/CDB023-macros.log;macro tables | planned_future_paths_declared |
| CDB024 | planned | rust-static | Implement proc-macro static detection and gaps | CDB022 | proc_macro tables;capture_gaps | proc macro fixture emits static rows/gaps | logs/CDB024-proc-macro.log;proc_macro tables;capture_gaps | planned_future_paths_declared |
| CDB025 | planned | rust-static | Implement build.rs static detection and gaps | CDB022 | build_scripts;capture_gaps | build script fixture emits static rows/gaps | logs/CDB025-build-static.log;build_scripts;capture_gaps | planned_future_paths_declared |
| CDB026 | planned | rust-static | Implement static include/path edge detection | CDB022 | static_include_edges | include fixture passes | logs/CDB026-include.log;static_include_edges | planned_future_paths_declared |
| CDB027 | planned | native | Implement native/linker static/gap rows | CDB025 | native/link tables;capture_gaps | native fixture emits rows/gaps | logs/CDB027-native.log;native/link tables;capture_gaps | planned_future_paths_declared |
| CDB028 | planned | proof | Implement no-mutation proof | CDB017 | no_mutation_proofs | clean/dirty git fixtures pass | logs/CDB028-no-mutation.log;no_mutation_proofs | planned_future_paths_declared |
| CDB029 | planned | cli | Implement codedb CLI scan/export/schema | CDB015;CDB017;CDB019;CDB022 | codedb CLI | JSON/NUON/CSV outputs validate | logs/CDB029-cli.log;codedb CLI | planned_future_paths_declared |
| CDB030 | planned | nu-plugin | Implement Nushell plugin commands | CDB029 | nu_plugin_codedb | Nu command smoke passes | logs/CDB030-nu-plugin.log;nu_plugin_codedb | planned_future_paths_declared |
| CDB031 | planned | doctor | Implement doctor checks | CDB029;CDB030 | codedb doctor | doctor reports Nu/Yazelix/Codex status | logs/CDB031-doctor.log;codedb doctor | planned_future_paths_declared |
| CDB032 | planned | mcp | Implement bounded read-only MCP server | CDB029 | codedb mcp serve | MCP page/limit/source guard tests pass | logs/CDB032-mcp.log;codedb mcp serve | planned_future_paths_declared |
| CDB033 | planned | unsafe | Implement unsafe build capture gate scaffold | CDB025;CDB032 | capture build | refuses without unsafe flag | logs/CDB033-unsafe-gate.log;capture build | planned_future_paths_declared |
| CDB034 | planned | unsafe | Implement optional build/proc-macro raw log capture | CDB033 | build capture rows | approved fixture captures logs or gaps | logs/CDB034-build-capture.log;build capture rows | planned_future_paths_declared |
| CDB035 | planned | exports | Implement envctl export contract | CDB029 | export manifests | envctl export validates | logs/CDB035-envctl-export.log;export manifests | planned_future_paths_declared |
| CDB036 | planned | integration | Implement meta repo selection inputs | CDB029 | --repo-id/--repo-path | meta selected repo scan works | logs/CDB036-meta.log;--repo-id/--repo-path | planned_future_paths_declared |
| CDB037 | planned | integration | Implement Codex bridge docs and sample MCP config | CDB032 | Codex bridge docs | manual config lint passes | logs/CDB037-codex-bridge.log;Codex bridge docs | planned_future_paths_declared |
| CDB038 | planned | integration | Implement Yazelix placement docs | CDB031 | Yazelix docs | host/runtime Nu distinction documented | logs/CDB038-yazelix.log;Yazelix docs | planned_future_paths_declared |
| CDB039 | planned | integration | Implement runner proof contract | CDB028;CDB029;CDB032 | proof export | runner-readable proof manifest exists | logs/CDB039-runner.log;proof export | planned_future_paths_declared |
| CDB040 | planned | integration | Implement GitKB/RTK/Kache/wild/Fenix docs | CDB009 | integration docs | facts/export boundaries clear | logs/CDB040-tooling.log;integration docs | planned_future_paths_declared |
| CDB041 | planned | fixtures | Create fixture matrix | CDB012;CDB013 | fixture workspace | fixtures present and documented | logs/CDB041-fixtures.log;fixture workspace | planned_future_paths_declared |
| CDB042 | planned | tests | Add deterministic scan tests | CDB041;CDB029 | test outputs | repeat scan checksums stable | logs/CDB042-determinism.log;test outputs | planned_future_paths_declared |
| CDB043 | planned | tests | Add security/no-leak tests | CDB041;CDB032 | test outputs | MCP/source secret tests pass | logs/CDB043-security-tests.log;test outputs | planned_future_paths_declared |
| CDB044 | planned | tests | Add no-mutation tests | CDB028;CDB041 | test outputs | clean/dirty no-mutation tests pass | logs/CDB044-no-mutation-tests.log;test outputs | planned_future_paths_declared |
| CDB045 | planned | tests | Add unsafe capture tests | CDB033;CDB034;CDB041 | test outputs | unsafe capture gate tests pass | logs/CDB045-unsafe-tests.log;test outputs | planned_future_paths_declared |
| CDB046 | planned | release | Run full local validation | CDB042;CDB043;CDB044;CDB045 | validation logs | fmt/clippy/test/doctor pass | logs/CDB046-validation.log;validation logs | planned_future_paths_declared |
| CDB047 | planned | release | Generate release manifest | CDB046 | release manifest | manifest checksums match | logs/CDB047-manifest.log;release manifest | planned_future_paths_declared |
| CDB048 | planned | release | Prepare handoff and backlog | CDB047 | handoff docs | capture gaps and MVP2 listed | logs/CDB048-handoff.log;handoff docs | planned_future_paths_declared |
| CDB049 | planned | yazelix-nu | Inspect Yazelix Nushell runtime bridge | CDB038 | YAZELIX_NUSHELL_RUNTIME.md | report cites runtime nu/config/initializer boundaries | cross-reference report;YAZELIX_NUSHELL_RUNTIME.md | planned_future_paths_declared |
| CDB050 | planned | packaging | Package nu_plugin_codedb as runtime tool | CDB049;CDB030 | nu_plugin_codedb runtime package | runtime tool metadata and `codedb --version` smoke pass | runtime package metadata;plugin/CLI smoke output | planned_future_paths_declared |
| CDB051 | planned | compat | Validate host Nu vs Yazelix runtime Nu protocol | CDB050 | codedb doctor --nu --yazelix | doctor reports protocol/runtime status and mismatch degrades explicitly | doctor output;protocol status row | planned_future_paths_declared |
| CDB052 | planned | nu-plugin | Implement transient nu --plugins smoke test | CDB051 | nu --plugins smoke | transient plugin command returns table-shaped output | test log;Nu output | planned_future_paths_declared |
| CDB053 | planned | nu-plugin | Implement temp-HOME plugin registry smoke test | CDB051 | temp HOME plugin add/use | registry test passes in isolated HOME and leaves real HOME unchanged | temp HOME artifact;test log | planned_future_paths_declared |
| CDB054 | planned | yazelix-init | Generate CodeDB extern/init bridge artifact | CDB050;CDB052 | codedb_init.nu/codedb_extern.nu | generated initializer has provenance/checksum and does not edit tracked config.nu | generated init/extern checksums | planned_future_paths_declared |
| CDB055 | planned | provenance | Verify generated initializer checksums/provenance | CDB054 | initializer manifest | checksum/provenance manifest validates | manifest rows;checksum report | planned_future_paths_declared |
| CDB056 | planned | syntax | Extend syntax validator path for CodeDB fixtures | CDB054 | nu --no-config-file --ide-check | temp-HOME syntax validation passes | syntax report | planned_future_paths_declared |
| CDB057 | planned | safety | Add no-real-HOME plugin registration test | CDB053 | HOME isolation test | real HOME unchanged before/after | before/after HOME hash/report | planned_future_paths_declared |
| CDB058 | planned | yazelix-smoke | Add Yazelix launch smoke with CodeDB disabled | CDB049;CDB056 | disabled smoke | Yazelix launch unaffected without CodeDB | launch smoke log | planned_future_paths_declared |
| CDB059 | planned | yazelix-smoke | Add Yazelix launch smoke with CodeDB enabled | CDB058;CDB054 | enabled smoke | Yazelix launch with CodeDB bridge passes without heavy startup import | launch smoke log | planned_future_paths_declared |
| CDB060 | planned | security | Add plugin stderr/trace secret-leak guard | CDB052;CDB032 | stderr/log/MCP leak tests | secret-looking fixtures are not leaked by default | redaction report;test log | planned_future_paths_declared |
| CDB061 | planned | storage | Add redb lock/plugin-GC behavior test | CDB014;CDB050 | redb lock/GC smoke | lock contention/GC behavior documented and safe | redb test log | planned_future_paths_declared |
| CDB062 | planned | codex | Add Codex bounded CLI/MCP invocation smoke | CDB032;CDB060 | Codex bridge smoke | bounded CLI/MCP output passes limits and exposes no raw source by default | MCP tool report;CLI output sample | planned_future_paths_declared |
| CDB063 | planned | envctl | Add envctl table rows for CodeDB runtime integration | CDB035;CDB055 | CodeDB envctl export rows | export includes runtime/tool/checksum rows | export sample;checksum rows | planned_future_paths_declared |
| CDB064 | complete | package | Verify ZIP extraction proof before construction | CDB005 | manifests/EXTRACTION_PROOF.json | EXTRACTION_PROOF.json parses and source ZIP SHA matches | manifests/EXTRACTION_PROOF.json;logs/CDB064-extraction-proof.log | complete_current_paths_exact |
| CDB065 | complete | package | Upgrade controlled task graph table and Markdown projection | CDB064 | execution/TASK_GRAPH.csv | DAG validates, dependencies resolve, all tasks have evidence paths | execution/TASK_GRAPH.csv;execution/TASK_GRAPH.md;logs/CDB065-task-graph-final.log | complete_current_paths_exact |
| CDB066 | complete | package | Complete checklist evidence map | CDB065 | manifests/CHECKLIST_COMPLETION.json | no checklist item is unmapped | CHECKLIST_COMPLETION.md;manifests/CHECKLIST_COMPLETION.json;logs/CDB066-checklist-completion.log | complete_current_paths_exact |
| CDB067 | complete | package | Validate and seal final execution package | CDB066 | manifests/PACKAGE_VALIDATION.json | PACKAGE_VALIDATION.json status is passed | manifests/PACKAGE_VALIDATION.json;manifests/PACK_MANIFEST.json;manifests/CHECKSUMS.sha256;logs/CDB067-final-validation.log | complete_current_paths_exact |
| CDB068 | complete | package-repair | Repair TASK_GRAPH CSV source-of-truth file linkage | CDB067 | execution/TASK_GRAPH.csv | TASK_GRAPH parses; all current artifact references are exact package-relative paths; completed task evidence logs exist; dependency graph remains acyclic; checksums resealed | logs/CDB068-csv-source-of-truth-repair.log;manifests/CSV_SOURCE_OF_TRUTH_REPAIR.json;execution/TASK_GRAPH.csv | complete_current_paths_exact |
| CDB069 | complete | audit-upgrade | Complete audit upgrade hardening without downgrades | CDB068 | docs/AUDIT_UPGRADE_COMPLETION.md | repo truth validates; checksum manifest validates; Nu smoke is wired; devShells.ci exists; Downloads package is documented non-authority | logs/CDB069-audit-upgrade-completion.log;docs/AUDIT_UPGRADE_COMPLETION.md;tasks/nu-plugin-audit-upgrade-completion | complete_current_paths_exact |

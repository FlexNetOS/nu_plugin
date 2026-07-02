# Execution-Package Checklist — `nu_plugin_codedb` V1.1

**Date:** 2026-07-01  
**Purpose:** checklist for turning `nu_plugin_codedb_v1_1_full_prd.md` into a professional, AI-navigable, Codex-ready execution package.  
**Status:** package-preparation and execution-readiness checklist; not implementation work.

---

## 1. Package readiness rule

The PRD is not enough. A professional executable package must make it hard for Codex or any future agent to drift.

Minimum package rule:

```text
Start file -> navigation graph -> goal/subgoal -> task graph -> task/file map -> PRD sections -> command ledger -> proof logs -> release manifest
```

Every task must answer:

1. What goal/subgoal does this implement?
2. What PRD section owns the requirement?
3. What file(s) may change?
4. What command(s) prove completion?
5. What logs/checksums prove no drift/no mutation/no secret leak?
6. What integration boundary must not be crossed?

---

## 2. P0 artifact checklist

| Artifact | Required | Purpose | Done criteria |
|---|---:|---|---|
| `CODEDB_START_HERE.md` | Yes | Single session entrypoint. | Points to readiness gate, navigation, PRD, task graph, first-run prompt. |
| `GOAL.md` | Yes | Short north-star goal. | Under 1000 chars; mentions Nushell plugin + full crate capture. |
| `SUBGOALS.md` | Yes | Linked subgoals. | Links to doctrine, storage, commands, integration, tests, release. |
| `ACCEPTANCE.md` | Yes | Release gates. | Lists measurable V1.1 acceptance gates. |
| `NAVIGATION.md` | Yes | Human file map. | Ordered file index with purpose. |
| `NAVIGATION.json` | Yes | Machine-readable file map. | JSON validates and matches markdown. |
| `DOC_GRAPH.md` | Yes | Read-order graph. | Shows what to read before each task class. |
| `READINESS_GATE.md` | Yes | Pre-edit checklist. | Requires task ID, PRD section, owner file, validation gate, raw log path. |
| `DRIFT_GUARD.md` | Yes | Anti-drift rules. | Stops context blasting, hidden edits, source overwrite, unsafe build execution. |
| `STOP_CONDITIONS.md` | Yes | Hard stop policy. | Includes secret, mutation, task ambiguity, plugin-version mismatch, unsafe execution. |
| `FIRST_RUN_PROMPT.md` | Yes | Pasteable Codex prompt. | References PRD, task graph, readiness gate, and no-mutation policy. |
| `nu_plugin_codedb_v1_1_full_prd.md` | Yes | Canonical PRD. | Standalone PRD; no external context required. |
| `TASK_GRAPH.csv` | Yes | Executable task table. | Unique IDs, dependencies, owners, artifacts, gates. |
| `TASK_FILE_MAP.csv` | Yes | Task-to-doc/file map. | Every task range maps to must-read/must-update files. |
| `COMMAND_LEDGER.csv` | Yes | Execution evidence. | Header exists before first command; every state-changing command recorded. |
| `WORKLOG.md` | Yes | Narrative work record. | Records task decisions, blockers, proof refs, checksum changes. |
| `PACK_MANIFEST.json` | Yes | Package integrity. | Every artifact has bytes + sha256. |
| `LINK_CHECK_REPORT.md` | Yes | Navigation proof. | All local links checked. |

---

## 3. AI-navigation-first checklist

- [ ] Create `CODEDB_START_HERE.md` with one mandatory path: read gate first, then navigation, then PRD, then task graph.
- [ ] Create `NAVIGATION.md` with a numbered file index.
- [ ] Create `NAVIGATION.json` with the same file order and purposes.
- [ ] Create `DOC_GRAPH.md` with dependency edges:
  - start -> readiness -> navigation -> PRD -> task graph -> first task;
  - PRD -> schema/commands/integration/test plan;
  - task graph -> task/file map -> command ledger.
- [ ] Create `DRIFT_GUARD.md` with explicit context-size and ownership limits.
- [ ] Create `STOP_CONDITIONS.md` with hard stops for source overwrite, hidden mutation, plugin version mismatch, unbounded MCP output, unsafe build execution, secret capture, and missing raw logs.
- [ ] Create `READINESS_GATE.md` requiring task ID, PRD section, source-of-truth owner, generated artifact status, validation command, raw log path, and no-secret path.
- [ ] Link every file from `CODEDB_START_HERE.md` and `NAVIGATION.md`.
- [ ] Run a local markdown link check and save `LINK_CHECK_REPORT.md`.

---

## 4. Task graph checklist

- [ ] Create `TASK_GRAPH.csv`.
- [ ] Use stable task IDs: `CDB000`–`CDB999`.
- [ ] Include columns:
  - `task_id`
  - `phase`
  - `title`
  - `depends_on`
  - `prd_sections`
  - `owner_surface`
  - `allowed_files`
  - `forbidden_actions`
  - `primary_artifact`
  - `validation_gate`
  - `raw_log_path`
  - `acceptance_signal`
  - `status`
- [ ] Ensure no duplicate task IDs.
- [ ] Ensure every P0 PRD requirement has at least one task.
- [ ] Ensure every task maps back to one PRD section.
- [ ] Add dependency edges for storage before scan, scan before export, export before MCP, fixtures before release.
- [ ] Add tasks for no-mutation proof and secret-policy tests before any reproduction task.
- [ ] Add tasks for host Nu and Yazelix Nu compatibility before declaring plugin install complete.
- [ ] Add tasks for Codex CLI and MCP bridge before Codex is allowed to use CodeDB.
- [ ] Add tasks for envctl export contract before envctl integration.
- [ ] Add tasks for meta project selection before multi-repo scanning.

---

## 5. Codex/Nushell conflict-bridge checklist

| Conflict | Required execution deliverable | Validation |
|---|---|---|
| Codex may not run inside Nushell | PRD section + CDB062 output, optionally `docs/CODEX_BRIDGE.md`. | `codedb --format json` works outside Nu. |
| Nu plugin registry differs by runtime | PRD section + CDB051 output, optionally `docs/NUSHELL_PLUGIN_COMPAT.md`. | `codedb doctor --nu --yazelix` reports both. |
| Codex output can context-blast | PRD section + CDB060/CDB062 output, optionally `docs/MCP_LIMITS.md`. | MCP max rows/bytes tests pass. |
| Codex may expose raw source through MCP | PRD section + CDB010/CDB060 output, optionally `docs/SECURITY_AND_SECRET_POLICY.md`. | MCP raw source tools disabled by default. |
| Unsafe capture needs human gate | PRD section + CDB010/CDB045 output, optionally `docs/UNSAFE_CAPTURE_POLICY.md`. | `capture build` refuses without explicit flag. |
| envctl owns generated config | PRD section + CDB035/CDB063 output, optionally `docs/ENVCTL_EXPORT_CONTRACT.md`. | envctl reads exports, not redb internals. |
| Codex config/MCP config may drift | `.codex/config.toml.tmpl` or generated fragment plan. | envctl validates checksums. |
| Codex direct shell may have different PATH | `docs/CODEX_BRIDGE.md` if split from integration docs. | absolute binary paths tested. |
| Codex sessions are disposable | WORKLOG/GitKB handoff discipline. | GitKB/worklog capture decisions. |

---

## 6. Professional documentation checklist

These are implementation deliverables for `CDB006`–`CDB012` and related integration tasks. They are not missing bootstrap-package files. Codex should create or fold them into consolidated docs only when the selected task requires it.

- [ ] `ARCHITECTURE.md`: system diagram, crates, data flow, runtime modes.
- [ ] `SCHEMA.md`: table groups, field-level schema, identity rules, checksums.
- [ ] `COMMANDS.md`: CLI, Nu plugin commands, MCP tools, flags, examples.
- [ ] `INTEGRATION_CONTRACTS.md`: Codex, Yazelix, meta, envctl, runner, GitKB, RTK, Kache/wild/Fenix.
- [ ] `SECURITY_AND_SECRET_POLICY.md`: source blob modes, secret detection, MCP leak guard.
- [ ] `UNSAFE_CAPTURE_POLICY.md`: build.rs/proc-macro execution gates, raw logs, approval flow.
- [ ] `NUSHELL_PLUGIN_COMPAT.md`: host/Yazelix Nu plugin registration and version strategy.
- [ ] `CODEX_BRIDGE.md`: CLI/MCP bridge, config fragments, output bounds, no-mutation rules.
- [ ] `META_INTEGRATION.md`: repo selection, project IDs, no meta replacement.
- [ ] `ENVCTL_EXPORT_CONTRACT.md`: exported tables/checksums/env facts.
- [ ] `YAZELIX_PLACEMENT.md`: where plugin/CLI appears in Yazelix runtime.
- [ ] `TEST_PLAN.md`: unit, fixture, integration, security, no-mutation, MCP, CLI, Nu plugin.
- [ ] `FIXTURE_MATRIX.md`: fixture definitions and expected rows.
- [ ] `RELEASE_GATE.md`: release proof requirements.
- [ ] `BACKLOG.md`: MVP2 features and downgrade exclusions.

---

## 7. Implementation repository layout checklist

Recommended repository layout after implementation begins:

```text
codedb_repo_or_execution_workspace/
  CODEDB_START_HERE.md
  GOAL.md
  SUBGOALS.md
  ACCEPTANCE.md
  NAVIGATION.md
  NAVIGATION.json
  DOC_GRAPH.md
  READINESS_GATE.md
  DRIFT_GUARD.md
  STOP_CONDITIONS.md
  FIRST_RUN_PROMPT.md
  prd/nu_plugin_codedb_v1_1_full_prd.md
  docs/ARCHITECTURE.md
  docs/SCHEMA.md
  docs/COMMANDS.md
  docs/INTEGRATION_CONTRACTS.md
  docs/SECURITY_AND_SECRET_POLICY.md
  docs/UNSAFE_CAPTURE_POLICY.md
  docs/NUSHELL_PLUGIN_COMPAT.md
  docs/CODEX_BRIDGE.md
  docs/ENVCTL_EXPORT_CONTRACT.md
  docs/META_INTEGRATION.md
  docs/YAZELIX_PLACEMENT.md
  docs/TEST_PLAN.md
  docs/FIXTURE_MATRIX.md
  docs/RELEASE_GATE.md
  execution/TASK_GRAPH.csv
  execution/TASK_FILE_MAP.csv
  execution/COMMAND_LEDGER.csv
  execution/WORKLOG.md
  manifests/PACK_MANIFEST.json
  manifests/LINK_CHECK_REPORT.md
  manifests/CHECKSUMS.sha256
```

---

## 8. Engineering implementation checklist

- [ ] Create Rust workspace skeleton.
- [ ] Add `nu_plugin_codedb` crate.
- [ ] Add `codedb` CLI crate.
- [ ] Add `codedb-core` crate.
- [ ] Add `codedb-store-redb` crate.
- [ ] Add `codedb-cargo` crate.
- [ ] Add `codedb-rust-static` crate.
- [ ] Add `codedb-build-capture` crate, gated and disabled by default.
- [ ] Add `codedb-mcp` crate.
- [ ] Add `codedb-fixtures` crate/folder.
- [ ] Add schema definitions and version metadata.
- [ ] Implement redb initialization and metadata record.
- [ ] Implement filesystem scanner.
- [ ] Implement exact source metadata capture.
- [ ] Implement source blob policy modes.
- [ ] Implement Cargo metadata capture.
- [ ] Implement Cargo source provenance capture.
- [ ] Implement static Rust item/macro/build-script detection.
- [ ] Implement `cfg`/feature/target/toolchain context capture.
- [ ] Implement capture gaps and validation errors.
- [ ] Implement no-mutation proof.
- [ ] Implement Nu plugin command outputs.
- [ ] Implement CLI JSON/NUON/CSV outputs.
- [ ] Implement read-only MCP server.
- [ ] Implement output pagination and byte limits.
- [ ] Implement backup/restore smoke.
- [ ] Implement fixtures and tests.
- [ ] Implement docs and examples.

---

## 9. Validation checklist

- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --all-targets --all-features` passes or documented exceptions exist.
- [ ] `cargo test` passes.
- [ ] `codedb doctor --nu` reports usable or degraded status clearly.
- [ ] `codedb doctor --yazelix` reports usable or degraded status clearly.
- [ ] `codedb doctor --codex` reports usable or degraded status clearly.
- [ ] Repeated scan of unchanged fixture produces same table checksums.
- [ ] Clean fixture repo remains clean after scan.
- [ ] Dirty fixture repo records pre-existing dirty state and does not worsen it.
- [ ] Secret-looking fixture follows configured policy.
- [ ] MCP cannot dump raw source by default.
- [ ] `capture build` refuses without unsafe flag.
- [ ] `capture build --unsafe-execute-build` preserves raw logs if implemented.
- [ ] redb backup and restore smoke passes.
- [ ] envctl export JSON/NUON/CSV validates.
- [ ] task graph has no duplicate IDs.
- [ ] all local markdown links resolve.
- [ ] package manifest checksums match actual files.

---

## 10. Package bootstrap and implementation release gates

### 10.1 Bootstrap package gate

This ZIP is ready for Codex only when:

- [x] `CODEDB_START_HERE.md` exists and points to `READINESS_GATE.md`.
- [x] PRD is standalone V1.1 full.
- [x] `TASK_GRAPH.csv` exists and has unique task IDs.
- [x] `TASK_FILE_MAP.csv` maps every task to docs/files.
- [x] `FIRST_RUN_PROMPT.md` includes no-mutation and unsafe-capture rules.
- [x] `STOP_CONDITIONS.md` includes plugin version mismatch and unsafe execution stop rules.
- [x] Link check passes.
- [x] Secret-pattern scan over package passes.
- [x] Manifest and checksums exist.
- [x] Scaffold tasks `CDB000`–`CDB005` are marked complete.

### 10.2 Implementation release gate

Implementation is not complete until selected tasks produce or fold in the following contract topics:

- [ ] Codex bridge: CLI/MCP path, absolute binary paths, output bounds, no auth hacks.
- [ ] Nushell plugin compatibility: host/runtime Nu protocol checks and registry isolation.
- [ ] envctl export contract: envctl reads exported rows/checksums, not redb internals.
- [ ] meta integration: selected repo graph input, no meta replacement.
- [ ] Yazelix placement: runtime tool + generated init/extern bridge, no tracked `config.nu` edit.
- [ ] Security and secret policy: source blob modes, MCP leak guard, log redaction.
- [ ] Release gate: no-mutation proof, table checksums, raw logs, manifest.

---

## 11. Professional readiness scorecard

| Area | Minimum ready score | Notes |
|---|---:|---|
| Navigation | 100% | No broken links; clear start path. |
| Task graph | 100% | Unique IDs, dependencies, gates. |
| PRD completeness | 95%+ | Known gaps must be explicit capture gaps, not omissions. |
| Integration clarity | 95%+ | Codex/Nu/Yazelix/meta/envctl boundaries documented. |
| Security | 100% | No raw secrets; MCP leak guard; source blob policy. |
| Build readiness | 90%+ | Rust workspace plan and fixtures clear. |
| Release proof | 100% | Manifest, logs, checksums, no-mutation proof required. |

---

## 12. Current implementation launch order

The execution package now includes completed package scaffold, documentation, extraction-proof, checklist, task-graph, and final-validation tasks: `CDB000`–`CDB012` plus `CDB064`–`CDB067`. These must not be recreated unless validation fails.

Next execution flow:

1. Read `CODEDB_START_HERE.md`, `READINESS_GATE.md`, `NAVIGATION.md`, `DOC_GRAPH.md`, `GOAL.md`, `SUBGOALS.md`, `ACCEPTANCE.md`, the PRD, and `execution/TASK_GRAPH.csv`.
2. Select the first `planned` implementation task whose dependencies are complete; normally `CDB013`.
3. Pass `READINESS_GATE.md` before touching implementation files.
4. Execute exactly one task at a time.
5. Update `execution/COMMAND_LEDGER.csv` and `execution/WORKLOG.md` for any state-changing command.
6. If package metadata changes, regenerate manifest/checksum/link/validation artifacts.
7. Treat the section 6 design docs as implementation-phase deliverables, not missing bootstrap files.

---

## 13. Integrated Yazelix/Nushell runtime bridge status

**Status:** integrated. The cross-reference report is the required research artifact for bridge tasks, and `TASK_GRAPH.csv` includes `CDB049`–`CDB063` for the Yazelix/Nushell bridge.

### 13.1 Runtime bridge targets

| Target | Required package artifact | Gate |
|---|---|---|
| Yazelix runtime Nu discovery | `YAZELIX_NUSHELL_RUNTIME.md` or equivalent section | `codedb doctor --yazelix` reports runtime Nu path/protocol status. |
| Plugin registration modes | `CODEDB_NU_PLUGIN_REGISTRATION.md` or equivalent section | transient and temp-HOME registry smoke tests pass. |
| Yazelix runtime tool packaging | `CODEDB_YAZELIX_RUNTIME_TOOL.md` or equivalent section | package exposes `codedb` and `nu_plugin_codedb` without global-only install. |
| Syntax validation | `CODEDB_NUSHELL_SYNTAX_GATE.md` or equivalent section | temp-HOME `nu --no-config-file --ide-check` gate passes. |
| Generated init/extern bridge | `CODEDB_YAZELIX_INIT_CONTRACT.md` or equivalent section | generated init/extern checksums and provenance recorded. |
| No tracked config mutation | task graph + no-mutation gate | tracked Yazelix `nushell/config/config.nu` is unchanged. |
| Codex bridge safety | `CODEX_BRIDGE.md`, MCP limits | bounded CLI/MCP smoke passes with no raw source by default. |
| envctl consumption | `ENVCTL_EXPORT_CONTRACT.md` | envctl reads CodeDB exports/checksums, not redb internals. |

### 12.2 Added task block

The following tasks are required and now appear in `TASK_GRAPH.csv`:

```text
CDB049 Inspect Yazelix Nushell runtime bridge
CDB050 Package nu_plugin_codedb as runtime tool
CDB051 Validate host Nu vs Yazelix runtime Nu protocol
CDB052 Implement transient nu --plugins smoke test
CDB053 Implement temp-HOME plugin registry smoke test
CDB054 Generate CodeDB extern/init bridge artifact
CDB055 Verify generated initializer checksums/provenance
CDB056 Extend syntax validator path for CodeDB fixtures
CDB057 Add no-real-HOME plugin registration test
CDB058 Add Yazelix launch smoke with CodeDB disabled
CDB059 Add Yazelix launch smoke with CodeDB enabled
CDB060 Add plugin stderr/trace secret-leak guard
CDB061 Add redb lock/plugin-GC behavior test
CDB062 Add Codex bounded CLI/MCP invocation smoke
CDB063 Add envctl table rows for CodeDB runtime integration
```

### 12.3 Execution gate matrix

| Gate ID | Gate | Required evidence |
|---|---|---|
| G0 | Package integrity | manifest, checksums, link check, duplicate task check, secret hygiene scan. |
| G1 | Readiness | task ID, PRD section, target surface, allowed files, raw log path, validation gate. |
| G2 | No mutation | before/after Git status and file hashes for scanned repos. |
| G3 | Store safety | redb schema version, lock test, backup/restore smoke, corruption refusal. |
| G4 | Nushell compatibility | host Nu and Yazelix runtime Nu protocol checks. |
| G5 | Yazelix bridge | generated initializer/extern only; tracked `config.nu` unchanged. |
| G6 | Codex bridge | bounded CLI/MCP outputs; no raw source by default; no auth hacks. |
| G7 | Unsafe execution | build/proc-macro execution refuses without explicit unsafe flag. |
| G8 | envctl export | export tables/checksums validate without envctl reading redb internals. |
| G9 | Release proof | fmt/clippy/test/doctor/fixture/MCP/no-leak/no-mutation logs retained. |

### 12.4 Remaining execution checklist

- [ ] During implementation, generate the standalone docs listed in section 6 or explicitly fold each topic into existing docs with a `covered_by` entry in `TASK_FILE_MAP.csv`.
- [ ] Implement the Rust workspace and crates from the PRD.
- [ ] Implement redb store init, schema version, locking, backup, and restore smoke.
- [ ] Implement read-only scan and no-mutation proof before any dynamic capture.
- [ ] Implement Nu plugin command surface and host/Yazelix runtime compatibility checks.
- [ ] Implement generated CodeDB init/extern bridge under Yazelix state paths only.
- [ ] Implement bounded CLI/MCP surfaces for Codex.
- [ ] Implement envctl export contract and sample exported tables.
- [ ] Implement fixture matrix, deterministic scan tests, no-leak tests, no-mutation tests, and unsafe-capture refusal tests.
- [ ] Run final release proof and regenerate package manifest/checksums.

### 12.5 Hard downgrade exclusions

Do not add these as shortcuts:

- direct tracked `nushell/config/config.nu` mutation;
- global-only plugin install;
- real-HOME plugin registry tests;
- unbounded MCP source reads;
- default build-script/proc-macro execution;
- DB-owned source truth before proof gates;
- envctl reading redb internals;
- Yazelix Zellij plugin ownership for CodeDB semantics.



---

## Final package-builder completion note

Generated at `2026-07-02T01:02:12.787260+00:00`. See `CHECKLIST_COMPLETION.md` and `manifests/CHECKLIST_COMPLETION.json` for item-level completion evidence. Implementation-phase checklist rows are not falsely marked as implemented; they are mapped to controlled planned tasks with dependencies, validation gates, stop conditions, and evidence paths in `execution/TASK_GRAPH.csv`.

# Remaining Execution Checklist — `nu_plugin_codedb` V1.1

**Generated:** 2026-07-01T00:00:00-05:00  
**Status:** clean package checklist after Yazelix/Nushell cross-reference integration.

## Package status

| Area | Status | Evidence |
|---|---|---|
| Clean full PRD | Done | `prd/nu_plugin_codedb_v1_1_full_prd.md` includes integrated section 24. |
| Yazelix/Nushell research | Done | `research/nushell_yazelix_cross_reference_report.md`. |
| Task graph bridge rows | Done | `execution/TASK_GRAPH.csv` includes `CDB049`–`CDB063`. |
| Duplicate task IDs | Verified | validation report. |
| Manifest/checksums | Verified | `manifests/PACK_MANIFEST.json`, `manifests/CHECKSUMS.sha256`. |
| Local markdown links | Verified | `manifests/LINK_CHECK_REPORT.md`. |
| Secret hygiene scan | Verified | validation report. |

## Remaining implementation work after package bootstrap

1. Generate or fill the detailed design docs listed in `nu_plugin_codedb_execution_package_checklist.md` section 6.
2. Convert task rows from `planned` to active one at a time, after passing `READINESS_GATE.md`.
3. Create the Rust workspace and crates.
4. Implement redb storage and schema versioning first.
5. Implement read-only scanner and no-mutation proof before any dynamic capture.
6. Implement Nu plugin and CLI outputs before MCP.
7. Implement Codex MCP only after pagination, byte limits, source-leak guard, and no-raw-source defaults pass.
8. Implement Yazelix generated initializer/extern bridge only after host/runtime Nu compatibility tests pass.
9. Implement envctl export contract without exposing redb internals.
10. Run final release gate and regenerate manifest/checksums.

## Non-negotiable execution gates

| Gate | Blocks until |
|---|---|
| G1 readiness | task ID, PRD section, target surface, allowed files, validation gate, and raw log path are known. |
| G2 no mutation | before/after repo status and source hash evidence exists. |
| G3 secret safety | raw source/blob policy and MCP source-leak guard pass. |
| G4 Nushell compatibility | host Nu and Yazelix runtime Nu protocol checks pass or degrade explicitly. |
| G5 Yazelix bridge | generated init/extern path is used; tracked config remains unchanged. |
| G6 Codex bridge | bounded CLI/MCP invocation works without auth/session hacks. |
| G7 release | fmt/clippy/test/doctor/fixtures/manifest/link/secret scans pass. |


## Codex anti-confusion rules

- `CDB000`–`CDB012` plus `CDB064`–`CDB067` are complete package/documentation/finalization tasks. Do not execute them again unless validation fails.
- A task is executable only when `status = planned` and all dependencies are `complete`.
- Every task must name its `target_surface`, `execution_gate`, `validation_gate`, `raw_log_path`, and `evidence_artifacts` before mutation.
- Do not read external/non-canonical artifacts; stale review artifacts are intentionally excluded from this package.
- The canonical product truth is `prd/nu_plugin_codedb_v1_1_full_prd.md`; execution truth is `execution/TASK_GRAPH.csv`.


## Final execution-package note

The first planned implementation task after this package build is `CDB013`. See `CHECKLIST_COMPLETION.md` for the package-builder checklist map.

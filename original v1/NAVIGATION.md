# Navigation

Use this as the file map. Codex should follow links and task IDs, not browse randomly.

## Launch-control path

1. [CODEX_START_HERE.md](CODEX_START_HERE.md)
2. [START_DECISION.md](START_DECISION.md)
3. [READINESS_GATE.md](READINESS_GATE.md)
4. [NAVIGATION.md](NAVIGATION.md)
5. [DOC_GRAPH.md](DOC_GRAPH.md)
6. [DRIFT_GUARD.md](DRIFT_GUARD.md)
7. [FIRST_RUN_PROMPT.md](FIRST_RUN_PROMPT.md)

## File index

| Order | File | Purpose |
|---:|---|---|
| 0 | [CODEX_START_HERE.md](CODEX_START_HERE.md) | Single session entrypoint. |
| 1 | [START_DECISION.md](START_DECISION.md) | Start/no-start decision. |
| 2 | [READINESS_GATE.md](READINESS_GATE.md) | Pre-edit launch checklist. |
| 3 | [STOP_CONDITIONS.md](STOP_CONDITIONS.md) | Hard stop rules. |
| 4 | [FIRST_RUN_PROMPT.md](FIRST_RUN_PROMPT.md) | Pasteable first-run Codex prompt. |
| 5 | [SESSION_MEMORY_PROTOCOL.md](SESSION_MEMORY_PROTOCOL.md) | Cross-session memory protocol. |
| 6 | [DOC_GRAPH.md](DOC_GRAPH.md) | Document dependency graph. |
| 7 | [DRIFT_GUARD.md](DRIFT_GUARD.md) | Anti-drift checklist. |
| 8 | [goal_pack/README.md](goal_pack/README.md) | Start point for compact goal pack. |
| 9 | [goal_pack/GOAL.md](goal_pack/GOAL.md) | Less-than-1000-char delivered vision. |
| 10 | [goal_pack/SUBGOALS.md](goal_pack/SUBGOALS.md) | Links to compact subgoals. |
| 11 | [goal_pack/EXECUTION_ORDER.md](goal_pack/EXECUTION_ORDER.md) | The production read/execute order. |
| 12 | [goal_pack/ACCEPTANCE.md](goal_pack/ACCEPTANCE.md) | Release acceptance gates. |
| 13 | [goal_pack/subgoals/01-envctl-table-doctrine.md](goal_pack/subgoals/01-envctl-table-doctrine.md) | envctl table-first doctrine. |
| 14 | [goal_pack/subgoals/02-memory-model.md](goal_pack/subgoals/02-memory-model.md) | Cross-session memory model. |
| 15 | [goal_pack/subgoals/03-meta-graph.md](goal_pack/subgoals/03-meta-graph.md) | meta coordination graph role. |
| 16 | [goal_pack/subgoals/04-gitkb-memory.md](goal_pack/subgoals/04-gitkb-memory.md) | GitKB durable memory role. |
| 17 | [goal_pack/subgoals/05-beads-task-layer.md](goal_pack/subgoals/05-beads-task-layer.md) | beads/br task layer role. |
| 18 | [goal_pack/subgoals/06-yazelix-runtime.md](goal_pack/subgoals/06-yazelix-runtime.md) | Yazelix operator runtime role. |
| 19 | [goal_pack/subgoals/07-rtk-cost-log-policy.md](goal_pack/subgoals/07-rtk-cost-log-policy.md) | RTK cost/log policy. |
| 20 | [goal_pack/subgoals/08-codex-operating-policy.md](goal_pack/subgoals/08-codex-operating-policy.md) | Codex rules. |
| 21 | [goal_pack/subgoals/09-generator-contract.md](goal_pack/subgoals/09-generator-contract.md) | Generated-file contract. |
| 22 | [goal_pack/subgoals/10-runner-release-gate.md](goal_pack/subgoals/10-runner-release-gate.md) | Runner release gate. |
| 23 | [execution_artifacts/active_prompt_addendum.md](execution_artifacts/active_prompt_addendum.md) | Locked envctl+Nushell table doctrine. |
| 24 | [execution_artifacts/envctl_table_doctrine.md](execution_artifacts/envctl_table_doctrine.md) | Detailed table/view and generator requirements. |
| 25 | [execution_artifacts/envctl_sync_requirements.md](execution_artifacts/envctl_sync_requirements.md) | System-wide envctl synchronization scope. |
| 26 | [execution_artifacts/revised_task_table.csv](execution_artifacts/revised_task_table.csv) | Authoritative execution task table. |
| 27 | [execution_artifacts/task_changelog.md](execution_artifacts/task_changelog.md) | What changed and why. |
| 28 | [execution_artifacts/blocked_decisions.md](execution_artifacts/blocked_decisions.md) | Locked and remaining decisions. |
| 29 | [execution_artifacts/codex_settings_blueprint.md](execution_artifacts/codex_settings_blueprint.md) | Codex config/auth/MCP/session policy. |
| 30 | [execution_artifacts/yazelix_comprehensive_setup.md](execution_artifacts/yazelix_comprehensive_setup.md) | Yazelix setup and component roles. |
| 31 | [execution_artifacts/meta_mcp_loop_policy_review.md](execution_artifacts/meta_mcp_loop_policy_review.md) | meta, MCP, loop policy. |
| 32 | [execution_artifacts/database_env_audit.md](execution_artifacts/database_env_audit.md) | DB/local-state audit. |
| 33 | [execution_artifacts/plugin_conflict_ownership_matrix.csv](execution_artifacts/plugin_conflict_ownership_matrix.csv) | Plugin/surface ownership matrix. |
| 34 | [execution_artifacts/coverage_matrix.md](execution_artifacts/coverage_matrix.md) | Requirement coverage by task IDs. |

## Rule

Every execution step must cite a task ID and a goal/subgoal link. If a file changes, update the ledger and relevant checksum manifest.

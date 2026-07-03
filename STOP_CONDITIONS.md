# STOP CONDITIONS

Stop before proceeding if:

- task ID is unclear or missing from the active authoritative task graph;
- the active task graph is ambiguous between V1.1 implementation and V1.2 planning lanes;
- PRD section ownership is unknown;
- a task would mutate source repos without a no-mutation proof plan;
- a generated file would be edited directly as source truth;
- a raw secret may enter tracked files, logs, MCP output, or prompts;
- build.rs/proc-macro execution is requested without explicit unsafe gate;
- Nu plugin protocol/version is unknown;
- the test would mutate real HOME/plugin registry;
- Codex MCP output is unbounded or raw-source-enabled by default;
- Yazelix tracked `nushell/config/config.nu` would be modified;
- envctl would read redb internals instead of exports;
- a planning artifact starts to imply code implementation or release proof that does not exist;
- raw failure logs cannot be preserved.

Record blocker, evidence, and next decision.

# Codex Bridge

Source: PRD sections 13.3 and 16.1.

## Purpose

Codex should consume CodeDB through bounded CLI/MCP table outputs, not whole-repo context blasts.

The bridge is deliberately outside the Nushell plugin registry:

```text
Codex -> codedb CLI or codedb MCP -> codedb-core -> redb store -> structured output
Nushell -> nu_plugin_codedb -> codedb-core -> the same redb store
```

Codex therefore does not need to load `nu_plugin_codedb` directly. This avoids
host-Nu/Yazelix-Nu registry and protocol ambiguity while preserving one CodeDB
store and one command contract.

## CLI bridge

Recommended Codex-safe calls:

```text
codedb export <table> --format json
codedb gaps --format json
codedb validation-errors --format json
codedb doctor --codex --format json
```

Use explicit repo selection for nondefault repositories:

```text
codedb scan --repo-id <meta_project_id> --repo-path <path> --store <path> --format json
codedb export meta_repo_selection --repo-id <meta_project_id> --repo-path <path> --format json
```

For automation, use absolute paths and select a machine-readable format. The
CLI remains the fallback when a Nu registry is missing, runtime Nu versions are
incompatible, or CodeDB is being used from a Codex shell that is not Nushell.

## Conflict rules

| Conflict | Bridge rule |
|---|---|
| Codex shell is not Nushell | Call `codedb` directly or start its read-only MCP server. |
| Host Nu and Yazelix Nu have different registries or versions | Validate with `codedb doctor --nu --yazelix`; keep the CLI fallback available. |
| A table is larger than the agent context | Use `--limit`/cursor pagination and bounded JSON; prefer summary commands. |
| A read request could mutate the repository | Use the read-only CLI/MCP surface and inspect the no-mutation proof. |
| Build or proc-macro capture requires execution | MCP blocks it; the CLI requires an explicit unsafe flag and approval provenance. |
| Codex and envctl both own configuration | envctl renders Codex/MCP fragments; CodeDB supplies command targets and exported facts only. |

CodeDB does not install browser sessions, copy authentication tokens, edit
Codex configuration, mutate tracked Yazelix configuration, or silently invoke
unsafe capture. Authentication remains the official external Codex auth flow.

## MCP bridge defaults

- Read-only tools only.
- Pagination required.
- Byte and row limits required.
- Raw source disabled by default.
- No browser/session/auth hacks.

## Sample MCP config

`examples/codex/codedb_mcp_config.json` is a lintable MCP server configuration
fragment for a local Codex environment. Operators must replace absolute paths before
use. The sample intentionally has no auth, browser, session-token, or secret fields.

The bridge target is:

```text
codedb mcp serve --repo-path <path> --store <path> --default-limit 50 --max-bytes 65536
```

The sample declares the policy that Codex may rely on:

| Policy | Value |
|---|---|
| access | read-only |
| output | bounded by row and byte limits |
| raw source | disabled by default |
| unsafe build capture | unavailable through MCP |
| mutation | forbidden |
| authentication | external official Codex auth only |

The sample is a configuration fragment, not an installer. Replace its
repository and store paths before use; keep credentials in the process
environment when a backend requires them, never in the fragment.

## No-mutation and unsafe-operation boundary

Ordinary scan, export, doctor, gap, validation, and MCP operations are
read-only with respect to the source repository. Store creation and declared
evidence outputs may write only their explicit output paths. A restore or
materialization operation must be run as an explicit CLI operation, outside the
Codex bridge's read-only MCP surface.

Dynamic build, proc-macro, and compiler capture is not part of this bridge. It
is unavailable through MCP and requires an explicit CLI approval gate,
provenance, isolated evidence paths, and a cleanup plan. A Codex integration
must not work around that gate with shell scripts or session tricks.

## Validation contract

Before enabling CodeDB as a Codex bridge, verify:

1. `codedb doctor --codex --format json` returns bounded output.
2. The MCP config keeps the default row and byte limits and has no auth or
   secret fields.
3. MCP raw-source, full-file, mutation, and unsafe-capture capabilities remain
   blocked.
4. `codedb prove no-mutation <repo> --format json` (or the equivalent test
   proof) shows the source tree was not changed.

The executable bounded-bridge smoke is the release evidence for these rules;
see `tests/test_codex_bounded_bridge.nu` and
`examples/codex/codedb_bounded_smoke_report.json`.

## Safety proof

CDB062 must prove bounded CLI/MCP invocation and no raw source exposure by default before Codex is allowed to use CodeDB as a bridge.

`tests/test_codex_bounded_bridge.nu` is the executable proof. It validates the
sample MCP config, runs Codex-safe CLI samples, and runs the MCP crate tests.
The smoke enforces:

- `examples/codex/codedb_mcp_config.json` keeps `--default-limit 50` and
  `--max-bytes 65536`
- the sample has no auth, token, browser-session, or secret environment fields
- `codedb doctor --codex --format json` stays below 50 rows and 65536 bytes
- `codedb scan fixtures/secret_like --format json` stays below 50 rows and
  65536 bytes
- MCP tests continue to prove row limits, byte limits, blocked raw-source tools,
  and metadata-only repository summaries
- raw source and secret-looking fixture values are absent from all smoke outputs

`examples/codex/codedb_bounded_smoke_report.json` records the stable contract
shape for this smoke. Live row counts and hashes are produced by the test.

# Codex Bridge

Source: PRD sections 13.3 and 16.1.

## Purpose

Codex should consume CodeDB through bounded CLI/MCP table outputs, not whole-repo context blasts.

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

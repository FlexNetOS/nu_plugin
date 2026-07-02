# Security and Secret Policy

Source: PRD section 15.

## Default posture

- Scans are read-only and deterministic.
- Source blob capture is policy-controlled.
- Secret-looking values are never emitted through MCP/CLI/Nu by default.
- Raw logs are preserved but may be local-only and redacted in summaries.

## Source blob modes

| Mode | Behavior |
|---|---|
| `metadata-only` | Capture path, size, hash, spans, and table facts; do not persist raw bytes. |
| `hashed-blob` | Persist blob by content hash under policy. |
| `redacted-export` | Export table facts with sensitive values elided. |
| `raw-local` | Local-only raw blob storage; never default for MCP. |

## Secret handling

Secret-looking material must become one of:

- `validation_errors` if it appears in an unsafe output path;
- redacted local evidence if needed for debugging;
- a policy row describing why it was not exported.

## MCP leak guard

MCP must default to summaries, table rows, and hashes. Raw source/blob tools are disabled unless a later explicit policy and approval gate is implemented.

## Plugin stderr and trace guard

Task `CDB060` adds `tests/test_plugin_secret_guard.nu` as the default guard for
Nu plugin transport output and MCP test output. The test copies the
`fixtures/secret_like` repository into a temporary directory, invokes
`nu_plugin_codedb` through `nu --plugins`, and checks both stdout and stderr for
known secret-looking fixture values.

The guard intentionally exercises metadata/table surfaces only:

- `codedb scan`
- `codedb source files`
- `codedb rust items`
- `codedb validation errors`
- `cargo test -p codedb-mcp --quiet`

The expected result is a redaction report made of labels, output hashes, row
counts, and `secret_like_values: absent`. Test failure messages must not echo
the secret-looking values themselves.

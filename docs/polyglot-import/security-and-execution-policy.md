# Security And Execution Policy

## Hard Boundaries

- no package-manager dependency installation or project script execution by default
- no raw source over MCP by default
- no credential dump
- no hidden Git mutation
- no source overwrite
- no dynamic runtime/build execution without an explicit future unsafe gate
- no claim of database-owned source truth until repeated round-trip proof exists

## Operational Rules

- Prefer pure Rust libraries on the default path where feasible.
- External CLIs are allowed only behind optional/gated commands.
- License review is required before code adoption.
- No downloaded runtimes or package caches in default tests.
- No heavyweight fixture blobs.
- Missing evidence must be recorded as QUESTION, GAP, or BLOCKER.

## Output Policy

- Nu, CLI, and MCP outputs must remain bounded and table-shaped by default.
- Raw source/blob reads remain blocked through MCP unless a future approved gate
  explicitly changes policy.
- Secret-looking values must be redacted, denied, or held as local-only evidence.

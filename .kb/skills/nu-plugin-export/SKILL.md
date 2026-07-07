---
name: nu-plugin-export
description: Rust-first nu_plugin/CodeDB export workflow for exporting database content into Nushell rows, files, or declared artifacts. Use when the user invokes /nu_plugin:export, asks to export redb, SQLite, Postgres, or other database rows/blobs, asks to create missing export tables/views/materializers, or needs an agent to turn a failed export into a GitKB task, new nu_plugin worktree, PR, and automerge-ready upgrade path.
---

# /nu_plugin:export

Use this skill to design and implement Rust-backed exports from database-backed
tables, blobs, and derived rows for `nu_plugin`/CodeDB work. The export must be
contract-first: identify the source table or database boundary, declare the
target format, create missing export components, validate the result, and
preserve a recovery path when it fails.

## Ground Rules

- Work from the active `nu_plugin` repo/worktree and verify it with
  `git status --short --branch` before edits.
- Prefer Rust for implementation. Put durable export behavior in the relevant
  Rust crate, not in one-off shell/Python glue, unless the repo already uses a
  script only as a validator.
- Treat database rows as derived or declared facts with provenance. Exported
  files, Nushell rows, JSON, NUON, CSV, or blobs must carry enough metadata to
  verify where they came from.
- Do not let downstream tools read or mutate database internals. Add exported
  tables, materialization rows, manifests, or CLI/Nu surfaces through
  CodeDB-owned boundaries.
- Default behavior must be bounded and read-only. Raw blobs or source-like
  content require an explicit target, limit, checksum, and safety policy.
- Use GitKB for task tracking and include the task slug in git commit messages
  when implementing a code change.

## Export Workflow

1. Identify the export source and target:
   - Database engine and boundary: redb store, SQLite, Postgres, or other.
   - Source table, view, query, blob namespace, or materialized row family.
   - Target format: Nushell table rows, JSON, NUON, CSV, file tree, archive, or
     another declared artifact.
   - Row limit, cursor/pagination, ordering, redaction, and blob policy.

2. Inspect existing code before inventing export surfaces:
   - Use `git-kb code symbols --branch <branch> --json` and related
     `callers`/`callees` commands for Rust symbols.
   - Search docs and commands for existing export/import tables.
   - Reuse existing helpers such as row builders, checksum helpers, schema
     version checks, backup/restore helpers, pagination, and validation rows.

3. Define the export contract before writing:
   - Stable row identity and deterministic ordering.
   - Source table, schema version, and query/materializer version.
   - Output format schema, column names, null/empty handling, and type mapping.
   - Provenance fields for source database, source row, source hash, and export
     timestamp when appropriate.
   - Checksums or manifests for file/blob exports.
   - Error rows for partial, skipped, redacted, unsupported, or failed exports.
   - Re-import or round-trip expectations when the exported rows can restore
     database or file state.

4. Implement in Rust:
   - Add types and exporter/materializer functions in the crate that owns the
     data.
   - Add missing export table initialization, view generation, manifest rows,
     indexes, or CLI/Nu command surfaces when the contract requires them.
   - Keep format-specific serialization centralized and tested.
   - Keep raw file/blob output bounded or blocked by default when exposed
     through CLI/MCP/Nu.

5. Validate:
   - Add focused tests for the export success path and missing-surface creation.
   - Add failure tests for unsupported target formats, bad schema versions,
     corrupt rows, missing blobs, duplicate row IDs, and redaction boundaries.
   - Run repo-appropriate gates, normally:
     `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features`,
     `cargo test --workspace -- --test-threads=1`, plus package/truth-surface
     validators when manifests or docs change.

## Failure Workflow

If the export fails and the fix is not a trivial same-turn correction, stop
treating it as an ad hoc export and create an upgrade task:

1. Preserve evidence:
   - User command or requested export.
   - Source database/table/blob boundary.
   - Target format/path.
   - Exact error, partial output, validation failure, and relevant logs.

2. Create a new `nu_plugin` worktree from latest `origin/master`:
   - Fetch and fast-forward the canonical checkout first when safe.
   - Use a branch named like `codex/export-<short-slug>`.
   - Do not stack on unrelated local branches unless the user explicitly asks.

3. Create a detailed GitKB task before code:
   - Slug pattern: `tasks/nu-plugin-export-<short-slug>`.
   - Include reproduction steps, expected export contract, observed failure,
     source tables, missing components, acceptance criteria, and validation plan.
   - Commit the GitKB task before implementation.

4. Implement with tests:
   - Add the smallest Rust export surface that satisfies the contract.
   - Commit code with the task slug in the message or body.
   - Push the branch, open a PR, and enable/request automerge when repository
     policy permits and checks are green.

5. Report completion:
   - PR URL and branch.
   - GitKB task commit and source commit.
   - Validation commands and results.
   - Any manual approval or automerge blocker.

## Completion Checklist

- Active repo/worktree verified.
- Source database/table/blob boundary identified.
- Target export format and safety policy declared.
- Existing Rust and Nu/CLI surfaces inspected.
- Export contract documented in code, tests, or GitKB task.
- Missing export components created or explicitly refused with evidence.
- Rust implementation and validation completed.
- Failure path created a GitKB task, fresh worktree, PR, and automerge request
  when required.

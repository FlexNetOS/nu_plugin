---
name: nu-plugin-import
description: Rust-first nu_plugin/CodeDB import workflow for importing any file into any database. Use when the user invokes /nu_plugin:import, asks to import a file into redb, SQLite, Postgres, or another database, asks to create missing tables/components for an import, or needs an agent to turn a failed import into a GitKB task, new nu_plugin worktree, PR, and automerge-ready upgrade path.
---

# /nu_plugin:import

Use this skill to design and implement Rust-backed imports from arbitrary files
into database-backed tables for `nu_plugin`/CodeDB work. The import must be
evidence-first: inspect the file, derive schema, create missing database
components, validate the import, and preserve a recovery path when it fails.

## Ground Rules

- Work from the active `nu_plugin` repo/worktree and verify it with
  `git status --short --branch` before edits.
- Prefer Rust for implementation. Put durable import behavior in the relevant
  Rust crate, not in one-off shell/Python glue, unless the repo already uses a
  script only as a validator.
- Treat source files as authoritative. Databases store derived rows, blobs,
  checksums, provenance, and validation errors.
- Do not read or mutate database internals from downstream tools. Add exported
  tables, migration rows, or materialization rows through CodeDB-owned surfaces.
- Default behavior must be read-only until the user or task explicitly requests
  mutation. Any write/import path needs an audit row and validation evidence.
- Use GitKB for task tracking and include the task slug in git commit messages
  when implementing a code change.

## Import Workflow

1. Identify the input file and target database:
   - File path, size, extension, encoding, and content hash.
   - Database engine and boundary: redb store, SQLite, Postgres, or other.
   - Target table names, primary keys, indexes, blob policy, and provenance
     rows.

2. Inspect existing code before inventing tables:
   - Use `git-kb code symbols --branch <branch> --json` and related
     `callers`/`callees` commands for Rust symbols.
   - Search docs and commands for existing export/import tables.
   - Reuse existing helpers such as row builders, checksum helpers, schema
     version checks, backup/restore helpers, and validation row patterns.

3. Define the table contract before writing:
   - Stable row identity.
   - Source file hash and source path provenance.
   - Schema version and migration/refusal behavior.
   - Error rows for partial, skipped, redacted, unsupported, or failed imports.
   - Round-trip/materialization expectations when the imported rows can restore
     files or database state.

4. Implement in Rust:
   - Add types and parser/import functions in the crate that owns the data.
   - Add database table initialization or migration code if the table does not
     exist.
   - Add export/CLI/Nu plugin surface only after the storage/import contract is
     tested.
   - Keep raw file/blob output bounded or blocked by default when exposed
     through CLI/MCP/Nu.

5. Validate:
   - Add focused tests for the import success path and missing-table creation.
   - Add failure tests for unsupported input, bad schema, corrupt data, or
     duplicate/conflicting rows.
   - Run repo-appropriate gates, normally:
     `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features`,
     `cargo test --workspace -- --test-threads=1`, plus package/truth-surface
     validators when manifests or docs change.

## Failure Workflow

If the import fails and the fix is not a trivial same-turn correction, stop
treating it as an ad hoc import and create an upgrade task:

1. Preserve evidence:
   - Exact command, input path, target database, error text, and relevant logs.
   - Whether any database files, source files, or generated rows changed.

2. Create a new `nu_plugin` worktree:
   - Fetch latest `origin`.
   - Create a focused branch/worktree named for the failure, for example
     `codex/import-<short-slug>`.
   - Do not reuse a dirty or ambiguous worktree.

3. Create a detailed GitKB task:
   - Slug pattern: `tasks/nu-plugin-import-<short-slug>`.
   - Include reproduction steps, expected import contract, observed failure,
     affected tables/components, acceptance criteria, and validation commands.
   - Commit the GitKB task before code edits.

4. Implement the upgrade from that task:
   - Use TDD: failing fixture or regression test first, then implementation.
   - Commit with the task slug in the message body or subject.
   - Push the branch and open a PR.
   - Enable/request automerge only when the repository policy and GitHub
     permissions allow it, and only after required checks are green.

5. Report the upgrade path:
   - PR URL, branch, GitKB commit, source commit, validation evidence, and any
     remaining manual approval needed for automerge.

## Completion Checklist

- [ ] File type and target database are identified.
- [ ] Required tables/components exist or are created by Rust code.
- [ ] Import success path has test evidence.
- [ ] Import failure path emits bounded validation/audit rows.
- [ ] No hidden source checkout or database mutation occurs outside the
      declared import path.
- [ ] GitKB task and PR workflow are used for non-trivial failures.
- [ ] Worktree, GitKB workspace, and PR/check state are reported from live
      commands.

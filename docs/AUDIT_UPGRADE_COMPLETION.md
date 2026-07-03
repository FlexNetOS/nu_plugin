# Audit upgrade completion

This document carries forward the findings from
`docs/original_package_cross_reference.md`.

## Authority

The authoritative source for continued work is the Git repository at
`/home/flexnetos/FlexNetOS/src/nu_plugin` and its `FlexNetOS/nu_plugin` remote.
The older `/home/flexnetos/Downloads/nu_plugin` package is audit evidence and a
legacy source snapshot. It must not be copied over the Git repository.

If an execution package is needed again, regenerate it from the Git repository
after validation. Do not downgrade implementation, manifests, task state, CI, or
documentation to match the older Downloads tree.

## Completed audit repairs

- The repository has a GitKB workflow and explicit
  `/home/flexnetos/FlexNetOS/usr/bin/git-kb` CLI fallback path.
- `scripts/truth_surface.py` validates current repo-native manifests.
- `manifests/CHECKSUMS.sha256`, `manifests/PACK_MANIFEST.json`, and
  `manifests/PACKAGE_VALIDATION.json` describe current tracked source.
- GitHub CI covers truth-surface validation, Rust workspace tests, Nix checks,
  and envctl inventory smoke.
- The flake exposes `checks.repo_truth_surface`,
  `checks.envctl_inventory_smoke`, `checks.nushell_syntax_smoke`, and
  `devShells.ci`.
- CI runs Nu syntax validation plus transient and plugin-registry plugin smokes
  through `nix develop .#ci`.

## Remaining product upgrades

The following are forward product work items. They are not reasons to revert to
the older package.

- Persist the full file/blob model into a redb-backed CodeDB store: complete for
  source blob rows and SHA-256 materialization helpers.
- Complete source blob table ownership and restore/materialization behavior:
  covered by `codedb-store-redb` backup/restore/materialize tests.
- Complete `codedb export envctl` and envctl file-materialization round trip:
  `codedb_materialization_targets` rows are included in `codedb export envctl`.
- Decide and prove whether `codedb-store-redb` is still planned metadata support
  or the authoritative live store for all generated tables: source blobs are
  authoritative; broader generated-table ownership remains scoped to exported
  datatable rows.
- Package and prove a complete external MCP server lifecycle, beyond bounded
  in-process handling: `codedb-mcp` exposes startup/shutdown/config lifecycle
  rows with raw source disabled by default.
- Expand parser bridges beyond deterministic JSON/JSONC and line-oriented text
  parsing without overclaiming unsupported native parsers: TOML is native;
  Nix/KDL/Nu/YAML/desktop/shell/conf remain line-oriented fallback rows.

## Execution truth policy

The original execution package task graph is preserved as historical execution
evidence. Current implementation state must be proven from current source,
current tests, current manifests, and current CI.

When task graph rows are reconciled, mark a row complete only when its current
gate has direct evidence. Otherwise keep it planned, split it into explicit
follow-up tasks, or mark it intentionally deferred.

# Polyglot Import Planning Package

This package captures the research-and-planning upgrade requested by
FlexNetOS/flexnetos_runner#215.

## Scope

The target capability is:

entire repo
  -> language/package/config/dependency/provenance import
  -> redb-backed CodeDB tables + content-addressed blobs + capture gaps
  -> bounded Nu/CLI/MCP views
  -> generated Rust crate artifact
  -> one release binary that embeds/verifies/queries/materializes the repo snapshot

This package is planning-first. It does not claim arbitrary language-to-Rust
translation, does not weaken V1.1 safety boundaries, and does not silently
supersede the current Rust-crate-first baseline.

## Document Map

| File | Purpose |
|---|---|
| research-ledger.md | Official-source research ledger with claim typing |
| language-import-surface.md | Tiered per-language capture doctrine |
| polyglot-schema-extension.md | Planned schema/table extensions |
| parser-and-indexer-tooling-matrix.md | Parser/indexer/tool comparison |
| package-manager-and-lockfile-matrix.md | Package marker and lockfile plan |
| whole-repo-import-architecture.md | End-to-end architecture and flow |
| single-binary-rust-crate-export.md | Generated crate and binary contract |
| proof-and-round-trip-gates.md | P0-P11 proof gates |
| security-and-execution-policy.md | Safety boundaries and escalation rules |
| github-issue-delivery-plan.md | Delivery strategy and issue dependency map |
| open-questions.md | Remaining unknowns and explicit gaps |

## Planning Boundaries

- V1.1 remains Rust-crate-first unless a later repo policy supersedes it.
- Source files remain authoritative input until proof gates promote another authority.
- Raw bytes, permissions, symlinks, encodings, line endings, and binary assets are captured, not normalized away.
- Unsupported facts become capture_gaps or validation_errors.
- Default behavior remains read-only and bounded.
- Generated crates/files are artifacts.

## Execution Order

1. Research ledger and current-state audit.
2. Schema, language/package inventory, raw byte/blob capture, and parser/indexer roadmap.
3. Baseline-language import-surface plans.
4. Generated single-binary export crate contract.
5. Proof gates, bounded views, and security policy.
6. Issue delivery, readiness packaging, and reseal.

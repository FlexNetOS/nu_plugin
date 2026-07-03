# Whole-Repo Import Architecture

## System Stance

The polyglot planning lane extends CodeDB from Rust-crate-first capture into
whole-repository observation while preserving current authority boundaries.

## Flow

repo root
  -> Tier 0 file/blob capture
  -> Tier 1 language/package/config detection
  -> Tier 2 parser-backed summaries
  -> Tier 3 module/symbol/import/reference rows
  -> Tier 4 optional external indexer rows
  -> Tier 5 explicit unsafe runtime/build facts
  -> redb-backed CodeDB snapshot
  -> bounded Nu/CLI/MCP views
  -> generated single-binary export artifact
  -> verify/materialize under proof gates

## Ownership Boundaries

- Source files remain authoritative input.
- Polyglot rows are observations, not source replacements.
- Generated crates/files are artifacts.
- MCP remains read-only and bounded by default.
- Any runtime/build execution stays behind explicit unsafe gates.

## Planned Crate Additions

- codedb-polyglot-core
- codedb-language-detect
- codedb-tree-sitter
- codedb-package-detect
- codedb-index-scip (optional or gated)
- codedb-export-crate
- codedb-fixtures-polyglot

## Fixture Families

- minimal-python
- minimal-ruby
- minimal-typescript
- mixed-rust-python-ts
- config-heavy
- binary-assets
- symlinks
- credential-like-files
- vendor-generated

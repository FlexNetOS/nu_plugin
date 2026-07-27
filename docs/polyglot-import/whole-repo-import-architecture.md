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

## Bounded View Architecture

Nu, CLI, and MCP expose the same logical polyglot views through a shared query
policy. A request is processed in this order:

```text
authenticated local/store scope
  -> allowlisted logical view and columns
  -> policy and secret classification
  -> deterministic filters and stable sort key
  -> hard scan/file/depth/time budgets
  -> row and serialized-byte ceilings
  -> redaction and response leak check
  -> page plus opaque continuation cursor
```

The default projection is metadata-first: repository-relative identities,
language/package kinds, hashes, spans, counts, capture gaps, and diagnostics.
Blob payloads, full-file text, environment values, command output, and
credential-like values are not default view columns. MCP never exposes raw
source/blob bytes. A separately named local Nu/CLI source command may do so
only under the explicit source-blob mode and approval rules in
[security and execution policy](security-and-execution-policy.md).

Every collection view must require or apply finite server-side ceilings for
rows and serialized bytes. Parser-backed views additionally apply finite input
bytes, files, nesting/depth, diagnostics, and elapsed-resource budgets where
relevant. Caller values may lower a ceiling but cannot raise the configured
maximum. Limits are enforced before serialization and rechecked against the
serialized response; filters, summaries, compression, alternate formats, and
error paths do not bypass them.

Rows use a documented total order with an immutable identity tie-breaker.
Continuation cursors are opaque and integrity-protected, and bind the store
identity/version, logical view, projection, normalized filter, order, policy
identity, and last emitted sort key. A cursor with a changed binding, invalid
encoding, expired snapshot, or unavailable store version fails closed. It is
never interpreted as a caller-controlled offset or query fragment.

Responses report the enforced limits and either completion or explicit
truncation with a continuation cursor. Silent truncation is forbidden. Exact
omitted counts are optional because computing them must not evade scan/time
budgets; when unavailable, the response records that the remainder is unknown.
Equivalent allowed requests have the same ordering, redaction decision, and
limit semantics across Nu, CLI, and MCP, even when their presentation formats
differ.

## Planned Crate Additions

- codedb-polyglot-core
- codedb-language-detect
- codedb-tree-sitter
- codedb-package-detect
- codedb-index-scip (optional or gated)
- codedb-export-crate
- codedb-fixtures-polyglot

## Fixture Families

Each fixture is a checked-in, immutable input tree. Its manifest records every
entry's relative path, entry kind, byte length, SHA-256 digest, and executable
bit. Symlinks record link text instead of target contents. Tests must compute
these facts from filesystem bytes; text decoding, newline normalization,
parsing, and Git filters are not part of Tier 0 capture.

| Family | Required raw-byte cases | Round-trip assertion |
|---|---|---|
| `minimal-python` | UTF-8 source, empty file, LF and CRLF files, and a final line without a newline | File bytes and executable bits match the fixture manifest |
| `minimal-ruby` | UTF-8 source, shebang, executable script, and non-ASCII identifier/comment bytes | File bytes and executable bits match; the shebang is not rewritten |
| `minimal-typescript` | UTF-8 source, JSON metadata, lockfile, and UTF-8 BOM case | Source, metadata, lockfile, and BOM bytes match independently |
| `mixed-rust-python-ts` | Same basename in different directories/languages plus mixed newline styles | Paths cannot alias; each blob resolves to its own manifest digest |
| `config-heavy` | Dotfiles, extensionless files, nested configuration, empty directories, and duplicate-content files | Names and directory structure match; duplicate blobs may deduplicate internally but materialize at every original path |
| `binary-assets` | NUL bytes, all byte values `00`-`ff`, invalid UTF-8, embedded CR/LF, zero-length blob, and a payload larger than one streaming chunk | Exact length and digest match without text conversion or truncation |
| `symlinks` | Relative in-tree link, dangling link, link to a directory, and an escaping-link negative case | Link text and entry kind match; capture never follows a link, and unsafe materialization fails closed |
| `credential-like-files` | Synthetic secret-shaped bytes in included, hash-only, and refused paths | Policy outcome is explicit; refused bytes never enter persisted blobs, exports, logs, or bounded views |
| `vendor-generated` | Deep paths, executable files, duplicate blobs, large generated text, binary output, and ignored files | Capture follows the declared policy rather than ignore heuristics; all admitted entries match the manifest |

Every family is exercised through raw capture, redb persistence, export-pack
verification, and materialization into a fresh directory. Parser or detector
failures may reduce higher-tier facts, but must not alter an admitted Tier 0
blob or its proof metadata.

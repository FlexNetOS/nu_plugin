# Parser And Indexer Tooling Matrix

This matrix narrows the candidates recorded in the research ledger into roles
for the issue-215 tier model. It is a planning comparison, not a dependency
selection or a claim that any candidate has shipped.

## Selection Constraints

- Tier 0 bytes and Tier 1 detections remain authoritative evidence when parsing
  is unsupported, incomplete, or fails.
- The safe default path must not install dependencies, execute repository
  scripts, evaluate build configuration, or contact a network service.
- An in-process library can be default-eligible only after its version, grammar
  set, license, resource limits, and deterministic fixture output are pinned.
- A subprocess or external indexer is gated. Its executable identity,
  arguments, environment, input snapshot, output digest, and failure status
  must be captured.
- Parser and indexer rows are derived observations. They never replace source
  blobs and must identify their tool, tool version, grammar/schema version, and
  originating file/blob.

## Candidate Matrix

| Candidate | Proposed role | Coverage | Default-path disposition | What it may emit | Main gaps or risks |
|---|---|---|---|---|---|
| Tree-sitter | shared syntax adapter | multi-language | Tier 2 candidate after grammar-by-grammar qualification | bounded node-kind, declaration, and import-like summaries | grammar quality, error recovery, and node vocabularies vary; no general semantic truth |
| ast-grep | structural-query adapter over tree-sitter | built-in multi-language set | optional Tier 2/3 helper, not the parser authority | matches for pinned structural rules | rule output is only as complete as the rule set; must not be presented as a complete symbol graph |
| Oxc | primary JS/TS syntax candidate | JS, JSX, TS, TSX | strongest default-path research candidate because it is Rust-native and separates parsing from semantics | modules, declarations, imports/exports, syntax diagnostics | final CST/comment retention and stable persisted-node mapping still require proof |
| SWC | alternate JS/TS syntax candidate | ECMAScript, TypeScript, Flow | default-eligible only as a pinned Rust library; transforms stay out of scope | modules, declarations, imports/exports, syntax diagnostics | broad transform ecosystem is unnecessary for read-only capture; mixed Oxc/SWC ownership could create divergent facts |
| Biome | web/config corroboration candidate | JS/TS/JSX/TSX, JSON, HTML, CSS | optional; use only where its parser surface fills an accepted coverage gap | syntax/config diagnostics and bounded web-language summaries | product surface is broader than parsing and must not introduce formatting or mutation |
| TypeScript Compiler API | type-aware JS/TS enrichment | JS/TS projects | Tier 3/4 gated subprocess or optional adapter | resolved symbols, references, types, project diagnostics | unstable API and Node/project-resolution assumptions; dependency installation and project scripts remain forbidden |
| Prism | Ruby syntax adapter | Ruby | strong Tier 2 candidate; in-process/default eligibility depends on the chosen portable binding | modules/classes, methods, requires, syntax diagnostics | name and dependency resolution remain separate |
| LibCST | lossless Python CST adapter | Python | gated subprocess unless a separately approved embedded runtime is adopted | modules, declarations, imports, formatting-preserving locations | Python runtime dependency; CST facts are not type or environment resolution |
| Ruff-derived parsing lane | Rust-native Python research lane | Python | research only until a supported stable crate API is available | syntax and import/lint-derived observations | internal Rust crates are explicitly unstable; do not bind persisted schema to private AST types |
| go/parser | Go syntax adapter | Go | Tier 2 candidate through a pinned helper; subprocess execution is gated | packages, declarations, imports, syntax diagnostics | full package loading, build tags, generated files, and module resolution need separate policy |
| SCIP family | normalized precise-index adapter | language-specific indexers for several baseline and stretch languages | Tier 4 optional and proof-gated | documents, symbols, occurrences, relationships, diagnostics | indexers require external toolchains and may resolve dependencies or build project state |
| CodeQL | security/semantic query adapter | supported CodeQL languages | Tier 4 optional and proof-gated; never a default parser | database/query-derived semantic and security observations | database creation is heavyweight and may invoke language-specific build modes; coverage is not universal |

Structured JSON, YAML, and TOML parsing should use pinned, non-executing data
parsers rather than a code indexer. Shell, Nix, Markdown, HTML, and CSS remain
marker-first unless a candidate is separately qualified for their dialect and
embedded-language boundaries.

## Recommended First-Wave Shape

| Language lane | Tier 2 first choice | Tier 3 boundary | Fallback |
|---|---|---|---|
| Rust | preserve the existing Rust-specialized path; evaluate `syn` only within that ownership boundary | existing Rust rows remain authoritative for specialized facts | emit capture gaps without weakening Tier 0/1 |
| JavaScript/TypeScript | run an Oxc-first fixture spike; compare SWC only against explicit missing requirements | lexical module/declaration/import/export rows only; type-aware Compiler API facts stay gated | retain raw bytes and detections plus a parser diagnostic/gap |
| Ruby | qualify Prism | syntactic module/class/method/require rows; no environment resolution | retain raw bytes and detections plus a parser diagnostic/gap |
| Python | compare LibCST completeness with the stability cost of a Ruff-derived lane before selecting a default | syntactic module/declaration/import rows; type resolution stays optional | marker/import scanning may be recorded as lower-confidence derived facts |
| Go | qualify `go/parser` through a pinned adapter | syntactic package/declaration/import rows; `go/packages`-style resolution stays gated | retain raw bytes and detections plus a parser diagnostic/gap |
| Other baseline languages | marker-first | no symbol/reference completeness claim | explicit unsupported-language or unsupported-dialect capture gap |

Do not combine Oxc and SWC outputs by default. The Oxc-first spike should test
syntax coverage, byte ranges, comments, malformed input, deterministic output,
and schema mapping. Promote SWC or a mixed strategy only when a recorded fixture
demonstrates a requirement Oxc cannot meet and the ownership rule for
conflicting facts is specified.

## Bounded Summary Plan

Tier 2 summaries are persisted and exposed as finite records, not raw AST dumps.
For each admitted source file, the adapter should:

1. Consume the already captured blob bytes and declared/detected language; it
   must not reread mutable project state for semantic resolution.
2. Record parser identity, parser/grammar version, options digest, input blob
   identity, status, elapsed/resource counters, and deterministic output digest.
3. Emit only normalized top-level facts needed by the planned schema:
   declaration kind/name/location, module or package declaration,
   import/export/require target text and location, and bounded diagnostics.
4. Apply configured limits for input bytes, parse depth or parser resources,
   facts per file, diagnostics per file, and serialized summary bytes. The
   actual limits are implementation decisions and must be fixed before a parser
   is enabled.
5. On unsupported syntax, timeout/resource exhaustion, crash, malformed output,
   or truncation, preserve all Tier 0/1 rows and append a typed `capture_gap` or
   `validation_error`. Partial output is marked incomplete and never silently
   treated as exhaustive.

Tier 3 may normalize Tier 2 facts into module, symbol, import, dependency, and
reference rows only where the adapter has an explicit confidence and ownership
contract. Tier 4 data is imported under a separate namespace keyed by indexer
and snapshot identity so it can corroborate, but cannot silently overwrite,
default-path observations.

Bounded Nu/CLI/MCP views should require pagination and projection, sort by
stable file/location/kind keys, report `complete`, `truncated`, and gap counts,
and omit source text and raw tool payloads by default. Raw AST/CST or SCIP/CodeQL
payloads, if retained for debugging, are content-addressed derived artifacts
behind the existing raw-read and security policy rather than inline summary
fields.

## Decision Gates

A candidate can move from research to the default path only after checked-in
fixtures prove deterministic results, malformed-input isolation, resource-limit
enforcement, byte-range validity, stable schema mapping, and unchanged Tier 0
round trips. Gated tools additionally require executable provenance, a
no-network/no-install test, and a fail-closed test for attempted project
execution.

Open decisions remain:

- accept or reject Oxc as the JS/TS Tier 2 owner after the fixture spike;
- choose a supported Python adapter without binding to an unstable private API;
- decide whether the Prism and Go adapters are embedded or isolated helpers;
- keep rust-analyzer/SCIP and CodeQL planning-only until a separately approved
  Tier 4 implementation and proof package exists.

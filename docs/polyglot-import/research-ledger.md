# Research Ledger

All claims in this document are labeled as FACT, INFERENCE, QUESTION, GAP, or
BLOCKER.

## Current-State Audit

- FACT: The current package is V1.1 and explicitly positions CodeDB as a
  Rust-native crate capture system with Git/source files remaining authoritative
  input. Evidence: GOAL.md, docs/ARCHITECTURE.md, docs/SCHEMA.md.
- FACT: Bounded, read-only default surfaces already exist for CLI, Nushell, and
  MCP, and raw source/blob reads remain blocked by default. Evidence:
  docs/COMMANDS.md, docs/CODEX_BRIDGE.md, docs/SECURITY_AND_SECRET_POLICY.md.
- FACT: The existing issue-212 roadmap (CDB070-CDB090) covers the bidirectional
  source-to-store loop and Rust-first gap closure, so issue 215 must extend that
  work rather than replace it. Evidence: FlexNetOS/flexnetos_runner#212 and the
  related GitKB tasks.
- INFERENCE: The safest execution model for issue 215 is to widen observable
  repository facts without promoting the database over source files.

## Official Source Ledger

### Multi-language parsing

- FACT: Tree-sitter provides a parser framework with official bindings and a
  documented parser API.
  Source: https://tree-sitter.github.io/tree-sitter/using-parsers/
- FACT: ast-grep exposes built-in language support using file-extension-based
  detection and has JS, Python, and Rust API surfaces.
  Sources:
  - https://ast-grep.github.io/reference/languages.html
  - https://ast-grep.github.io/reference/api.html
- FACT: Oxc documents a high-performance Rust parser for JavaScript,
  TypeScript, JSX, and TSX and separates parsing from later semantic analysis.
  Sources:
  - https://oxc.rs/docs/guide/usage/parser.html
  - https://oxc.rs/docs/learn/architecture/parser.html
- FACT: SWC exposes parser APIs for ECMAScript, TypeScript, and Flow through
  its core packages and Rust parser crates.
  Sources:
  - https://swc.rs/docs/usage/core
  - https://rustdoc.swc.rs/swc_ecma_parser/index.html
- FACT: Prism is a portable, error-tolerant Ruby parser bundled with CRuby 3.3+
  and published with Ruby, Rust, Java, and C references.
  Sources:
  - https://ruby.github.io/prism/
  - https://ruby.github.io/prism/rb/Prism.html
- FACT: LibCST parses Python source into a CST while preserving formatting,
  whitespace, and comments.
  Source: https://libcst.readthedocs.io/en/latest/

### Code intelligence and indexing

- FACT: SCIP is a language-agnostic code intelligence protocol with documented
  indexers for TypeScript/JavaScript, Python, Ruby, Rust, and more.
  Sources:
  - https://github.com/sourcegraph/scip
  - https://sourcegraph.com/docs/code-navigation/writing-an-indexer
  - https://sourcegraph.com/docs/code-navigation/precise-code-navigation
- FACT: Sourcegraph lists rust-analyzer, scip-typescript, scip-python, and
  scip-ruby among supported or recommended indexer routes.
  Source: https://sourcegraph.com/docs/code-navigation/writing-an-indexer
- FACT: GitHub documents language support for core features and notes that PHP
  and Scala are not supported by CodeQL in the same first-party way as several
  other core languages.
  Source: https://docs.github.com/en/get-started/learning-about-github/github-language-support?external_link=true

### Language-specific capture candidates

- FACT: Ruff is a Rust-written Python linter/formatter with unstable internal
  Rust crate interfaces, making it promising for metadata/lint surfaces but not
  yet a stable parser API contract.
  Sources:
  - https://docs.astral.sh/ruff/
  - https://docs.astral.sh/ruff/versioning/
- FACT: basedpyright is a stricter fork of Pyright that is installable from
  PyPI and designed for both CLI and language-server use.
  Source: https://docs.basedpyright.com/
- FACT: uv.lock has a versioned schema and uv export can convert lockfile data
  into multiple formats, making it a good metadata source without requiring
  install in the default path.
  Sources:
  - https://docs.astral.sh/uv/concepts/projects/sync/
  - https://docs.astral.sh/uv/concepts/resolution/
  - https://docs.astral.sh/uv/concepts/projects/export/
- FACT: The TypeScript compiler API is documented but explicitly not promised as
  a stable API.
  Source: https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API
- FACT: Biome covers JavaScript, TypeScript, JSX, TSX, JSON, HTML, and CSS, and
  exposes parser-related configuration knobs.
  Sources:
  - https://biomejs.dev/
  - https://biomejs.dev/reference/configuration/
- FACT: The Go standard go/parser package is a documented Go source parser,
  while more complete package loading is delegated to go/packages.
  Source: https://pkg.go.dev/go/parser

## Implications

- INFERENCE: Tier 2 parser-backed summaries should prefer pure-Rust or
  embeddable parser libraries on the default path when feasible.
- INFERENCE: Tier 4 indexers such as SCIP should remain optional and gated
  because several recommended flows assume external runtimes, installs, or CI.
- QUESTION: Whether rust-analyzer SCIP/export should be an initial V1.2 lane or
  a later optional lane.
- QUESTION: Whether the repo should standardize on Oxc, SWC, Biome, or a mixed
  JS/TS parser strategy.
- GAP: The current repo has no dedicated whole-repo language/package detector
  crate or schema family.
- GAP: The current repo does not yet have a generated single-binary export
  artifact contract for non-Rust whole-repo snapshots.
- BLOCKER: Live issue creation in FlexNetOS/nu_plugin remains unavailable to
  this integration path, so exact checked-in issue drafts are the required
  fallback unless access changes.

# Parser And Indexer Tooling Matrix

| Candidate | Domain | Strengths | Default or gated | Risks and notes |
|---|---|---|---|---|
| Tree-sitter | multi-language parsing | broad grammar ecosystem, official bindings, incremental parsing model | candidate for Tier 2 default research | grammar quality varies by ecosystem |
| ast-grep | multi-language structural matching | built on tree-sitter, CLI plus JS/Python/Rust APIs, built-in language table | candidate for Tier 2 summary and pattern extraction | best suited to structural matching, not full semantic truth |
| Oxc | JS/TS parser and semantics | fast Rust parser for JS/TS/JSX/TSX, parser/semantic split, Rust-native | strong default-path candidate for JS/TS planning | comments/printing and downstream ecosystem choices still matter |
| SWC | JS/TS/Flow parser | mature parser APIs across Node and Rust crates, strong test coverage | candidate, possibly gated or mixed | broader transform toolchain can tempt non-default mutation workflows |
| Biome | web formatter/linter/parser surfaces | covers JS/TS/JSON/HTML/CSS; parser config surfaced in docs | candidate for config-aware parsing and lint signals | main product is broader than parsing alone |
| TypeScript Compiler API | JS/TS compiler-aware AST and type model | richest language-native semantics for TS/JS | gated or optional | API is documented as unstable and often assumes Node/npm ecosystem |
| Prism | Ruby parser | official Ruby parser, bundled with CRuby 3.3+, portable references | strong Ruby parser candidate | semantic resolution still separate |
| LibCST | Python CST | preserves formatting/comments, codemod-friendly | strong Python CST candidate | Python implementation, not pure-Rust default |
| Ruff-derived parsing lane | Python metadata and lint parser surfaces | very fast, Rust-based ecosystem | research candidate | Rust crates are explicitly unstable today |
| go/parser | Go AST parsing | standard-library parser, documented API | default Go syntax candidate | full package and build-tag resolution needs more than parser alone |
| SCIP family | external indexer protocol | precise cross-repo code intelligence | Tier 4 optional or gated | usually assumes external toolchains and setup |
| CodeQL | external index and query platform | deep semantic/security query ecosystem | Tier 4 optional or gated | not a default-path local parser strategy |

## Recommendation

- Default-path planning should prefer detector + parser combinations that do not
  require package installation or project script execution.
- External indexers remain optional and proof-gated.
- JS/TS likely needs a deliberate choice between Oxc-first, SWC-first, or mixed
  strategies before implementation work begins.

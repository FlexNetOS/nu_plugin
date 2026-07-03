# Language Import Surface

This document defines the per-language import surface using the issue-215 tier
model.

## Import Tiers

- Tier 0: raw bytes, metadata, checksums, permissions, symlinks, binary policy
- Tier 1: language detection and package/config/lockfile detection
- Tier 2: parser-backed CST/AST summaries
- Tier 3: symbol/import/dependency/reference rows
- Tier 4: optional external indexer rows such as SCIP or CodeQL
- Tier 5: dynamic runtime/build facts behind explicit unsafe gates

## Baseline Languages

| Language | Extensions | Package/config markers | Tier 2 candidates | Tier 3 default | Tier 4 optional | Risks and gaps |
|---|---|---|---|---|---|---|
| Rust | .rs | Cargo.toml, Cargo.lock, .cargo/config | existing Rust-static path, syn lane | Rust-specialized rows already exist | rust-analyzer, SCIP | keep V1.1 rows authoritative for Rust specialization |
| Python | .py, .pyi, .py3 | pyproject.toml, requirements files, uv.lock | Ruff-derived parsing lane, LibCST | module/import/config rows | basedpyright, scip-python | no dependency install by default |
| Ruby | .rb, .rbw, .gemspec | Gemfile, Gemfile.lock | Prism | module/import/config rows | scip-ruby | no bundle install by default |
| JavaScript | .js, .mjs, .cjs, .jsx | package.json, lockfiles | Oxc, SWC, Biome, TypeScript API | module/import/config rows | scip-typescript, CodeQL | external toolchains stay gated |
| TypeScript | .ts, .tsx, .cts, .mts | package.json, tsconfig files, lockfiles | Oxc, SWC, TypeScript API, Biome | module/import/config rows | scip-typescript, CodeQL | type-aware flows can require project install |
| Go | .go | go.mod, go.sum, vendor | go/parser | package/import/config rows | SCIP or gopls lane | build tags and package loading need care |
| Shell/Bash | .sh, .bash, .zsh, shebang | shell scripts, CI files | marker discovery first | scripts/config rows | none by default | avoid script execution |
| Nix | .nix | flake.nix, flake.lock, shell.nix, default.nix | marker discovery first | config/build rows | optional later | avoid eval/build by default |
| JSON | .json | config and manifest files | native structured parse | config/dependency rows | none by default | dialect differences vary |
| YAML | .yml, .yaml | CI/config/deploy files | structured parse | config rows | none by default | schema meaning is contextual |
| TOML | .toml | project/config manifests | structured parse | config/package rows | none by default | preserve ordering only if needed |
| Markdown | .md | docs and prompt surfaces | marker discovery, optional code-block parsing | doc/config rows | none by default | embedded code blocks are mixed-language |
| HTML | .html, .htm, .xhtml | web/config surfaces | ast-grep or web parser later | config/module rows | optional later | mixed script/style blocks complicate ownership |
| CSS | .css | web/theme/config surfaces | Biome or SWC CSS parser later | config/style rows | optional later | preprocessors stay out-of-scope by default |

## Stretch Languages

| Language family | Planning status | Notes |
|---|---|---|
| Java / Kotlin | stretch | keep in Tier 4 optional indexer planning unless later promoted |
| C / C++ | stretch | optional indexer lane only |
| C# | stretch | optional indexer lane only |
| PHP | stretch | optional indexer lane only |
| Swift | stretch | parser/indexer research only |
| Lua | stretch | parser/indexer research only |

## Safe Default Rows

The default path should always be able to emit:

- source file and blob identity rows
- language detection rows
- package manager and lockfile rows
- config/build marker rows
- capture gaps for unsupported or unsafe observations
- validation errors for malformed or policy-invalid inputs

Tier 2 and deeper rows are add-on facts, not replacements for Tier 0 and Tier 1
evidence.

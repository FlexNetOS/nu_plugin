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
| Python | .py, .pyi, .py3 | pyproject.toml, requirements files, setup.py, setup.cfg, Pipfile, uv.lock, poetry.lock | LibCST; Ruff-derived research lane | module/declaration/import/config rows | basedpyright, scip-python | no dependency install or Python code execution by default |
| Ruby | .rb, .rbw, .rake, .gemspec, extensionless Ruby scripts | Gemfile, gems.rb, Gemfile.lock, gems.locked, Rakefile, .ruby-version | Prism | module/class/method/require/package/config rows | scip-ruby | no Ruby, Bundler, gem, or Rake execution by default |
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

## Python Capture Plan

The default Python lane is static and snapshot-bound. It reads captured file
bytes and metadata only; it does not import modules, execute `setup.py`, load
plugins, create a virtual environment, resolve an interpreter environment, or
run `pip`, `uv`, Poetry, Pipenv, or another package manager.

### Coverage by tier

| Tier | Python coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for `.py`, `.pyi`, and `.py3` files, including empty files and mixed newline styles | preserve the source blob identity and byte ranges |
| Tier 1 | language detection plus `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements*.txt`, constraints files, `Pipfile`, `Pipfile.lock`, `uv.lock`, and `poetry.lock` markers | marker presence is evidence, not proof that a tool is installed or that an environment resolves |
| Tier 2 | bounded syntactic module, declaration, import, and diagnostic summaries | evaluate LibCST as the formatting-preserving adapter; keep a Ruff-derived adapter research-only until a supported stable API is selected |
| Tier 3 | normalized module, declaration, import-target, package/config, and dependency rows derived from admitted Tier 1/2 facts | unresolved, relative, conditional, dynamic, wildcard, and type-checking-only imports retain their syntax and confidence instead of being guessed |
| Tier 4 | optional basedpyright/Pyright or `scip-python` observations | gated subprocess/indexer rows use a separate namespace and record tool version, arguments, environment policy, input snapshot, output digest, and failure status |
| Tier 5 | runtime imports, installed-distribution state, test discovery, and build/backend execution | out of the safe default path and admitted only by a separate unsafe approval |

LibCST is the first completeness candidate because it preserves Python
formatting, whitespace, and comments, but using it as a subprocess remains
gated unless an embedded runtime is separately approved. A Ruff-derived lane is
attractive for Rust-native parsing but must not bind the persisted schema to
Ruff's unstable internal crates. basedpyright/Pyright and `scip-python` are
optional semantic/indexer corroboration, never prerequisites for Tier 0 or
Tier 1 capture.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-python` | UTF-8 source, empty source, `.pyi`, LF and CRLF, a final line without a newline, one valid import, and exact byte/blob round trip |
| `python-packaging` | `pyproject.toml` project and build-system tables, representative requirements/constraints syntax, and checked-in `uv.lock`, Poetry, and Pipenv markers without refresh, sync, or install |
| `python-import-shapes` | absolute, relative, aliased, wildcard, conditional, `TYPE_CHECKING`, and dynamic-import examples; dynamic targets remain unresolved unless literal syntax is safely observable |
| `python-malformed-and-bounded` | invalid syntax, invalid package metadata, oversized/deep input, fact truncation, and parser failure while Tier 0/1 evidence remains available |

Each fixture must prove deterministic ordering and output, valid byte ranges,
bounded diagnostics/fact counts, no network access, no dependency installation,
and no repository code execution. Unsupported encodings or dialects, parser
unavailability, malformed syntax, ambiguous package metadata, unresolved
imports, truncation, and optional-indexer failure produce typed `capture_gap` or
`validation_error` rows rather than a false completeness claim.

## Ruby Capture Plan

The default Ruby lane is static, offline, and snapshot-bound. It reads admitted
file bytes and metadata without loading project code, evaluating a gemspec or
Bundler DSL, resolving the load path, installing gems, or running `ruby`,
`bundle`, `gem`, `rake`, or project-provided executables.

### Coverage by tier

| Tier | Ruby coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for `.rb`, `.rbw`, `.rake`, `.gemspec`, recognized extensionless scripts, and Ruby package/config files | preserve source blob identity, byte ranges, executable bits, shebang bytes, encoding magic comments, and newline style |
| Tier 1 | extension, shebang, and conventional-name detection plus `Gemfile`, `gems.rb`, `Gemfile.lock`, `gems.locked`, `.gemspec`, `Rakefile`, and `.ruby-version` markers | marker presence does not prove that Ruby, Bundler, a gem, or a compatible platform is installed |
| Tier 2 | bounded Prism-backed syntax, module/class, method, constant, `require`/`require_relative` literal, and diagnostic summaries | Prism is the primary candidate; binding choice and version must be qualified, while parser failure leaves Tier 0/1 evidence intact |
| Tier 3 | normalized declaration, literal require-target, package/config, declared dependency, and locked dependency rows derived from admitted Tier 1/2 facts | metaprogrammed declarations, computed require targets, autoload conventions, monkey patches, and runtime load-path resolution remain unresolved rather than inferred |
| Tier 4 | optional `scip-ruby` observations | gated indexer facts use a separate namespace and record tool version, arguments, environment policy, input snapshot, output digest, and failure status |
| Tier 5 | runtime requires, Bundler resolution, installed-gem state, native-extension builds, tests, Rake tasks, and project initialization | excluded from the safe default path and admitted only through a separate unsafe approval |

Prism provides portable, error-tolerant Ruby syntax coverage and is the first
Tier 2 candidate, but the importer must pin and report the selected adapter and
Prism version. Syntax facts never imply Ruby name or method resolution.
`scip-ruby` is optional semantic corroboration and is not required for byte,
marker, package, or lockfile capture.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-ruby` | UTF-8 source, empty source, shebang and executable bit, encoding magic comment, non-ASCII bytes, LF and CRLF, final line without a newline, literal `require`, and exact byte/blob round trip |
| `ruby-packaging` | inert `Gemfile` and `.gemspec` source markers, `gems.rb`/`gems.locked` aliases, and a checked-in `Gemfile.lock` containing registry, git, path, platform, dependency, Ruby-version, and Bundler-version sections without install or lock refresh |
| `ruby-syntax-shapes` | nested modules/classes, singleton and instance methods, reopened classes, aliases, literal and computed `require`/`require_relative`, autoload, conditionals, and representative metaprogramming; dynamic targets remain unresolved |
| `ruby-malformed-and-bounded` | invalid syntax, invalid lockfile structure, unknown lockfile sections, oversized/deep input, fact truncation, and parser failure while Tier 0/1 evidence remains available |

Each fixture must prove deterministic ordering and output, valid byte ranges,
bounded diagnostics and fact counts, no network access, no dependency
installation, and no Ruby or repository code execution. Unsupported encodings
or dialects, ambiguous shebangs, parser unavailability, syntax errors, computed
requires, unresolved constants, lockfile parse failures, truncation, and
optional-indexer failures produce typed `capture_gap` or `validation_error`
rows instead of false completeness.

## JavaScript And TypeScript Capture Plan

The default JavaScript/TypeScript lane is static, offline, snapshot-bound, and
syntax-first. It reads admitted files without resolving or installing packages,
loading project plugins, executing configuration, transpiling a project, or
running `node`, `deno`, `bun`, `npm`, `npx`, `pnpm`, `yarn`, `tsc`, or package
scripts. JavaScript-to-TypeScript or language-to-Rust translation is not an
import guarantee: exact source bytes remain authoritative, while parser and
compiler observations are derived, versioned facts.

### Coverage by tier

| Tier | JavaScript/TypeScript coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for `.js`, `.mjs`, `.cjs`, `.jsx`, `.ts`, `.mts`, `.cts`, and `.tsx`, including declaration files such as `.d.ts`, `.d.mts`, and `.d.cts` | preserve source blob identity, byte ranges, executable bits, shebang bytes, newline style, and extension; never rewrite between module or language forms |
| Tier 1 | language/dialect detection plus `package.json`, workspace manifests, recognized lockfiles, `tsconfig*.json`, `jsconfig*.json`, and conventional tool-config markers | distinguish extension evidence from parser mode and package `"type"`; marker presence does not prove that a runtime, dependency, plugin, or build is available |
| Tier 2 | bounded syntax, declaration, import/export, JSX/TSX, and diagnostic summaries | qualify the selected Oxc, SWC, Biome, or TypeScript compiler API adapter and version; record parse options and dialect; recovery or parser disagreement must not silently promote uncertain facts |
| Tier 3 | normalized module, declaration, literal dependency/import target, package/workspace/config, and dependency rows derived from admitted Tier 1/2 facts | retain type-only, dynamic, conditional, aliased, CommonJS, URL, package-subpath, and unresolved imports explicitly; do not claim Node/bundler resolution, inferred runtime behavior, or type correctness |
| Tier 4 | optional `scip-typescript`, CodeQL, or separately qualified type-aware TypeScript project observations | gated subprocess/indexer rows use a separate namespace and record tool version, arguments, environment policy, input snapshot, output digest, truncation, and failure status |
| Tier 5 | package resolution, plugin loading, generated types, project references requiring builds, runtime module loading, tests, bundling, transpilation, and application behavior | excluded from the safe default path and admitted only through a separate unsafe approval |

Oxc is the first Rust-native candidate for fast JS/TS/JSX/TSX syntax and
semantic summaries. SWC is a mature parser/transform ecosystem, but the import
adapter must use parsing only and must not imply transform round-trip fidelity.
Biome is useful as an error-tolerant parser and diagnostic corroboration lane;
its formatter and linter behavior are outside default capture. The TypeScript
compiler API is the reference candidate for TypeScript syntax kinds, declaration
forms, and optional type-aware observations, but invoking it requires a pinned,
gated toolchain and must not trigger project loading, module resolution, plugin
execution, emit, or build. No candidate is permitted to replace the exact source
blob or to support a claim of lossless conversion into Rust.

All human and machine views are bounded. Each request applies byte, file,
diagnostic, symbol, import-edge, and nesting limits; uses deterministic
path/source-order plus stable tie-breakers; and reports truncation with omitted
counts or a typed `capture_gap`. Source excerpts are opt-in and bounded under
the existing secret policy. A parser crash, timeout, unsupported syntax mode, or
disagreement leaves Tier 0/1 evidence queryable.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-typescript` | UTF-8 `.ts`, `.tsx`, `.d.ts`, empty source, LF and CRLF, a final line without a newline, type-only and value imports/exports, declarations, JSX/TSX, and exact byte/blob round trip |
| `minimal-javascript` | `.js`, `.mjs`, `.cjs`, and `.jsx`; ESM and CommonJS forms; shebang/executable source; package `"type"` interactions; dynamic import and `require`; JSX; and exact byte/blob round trip |
| `javascript-typescript-packaging` | `package.json` dependencies and scripts as inert data, npm/pnpm/Yarn/Bun workspace declarations, checked-in lockfile variants, `tsconfig`/`jsconfig` inheritance and project-reference text, and conflicting nested package boundaries without install, lock refresh, or build |
| `javascript-typescript-syntax-shapes` | decorators and other version-sensitive syntax, ambient/global/module declarations, namespaces, enums, generics, import attributes, path aliases, package subpaths, conditional/dynamic imports, CommonJS interop, JSX/TSX ambiguity, and parser-mode disagreement |
| `javascript-typescript-malformed-and-bounded` | invalid JSON/config and syntax, unsupported lockfile versions, missing local config/workspace references, oversized/deep inputs, fact/diagnostic truncation, parser unavailability, and optional-indexer failure while Tier 0/1 evidence remains available |

Fixtures must prove deterministic ordering and output, valid byte ranges,
bounded excerpts/diagnostics/fact counts, secret redaction, no network access,
no cache or generated-file writes, no dependency installation, and no runtime,
package-manager, compiler, bundler, project-script, or repository-code
execution. Unsupported encodings, dialects, configuration inheritance, parser
versions, module targets, computed specifiers, ambiguous package boundaries,
unresolved aliases, truncation, and optional-tool failures produce typed
`capture_gap` or `validation_error` rows rather than fabricated resolution or
completeness.

## Go Capture Plan

The default Go lane is static, offline, and snapshot-bound. It reads admitted
source, module, checksum, workspace, vendoring, and toolchain configuration
files without invoking the Go toolchain, loading packages, resolving modules,
generating source, compiling, testing, or accessing module proxies.

### Coverage by tier

| Tier | Go coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for `.go`, `go.mod`, `go.sum`, `go.work`, `go.work.sum`, and `vendor/modules.txt` | preserve source blob identity, byte ranges, permissions, newline style, and file role |
| Tier 1 | Go source detection plus module, workspace, checksum, vendoring, and conventional tool-config markers | marker presence does not prove that a Go toolchain is installed, a module graph resolves, checksums are valid, or vendored content is complete |
| Tier 2 | bounded `go/parser`-backed package-clause, declaration, import, build-constraint, and syntax-diagnostic summaries | qualify and version the pinned parser adapter; record parser mode, retain comments/build constraints as observable syntax, and keep Tier 0/1 evidence when parsing fails |
| Tier 3 | normalized package, declaration, literal import-target, module directive, requirement, replacement, exclusion, retract, toolchain, workspace-use/replace, checksum, and vendoring rows | keep local replacements, versions, indirect annotations, build constraints, platform-specific files, generated-file markers, and unresolved imports explicit; do not claim package selection or Minimal Version Selection results |
| Tier 4 | optional SCIP or `gopls` observations | gated indexer facts use a separate namespace and record tool version, arguments, environment policy, input snapshot, output digest, truncation, and failure status |
| Tier 5 | `go list`, package loading, module download, proxy/checksum-database access, generation, cgo, compilation, tests, vet, and runtime behavior | excluded from the safe default path and admitted only through a separate unsafe approval |

`go/parser` is the first Tier 2 candidate because it supplies standard Go
syntax trees without requiring package loading. Its observations do not imply
that build tags select a file, imports resolve, module replacements exist, or a
package compiles. Module, workspace, checksum, and vendoring files are parsed as
inert captured data; the importer never runs `go mod tidy`, `go mod download`,
`go work sync`, `go generate`, or another Go command in the default lane.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-go` | package clauses, declarations, grouped and aliased imports, blank and dot imports, comments, empty source, LF and CRLF, a final line without a newline, and exact byte/blob round trip |
| `go-modules-and-workspaces` | representative `go.mod` directives, local and versioned replacements, exclusions, retractions, toolchain/version declarations, `go.work` use/replace directives, checked-in sums, and nested modules without graph resolution or file mutation |
| `go-build-shapes` | modern and legacy build constraints, filename platform suffixes, internal/test/external-test packages, generated-file markers, embed directives as syntax, cgo imports as syntax, and vendor metadata without selecting a build context |
| `go-malformed-and-bounded` | invalid source, malformed module/workspace/checksum/vendor records, unknown directives, oversized/deep input, fact truncation, and parser failure while Tier 0/1 evidence remains available |

Fixtures must prove deterministic ordering and output, valid byte ranges,
bounded diagnostics/fact counts, no network access, no dependency installation,
no toolchain or repository-code execution, and no mutation of module, sum,
workspace, vendor, generated, or cache files. Unsupported Go versions or
directives, ambiguous module boundaries, malformed syntax/configuration,
unresolved imports or replacements, build-context uncertainty, truncation, and
optional-indexer failure produce typed `capture_gap` or `validation_error` rows.

## Shell Capture Plan

The default Shell lane is marker-first, static, offline, and snapshot-bound. It
captures shell source and shell-bearing configuration without sourcing files,
expanding substitutions, following runtime includes, loading profiles, or
invoking a shell, task runner, CI runner, or commands named by the source.

### Coverage by tier

| Tier | Shell coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for recognized shell extensions, extensionless executable scripts with admitted shebangs, profile/RC files, and shell-bearing CI/task configuration | preserve source blob identity, byte ranges, executable bits, symlink policy, shebang bytes, newline style, extension, and conventional filename |
| Tier 1 | extension, shebang, conventional-name, and bounded embedded-shell marker detection for POSIX `sh`, Bash, Zsh, and explicitly admitted dialects | record the detection evidence and dialect confidence; a shebang or config key does not prove that its interpreter, commands, includes, or environment exist |
| Tier 2 | marker discovery by default; optional separately qualified parser may emit bounded commands, functions, assignments, redirects, pipelines, control structures, comments, and syntax diagnostics | parser and dialect versions must be recorded; unsupported constructs and embedded-language boundaries remain gaps, and parser recovery never authorizes evaluation |
| Tier 3 | normalized script/config, function, literal include target, literal command-name, declared environment-key, and CI/task shell-block rows derived from admitted syntax | computed commands, aliases, functions, `eval`, substitutions, expansions, sourced values, PATH lookup, globbing, and control/data-flow remain unresolved rather than simulated |
| Tier 4 | no default indexer; optional shell analysis observations may be admitted only after separate qualification | gated tool rows use a separate namespace and record version, arguments, dialect, environment policy, input snapshot, output digest, truncation, and failure status |
| Tier 5 | sourcing, expansion, command lookup, process execution, profile loading, task/CI execution, and runtime environment or filesystem effects | excluded from the safe default path and admitted only through a separate unsafe approval |

Shell detection must distinguish file identity from inferred dialect. For
example, `.sh` does not guarantee Bash, `/usr/bin/env` shebangs require bounded
token parsing, and shell blocks embedded in YAML, TOML, JSON, Markdown, Make,
container, or CI files retain both their host-config provenance and their
bounded region. Here-documents and quoted strings are not recursively classified
as executable languages unless an explicit, separately qualified rule applies.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-shell` | POSIX and Bash shebangs, `/usr/bin/env` forms, executable and non-executable files, extensionless scripts, functions, assignments, pipelines, redirects, comments, LF and CRLF, final line without a newline, and exact byte/blob round trip |
| `shell-config-surfaces` | `.profile`, shell RC files, `*.env`-style non-secret fixture data, CI/task/container shell blocks, literal `source`/`.` targets, and nested quoting while preserving host-file and byte-range provenance |
| `shell-dialect-and-dynamic` | POSIX, Bash, and Zsh markers; arrays, process substitution, here-documents, command/arithmetic/parameter substitution, `eval`, computed includes, aliases, globs, traps, and dialect disagreement without expansion or execution |
| `shell-malformed-and-bounded` | invalid syntax, ambiguous or unsupported shebangs, unterminated constructs, oversized/deep input, embedded-block truncation, parser unavailability, and optional-tool failure while Tier 0/1 evidence remains available |

Fixtures must prove deterministic ordering and output, valid byte ranges,
bounded excerpts/diagnostics/fact counts, secret redaction, no network access,
no environment or profile loading, and no interpreter, command, task, CI, or
repository-code execution. Unsupported dialects, ambiguous shebangs, computed
includes or commands, embedded-language uncertainty, malformed syntax,
truncation, and optional-tool failures produce typed `capture_gap` or
`validation_error` rows.

## Nix Capture Plan

The default Nix lane is marker-first, static, offline, and snapshot-bound. It
captures Nix expressions, flakes, locks, and related configuration as inert
data without evaluating expressions, resolving flake references, consulting
registries or channels, instantiating derivations, entering a development
shell, or reading paths beyond the admitted snapshot.

### Coverage by tier

| Tier | Nix coverage | Required provenance or boundary |
|---|---|---|
| Tier 0 | exact bytes and metadata for `.nix`, `flake.nix`, `flake.lock`, `default.nix`, `shell.nix`, and admitted Nix config files | preserve source blob identity, byte ranges, permissions, symlink policy, newline style, and conventional file role |
| Tier 1 | extension and conventional-name detection plus flake, lockfile, shell, default-expression, and bounded Nix configuration markers | marker presence does not prove evaluation success, purity, system support, input availability, or a valid store path |
| Tier 2 | marker discovery by default; optional separately qualified parser may emit bounded expression, attribute, binding, literal import/path/URI, function, and syntax-diagnostic summaries | record parser/version and accepted Nix dialect; dynamic attributes, interpolation, path semantics, and parser recovery remain qualified syntax facts only |
| Tier 3 | normalized config/build marker, literal input/reference, flake-lock node/edge, declared output-key, literal import target, and selected non-secret Nix setting rows derived from admitted syntax or structured lock data | retain `follows`, indirect/path/git/archive references, `narHash`, revisions, systems, overlays, dynamic imports, interpolation, and unresolved local paths without fetching, normalizing, or evaluating them |
| Tier 4 | no default indexer; optional Nix analysis observations may be admitted only after separate qualification | gated tool rows use a separate namespace and record version, arguments, feature flags, environment policy, input snapshot, output digest, truncation, and failure status |
| Tier 5 | `nix eval`, flake metadata/update/lock, registry/channel lookup, store realization, build, develop/shell execution, import-from-derivation, and runtime behavior | excluded from the safe default path and admitted only through a separate unsafe approval |

`flake.lock` is parsed as structured captured data, not refreshed or treated as
proof that an input can be fetched. Relative and absolute path literals,
angle-bracket lookup, `builtins.getEnv`, `builtins.fetch*`,
import-from-derivation, interpolation, and other evaluation-dependent forms are
recorded as syntax markers or gaps. The default lane never runs `nix`,
`nix-instantiate`, `nix-shell`, or a command exposed by a flake or development
shell.

### Required fixtures and gaps

| Fixture | Required observations |
|---|---|
| `minimal-nix` | literals, lists, attribute sets, recursive bindings, functions, `let`/`with`/`inherit`, comments, empty source, LF and CRLF, final line without a newline, and exact byte/blob round trip |
| `nix-flake-and-lock` | flake inputs/outputs as syntax, representative locked/original nodes and `follows` edges, revisions and hashes, path/git/archive/indirect references, multiple systems, and malformed or unknown lockfile versions without update or fetch |
| `nix-config-surfaces` | `default.nix`, `shell.nix`, overlays/modules, literal and dynamic imports, NixOS/Home Manager-style option assignments as syntax, and admitted non-secret Nix settings while preserving file and byte-range provenance |
| `nix-evaluation-boundaries` | environment reads, fetch builtins, angle-bracket lookup, path interpolation, impure/current-system forms, derivations, import-from-derivation markers, infinite-recursion-shaped syntax, and unsupported experimental constructs without evaluation |
| `nix-malformed-and-bounded` | invalid syntax, invalid lock JSON, missing local references, oversized/deep expressions or lock graphs, fact truncation, parser unavailability, and optional-tool failure while Tier 0/1 evidence remains available |

Fixtures must prove deterministic ordering and output, valid byte ranges,
bounded excerpts/diagnostics/fact counts, secret redaction, no network access,
no reads outside admitted inputs, no writes to the Nix store or caches, and no
evaluation, fetch, build, shell, activation, or repository-code execution.
Unsupported dialect/features, dynamic attributes/imports, missing paths,
ambiguous configuration semantics, malformed expressions or locks, truncation,
and optional-tool failures produce typed `capture_gap` or `validation_error`
rows rather than claims of evaluability, reproducibility, or build success.

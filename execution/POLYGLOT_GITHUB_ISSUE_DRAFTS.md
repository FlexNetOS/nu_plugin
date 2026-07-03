# Polyglot Issue Drafts

These drafts mirror the required delivery set from issue 215. Use them directly
if live issue creation in FlexNetOS/nu_plugin remains blocked.

## CDB091 Research current polyglot parsing/indexing/tooling landscape

Mission: build the research ledger first, audit the current V1.1 and issue-212
state, and capture official sources for the polyglot planning lane.

Evidence to inspect:
- current repo truth surfaces listed in issue 215
- FlexNetOS/flexnetos_runner#212
- official docs for Tree-sitter, ast-grep, SCIP, CodeQL, and language-specific tooling

Files to create/update:
- docs/polyglot-import/README.md
- docs/polyglot-import/research-ledger.md
- docs/polyglot-import/open-questions.md
- docs/polyglot-import/github-issue-delivery-plan.md
- execution/POLYGLOT_TASK_GRAPH.csv
- execution/POLYGLOT_TASK_FILE_MAP.csv
- execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md

Acceptance criteria:
- research ledger records FACT, INFERENCE, QUESTION, GAP, BLOCKER
- current-state audit explains relationship to V1.1 and issue 212
- official sources are captured for the parser/indexer/tooling landscape

Validation:
- git diff --check
- Python CSV parse check

Safety constraints:
- no arbitrary language-to-Rust translation claims
- no vendored external code as evidence
- no package-manager or project-script execution by default

Dependencies: none

## CDB092 Design polyglot schema extension

Mission: design the schema extension without breaking or silently replacing Rust-first tables.
Evidence to inspect: schema, architecture, goal, security docs, core crate sources, and CDB091.
Files to create/update: docs/polyglot-import/polyglot-schema-extension.md, docs/polyglot-import/language-import-surface.md, docs/polyglot-import/whole-repo-import-architecture.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv.
Acceptance criteria: planned tables cover detection, package/lockfile, module graph, config/build, validation, and single-binary export groups; new rows link to source files, blobs, parser/tool versions, and proof rows.
Validation: git diff --check.
Safety constraints: no source-truth promotion without proof; no loss of bytes, symlinks, permissions, or binary policy.
Dependencies: CDB091.

## CDB093 Implement language detection and package marker inventory

Mission: define Tier 0 and Tier 1 whole-repo detection.
Evidence to inspect: commands, tests, security docs, CLI/plugin surfaces, and CDB091 research.
Files to create/update: docs/polyglot-import/language-import-surface.md, docs/polyglot-import/package-manager-and-lockfile-matrix.md, docs/polyglot-import/whole-repo-import-architecture.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: baseline languages are explicitly covered; package/lockfile coverage includes dependency provenance without install; candidate default-path crates are called out.
Validation: git diff --check; Python CSV parse check.
Safety constraints: no package-manager dependency installation; no project script execution; no hidden scan expansion outside the repo boundary.
Dependencies: CDB091.

## CDB094 Implement raw whole-repo byte/blob import fixtures

Mission: define the raw byte/blob fixture family before semantic layers are trusted.
Evidence to inspect: V1.1 goal/acceptance/security/test docs and relevant bidirectional lossless/symlink/out-dir tasks.
Files to create/update: docs/polyglot-import/proof-and-round-trip-gates.md, docs/polyglot-import/security-and-execution-policy.md, docs/polyglot-import/whole-repo-import-architecture.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv.
Acceptance criteria: fixture families cover binary assets, symlinks, credential-like files, vendor-generated trees, and mixed-language trees; P2/P5/P6/P9 are explicitly fed.
Validation: git diff --check.
Safety constraints: no raw credential dumps; no raw source over MCP by default; no overwrite/materialization from this layer.
Dependencies: CDB091.

## CDB095 Add Tree-sitter/ast-grep parser-backed summary prototype

Mission: define Tier 2 parser-backed summary planning and the boundary to optional Tier 4 indexers.
Evidence to inspect: Tree-sitter docs, ast-grep docs, architecture/schema/commands docs, and existing Rust static capture surfaces.
Files to create/update: docs/polyglot-import/parser-and-indexer-tooling-matrix.md, docs/polyglot-import/language-import-surface.md, docs/polyglot-import/whole-repo-import-architecture.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv.
Acceptance criteria: Tree-sitter and ast-grep are evaluated from official docs; Tier 2 CST/AST summaries are separated from Tier 3 graph facts; optional indexers such as SCIP and CodeQL remain gated.
Validation: git diff --check.
Safety constraints: no mandatory external CLI in default tests; no unsupported semantic claims.
Dependencies: CDB091.

## CDB096 Add Python import surface fixture plan

Mission: define the Python capture plan.
Evidence to inspect: Ruff, LibCST, basedpyright/pyright, uv lockfile docs, test/security docs.
Files to create/update: docs/polyglot-import/language-import-surface.md, docs/polyglot-import/parser-and-indexer-tooling-matrix.md, docs/polyglot-import/package-manager-and-lockfile-matrix.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: Python coverage includes package/config/lockfile markers, parser/indexer candidates, capture gaps, and fixtures such as minimal-python.
Validation: git diff --check.
Safety constraints: no pip/uv install by default; no code execution in static capture.
Dependencies: CDB093.

## CDB097 Add Ruby import surface fixture plan

Mission: define the Ruby capture plan.
Evidence to inspect: Prism, Bundler/Gemfile.lock, scip-ruby, test/security docs.
Files to create/update: docs/polyglot-import/language-import-surface.md, docs/polyglot-import/parser-and-indexer-tooling-matrix.md, docs/polyglot-import/package-manager-and-lockfile-matrix.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: Ruby coverage includes parser/indexer candidates, lockfile provenance, risks, and fixtures such as minimal-ruby.
Validation: git diff --check.
Safety constraints: no bundle install or app boot by default.
Dependencies: CDB093.

## CDB098 Add TypeScript/JavaScript import surface fixture plan

Mission: define the JS/TS capture plan.
Evidence to inspect: Oxc, SWC, Biome, TypeScript compiler API, lockfile docs, and command/test/security docs.
Files to create/update: docs/polyglot-import/language-import-surface.md, docs/polyglot-import/parser-and-indexer-tooling-matrix.md, docs/polyglot-import/package-manager-and-lockfile-matrix.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: JS/TS/JSX/TSX coverage includes parser/compiler candidates, workspace/lockfile metadata, bounded-view constraints, and fixtures such as minimal-typescript.
Validation: git diff --check.
Safety constraints: no package-manager install or project build by default; no overclaim of lossless language-to-Rust translation.
Dependencies: CDB093.

## CDB099 Add Go/Shell/Nix/config import surface fixture plan

Mission: define baseline capture for Go, Shell, Nix, and generic config/doc surfaces, plus stretch-language notes.
Evidence to inspect: go/parser, metadata docs, test/security/integration docs, and CDB091 research.
Files to create/update: docs/polyglot-import/language-import-surface.md, docs/polyglot-import/parser-and-indexer-tooling-matrix.md, docs/polyglot-import/package-manager-and-lockfile-matrix.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: Go/Shell/Nix/config/doc coverage is explicit and stretch languages are documented without inflating implementation scope.
Validation: git diff --check.
Safety constraints: no shell execution; no Nix build/eval by default.
Dependencies: CDB093.

## CDB100 Design single-binary Rust export crate generator

Mission: define the generated crate layout and binary-export contract.
Evidence to inspect: current architecture/schema/security docs, issue 212, and CDB091 research.
Files to create/update: docs/polyglot-import/single-binary-rust-crate-export.md, docs/polyglot-import/whole-repo-import-architecture.md, docs/polyglot-import/proof-and-round-trip-gates.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv.
Acceptance criteria: generated layout, artifact boundaries, checksum/manifest rules, offline-build feasibility, and redaction policy are explicit.
Validation: git diff --check.
Safety constraints: generated crates/files are artifacts; no source overwrite by default; no credential embedding.
Dependencies: CDB091.

## CDB101 Prototype generated export crate verify/list/materialize commands

Mission: define the artifact command surface.
Evidence to inspect: commands/bridge/security docs, CLI/MCP sources, and CDB100.
Files to create/update: docs/polyglot-import/single-binary-rust-crate-export.md, docs/polyglot-import/proof-and-round-trip-gates.md, docs/polyglot-import/security-and-execution-policy.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: verify/list/schema/summary/export/materialize/license report are explicitly planned with bounded behavior.
Validation: git diff --check.
Safety constraints: no raw-source-over-MCP escape hatch; no hidden mutation shortcuts.
Dependencies: CDB100.

## CDB102 Add proof gates for DB import -> crate export -> materialize round trip

Mission: turn the P0-P11 gate list into a concrete proof plan.
Evidence to inspect: acceptance/readiness/stop/security/test docs and CDB094, CDB100, CDB101.
Files to create/update: docs/polyglot-import/proof-and-round-trip-gates.md, docs/polyglot-import/README.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: every gate has scope, artifacts, negative cases, and authority boundaries.
Validation: git diff --check.
Safety constraints: no weak or indirect evidence qualifies as completion.
Dependencies: CDB094, CDB100, CDB101.

## CDB103 Add bounded Nu/CLI/MCP polyglot views

Mission: define bounded polyglot query/view surfaces.
Evidence to inspect: commands/bridge/integration/security docs and CDB095.
Files to create/update: docs/polyglot-import/whole-repo-import-architecture.md, docs/polyglot-import/security-and-execution-policy.md, docs/polyglot-import/README.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: bounded table-shaped views are defined and MCP remains read-only with no raw blob/source path by default.
Validation: git diff --check.
Safety constraints: no unbounded dump commands; no mutation verbs in default flows.
Dependencies: CDB095.

## CDB104 Add security/no-script-execution/no-credential-leak gates

Mission: translate the hard boundaries into policy and proof requirements.
Evidence to inspect: stop/acceptance/security/integration docs and CDB071, CDB083, CDB102.
Files to create/update: docs/polyglot-import/security-and-execution-policy.md, docs/polyglot-import/proof-and-round-trip-gates.md, execution/POLYGLOT_TASK_GRAPH.csv, execution/POLYGLOT_TASK_FILE_MAP.csv, execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md.
Acceptance criteria: all hard boundaries are preserved as design policy and future validation expectations.
Validation: git diff --check.
Safety constraints: task itself must not trigger unsafe execution.
Dependencies: CDB102, CDB103.

## CDB105 Release/readiness gate for polyglot import V1.2 planning

Mission: seal the planning package, issue delivery, and readiness narrative.
Evidence to inspect: all deliverables from CDB091-CDB104 plus repo truth surfaces and manifest/checksum files.
Files to create/update: execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md, NAVIGATION.md, NAVIGATION.json, DOC_GRAPH.md, HANDOFF.md, ACCEPTANCE.md, READINESS_GATE.md, STOP_CONDITIONS.md, execution/WORKLOG.md, execution/COMMAND_LEDGER.csv, manifests/PACK_MANIFEST.json, manifests/CHECKSUMS.sha256, and optional PRD addendum.
Acceptance criteria: issue delivery exists, truth surfaces are updated, V1.1 remains baseline, and reseal expectations are explicit.
Validation: git diff --check; Python CSV parse check; cargo gates only if code changes.
Safety constraints: do not silently supersede V1.1; do not present research deliverables as completed implementation.
Dependencies: CDB091 through CDB104.

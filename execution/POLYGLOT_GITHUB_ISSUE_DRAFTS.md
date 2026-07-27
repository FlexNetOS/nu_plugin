# Polyglot GitHub Issue Drafts

These are ready-to-post issue bodies for the V1.2 polyglot **planning** lane.
They do not claim that polyglot import, generated export crates, or round-trip
execution have shipped. The issue IDs mirror `POLYGLOT_TASK_GRAPH.csv`; preserve
the dependency order when posting.

Direct issue creation in `FlexNetOS/nu_plugin` was unavailable to the planning
run, so this checked-in file is the authoritative delivery artifact until a
maintainer posts the issues. When posting, add the resulting GitHub URL beside
the matching heading without changing the issue's scope or acceptance gates.

Suggested labels for every issue: `codedb`, `v1.2`, `polyglot`, `planning`.

---

## CDB091 — Research the current polyglot parsing and indexing landscape

**Depends on:** none
**Suggested additional labels:** `research`, `architecture`

### Summary

Establish the evidence base for extending CodeDB from Rust-crate-first capture
to whole-repository polyglot import. Audit the current CodeDB surface and record
official-source findings for parsers, indexers, language tooling, package
managers, and provenance formats.

### Deliverables

- `docs/polyglot-import/research-ledger.md`
- `execution/POLYGLOT_TASK_GRAPH.csv`
- `execution/POLYGLOT_TASK_FILE_MAP.csv`

### Acceptance criteria

- [ ] Every external claim is typed and linked to an official source.
- [ ] Current CodeDB capabilities are separated from proposed V1.2 work.
- [ ] Parser/indexer candidates and material capability gaps are recorded.
- [ ] The planning graph and file map parse as CSV and contain CDB091-CDB105.
- [ ] Unresolved findings are recorded as questions or gaps, not facts.

### Safety boundaries

- Do not claim arbitrary language-to-Rust translation.
- Do not vendor external code as research evidence.
- Do not run package-manager installs or project scripts.

### Validation

- `git diff --check`
- Parse both polyglot CSV files with Python's `csv` module.

---

## CDB092 — Design the polyglot schema extension

**Depends on:** CDB091
**Suggested additional labels:** `schema`, `architecture`

### Summary

Design additive tables and relationships for language, package, dependency,
parser, symbol, proof, and capture-gap facts while preserving the existing
Rust-first schema and content-addressed source/blob provenance.

### Deliverable

- `docs/polyglot-import/polyglot-schema-extension.md`

### Acceptance criteria

- [ ] Every proposed row traces to source files/blobs and parser/tool versions.
- [ ] Raw-byte identity, provenance, and capture gaps remain first-class.
- [ ] The extension is additive and does not break existing Rust-first tables.
- [ ] Observed facts, inferred facts, and unsupported facts are distinguishable.
- [ ] Authority promotion requires the round-trip proof gates from CDB102.

### Safety boundaries

- Do not promote generated or parsed rows to source truth without proof.
- Do not normalize away bytes, permissions, symlinks, encodings, or line endings.

### Validation

- `git diff --check`

---

## CDB093 — Define language detection and package-marker inventory

**Depends on:** CDB091
**Suggested additional labels:** `discovery`, `packages`

### Summary

Specify deterministic, read-only repository discovery for language candidates,
package/workspace markers, manifests, lockfiles, configuration files, and
provenance inputs. Define baseline Tier 0 raw capture and Tier 1 detection.

### Deliverables

- `docs/polyglot-import/language-import-surface.md`
- `docs/polyglot-import/package-manager-and-lockfile-matrix.md`

### Acceptance criteria

- [ ] Tier 0 raw-byte capture and Tier 1 marker detection are defined.
- [ ] Detection is deterministic and records why each language/package matched.
- [ ] Ambiguous, generated, vendored, and ignored inputs have explicit handling.
- [ ] Package/workspace manifests and lockfiles retain path and checksum provenance.
- [ ] Detection requires neither dependency installation nor project execution.

### Safety boundaries

- Do not install dependencies or run package/project scripts.
- Do not expand beyond the selected repository boundary implicitly.

### Validation

- `git diff --check`
- Parse both polyglot CSV files with Python's `csv` module.

---

## CDB094 — Plan raw whole-repository byte/blob import fixtures

**Depends on:** CDB091
**Suggested additional labels:** `fixtures`, `round-trip`

### Summary

Define fixture families that prove byte-exact capture of text, binary assets,
permissions, symlinks, unusual encodings, line endings, empty files, and nested
repository layouts before parser-backed enrichment is attempted.

### Deliverables

- `docs/polyglot-import/proof-and-round-trip-gates.md`
- `docs/polyglot-import/whole-repo-import-architecture.md`

### Acceptance criteria

- [ ] Fixture families cover byte, metadata, symlink, binary, and boundary cases.
- [ ] Expected hashes and provenance are defined for every fixture.
- [ ] Secret-like fixtures use inert synthetic values only.
- [ ] Fixtures feed proof gates P2, P5, P6, and P9.
- [ ] Materialization and overwrite behavior remain outside raw capture.

### Safety boundaries

- Do not ingest real credential dumps.
- Do not expose raw source through MCP by default.
- Do not mutate or overwrite the source repository.

### Validation

- `git diff --check`

---

## CDB095 — Plan a parser-backed bounded-summary prototype

**Depends on:** CDB091
**Suggested additional labels:** `parsing`, `prototype`

### Summary

Compare Tree-sitter, ast-grep, and language-specific tooling, then define a
bounded prototype that enriches raw capture with parser-versioned summaries
without making unsupported compiler-semantic claims.

### Deliverable

- `docs/polyglot-import/parser-and-indexer-tooling-matrix.md`

### Acceptance criteria

- [ ] Candidate tools are compared by language coverage, license, runtime, and fidelity.
- [ ] Tier 2 syntax summaries and optional Tier 4 index data have explicit boundaries.
- [ ] Parse errors and unsupported constructs become structured gaps.
- [ ] Summary size and query result bounds are specified.
- [ ] Default tests do not require an external parser CLI.

### Safety boundaries

- Do not make compiler-equivalence or complete-semantic claims.
- Do not make external CLIs mandatory for the default path.

### Validation

- `git diff --check`

---

## CDB096 — Define the Python import-surface fixture plan

**Depends on:** CDB093
**Suggested additional labels:** `python`, `fixtures`

### Summary

Specify Python source, package, workspace, dependency, type, configuration, and
tooling fixtures, including bounded optional lanes for Ruff, LibCST, and
basedpyright-derived observations.

### Deliverables

- Python sections in `language-import-surface.md`
- Python rows in `package-manager-and-lockfile-matrix.md`
- Python candidates in `parser-and-indexer-tooling-matrix.md`

### Acceptance criteria

- [ ] `.py`/`.pyi`, `pyproject.toml`, lockfiles, and common workspace layouts are covered.
- [ ] Imports, package metadata, and type/config observations retain provenance.
- [ ] Tool-derived facts identify tool name and version.
- [ ] Missing interpreters or tools produce gaps rather than failed default capture.
- [ ] Static capture requires no environment creation or dependency install.

### Safety boundaries

- Do not run `pip`, `uv`, setup hooks, or imported project code by default.

### Validation

- `git diff --check`

---

## CDB097 — Define the Ruby import-surface fixture plan

**Depends on:** CDB093
**Suggested additional labels:** `ruby`, `fixtures`

### Summary

Specify Ruby source, gem, workspace, dependency, configuration, and lockfile
fixtures, including Prism syntax observations and Bundler provenance.

### Deliverables

- Ruby sections in `language-import-surface.md`
- Ruby rows in `package-manager-and-lockfile-matrix.md`
- Ruby candidates in `parser-and-indexer-tooling-matrix.md`

### Acceptance criteria

- [ ] `.rb`, gemspec, `Gemfile`, and `Gemfile.lock` inputs are covered.
- [ ] Multiple platforms/sources and lockfile provenance are retained.
- [ ] Prism-derived summaries are parser-versioned and bounded.
- [ ] Unsupported DSL or runtime behavior becomes a gap.
- [ ] Default capture does not boot an application or resolve dependencies.

### Safety boundaries

- Do not run `bundle install`, gem hooks, Rails boot, or project code by default.

### Validation

- `git diff --check`

---

## CDB098 — Define the TypeScript and JavaScript import-surface fixture plan

**Depends on:** CDB093
**Suggested additional labels:** `javascript`, `typescript`, `fixtures`

### Summary

Specify JavaScript/TypeScript source, module, workspace, package-manager,
lockfile, and configuration fixtures, with bounded candidate lanes for Oxc,
SWC, Biome, and the TypeScript compiler API.

### Deliverables

- JS/TS sections in `language-import-surface.md`
- JS/TS rows in `package-manager-and-lockfile-matrix.md`
- JS/TS candidates in `parser-and-indexer-tooling-matrix.md`

### Acceptance criteria

- [ ] JS, JSX, TS, TSX, module modes, and monorepo layouts are covered.
- [ ] npm, pnpm, Yarn, and Bun markers/lockfiles retain exact provenance.
- [ ] Parser/compiler-derived summaries identify tool and version.
- [ ] Generated/vendor directory policy is explicit.
- [ ] Default capture runs no install, lifecycle hook, build, or bundler.

### Safety boundaries

- Do not install packages or run project builds/scripts by default.
- Do not claim lossless JavaScript/TypeScript-to-Rust translation.

### Validation

- `git diff --check`

---

## CDB099 — Define Go, Shell, Nix, and configuration import fixtures

**Depends on:** CDB093
**Suggested additional labels:** `go`, `shell`, `nix`, `fixtures`

### Summary

Specify baseline fixtures for Go, shell languages, Nix, and common structured
configuration, and record stretch-language notes for Java/Kotlin, C/C++,
C#, PHP, Swift, and Lua.

### Deliverable

- Go/Shell/Nix/config sections in `language-import-surface.md`

### Acceptance criteria

- [ ] Go modules/workspaces and checksum files are covered.
- [ ] Shell shebangs/dialects are detected without executing scripts.
- [ ] Nix files, flakes, and locks are captured without evaluation/build.
- [ ] JSON, YAML, TOML, XML, INI, and similar config retain raw bytes and parse status.
- [ ] Stretch languages are explicitly non-baseline and produce honest gaps.

### Safety boundaries

- Do not execute shell files.
- Do not run Nix evaluation/build or language package installation by default.

### Validation

- `git diff --check`

---

## CDB100 — Design the generated single-binary Rust export crate

**Depends on:** CDB091
**Suggested additional labels:** `export`, `rust`

### Summary

Design a generated Rust crate that embeds a verified repository snapshot and
builds one binary for bounded inspection, verification, export, and controlled
materialization. Keep crate generation distinct from language translation.

### Deliverable

- `docs/polyglot-import/single-binary-rust-crate-export.md`

### Acceptance criteria

- [ ] Generated crate layout, manifest, embedded indexes, and blob strategy are defined.
- [ ] Artifact provenance binds source snapshot, schema, and generator versions.
- [ ] Deterministic generation and reproducible verification inputs are specified.
- [ ] Raw export is non-default and policy-gated.
- [ ] Materialization never overwrites source by default.

### Safety boundaries

- Do not embed credentials or unredacted secret material.
- Do not overwrite source files or imply semantic language-to-Rust conversion.

### Validation

- `git diff --check`

---

## CDB101 — Specify generated export-crate commands

**Depends on:** CDB100
**Suggested additional labels:** `cli`, `export`

### Summary

Define the bounded command surface for a generated export binary: `verify`,
`list`, `schema`, `summary`, `export`, `materialize`, and `license-report`.

### Deliverables

- Command contract in `single-binary-rust-crate-export.md`
- Related proof/security entries for the command surface

### Acceptance criteria

- [ ] Every command has stable inputs, bounded outputs, exit behavior, and error shape.
- [ ] `verify` checks embedded hashes and artifact provenance.
- [ ] `list`, `schema`, and `summary` do not emit raw source by default.
- [ ] `export` and `materialize` require explicit destinations and collision policy.
- [ ] License/provenance reporting is available without project execution.

### Safety boundaries

- Do not add a raw-source-over-MCP escape hatch.
- Do not hide mutation or overwrite behind read/query commands.

### Validation

- `git diff --check`

---

## CDB102 — Define DB-to-crate-to-materialization proof gates

**Depends on:** CDB094, CDB100, CDB101
**Suggested additional labels:** `verification`, `round-trip`

### Summary

Define direct proof from repository import through database/blob storage,
generated crate export, binary build, verification, materialization, and
byte/metadata comparison.

### Deliverable

- `docs/polyglot-import/proof-and-round-trip-gates.md`

### Acceptance criteria

- [ ] Gates P0-P11 have explicit inputs, commands, expected evidence, and failure states.
- [ ] The proof chain binds source snapshot, database state, generated crate, and binary.
- [ ] Materialized bytes and supported metadata compare directly with the source snapshot.
- [ ] Secret, boundary, bounded-output, and no-script guarantees are tested.
- [ ] Missing or indirect evidence cannot satisfy a gate.

### Safety boundaries

- Do not accept documentation, mocks, or inferred status as runtime proof.
- Record missing evidence as a gap, question, or blocker.

### Validation

- `git diff --check`
- Parse both polyglot CSV files with Python's `csv` module.

---

## CDB103 — Define bounded Nu, CLI, and MCP polyglot views

**Depends on:** CDB095
**Suggested additional labels:** `nushell`, `mcp`, `cli`

### Summary

Specify consistent bounded views for repository, language, package, dependency,
parser, symbol-summary, proof, and capture-gap facts across Nu, CLI, and MCP.

### Deliverables

- View contracts in `whole-repo-import-architecture.md`
- Boundary rules in `security-and-execution-policy.md`

### Acceptance criteria

- [ ] All collection endpoints require limits and support deterministic pagination.
- [ ] Nu output is table-shaped and CLI/MCP shapes are documented.
- [ ] Raw blobs/source are unavailable through MCP by default.
- [ ] Provenance and capture gaps are visible with every derived fact.
- [ ] Query/view commands expose no default mutation verbs.

### Safety boundaries

- Do not add unbounded dump endpoints.
- Do not expose raw source or mutation through default MCP capability.

### Validation

- `git diff --check`

---

## CDB104 — Define no-script-execution and credential-leak gates

**Depends on:** CDB102, CDB103
**Suggested additional labels:** `security`, `verification`

### Summary

Define enforceable gates for repository boundaries, secret hygiene, inert
default capture, bounded output, generated-artifact review, and explicitly
approved escalation paths.

### Deliverables

- `docs/polyglot-import/security-and-execution-policy.md`
- Security gates in `proof-and-round-trip-gates.md`

### Acceptance criteria

- [ ] Default import executes no project, package-manager, build, or language hooks.
- [ ] Synthetic canaries prove credentials do not enter logs, manifests, or views.
- [ ] Raw blob access and any unsafe parser/runtime path require explicit policy.
- [ ] Boundary escapes, symlink escapes, and output-limit failures stop safely.
- [ ] Missing evidence remains a gap/blocker and never relaxes policy.

### Safety boundaries

- This planning issue must not itself trigger unsafe execution.
- Do not use real secrets in fixtures or proof logs.

### Validation

- `git diff --check`

---

## CDB105 — Seal the V1.2 polyglot planning package

**Depends on:** CDB091, CDB092, CDB093, CDB094, CDB095, CDB096, CDB097,
CDB098, CDB099, CDB100, CDB101, CDB102, CDB103, CDB104
**Suggested additional labels:** `release-planning`, `documentation`

### Summary

Complete the issue-delivery handoff and align the package truth surfaces so a
future implementation wave can start from one deterministic planning rail
without confusing it with the shipped V1.1 baseline.

### Deliverables

- `execution/POLYGLOT_GITHUB_ISSUE_DRAFTS.md`
- `NAVIGATION.md` and `NAVIGATION.json`
- `DOC_GRAPH.md`
- `HANDOFF.md`
- `ACCEPTANCE.md`
- `READINESS_GATE.md`
- `STOP_CONDITIONS.md`

### Acceptance criteria

- [ ] Ready-to-post drafts exist for CDB091-CDB105 with dependencies and safety gates.
- [ ] Human and machine navigation include the V1.2 planning lane and draft artifact.
- [ ] The document graph identifies the authoritative planning graph/file map.
- [ ] Handoff, acceptance, readiness, and stop surfaces agree on planning-only status.
- [ ] No draft or truth surface claims completed polyglot implementation or release proof.
- [ ] JSON parses, local links resolve, and `git diff --check` passes.

### Safety boundaries

- Do not silently supersede V1.1 or the V1.1 authoritative task graph.
- Do not present research/planning deliverables as implemented runtime capability.
- Do not post issues automatically without repository write authorization.

### Validation

- `git diff --check`
- Parse `NAVIGATION.json` and both polyglot CSV files.
- Run cargo gates only if code changes are introduced.

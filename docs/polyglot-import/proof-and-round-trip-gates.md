# Proof And Round-Trip Gates

This document is the acceptance contract for the polyglot-import planning
lane. A gate is `pass` only when its required evidence is present,
machine-readable where specified, bound to the same source snapshot and policy,
and independently verifiable. `Not run`, `unsupported`, `partial`, and
`inconclusive` are not passing results.

## Gate Set

| Gate | Requirement | Required evidence | Detailed contract |
|---|---|---|---|
| P0 | Research ledger complete | official-source research package | [P0](#p0-research-ledger-gate), [research ledger](research-ledger.md) |
| P1 | Current-state audit complete | live repository truth surfaces | [P1](#p1-current-state-audit-gate), [architecture](whole-repo-import-architecture.md) |
| P2 | Raw repository byte capture fixture passes | fixture and canonical manifest evidence | [P2](#p2-raw-byte-fixture-gate), [fixture families](whole-repo-import-architecture.md#fixture-families) |
| P3 | Language detection fixture passes | versioned detector output | [P3](#p3-language-detection-gate), [language surface](language-import-surface.md) |
| P4 | Parser-backed summary fixture passes for baseline languages | parser-run and bounded-summary evidence | [P4](#p4-parser-backed-summary-gate), [tooling matrix](parser-and-indexer-tooling-matrix.md) |
| P5 | Package/lockfile matrix fixture passes | inert metadata and lockfile evidence | [P5](#p5-package-and-lockfile-gate), [package matrix](package-manager-and-lockfile-matrix.md) |
| P6 | redb import/export manifest verifies | CodeDB manifest evidence | [P6](#p6-codedb-manifest-gate), [schema extension](polyglot-schema-extension.md) |
| P7 | Generated Rust crate builds | reproducible build and test evidence | [P7](#p7-generated-crate-build-gate), [crate export](single-binary-rust-crate-export.md) |
| P8 | Single binary verifies embedded pack | embedded-pack verification evidence | [P8](#p8-single-binary-verification-gate), [command contract](single-binary-rust-crate-export.md#command-contract) |
| P9 | Materialization proof matches allowed original bytes/metadata | independent round-trip proof | [P9](#p9-materialization-gate), [security policy](security-and-execution-policy.md) |
| P10 | Bounded Nu/CLI/MCP views pass | view, limit, redaction, and policy evidence | [P10](#p10-bounded-view-gate), [security policy](security-and-execution-policy.md) |
| P11 | GitHub issue backlog or issue drafts exist | dependency-ordered delivery evidence | [P11](#p11-delivery-backlog-gate), [delivery plan](github-issue-delivery-plan.md) |

## Gate Interpretation

- A gate is satisfied only by direct evidence matching its scope. A downstream
  pass does not imply that an upstream gate passed.
- Every receipt identifies the gate, fixture or corpus, source snapshot digest,
  capture-policy digest, schema version, tool versions, result, and the digests
  of its inputs and outputs.
- Evidence generated from a different snapshot, policy, schema, or tool
  configuration is not interchangeable. Reuse requires an exact digest match.
- Ordering, IDs, manifests, and receipts are deterministic. Repeating a gate
  with identical inputs must produce the same semantic result; permitted
  timestamps or host diagnostics remain outside the compared payload.
- Weakly consistent evidence, screenshots, and narrative-only claims are not
  proof when a machine-readable artifact is required.
- A skipped, truncated, timed-out, or unsupported case produces a typed
  `capture_gap` or `validation_error` and fails any gate that requires that
  case.
- Negative-path checks are required where a safety boundary applies. A
  fail-open result fails the gate.
- P0 and P1 establish the qualified plan and baseline. P2-P5 prove capture and
  enrichment. P6-P9 form the required round trip. P10 proves bounded exposure,
  and P11 makes every remaining implementation or evidence gap deliverable.

## P0 Research Ledger Gate

P0 passes when [the research ledger](research-ledger.md) covers every external
claim used by the language, parser/indexer, package-manager, schema, export, and
security plans. Each entry must identify an official or primary source, access
date, claim type, exact supported claim, affected decision, and any
qualification or unresolved conflict.

The evidence bundle contains a canonical ledger digest plus a link check or
recorded immutable source reference. Derived recommendations must be
distinguished from source facts. A missing source, an unsupported capability
claim, reliance on a search-result snippet, or an unresolved conflict presented
as settled fails P0. Explicit unknowns are acceptable only when recorded in
[open questions](open-questions.md) and routed to a P11 delivery item.

## P1 Current-State Audit Gate

P1 passes when a revision-bound audit records what the live repository
actually implements before the extension: workspace/crate inventory, V1.1
schema and identity rules, redb storage and export paths, Nu/CLI/MCP surfaces,
proof machinery, safety controls, and existing tests or fixtures. The audit
must separate implemented behavior from proposals in this planning package.

Evidence records the audited commit, relevant file digests, commands or
inspection method, discovered gaps, and the compatibility constraints carried
into the [schema extension](polyglot-schema-extension.md#1-compatibility-contract)
and [whole-repository architecture](whole-repo-import-architecture.md).
Auditing only documentation, silently assuming a planned crate exists, or
changing the Rust-first authority boundary without an explicit decision fails
P1.

## P2 Raw-Byte Fixture Gate

P2 runs every family in
[the fixture-family contract](whole-repo-import-architecture.md#fixture-families)
from an immutable fixture manifest. Before import, the harness independently
reads the fixture tree and records, in canonical relative-path order:

- entry kind (`file`, `directory`, or `symlink`);
- raw byte length and SHA-256 for every regular file;
- executable-bit state where the platform exposes it;
- symlink link text as bytes or an explicit platform limitation; and
- the fixture-manifest digest and capture-policy digest.

The imported snapshot must contain one path reference for every admitted
regular file and a content digest over exactly the original bytes. Blob
deduplication is allowed only when byte length and digest are identical; it
must not collapse distinct paths. P2 fails on a missing or extra admitted
entry, decoding or newline conversion, BOM removal, truncation, symlink
following, unstable ordering, an unexplained policy omission, or any mismatch
between independently computed and stored facts.

Two fresh imports of the same fixture and policy must produce the same
canonical path/blob manifest. The source fixture tree must have the same
manifest before and after both imports. Tier 1-5 failures must not mutate or
invalidate passing Tier 0 evidence.

## P3 Language Detection Gate

P3 runs the Tier 1 cases declared by the
[language import surface](language-import-surface.md#import-tiers), including
extensions, conventional filenames, shebangs, ambiguous files, extensionless
files, binary inputs, and conflicting markers. Each result references the
exact `source_file_id` and source-blob digest plus detector implementation
version, rule-set digest, matched rule, language kind, and enumerated
confidence.

Expected exact detections must match. Ambiguous or unsupported inputs must
remain explicit gaps; conflicting exact detections must become validation
errors. The detector may inspect bounded captured bytes but may not execute a
file, invoke a project toolchain, install dependencies, or alter Tier 0 data.
Two runs over the same snapshot and rules must produce identical,
canonically ordered rows. Invented certainty, unversioned rules, dropped
unknowns, or a detector/parser blob mismatch fails P3.

## P4 Parser-Backed Summary Gate

P4 exercises at least the promoted baseline-language adapters specified by the
[language plans](language-import-surface.md#baseline-languages) and qualified
by the [parser/indexer tooling matrix](parser-and-indexer-tooling-matrix.md).
For each required valid, malformed, oversized, and unsupported fixture, the
evidence binds the parser and grammar/configuration versions to the exact
source blob and records parse mode, status, diagnostics digest, summary digest,
limits, truncation, and elapsed-resource policy.

Valid fixtures must emit the expected bounded syntax/module/declaration/import
facts with valid source spans and deterministic ordering. Malformed input,
parser absence, timeout, crash, recovery ambiguity, or limit exhaustion must
produce a typed gap/error while leaving P2/P3 facts available. Parser summaries
never replace source bytes or claim semantic resolution they did not perform.
Executing repository code, loading project plugins, silently changing dialect,
or accepting unreported truncation fails P4.

## P5 Package And Lockfile Gate

P5 runs every promoted manifest, workspace, package marker, and lockfile family
in the [package-manager and lockfile matrix](package-manager-and-lockfile-matrix.md).
The expected rows cover manager/root detection, package membership, declared
dependencies, locked packages, source/integrity fields when present, unresolved
edges, malformed inputs, and unsupported format versions. Every row references
the exact manifest or lockfile blob and parser/detector version.

Tests are static and offline: they must not resolve, refresh, normalize, or
rewrite a lockfile; install dependencies; execute manifest code; or consult
mutable registries. Repeated runs must yield the same canonical rows. Missing
declared entries, guessed resolutions, loss of target/optional/source
qualifiers, treating a dynamic manifest as executed truth, or mutation of an
input file fails P5. Cargo projections must retain links to authoritative
Rust-specialized rows as required by
[the compatibility contract](polyglot-schema-extension.md#1-compatibility-contract).

## Required Round Trip

```text
repo input (P2-P5)
  -> import to CodeDB
  -> verify DB/export manifest (P6)
  -> generate and build export crate (P7)
  -> verify embedded pack with the single binary (P8)
  -> materialize approved output
  -> independently compare bytes + metadata + policy (P9)
```

P6-P9 must cite the same source snapshot and capture-policy digest. Each stage
verifies its input before producing output and records the digest consumed and
produced, forming an unbroken proof chain.

## P6 CodeDB Manifest Gate

P6 opens the completed redb snapshot through the supported reader and verifies
schema/version compatibility, table registry, row-envelope invariants,
referential integrity, content-addressed blob length/digests, path uniqueness,
capture gaps/errors, and deterministic table/row counts. The export manifest
must include all required V1.1 and
[V1.2 registry tables](polyglot-schema-extension.md#3-table-registry), including
empty tables, and bind each exported artifact to the snapshot, source, policy,
and schema digests.

A second export from the same verified snapshot must have the same canonical
manifest and artifact digests. Unknown incompatible schema versions, missing
tables, dangling references, digest/count mismatches, non-canonical ordering,
unreported gaps, or an export that bypasses database verification fails P6.
No database fact is promoted to source authority by this gate alone.

## P7 Generated Crate Build Gate

P7 generates the layout and command surface defined by
[the single-binary crate contract](single-binary-rust-crate-export.md) from the
P6 manifest. Evidence includes generator version/configuration, generated-tree
manifest, locked dependency graph, offline-feasibility result, compiler/target
identity, build command, build log digest, test results, and binary digest.

The generated crate must compile with locked dependencies and its verification
and materialization tests must pass. Repeating generation from identical input
must reproduce the generated semantic content; any documented platform-specific
binary variance must be isolated and qualified. Undeclared network access,
dependency refresh, build-script execution outside the approved policy,
embedding unlisted bytes, a dirty generated tree, warnings designated fatal by
the crate policy, or skipped required tests fails P7.

## P8 Single-Binary Verification Gate

P8 invokes the built binary's
[`verify` command](single-binary-rust-crate-export.md#command-contract) in a
clean, offline environment. Before any query, export, or materialization, the
binary must verify the embedded manifest, compressed pack, checksums, schema
version, entry lengths, blob digests, license inventory, redaction policy, and
their binding to the P6 snapshot and P7 binary.

Evidence includes the binary digest, embedded-pack digest, canonical
verification receipt, and negative tests for a modified manifest, pack,
checksum, truncated payload, unknown required schema, and decompression limit.
Every corruption case must fail closed before publication and must not leave
output. Verification based only on an outer archive checksum, post-write
verification, unbounded decompression, or acceptance of an unlisted embedded
entry fails P8.

## P9 Materialization Gate

P9 materializes each admitted fixture into a new, empty output root and builds
an observed manifest without using the import or export manifest as its source
of truth. For every admitted regular file, relative path, entry kind, byte
length, SHA-256, and executable bit must equal the pre-import fixture manifest.
Empty files, invalid UTF-8, BOMs, mixed newline styles, NUL-containing blobs,
multi-chunk blobs, duplicate-content paths, and extensionless files are all
compared as raw bytes.

For symlinks, the observed entry must remain a symlink with identical link
text on supported platforms. A declared metadata-only platform result is
acceptable only when the manifest records that limitation; copying target
bytes as a regular file is never equivalent. Empty directories are compared
when the export format declares directory preservation.

The gate also requires:

- no undeclared output and no write outside the selected output root;
- deterministic refusal before publication for escaping links, refused
  credential-like bytes, digest mismatch, or an occupied destination;
- rollback evidence showing no partial tree after a publication failure;
- unchanged source-fixture and CodeDB snapshot manifests; and
- a machine-readable receipt binding fixture, policy, snapshot, export pack,
  materialized tree, and comparison-result digests.

Higher-tier parser, language, package, and symbol rows are not accepted as
substitutes for P2 or P9 byte evidence. The materializer must also satisfy the
write, overwrite, symlink, and credential rules in
[the security policy](security-and-execution-policy.md).

## P10 Bounded View Gate

P10 tests equivalent allowed queries through each implemented Nu, CLI, and MCP
surface. Fixtures cover empty, normal, at-limit, over-limit, unauthorized raw
source, secret-shaped, binary, malformed-filter, and adversarial pagination
cases. Evidence records requested and enforced byte/row/file/depth/time limits,
stable ordering and cursors, omitted counts or explicit truncation, redaction
decisions, authorization/policy identity, and response digest.

Default views are read-only, offline, and metadata-first. Raw/full source is
non-default and requires the policy described in
[security and execution policy](security-and-execution-policy.md); MCP must not
expose raw source by default. Limit bypass through pagination, filters,
compression, errors, logs, or alternate surfaces fails P10. So do secret
leakage, repository/build execution, writes caused by a query, unstable
ordering, silent truncation, or materially inconsistent policy enforcement
between Nu, CLI, and MCP.

## P11 Delivery Backlog Gate

P11 passes when the [GitHub issue delivery plan](github-issue-delivery-plan.md)
has either created issues or deterministic, review-ready issue drafts for all
implementation work and every unresolved P0-P10 gap. Each item includes scope,
non-goals, dependencies, affected components, acceptance criteria naming the
applicable P-gates, required fixtures/evidence, security constraints, and a
stable link back to this contract.

The dependency graph must be acyclic and preserve the proof order: byte capture
cannot depend on parser success, export/build work follows the schema and
manifest contract, and materialization follows embedded verification. Every
open question has an owner or issue mapping, and every issue/draft has a stable
identifier in the delivery map. A prose wish list, missing acceptance evidence,
or an issue that weakens P2/P9 byte fidelity or default safety fails P11.

## Cross-Gate Negative Cases

The responsible gate must include and retain evidence for these cases:

| Negative case | Required gate coverage |
|---|---|
| Raw source or credential-like bytes exposed through MCP, CLI, logs, errors, or issue drafts | P8, P10, P11 |
| Unsafe overwrite, path traversal, escaping symlink, or partial output during materialization | P8, P9 |
| Dynamic runtime/build or package-manager execution without explicit approval | P3, P4, P5, P7, P10 |
| Secret leakage through persisted blobs, exports, embedded packs, materialized output, or diagnostics | P2, P6, P8, P9, P10 |
| Schema drift that breaks V1.1/Rust-first rows or promotes derived polyglot rows | P1, P5, P6 |
| Corrupt, truncated, oversized, or digest-mismatched snapshot/export data | P6, P8, P9 |
| Parser/detector failure altering Tier 0 bytes or hiding required gaps | P2, P3, P4 |

## Proof Chain Completion

The complete lane passes only when P0-P11 are individually `pass`, all
required negative cases have fail-closed evidence, and the P2-P9 digest chain
is unbroken. The final proof index lists each gate receipt and its digest,
source snapshot, policy, schema version, and disposition. Any changed bound
input invalidates that gate and every downstream receipt that consumed it.

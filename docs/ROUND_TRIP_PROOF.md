# Round-Trip Proof

## Purpose

Round-trip proof shows that CodeDB can capture source state, create a reviewed
artifact or change plan, materialize it in isolation, and re-scan the result
without losing evidence or mutating source without approval.

## Required Proof Chain

1. Capture the starting source snapshot and blob refs.
2. Record object/provenance rows for every artifact involved.
3. Generate a reviewed plan or patch plan with no apply.
4. Materialize into an isolated worktree or generated-artifact directory.
5. Run deterministic proof gates against that isolated output.
6. Compare the re-scan to the stored plan and source snapshot.
7. Emit `FACT`, `QUESTION`, or `GAP` rows. Missing evidence is never treated as
   a fact.

## Lossless Coverage

Round-trip proof must cover or explicitly gap:

- comments, attributes, formatting, newlines, and BOMs;
- permissions, executable bits, symlinks, and platform limitations;
- binary assets and non-Rust assets;
- generated `OUT_DIR` artifacts;
- source drift after plan creation;
- failed materialization or apply recovery.

## CDB072 Artifact Proof

The redb store now treats the source blob as the byte authority for
materialization. Comments, attributes, formatting, newlines, BOMs, binary
payloads, and non-Rust assets are preserved as exact blob bytes rather than
normalized text.

When capture starts from a filesystem path, CodeDB also records source-file
artifact metadata. On Unix platforms, materialization reapplies the captured
mode bits so executable source artifacts keep their executable state when
restored into an isolated output path. Raw blob capture records an explicit
permission-capture gap because it has no source filesystem metadata.

Generated `OUT_DIR` reproduction is represented by a CDB080 gap until dynamic
capture emits checksum-bound generated artifact manifests. Symlink and
platform-specific materialization limits remain active in CDB081.

## CDB074 Isolated Patch Proof

Patch artifacts are generated only into isolated targets. The core helper
refuses targets that are equal to or nested under the source checkout, rejects
absolute or escaping patch paths, and requires a proof-gate label before writing
any patch artifact. Focused tests prove the source checkout remains unchanged
while the patch file is written under the isolated worktree.

## Acceptance Gate

A round-trip is accepted only when all referenced blobs, checksums, object IDs,
plan IDs, proof command IDs, and re-scan rows match the expected source
snapshot or produce explicit conflict/recovery rows.

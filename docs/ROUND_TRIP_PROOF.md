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

## Acceptance Gate

A round-trip is accepted only when all referenced blobs, checksums, object IDs,
plan IDs, proof command IDs, and re-scan rows match the expected source
snapshot or produce explicit conflict/recovery rows.

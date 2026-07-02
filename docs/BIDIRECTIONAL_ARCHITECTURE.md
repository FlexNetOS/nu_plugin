# Bidirectional Architecture

## Current One-Way Authority

Current CodeDB authority flows from repository inputs into table and blob rows:

```text
Rust repo/files -> CodeDB tables/blobs/proof rows -> Nu/CLI/MCP/envctl exports
```

This remains the foundation. Bidirectional work must not weaken the existing
read-only capture, bounded MCP output, envctl downstream boundary, or unsafe
execution gates.

## Target Loop

The bidirectional loop adds reviewable object and plan layers:

```text
source
  -> capture tables and source blobs
  -> object/provenance graph
  -> change-plan graph
  -> patch plan
  -> isolated worktree materialization
  -> proof gates
  -> operator-approved apply
  -> re-scan and drift verification
```

## Surfaces

| Surface | Responsibility | Default Mutation |
|---|---|---:|
| CLI | export, doctor, plan validation, isolated proof commands | none |
| Nu plugin | table cockpit and structured import/export rows | none |
| MCP | bounded read-only summaries and plan status | none |
| redb store | source/blob identity, plan rows, provenance rows | internal store writes only |
| isolated worktree | patch generation and proof sandbox | allowed after task gate |
| source checkout | operator-approved apply only | forbidden until Phase 5 |

## Artifact Materialization

Source blobs remain content-addressed by SHA-256 and are materialized from the
stored bytes. Files captured from disk also carry source-file metadata rows for
artifact kind, readonly state, and Unix mode where available. This lets isolated
materialization preserve exact bytes and executable-bit state without granting
any direct source-checkout mutation path.

## Store Schema Evolution

The current redb store schema is `1.0.0`. Readers support that schema only.
Unknown future schemas fail closed with `UnsupportedSchemaVersion`; they are not
silently coerced to the current version. The migration test matrix is:

| Observed schema | Behavior |
|---|---|
| `1.0.0` | read metadata and tables |
| unknown future value | refuse open/report, require migration tooling |
| corrupt or unreadable store | use backup/restore validation before reuse |

## Required Object Layers

- source snapshot rows with stable blob refs;
- object identity rows for files, spans, items, generated artifacts, and
  anonymous/unstable nodes;
- provenance rows for capture, plan generation, proof, approval, apply, and
  manual decision events;
- conflict rows for source drift versus stored plans;
- recovery rows for failed materialization and apply attempts.

## CDB076 Sync Semantics

Bidirectional sync is modeled as explicit source-to-store or store-to-source
verification. A sync check first compares the current source snapshot to the
plan snapshot. Drift produces `plan_conflicts` and blocks apply. If the source
snapshot is stable, the final re-scan snapshot is compared with the expected
post-sync snapshot. A match emits `sync_verifications`; a mismatch emits
`recovery_rows` with the configured recovery reference.

CDB087 hardens the same rule at the apply gate: even an `approved_for_apply`
plan is refused with `SourceDrift` when its stored source snapshot no longer
matches the current source snapshot. Operator approval, stop-condition proof,
and recovery references cannot override that stale-plan conflict.

## Non-Goals For This Planning PR

- direct source overwrite;
- unbounded MCP reads;
- raw source/blob dump tools;
- build-script or proc-macro execution without explicit unsafe approval;
- declaring compiler-observed facts complete when evidence is missing.

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

New redb and PostgreSQL stores emit schema `1.0.0`. Normal report, query, and
materialization opens accept only that current schema and never run schema DDL
or an implicit migration. Schema `0.9.0` is the only legacy version recognized
by the explicit migration entrypoints; it is migrate-only and remains
unreadable through normal opens.

Both backends use the backend-neutral migration planner to resolve an ordered,
explicitly registered path. The only registered paths are:

- redb: `redb_legacy_v0_9_to_v1`;
- PostgreSQL: `postgresql_legacy_content_rows_to_v1`.

The migration and refusal policy is:

| Observed schema or layout | Non-mutating open | Explicit migration | Backup and rollback |
|---|---|---|---|
| current `1.0.0` | validate metadata/layout, then allow data access | no migration steps and no backup | not required |
| known legacy `0.9.0` | refuse and require an explicit migration | apply the single registered backend step, preserve captured bytes, then validate `1.0.0` | redb creates a checksum-bound file copy; PostgreSQL creates a transactional table snapshot; both expose explicit rollback |
| unknown, future, or malformed version | refuse before captured blob/path access | no route is guessed and no store mutation is allowed | no migration backup is retained |
| incomplete or corrupt layout/data | refuse or fail validation | abort the migration rather than publish a partial current layout | restore the validated redb backup or let the PostgreSQL migration transaction roll back its schema and backup creation |

redb reports unsupported versions as `UnsupportedSchemaVersion`. PostgreSQL
reports the unsupported version without exposing connection details. Operator
approval cannot override either refusal, and backup/restore is recovery
evidence rather than permission to infer an unregistered migration.

The required test matrix covers:

- strict schema parsing plus refusal of unknown, downgrade, ambiguous, cyclic,
  and overshooting migration routes in the shared planner;
- redb refusal on ordinary read, refusal before backup or mutation, exact-byte
  migration from `0.9.0`, checksum-bound backup, and explicit rollback;
- live PostgreSQL refusal before blob access, explicit legacy conversion,
  transactional backup, exact-byte rollback, and atomic rollback of a failed
  migration.

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

CDB088 models failed materialization/apply recovery as auditable rows. A failed
attempt writes an `apply_attempts` row with failure evidence and a
`recovery_rows` row only after the restored worktree/source snapshot matches
the plan's stored source snapshot. Partial outputs are referenced through a
quarantine ref; recovery is not complete while the restored snapshot still
differs from the plan snapshot.

CDB089 tightens approval provenance. Operator decisions must include a decision
ID, actor, timestamp, evidence reference, and manual-decision reference before
the apply gate can emit `operator_decisions` or `apply_attempts`.

## Non-Goals For This Planning PR

- direct source overwrite;
- unbounded MCP reads;
- raw source/blob dump tools;
- build-script or proc-macro execution without explicit unsafe approval;
- declaring compiler-observed facts complete when evidence is missing.

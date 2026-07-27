# Mutation Policy

## Non-Negotiable Defaults

- Default commands remain read-only.
- MCP remains read-only and bounded by default.
- No hidden Git mutation.
- No direct source overwrite until controlled apply gates exist.
- No raw secrets in tracked files, logs, MCP output, prompts, manifests, or
  generated artifacts.
- Build-script/proc-macro execution requires explicit unsafe approval.
- Missing evidence is QUESTION or GAP, not FACT.

## Allowed Mutation By Phase

| Phase | Source Checkout | Isolated Worktree | redb Store | Logs/Manifests |
|---|---:|---:|---:|---:|
| Phase 0 | no | no | task/doc rows only | yes |
| Phase 1 | no | no | hardening/proof rows | yes |
| Phase 2 | no | generated artifacts only | source/blob rows | yes |
| Phase 3 | no | no | change-plan rows | yes |
| Phase 4 | no | yes | patch-plan rows | yes |
| Phase 5 | operator-approved only | yes | apply/provenance rows | yes |
| Phase 6 | operator-approved only | yes | sync/conflict rows | yes |

## CDB074 Isolated Patch Guard

Patch generation may write only under an isolated target path. Targets inside
the source checkout are refused, patch artifact paths must be relative, and a
proof gate is required before writing the patch artifact. This keeps source
checkout mutation unavailable until the operator-approved CDB075 apply gate.

## CDB075 Operator Apply Gate

Apply intent is refused unless approval provenance, stop-condition proof,
manual-decision evidence, source-snapshot stability, and recovery references are
all present. The CDB075 implementation records apply readiness rows only; it
does not add a default CLI, Nu, or MCP source overwrite command.

The gate is covered by paired tests: incomplete operator provenance is refused,
while a matching approval, passing stop proof, stable source snapshot, manual
decision evidence, and recovery reference produces `operator_decisions` and
`apply_attempts` readiness rows. These tests prove apply intent validation; they
do not authorize direct source-checkout mutation.

## MCP Raw Data Policy

MCP exposes bounded summaries only. Raw source/blob tools and raw blob/source
table aliases are blocked by default; requests receive validation rows rather
than file bytes or blob payloads.

## CDB083 MCP Raw Source/Blob Denial

The blocked-tool policy covers raw source, raw blob, source/blob, artifact-blob,
and full-file-dump aliases. Requests for raw source/blob table pages likewise
return one bounded `raw_blob_table_blocked` validation row without consulting
the backend. The denial response must not contain request paths, source bytes,
blob payloads, or secret-looking sentinel values.

The evidence gate is the MCP raw-source denial test together with the Nu
no-leak protocol test:
`cargo test -p codedb-mcp raw_source` and
`nu --no-config-file tests/test_security_no_leak.nu`.

## Stop Rules

Stop if an operation would:

- mutate source without a selected CDB task and operator approval;
- execute build scripts or proc macros without the unsafe gate;
- expose raw source/blob bytes through MCP by default;
- use envctl to read redb internals;
- rewrite Git history or reset user changes;
- treat unknown or missing evidence as complete.

# CodeDB Commands

Source: PRD section 13.

## CLI surface

| Command | Output | Default safety |
|---|---|---|
| `codedb scan <path>` | scan summary + table rows | read-only |
| `codedb export --format json|nuon|csv` | bounded table export | read-only |
| `codedb schema` | schema/version metadata | read-only |
| `codedb doctor --nu` | host Nushell status | read-only |
| `codedb doctor --yazelix` | Yazelix runtime Nu status | read-only |
| `codedb doctor --codex` | CLI/MCP bridge status | read-only |
| `codedb archive` | archive manifest + checksums | read-only except declared archive output |
| `codedb restore --verify` | restore validation report | refuses unsafe overwrite |
| `codedb capture build <repo>` | compiler/build rows plus optional persisted receipt | refuses without the explicit execution flag |
| `codedb capture compiler <source.rs>` | bounded macro/HIR/MIR/rustdoc metadata + persisted full artifacts | refuses without the explicit execution flag |
| `codedb reproduce --approval-id <sha256> [--package-id <id>]` | verified OUT_DIR reproduction rows | writes only to a new declared artifact directory; multi-package receipts require an exact package selector |

Approved dynamic capture is non-interactive and requires complete provenance:

```bash
codedb capture build /repo \
  --unsafe-execute-build \
  --approver operator-name \
  --task-id CDB078,CDB079,CDB080,CDB082 \
  --before-state source-snapshot-recorded \
  --cleanup-plan remove-isolated-sandbox \
  --raw-log /evidence/capture.log \
  --store /evidence/capture.redb \
  --format json
```

The raw log must be outside `/repo`. When `--store` is supplied, CodeDB
persists a checksum-addressed JSON receipt at
`dynamic-build-captures/<approval-id>.json`. Reproduce an observed OUT_DIR
from that receipt with:

```bash
codedb reproduce \
  --approval-id <approval-id> \
  --store /evidence/capture.redb \
  --artifact-dir /evidence/reproduced-out-dir \
  --format json
```

The artifact directory must not already exist. CodeDB verifies every emitted
file or symlink against the captured reproduction digest and does not mutate
the source repository. A receipt containing OUT_DIR artifacts from more than
one package refuses reproduction until `--package-id <exact-captured-package-id>`
selects one package. The exact IDs are present on the capture's
`out_dir_artifacts` rows. This prevents identically named artifacts such as
`generated.rs` from different build scripts from being flattened into one
output tree. Single-package receipts retain the command shape shown above.

Compiler-observed expansion, resolution, hygiene, HIR, MIR, and rustdoc
public-API evidence use the same named approval provenance:

```bash
codedb capture compiler /repo/src/lib.rs \
  --repo-path /repo \
  --unsafe-execute-build \
  --approver operator-name \
  --task-id CDB077,CDB085 \
  --before-state source-sha256-recorded \
  --cleanup-plan remove-isolated-compiler-sandbox \
  --evidence-dir /evidence/compiler \
  --store /evidence/compiler.redb \
  --crate-name crate_name \
  --edition 2024 \
  --format json
```

Stdout contains bounded metadata, context hashes, toolchain hashes, semantic
hashes, public-API hashes, and artifact paths. Full compiler artifacts and the
raw summary log are written only beneath the new external evidence directory
and persisted as content-addressed store blobs. This command is local CLI only;
MCP has no dynamic execution path.

## Nushell plugin commands

```nu
codedb scan <path>
codedb export --format nuon
codedb schema
codedb doctor --nu
codedb doctor --yazelix
codedb tables
codedb gaps
codedb validation-errors
```

All Nu plugin commands return tables/records/lists, not raw terminal text dumps.

## MCP tool surface

MCP tools are read-only and bounded by default:

| Tool | Purpose | Guard |
|---|---|---|
| `codedb_list_tables` | table inventory | row/byte limits |
| `codedb_query_table` | bounded table read | pagination required |
| `codedb_get_gaps` | capture gap report | no raw source |
| `codedb_get_validation_errors` | validation report | no raw source |
| `codedb_export_summary` | compact export summary | byte limit |

Raw source/blob reads are disabled by default and require an explicit future policy gate.

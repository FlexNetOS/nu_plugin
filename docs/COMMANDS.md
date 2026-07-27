# CodeDB command reference

This is the canonical V1.1 command contract from
[PRD section 13](../prd/nu_plugin_codedb_v1_1_full_prd.md#13-command-surface).
Every command returns structured, machine-readable data. Commands that can
produce many rows use `--limit` and `--cursor`; CLI serialization uses
`--format nuon|json|csv`.

## Nushell plugin

The plugin is a table-oriented operator surface. It returns Nushell
records, lists, or tables rather than terminal-formatted text.

| Command | Important options | Result |
|---|---|---|
| `codedb scan <repo_path>` | `--store <path>`, `--profile static` | Repository scan rows and summary |
| `codedb fs entries` | `--store <path>`, `--limit <n>`, `--cursor <cursor>` | Filesystem entry page |
| `codedb source files` | `--store <path>`, `--limit <n>`, `--cursor <cursor>` | Source-file page |
| `codedb cargo packages` | `--store <path>` | Cargo package rows |
| `codedb cargo deps` | `--store <path>` | Cargo dependency rows |
| `codedb cargo sources` | `--store <path>` | Cargo source rows |
| `codedb rust items` | `--store <path>`, `--limit <n>`, `--cursor <cursor>` | Rust item page |
| `codedb rust macros` | `--store <path>`, `--limit <n>`, `--cursor <cursor>` | Macro page |
| `codedb rust cfg` | `--store <path>` | Conditional-compilation rows |
| `codedb build scripts` | `--store <path>` | Static build-script rows |
| `codedb capture build <repo_path>` | `--unsafe-execute-build`, `--store <path>` | Approved dynamic build capture |
| `codedb gaps` | `--store <path>` | Capture-gap rows |
| `codedb validation errors` | `--store <path>` | Validation-error rows |
| `codedb schema` | `--store <path>` | Schema and version rows |
| `codedb export <table>` | `--format nuon|json|csv`, `--store <path>` | Serialized table rows |
| `codedb backup` | `--store <path>`, `--out <path>` | Backup receipt |
| `codedb restore` | `--backup <path>`, `--store <path>` | Validated restore receipt |
| `codedb prove no-mutation <repo_path>` | `--store <path>` | Before/after no-mutation proof |
| `codedb verify <repo_path>` | `--store <path>` | Verification rows |
| `codedb doctor` | `--nu`, `--yazelix`, `--codex`, `--meta`, `--envctl` | Selected integration health rows |

Examples:

```nu
register /absolute/path/to/nu_plugin_codedb

codedb scan /work/repo --store /work/evidence/repo.redb --profile static
codedb rust items --store /work/evidence/repo.redb --limit 100 --cursor 0
codedb export rust_items --store /work/evidence/repo.redb --format nuon
codedb doctor --nu --yazelix
```

## CLI parity

The `codedb` executable exposes the same logical command families for Codex,
automation runners, shell scripts, and other non-interactive callers. Pass
absolute paths in automation and select a machine-readable format explicitly.

```bash
codedb scan /path/to/repo \
  --store /path/to/evidence/repo.redb \
  --format json

codedb export rust_items \
  --repo-path /path/to/repo \
  --store /path/to/evidence/repo.redb \
  --format nuon

codedb verify /path/to/repo \
  --store /path/to/evidence/repo.redb \
  --format json

codedb doctor --nu --yazelix --codex --format json

codedb mcp serve \
  --repo-path /path/to/repo \
  --store /path/to/evidence/repo.redb \
  --readonly \
  --max-rows 200 \
  --max-bytes 65536
```

Scanning and querying are read-only with respect to the source repository.
`backup`, `restore`, and other materialization commands may write only their
declared output or store paths. Restore must validate its input and refuse an
unsafe source overwrite.

## Approved dynamic capture and reproduction

Dynamic build, proc-macro, and compiler capture is not part of the ordinary
read-only path. It is local CLI/plugin functionality only, is never exposed
through MCP, and refuses to run without explicit approval provenance.

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
output tree. Each artifact row also carries a stable `artifact_group_id`,
derived from its Cargo package identity and normalized isolated-target OUT_DIR
execution path. If one package has artifacts from multiple compilation-unit
OUT_DIRs, reproduction additionally requires
`--artifact-group <exact-captured-artifact-group-id>`. Single-package,
single-group receipts retain the command shape shown above.

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

Ordinary Rust tests send broker evidence to a temporary directory and never
rewrite the checked proof tree. Regenerate `logs/compiler-observed/` only during
an explicit final proof seal:

```bash
cargo test -p codedb-rust-static \
  compiler_broker::regenerate_tracked_compiler_evidence \
  -- --ignored --exact --nocapture
```

## MCP tool surface

Start the stdio server through the trusted CLI front door:

```bash
codedb mcp serve \
  --repo-path /absolute/path/to/repo \
  --store /absolute/path/to/repo.redb \
  --readonly \
  --max-rows 200 \
  --max-bytes 65536
```

V1.1 MCP is read-only and bounded. Table reads require pagination, responses
have row and byte ceilings, and raw source/blob tables are unavailable.

| Tool | Purpose |
|---|---|
| `codedb_schema` | Return bounded schema/version metadata |
| `codedb_list_tables` | List available tables |
| `codedb_get_table_page` | Read one bounded, paginated table page |
| `codedb_get_store_summary` | Return bounded store metadata (implemented additive tool) |
| `codedb_get_capture_gaps` | Return capture-gap rows |
| `codedb_get_validation_errors` | Return validation-error rows |
| `codedb_get_repo_summary` | Summarize repository capture facts |
| `codedb_get_cargo_summary` | Summarize Cargo facts |
| `codedb_get_rust_item_summary` | Summarize Rust item facts |
| `codedb_get_macro_summary` | Summarize macro facts |
| `codedb_get_build_script_summary` | Summarize static build-script facts |
| `codedb_get_no_mutation_proof` | Return a bounded no-mutation proof |

All MCP tool requests accept the bounded selection fields applicable to the
tool: `repo_path`, `table`, `cursor`, `limit`, and `max_bytes`.

The following capabilities are blocked by default:

```text
raw_source_blob_read
full_file_dump
unsafe_build_capture
source_overwrite
patch_apply
git_mutation
unbounded_table_dump
```

MCP does not provide apply, approve, deploy, restore, dynamic execution, or
other mutation entry points. Raw source/blob access requires a separate future
policy gate and is not a V1.1 command.

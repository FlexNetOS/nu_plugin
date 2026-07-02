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
| `codedb capture build` | refusal unless unsafe flag present | blocked by default |

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

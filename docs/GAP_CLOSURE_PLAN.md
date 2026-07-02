# Gap Closure Plan

Source: issue 212 V1.1 gap closure list.

| Task | Gap | Closure Direction | Evidence Gate |
|---|---|---|---|
| CDB077 | macro expansion beyond static `macro_rules!` inventory | compiler-observed expansion rail or explicit GAP rows | fixture proves dynamic expansion facts or gated refusal |
| CDB078 | proc-macro execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB079 | build-script execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB080 | generated `OUT_DIR` artifact reproduction | controlled reproduction artifacts | checksum-bound generated artifacts or GAP |
| CDB081 | symlink materialization/platform limitations | platform capability rows | symlink support matrix and safe fallback |
| CDB082 | native/linker facts requiring dynamic build execution | approved dynamic build capture | native/link rows or GAP |
| CDB083 | raw source/blob reads through MCP blocked by default | MCP denial and bounded output tests | no raw source/blob leak proof |
| CDB084 | stable object identity for anonymous/unstable syntax nodes | identity keys and instability policy | repeat scan identity tests |
| CDB085 | semantic hashing and public API hashing | documented hash inputs and tests | stable hash fixtures |
| CDB086 | store migrations/backwards-compatible schema evolution | migration/refusal policy | migration tests and unknown-schema refusal |
| CDB087 | conflict detection between source drift and stored plans | source snapshot conflict rows | stale plan cannot apply silently |
| CDB088 | recovery from failed materialization/apply attempts | recovery rows and rollback/quarantine | failed apply fixture |
| CDB089 | provenance for operator approvals and manual decisions | decision IDs and evidence refs | apply gate refuses missing approval |

Every gap remains active until its evidence gate is proven. Partial evidence
must be recorded as `QUESTION` or `GAP`, not `FACT`.

## Closed By CDB072

- Exact source blob bytes now cover comments, attributes, formatting, newlines,
  BOMs, binary payloads, and non-Rust assets.
- Source-file capture records readonly state and Unix mode metadata; Unix
  materialization reapplies mode bits.
- Raw blob capture records permission metadata as an explicit gap because no
  filesystem source exists for that API surface.

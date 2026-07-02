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

## Closed By CDB077

- Static macro capture now emits explicit `compiler_observed_expansion` gate
  rows with `gap` status for macro definitions and invocations.
- The focused fixture proves CodeDB does not claim dynamic expansion or hygiene
  facts from syntax-only capture.

## Closed By CDB078

- Dynamic capture default refusal now records a dedicated
  `proc_macro_execution` gap with required flag `--unsafe-execute-build`.
- Approved dynamic capture scaffold records unsafe approval provenance with
  status, flag, and approver.

## Closed By CDB079

- Dynamic capture default refusal records `build_script_execution` with
  required flag `--unsafe-execute-build`.
- Approved fixture capture records approval provenance, build-script run rows,
  raw log rows, and observed Cargo warning output.

## Closed By CDB080

- Approved dynamic capture now records `out_dir_artifacts` as an explicit GAP
  until generated artifact manifests include relative paths, sha256 checksums,
  Cargo `OUT_DIR` provenance, target/rustc environment, and filesystem metadata.
- The focused `out_dir_generator` fixture proves CodeDB does not silently claim
  generated artifact reproduction when only raw build logs are available.

## Closed By CDB081

- Core rows now model `platform_materialization_capabilities` for symlink
  materialization.
- Platforms that cannot create symlinks emit `metadata_only_fallback` rows that
  preserve link targets without materializing links as regular files.
- Unix fixture coverage proves scans capture symlink targets with
  `symlink_metadata` and emit supported materialization rows.

## Closed By CDB082

- Approved dynamic build capture now parses Cargo JSON
  `build-script-executed` messages for native `linked_libs` and `linked_paths`.
- Native/linker facts are emitted as `native_link_facts` rows only when the
  explicit unsafe build gate ran.
- Default/refused capture records `native_linker_dynamic_facts` as a GAP with
  required flag `--unsafe-execute-build`.

## Closed By CDB083

- MCP raw source/blob tool aliases are explicitly blocked by default.
- MCP table-page requests for raw blob/source tables return bounded
  `raw_blob_table_blocked` validation rows instead of raw bytes.
- Tests prove raw source summaries and blocked table responses do not leak
  source secret sentinels.

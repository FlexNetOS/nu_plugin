# Gap Closure Plan

Source: issue 212 V1.1 gap closure list.

| Task | Gap | Closure Direction | Evidence Gate |
|---|---|---|---|
| CDB077 | macro expansion beyond static `macro_rules!` inventory | compiler-observed expansion, resolution, and hygiene rail | fixture proves compiler-observed expansion, resolution, and hygiene facts |
| CDB078 | proc-macro execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB079 | build-script execution gate | unsafe approval and provenance model | default refusal plus approved fixture proof |
| CDB080 | generated `OUT_DIR` artifact reproduction | controlled reproduction artifacts | checksum-bound generated artifacts and environment provenance |
| CDB081 | symlink materialization/platform limitations | platform capability rows | symlink support matrix and safe fallback |
| CDB082 | native/linker facts requiring dynamic build execution | approved dynamic build capture | approved dynamic native/link rows with provenance |
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

## CDB077 Interim Evidence - Still Active

- Static macro capture now emits explicit `compiler_observed_expansion` gate
  rows with `gap` status for macro definitions and invocations.
- The focused fixture proves CodeDB does not claim dynamic expansion or hygiene
  facts from syntax-only capture.

## CDB078 Interim Evidence - Still Active

- Dynamic capture default refusal now records a dedicated
  `proc_macro_execution` gap with required flag `--unsafe-execute-build`.
- Approved dynamic capture scaffold records unsafe approval provenance with
  status, flag, and approver.

## CDB079 Interim Evidence - Still Active

- Dynamic capture default refusal records `build_script_execution` with
  required flag `--unsafe-execute-build`.
- Approved fixture capture records approval provenance, build-script run rows,
  raw log rows, and observed Cargo warning output.

## CDB080 Interim Evidence - Still Active

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

## CDB082 Interim Evidence - Still Active

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

## Closed By CDB084

- Rust item rows now include identity classification and notes.
- Named syntax rows are marked `stable_named`.
- Anonymous impl rows receive deterministic scan-order IDs and are marked
  `unstable_anonymous` so source-drift-sensitive identity cannot be treated as
  a permanent semantic key.

## CDB085 Interim Evidence - Still Active

- Static Rust capture now emits semantic and public API hash reports from
  normalized item rows.
- Hash inputs include path, module path, item kind, name, visibility, identity
  kind, and identity note.
- Public API hashes include only public item rows; private item drift changes
  the semantic hash while leaving the public API hash stable.
- The report documents that these hashes exclude function bodies, type layout,
  macro expansion, and rustc semantic checks.

## Closed By CDB086

- redb store reads now refuse unknown schema versions instead of silently
  treating them as current.
- Migration policy is explicit: schema `1.0.0` is supported, future unknown
  schemas fail closed, and backup/restore remains the recovery proof.
- Tests mutate a store to a future schema and assert
  `UnsupportedSchemaVersion`.

## Mandatory closure semantics

A GAP proves that CodeDB detected missing truth; it never proves that the capability was delivered. Every task in this plan remains active until its positive implementation path and failure path both have executable, current-head tests. Any remaining GAP blocks CDB090 and release readiness.

# CodeDB V1.1 Schema

This document is the normative table, identity, and checksum contract for
CodeDB V1.1. It refines PRD sections 10–12. The PRD remains product truth; this
document fixes the names and minimum fields that implementations, exports,
fixtures, and schema introspection must share.

## 1. Conventions

### 1.1 Logical types

| Type | Contract |
|---|---|
| `id` | Lowercase, domain-separated BLAKE3-256 digest encoded as `prefix_` plus 64 lowercase hex characters. |
| `hash` | `blake3:` or `sha256:` plus 64 lowercase hex characters. BLAKE3 is used for CodeDB content and identity hashes; SHA-256 is permitted for external artifacts and manifests. |
| `text` | UTF-8 string. Paths are stored separately from raw platform path bytes when those bytes are not UTF-8. |
| `bytes` | Exact bytes stored in a blob table, never lossy text. |
| `u64` / `i64` | Unsigned/signed 64-bit integer. |
| `bool` | `true` or `false`. |
| `timestamp` | RFC 3339 UTC timestamp. Timestamps are facts, not identity inputs unless a table explicitly says otherwise. |
| `enum` | Closed lowercase `snake_case` value set published by `schema_introspection`. Unknown values fail validation. |
| `json` | Canonical JSON: UTF-8, sorted object keys, no insignificant whitespace, and integers represented exactly. |

All exported rows use `snake_case` field names. Required means present and
non-null. Optional means present with a value or `null`; omission is not a
third state. Repeated values are ordered lists unless modeled as child rows.

### 1.2 Common row envelope

Every queryable row has these fields:

| Field | Type | Required | Meaning |
|---|---|---:|---|
| `row_id` | `id` | yes | Stable ID of this table row. |
| `schema_version` | `u64` | yes | Schema version under which the row was encoded. V1.1 starts at `1`. |
| `capture_run_id` | `id` | yes, except store/schema metadata | Run that observed or produced the fact. |
| `context_id` | `id` | yes, except context-free store facts | Compilation/capture context. |
| `provenance_kind` | `enum` | yes | `filesystem`, `cargo`, `static_parse`, `compiler`, `build`, `operator`, `derived`, or `generated`. |
| `observed_at` | `timestamp` | yes | Observation time; excluded from stable row identity and table checksums. |

Rows that describe a source location also carry `source_file_id` and optional
`source_span_id`. Rows derived from exact bytes carry the appropriate
`blob_hash`. A producer must not invent an empty ID for unavailable evidence:
it uses `null` only for an optional field and emits a `capture_gaps` row when
the unavailable fact affects completeness.

### 1.3 Canonical encoding

Identity and checksum inputs use canonical JSON with:

- keys sorted by UTF-8 byte order;
- lists retained in contract-defined order, or sorted by referenced ID when
  their order is semantically irrelevant;
- IDs and hashes lowercased;
- repository-relative paths normalized to `/`, with neither `.` nor `..`
  segments and no leading `/`;
- no timestamps, machine-local absolute paths, redb page/layout details, or
  export pagination metadata unless explicitly named as an identity input.

## 2. ID domains and construction

IDs are computed as:

```text
id = <prefix> + "_" + hex(blake3(
  "codedb.v1\0" + <domain> + "\0" + canonical_json(<identity fields>)
))
```

| ID | Prefix | Identity fields |
|---|---|---|
| Store | `sto` | `store_uuid` assigned once at creation |
| Schema version | `sch` | `schema_name`, `version`, `schema_digest` |
| Repository/workspace | `wrk` | normalized origin identity when available, otherwise root manifest hash |
| Package/crate | `pkg` | `workspace_id`, Cargo package name, version, source ID, manifest-relative path |
| Context | `ctx` | toolchain, host, target, feature set, cfg set, Cargo lock hash, profile, edition |
| Capture run | `run` | `workspace_id`, `context_id`, input-manifest hash, capture mode, tool version |
| Blob | `blb` | exact byte length and BLAKE3 content hash |
| Source file | `fil` | `workspace_id`, normalized relative path, source blob hash |
| Source span | `spn` | `source_file_id`, byte start, byte end |
| Object | `obj` | fields in section 4 |
| Proof | `prf` | proof kind, subject ID, input checksum, result checksum |
| Generic row | `row` | table name and that table's declared natural key |

`row_id` is not a database sequence and must survive export/import. A table
with a domain ID uses that domain ID as its `row_id`; other tables use `row`.
Identical inputs under identical schema and context produce identical IDs.
Hash collisions are fatal validation errors, never resolved by adding a random
or sequential suffix.

## 3. Table registry

This registry is exhaustive for V1.1. A table may be empty, but must appear in
schema introspection and export manifests. Tables sharing a name across PRD
sections (for example `cargo_profiles`) are one table.

| Group | Tables |
|---|---|
| Core identity and provenance | `codedb_stores`, `codedb_schema_versions`, `codedb_contexts`, `capture_runs`, `capture_inputs`, `capture_gaps`, `validation_errors`, `object_identities`, `object_hashes`, `git_provenance`, `no_mutation_proofs`, `toolchains`, `rustc_versions`, `cargo_versions`, `target_triples`, `host_triples`, `target_cfgs`, `feature_sets`, `cfg_predicates`, `cfg_eval_results` |
| Store and blobs | `source_blobs`, `artifact_blobs`, `blob_indexes`, `blob_ref_counts`, `redb_store_facts`, `redb_backups`, `redb_restore_tests` |
| Filesystem and source | `filesystem_entries`, `crate_input_files`, `non_rust_assets`, `ignored_files`, `symlink_edges`, `file_classification_rules`, `source_files`, `source_snapshots`, `source_spans`, `source_comments`, `source_doc_comments`, `source_attributes`, `source_byte_facts` |
| Cargo | `cargo_workspaces`, `cargo_packages`, `cargo_targets`, `cargo_dependencies`, `cargo_resolve_nodes`, `cargo_features`, `cargo_locks`, `cargo_profiles`, `cargo_configs`, `cargo_sources`, `cargo_patch_overrides`, `cargo_replace_overrides`, `cargo_registry_sources`, `cargo_git_sources`, `cargo_path_sources` |
| Rust static | `rust_modules`, `rust_items`, `rust_functions`, `rust_structs`, `rust_enums`, `rust_traits`, `rust_impls`, `rust_imports`, `rust_attributes`, `rust_visibility`, `rust_generics`, `rust_where_clauses`, `rust_types`, `rust_symbol_refs_static` |
| Declarative macros | `macro_definitions`, `macro_rules`, `macro_matchers`, `macro_transcribers`, `macro_fragments`, `macro_invocations`, `macro_resolution_static`, `macro_expansion_events`, `macro_expansion_edges`, `macro_hygiene_contexts` |
| Procedural macros | `proc_macro_crates`, `proc_macro_artifacts`, `proc_macro_invocations`, `proc_macro_input_token_streams`, `proc_macro_output_token_streams`, `proc_macro_panics`, `proc_macro_env`, `proc_macro_file_access` |
| Build and native | `build_scripts`, `build_script_runs`, `build_script_env`, `build_script_stdout`, `build_script_stderr`, `build_script_cargo_instructions`, `out_dir_artifacts`, `generated_rust_files`, `rerun_if_changed`, `rerun_if_env_changed`, `native_libraries`, `linker_tools`, `link_args`, `link_search_paths`, `pkg_config_results`, `cc_invocations`, `system_library_facts` |
| Static paths and package metadata | `static_include_edges`, `static_path_references`, `fixture_assets`, `example_assets`, `bench_assets`, `doc_assets`, `license_files`, `readme_files`, `package_metadata`, `publish_metadata`, `workspace_metadata`, `compliance_flags` |
| Proof and artifacts | `generated_crates`, `generated_files`, `artifact_files`, `generation_runs`, `compile_runs`, `test_runs`, `rustdoc_runs`, `reproduction_proofs`, `public_api_deltas`, `semantic_hashes` |
| Agent and export | `export_manifests`, `table_checksums`, `mcp_tool_calls`, `mcp_response_limits`, `pagination_cursors`, `schema_introspection`, `source_leak_policy` |

## 4. Object identity

`object_identities` has the following field contract:

| Field | Type | Required | Rule |
|---|---|---:|---|
| `object_id` | `id` | yes | Also the row ID. |
| `object_kind` | `enum` | yes | Schema-published Rust/source object kind. |
| `stable_name` | `text` | no | Compiler/source name when one exists. |
| `module_path` | `text` | no | Canonical Rust module path. |
| `crate_id` | `id` | yes | Owning Cargo package/crate. |
| `package_id` | `id` | yes | Owning Cargo package. |
| `edition` | `enum` | yes | Rust edition. |
| `visibility` | `text` | no | Normalized visibility. |
| `source_span_id` | `id` | no | Exact byte anchor. |
| `source_blob_hash` | `hash` | yes | Exact source bytes containing the object. |
| `token_hash` | `hash` | no | Normalized token identity. |
| `syntax_hash` | `hash` | no | Normalized syntax identity. |
| `context_hash` | `hash` | yes | Digest of the referenced context. |
| `public_api_hash` | `hash` | no | Public API projection hash. |
| `semantic_hash` | `hash` | no | Evidence-qualified semantic hash. |
| `identity_status` | `enum` | yes | `stable` or `unstable_identity`. |

The `object_id` identity input is:

```text
schema_version + workspace_id + crate_id + package_id + object_kind
+ stable_name + module_path + source_span_id + context_hash + source_blob_hash
```

Source, token, syntax, public API, context, and proof identity are distinct.
Equality of one hash never implies equality of another. If a stable object ID
cannot be derived, the row uses a deterministic best-effort ID, sets
`identity_status = unstable_identity`, and references a `capture_gaps` row.

`object_hashes` contains `object_id`, `hash_kind`, `hash`, `algorithm`,
`normalization_version`, and `evidence_blob_hash`. Its natural key is
`(object_id, hash_kind, normalization_version)`.

## 5. Field contracts by table family

The fields below are in addition to the common row envelope. A listed field is
required unless suffixed with `?`. The fields before `->` form the natural key;
the remainder is payload.

### 5.1 Core, context, and provenance

```text
codedb_stores:
  store_id -> store_uuid, created_at, codedb_version, redb_format_version
codedb_schema_versions:
  schema_version -> schema_name, schema_digest, migration_state
codedb_contexts:
  context_id -> toolchain_id, rustc_version_id, cargo_version_id,
  host_triple_id, target_triple_id, feature_set_id, cfg_set_hash,
  cargo_lock_hash?, profile, edition, environment_facts_hash?
capture_runs:
  capture_run_id -> workspace_id, context_id, mode, codedb_version,
  input_manifest_hash, started_at, completed_at?, status
capture_inputs:
  capture_run_id + input_kind + input_id -> input_hash, inclusion_reason
capture_gaps:
  capture_run_id + gap_kind + subject_id? + detail_hash -> severity,
  description, required_backend?, resolution_status
validation_errors:
  capture_run_id + error_code + subject_id? + detail_hash -> severity,
  message, validator, resolution_status
git_provenance:
  capture_run_id + workspace_id -> head_commit?, branch?, dirty,
  status_blob_hash?, submodule_state_hash?
no_mutation_proofs:
  proof_id -> workspace_id, before_manifest_hash, after_manifest_hash,
  git_before_hash?, git_after_hash?, status, degraded_reason?
toolchains / rustc_versions / cargo_versions:
  row_id -> version, verbose_version_hash?, executable_hash?
target_triples / host_triples:
  row_id -> triple
target_cfgs:
  context_id + cfg_key + cfg_value? -> source
feature_sets:
  feature_set_id -> package_id, sorted_features, feature_set_hash
cfg_predicates:
  row_id -> source_span_id?, expression, expression_hash
cfg_eval_results:
  predicate_id + context_id -> result, evidence_kind
```

### 5.2 Blobs and redb

```text
source_blobs / artifact_blobs:
  blob_id -> content_hash, byte_length, bytes, media_type?,
  encoding_status?, compression
blob_indexes:
  blob_id + index_kind + index_version -> index_blob_hash
blob_ref_counts:
  blob_id -> reference_count, verified_at
redb_store_facts:
  store_id + fact_key -> fact_value, fact_hash
redb_backups:
  row_id -> store_id, schema_version, backup_hash, byte_length,
  created_at, clean_close_checksum
redb_restore_tests:
  row_id -> backup_id, restored_store_hash, validation_hash, status
```

`source_blobs.bytes` may be withheld from an export or MCP response by policy,
but the stored row and checksum remain defined. Withholding is recorded by
`source_leak_policy`; it is not represented as an empty blob.

### 5.3 Filesystem and exact source

```text
filesystem_entries:
  workspace_id + normalized_path -> entry_kind, raw_path_blob_hash?,
  content_hash?, size, mode?, modified_at?, symlink_target?,
  classification, inclusion_status
crate_input_files:
  crate_id + source_file_id -> input_kind, inclusion_reason
non_rust_assets / ignored_files:
  workspace_id + normalized_path -> content_hash?, classification,
  classification_rule_id, reason
symlink_edges:
  workspace_id + source_path + target_path -> target_exists, escapes_root
file_classification_rules:
  rule_id -> priority, matcher_kind, matcher, classification, action
source_files:
  source_file_id -> workspace_id, normalized_path, blob_id, crate_id?,
  encoding_status, newline_style, bom_status, mode?
source_snapshots:
  row_id -> workspace_id, capture_run_id, file_manifest_hash, file_count
source_spans:
  source_span_id -> source_file_id, byte_start, byte_end, line_start?,
  column_start?, line_end?, column_end?
source_comments / source_doc_comments:
  row_id -> source_file_id, source_span_id, blob_id, comment_kind
source_attributes:
  row_id -> owner_object_id?, source_span_id, attribute_path, token_blob_id
source_byte_facts:
  source_file_id -> byte_length, content_hash, encoding_status,
  newline_style, bom_status
```

`fixture_assets`, `example_assets`, `bench_assets`, `doc_assets`,
`license_files`, and `readme_files` use the natural key
`(package_id, source_file_id)` and add `asset_kind`, `classification`, and
`inclusion_reason`.

### 5.4 Cargo

```text
cargo_workspaces:
  workspace_id -> root_manifest_file_id, resolver?, members_hash,
  default_members_hash, workspace_metadata_hash?
cargo_packages:
  package_id -> workspace_id, cargo_package_id, name, version, source_id?,
  manifest_file_id, edition, links?, publish_allowed?
cargo_targets:
  row_id -> package_id, name, target_kinds, crate_types, src_file_id,
  edition, required_features
cargo_dependencies:
  row_id -> package_id, dependency_package_id?, name, requirement,
  dependency_kind, target_predicate?, rename?, optional, uses_default_features,
  requested_features, source_id?
cargo_resolve_nodes:
  package_id + context_id -> enabled_features, dependency_ids
cargo_features:
  package_id + feature_name -> enables
cargo_locks:
  workspace_id + lock_blob_hash -> lock_version?, package_set_hash
cargo_profiles:
  workspace_id + profile_name -> settings_json, settings_hash
cargo_configs:
  workspace_id + config_file_id -> normalized_config_hash
cargo_sources:
  source_id -> source_kind, canonical_location, precise_revision?,
  checksum?, provenance_status
```

Override tables contain `workspace_id`, affected package/source selector,
replacement source ID, and source span. Registry, Git, and path source tables
reference `source_id` and add respectively registry URL/index checksum; Git
URL/revision/commit; or normalized path/path manifest hash.

### 5.5 Rust static facts

All Rust static rows reference `object_id`, `crate_id`, `source_file_id`,
`source_span_id`, and `context_id`. Specialized tables add:

| Tables | Required specialized fields |
|---|---|
| `rust_modules` | `module_path`, `parent_module_id?`, `file_id`, `inline` |
| `rust_items` | `item_kind`, `name?`, `module_id`, `visibility`, `token_hash`, `syntax_hash` |
| `rust_functions` | `object_id`, `async`, `const`, `unsafe`, `abi?`, `signature_hash` |
| `rust_structs`, `rust_enums` | `object_id`, `shape_kind`, `field_or_variant_set_hash` |
| `rust_traits` | `object_id`, `unsafe`, `auto`, `supertrait_hash` |
| `rust_impls` | `object_id`, `trait_object_id?`, `self_type_hash`, `negative` |
| `rust_imports` | `object_id`, `path`, `alias?`, `glob`, `resolved_object_id?` |
| `rust_attributes` | `owner_object_id`, `attribute_path`, `token_blob_id` |
| `rust_visibility` | `owner_object_id`, `visibility_kind`, `restricted_path?` |
| `rust_generics` | `owner_object_id`, `parameter_index`, `parameter_kind`, `name`, `bounds_hash?` |
| `rust_where_clauses` | `owner_object_id`, `predicate_index`, `predicate_hash`, `token_blob_id` |
| `rust_types` | `type_id`, `owner_object_id`, `type_kind`, `normalized_hash`, `token_blob_id` |
| `rust_symbol_refs_static` | `from_object_id`, `source_span_id`, `path`, `ref_kind`, `to_object_id?`, `resolution_status` |

Static extraction is conservative. An unresolved reference or parser
uncertainty is not silently discarded; it creates `capture_gaps` or
`validation_errors`.

### 5.6 Macros, build scripts, and native facts

Declarative macro rows reference the owning definition/invocation object and
exact token blobs. Matcher, transcriber, fragment, expansion, and hygiene rows
add an ordinal so their natural key is deterministic. Resolution/edge rows
carry `from_id`, `to_id?`, `resolution_status`, and evidence kind.

Procedural macro rows include `proc_macro_crate_id`, `invocation_id`, input
token blob ID, output token blob ID?, artifact blob ID?, approval ID, and run
ID as applicable. `proc_macro_panics`, `proc_macro_env`, and
`proc_macro_file_access` also contain their exact message/value/path evidence
or a policy-redacted hash.

```text
build_scripts:
  package_id + source_file_id -> object_id, static_only
build_script_runs:
  row_id -> build_script_id, context_id, approval_id, exit_code?,
  stdout_blob_id, stderr_blob_id, out_dir_manifest_hash, status
build_script_env:
  build_script_run_id + key -> value_hash, value_blob_id?, redacted
build_script_cargo_instructions:
  build_script_run_id + ordinal -> instruction_kind, key?, value_blob_id
out_dir_artifacts / generated_rust_files:
  build_script_run_id + normalized_path -> blob_id, size, classification
rerun_if_changed / rerun_if_env_changed:
  build_script_run_id + ordinal -> path? or key, normalized_value
native_libraries:
  build_script_run_id + name + kind -> modifiers, rename?
linker_tools:
  context_id + executable_hash -> path_hash, version?
link_args / link_search_paths:
  build_script_run_id + ordinal -> value, target_selector?
pkg_config_results / cc_invocations:
  row_id -> build_script_run_id, command_hash, environment_hash,
  stdout_blob_id, stderr_blob_id, exit_code
system_library_facts:
  context_id + library_name + fact_kind -> value, evidence_hash
```

Dynamic build or proc-macro facts require explicit unsafe approval. Static
detection remains valid without execution, and every unavailable dynamic fact
produces a gap row.

### 5.7 Static paths and metadata

`static_include_edges` and `static_path_references` contain source object/span,
macro/path kind, raw path token, normalized target path?, target file ID?, and
resolution status. Package, publish, and workspace metadata rows use
`(owner_id, metadata_key)` as their natural key and store a canonical value
blob/hash. `compliance_flags` contains owner ID, flag kind, status, evidence
hash, and explanation.

### 5.8 Generation and proof

```text
generation_runs:
  row_id -> capture_run_id, generator_version, request_hash,
  artifact_manifest_hash, status
generated_crates:
  generation_run_id + package_id -> output_root_hash, manifest_file_id
generated_files / artifact_files:
  generation_run_id + normalized_path -> blob_id, mode?, artifact_kind
compile_runs / test_runs / rustdoc_runs:
  row_id -> generation_run_id?, context_id, command_hash, environment_hash,
  stdout_blob_id, stderr_blob_id, exit_code, output_manifest_hash, status
reproduction_proofs:
  proof_id -> capture_run_id, generation_run_id, input_manifest_hash,
  output_manifest_hash, compile_run_id?, test_run_id?, status
public_api_deltas:
  row_id -> before_public_api_hash, after_public_api_hash, delta_blob_id, status
semantic_hashes:
  row_id -> subject_id, hash_kind, normalization_version, hash,
  evidence_scope, limitations
```

### 5.9 Export, MCP, and policy

```text
export_manifests:
  row_id -> capture_run_id, schema_digest, format, requested_tables,
  table_checksum_set_hash, export_file_manifest_hash, created_at
table_checksums:
  capture_run_id + table_name + checksum_version -> row_count,
  checksum, schema_digest
mcp_tool_calls:
  row_id -> tool_name, request_hash, response_hash, returned_rows,
  returned_bytes, truncated, cursor_id?, policy_decision
mcp_response_limits:
  tool_name -> max_rows, max_bytes, source_bytes_allowed
pagination_cursors:
  cursor_id -> table_name, query_hash, last_sort_key, expires_at?
schema_introspection:
  schema_version + table_name + field_name -> ordinal, logical_type,
  required, identity_input, checksum_input, references_table?
source_leak_policy:
  policy_version + data_kind -> default_action, allowed_surface,
  requires_approval, max_bytes?
```

MCP is read-only and bounded. Cursor IDs are integrity-protected but ephemeral;
they are excluded from table checksums.

## 6. Referential and integrity rules

1. Every non-null `*_id` references an existing row of its declared domain.
2. Every run-scoped row references one `capture_runs` row and the same
   workspace/context lineage, unless schema introspection marks it derived
   across runs.
3. Byte ranges are half-open: `byte_start <= byte_end <= blob.byte_length`.
4. Normalized source paths are unique per workspace snapshot. Case collisions
   are validation errors on case-insensitive targets.
5. Blob length and content hash are verified on every import and restore.
6. `capture_runs.status = complete` is forbidden while fatal validation errors
   are unresolved. Known incomplete observation must have a gap row.
7. Deleting a referenced blob is forbidden. `blob_ref_counts` is derived and
   must equal the count obtained from all blob references.
8. A schema version not explicitly supported by the binary fails closed; no
   schema-less writes or implicit migration are allowed.

## 7. Checksums and deterministic export

For table `T`, sort rows by `row_id`, project only fields where
`schema_introspection.checksum_input = true`, encode each projected row as
canonical JSON, and compute:

```text
table_checksum = blake3(
  "codedb.table.v1\0" + schema_digest + "\0" + T + "\0"
  + row_count_decimal + "\0"
  + canonical_row_1 + "\n" + ... + canonical_row_n + "\n"
)
```

The checksum projection excludes `observed_at`, export timestamps, pagination
cursors, database layout, and machine-local presentation paths. It includes
stable IDs, all semantic payload, provenance references, blob hashes, gap and
validation state, and policy decisions. Exact blob bytes are represented by
their content hash and byte length rather than duplicated into the projection.

An export checksum set sorts `(table_name, table_checksum)` by table name and
hashes the canonical JSON list with the `codedb.export-set.v1` domain. A
repeated unchanged scan in the same declared context must produce identical
row IDs, row counts, table checksums, and checksum-set hash. A change in
schema, inputs, context, gap status, or evidence must change the relevant
checksum.

## 8. Static semantic hash inputs (CDB085)

Static semantic hashes are built from normalized Rust item rows: relative
path, module path, item kind, item name, visibility, identity kind, and
identity note. Public API hashes use the same normalized inputs but include
only public rows.

They intentionally exclude function bodies, type layout, macro expansion, and
rustc semantic checks. These limitations must be recorded in
`semantic_hashes.limitations`; the hashes are proof aids and never replace
Cargo, rustc, test, or reproduction gates.

## 9. Acceptance invariants

The schema is accepted only when:

- every table in section 3 appears exactly once in schema introspection;
- every field has a logical type, nullability, identity/checksum flags, and
  reference target where applicable;
- all IDs match their domain prefix and recompute from canonical inputs;
- all references, spans, blob hashes, and ref counts validate;
- missing or uncertain facts are explicit gaps/errors;
- unchanged fixtures reproduce identical table checksums;
- source bytes and sensitive values obey export/MCP policy without corrupting
  stored identity;
- no-mutation, backup/restore, and reproduction proof rows retain exact hashed
  evidence.

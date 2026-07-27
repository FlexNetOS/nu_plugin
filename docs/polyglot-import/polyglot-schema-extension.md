# Polyglot Schema Extension

This document plans the V1.2 polyglot tables as an additive extension to the
normative V1.1 contract in `docs/SCHEMA.md`. It fixes table responsibilities,
minimum fields, identity inputs, and compatibility rules for later
implementation. It does not declare these tables implemented.

## 1. Compatibility Contract

The extension has the following non-negotiable rules:

1. V1.1 table names, fields, natural keys, checksums, and ID domains do not
   change. In particular, the `source_files`, `source_blobs`, `source_spans`,
   `cargo_*`, `rust_*`, `capture_gaps`, `validation_errors`, and proof tables
   remain valid without migration.
2. Polyglot tables use the V1.1 common row envelope and canonical encoding.
   They reference existing `workspace_id`, `source_file_id`, `blob_id`,
   `source_span_id`, `capture_run_id`, and `context_id` values rather than
   creating parallel file, blob, span, run, or context identities.
3. Rust-specialized tables remain authoritative for Rust facts. Shared graph
   rows are optional, derived projections and never replace or weaken a
   specialized Rust row.
4. A projection is reproducible from its specialized source rows. Importers
   must not dual-write independent Rust and polyglot facts that can drift.
5. Stored bytes and metadata are not normalized away. Parser summaries refer
   to the exact `source_files.blob_id` observed by the parser.
6. CodeDB remains a fact store. No database row becomes authoritative source
   merely because it parsed successfully; source-truth promotion requires the
   repeated P6-P9 export and round-trip evidence defined in
   `proof-and-round-trip-gates.md`.

The V1.2 schema version is registered alongside V1.1. Readers that only know
V1.1 may ignore unknown V1.2 tables and continue reading all V1.1 tables.
Writers must never encode a V1.2 row under schema version `1`.

## 2. Common Extension Conventions

All tables below include the V1.1 common row envelope:

```text
row_id, schema_version, capture_run_id, context_id, provenance_kind,
observed_at
```

`context_id` is required for parser-, resolver-, build-, or runtime-dependent
facts. It may be null only for context-free byte or marker observations, using
the same exception represented by V1.1 context-free store facts. Optional
fields are written as null, not omitted.

New generic row IDs use the V1.1 `row` prefix and identity construction:

```text
row_id = row_<blake3("codedb.v1\0" + table_name + "\0"
                     + canonical_json(natural_key))>
```

The existing `fil`, `blb`, `spn`, `ctx`, `run`, and `prf` domains are reused.
No polyglot-specific alias may be minted for an existing entity. Natural keys
include versioned parser evidence where that evidence can change the emitted
fact.

### 2.1 Evidence and confidence

Rows produced from parsing or indexing carry:

```text
parser_version_id?, indexer_version_id?, evidence_kind, confidence,
source_file_id, source_blob_hash, source_span_id?
```

`evidence_kind` is one of `byte_match`, `manifest_parse`, `cst`, `ast`,
`static_index`, `compiler`, `runtime`, or `derived_projection`. `confidence`
is `exact`, `parser_reported`, `indexer_reported`, or `heuristic`; it is not a
floating-point score. A heuristic row must reference a capture gap that states
what stronger evidence is absent.

## 3. Table Registry

| Group | Tables |
|---|---|
| Detection and tooling | `language_kinds`, `language_detectors`, `language_detections`, `parser_backends`, `parser_versions`, `parser_runs`, `indexer_backends`, `indexer_versions` |
| Package and lockfile | `repo_package_managers`, `repo_packages`, `repo_package_members`, `repo_lockfiles`, `repo_lockfile_packages`, `repo_dependency_edges` |
| Shared structural graph | `polyglot_modules`, `polyglot_symbols`, `polyglot_imports`, `polyglot_references`, `polyglot_call_edges`, `polyglot_projection_links` |
| Config and build | `polyglot_config_files`, `polyglot_build_files`, `polyglot_runtime_scripts`, `polyglot_generated_files` |
| Validation | `polyglot_capture_gaps`, `polyglot_validation_errors` |
| Single-binary export | `single_binary_export_runs`, `single_binary_embedded_blobs`, `single_binary_materialization_proofs` |

All registry entries must appear in schema introspection and export manifests,
even when empty.

## 4. Detection and Tool Provenance

```text
language_kinds:
  language_kind -> display_name, tier_ceiling, extensions,
  filename_markers, shebang_markers

language_detectors:
  detector_id -> name, detector_kind, implementation_version,
  rule_set_hash

language_detections:
  source_file_id + detector_id + rule_set_hash ->
  language_kind, confidence, matched_rule, source_blob_hash

parser_backends:
  parser_backend_id -> name, parser_family, supported_language_kinds,
  execution_policy

parser_versions:
  parser_version_id -> parser_backend_id, version, grammar_hash?,
  executable_hash?, configuration_hash

parser_runs:
  parser_run_id -> parser_version_id, source_file_id, source_blob_hash,
  parse_mode, result_status, diagnostics_hash?, summary_hash?

indexer_backends:
  indexer_backend_id -> name, index_format, supported_language_kinds,
  execution_policy

indexer_versions:
  indexer_version_id -> indexer_backend_id, version, executable_hash?,
  configuration_hash
```

A file may have multiple detection rows. A parser run must select one declared
language and must consume the same blob referenced by the selected detection.
Conflicting exact detections are validation errors; uncertain or unsupported
detections are capture gaps. Tool version, grammar, and configuration hashes
are part of reproducibility and may not be replaced by an unversioned tool
name.

## 5. Package and Lockfile Facts

```text
repo_package_managers:
  workspace_id + manager_kind + manager_root_file_id ->
  manager_version?, workspace_root_path, detection_source

repo_packages:
  repo_package_id -> workspace_id, package_manager_id, name, version?,
  manifest_file_id, package_root_path, source_locator?, package_kind

repo_package_members:
  parent_repo_package_id + member_repo_package_id -> membership_kind,
  declaration_span_id?

repo_lockfiles:
  lockfile_id -> workspace_id, package_manager_id, source_file_id,
  source_blob_hash, format_version?, parse_status

repo_lockfile_packages:
  lockfile_id + locked_package_key -> name, version?, source_locator?,
  integrity_hash?, metadata_hash

repo_dependency_edges:
  depender_repo_package_id + dependency_name + dependency_kind
  + target_predicate? + declaration_span_id? ->
  dependency_repo_package_id?, locked_package_key?, requirement?,
  resolved_version?, optional, source_file_id, source_blob_hash
```

`repo_packages` is the cross-ecosystem package identity. Cargo remains in the
existing `cargo_*` tables. A Rust package may be represented in
`repo_packages` only as a deterministic projection linked through
`polyglot_projection_links`; the projection cannot replace `cargo_packages`,
`cargo_dependencies`, or `cargo_locks`.

Lockfile parsing is observational. It must not install, resolve, refresh, or
rewrite dependencies. Unresolved names remain nullable foreign keys with exact
declaration evidence; they are not silently dropped.

## 6. Shared Structural Graph

```text
polyglot_modules:
  module_id -> workspace_id, repo_package_id?, language_kind,
  canonical_name?, source_file_id, source_blob_hash, source_span_id?,
  parser_run_id?, identity_status

polyglot_symbols:
  symbol_id -> module_id, language_kind, symbol_kind, stable_name?,
  qualified_name?, visibility?, source_file_id, source_blob_hash,
  source_span_id?, parser_run_id?, indexer_version_id?, identity_status

polyglot_imports:
  importing_module_id + source_span_id + imported_text ->
  imported_module_id?, import_kind, alias?, condition_text?,
  parser_run_id, resolution_status

polyglot_references:
  referring_symbol_id? + source_span_id + referenced_text ->
  referenced_symbol_id?, reference_kind, parser_run_id?,
  indexer_version_id?, resolution_status

polyglot_call_edges:
  caller_symbol_id + call_site_span_id + callee_text ->
  callee_symbol_id?, dispatch_kind, parser_run_id?,
  indexer_version_id?, resolution_status

polyglot_projection_links:
  polyglot_row_id + specialized_table + specialized_row_id ->
  projection_kind, projection_version, projection_hash
```

Module and symbol identities are scoped by workspace, package when known,
language, exact source blob, source span when available, context, and parser or
indexer normalization version. Rows without a stable source anchor set
`identity_status = unstable_identity` and reference a capture gap.

Unresolved imports, references, and calls remain useful facts:
`resolution_status` is `resolved`, `ambiguous`, `unresolved`, or
`not_attempted`. Importers must not invent target IDs or discard ambiguous
candidates.

## 7. Config, Build, Runtime, and Generated Surfaces

```text
polyglot_config_files:
  source_file_id + config_kind -> language_kind?, repo_package_id?,
  source_blob_hash, parser_version_id?, schema_uri?, parse_status

polyglot_build_files:
  source_file_id + build_system_kind -> repo_package_id?,
  source_blob_hash, parser_version_id?, execution_required

polyglot_runtime_scripts:
  source_file_id + runtime_kind -> repo_package_id?, source_blob_hash,
  entrypoint_name?, execution_status, approval_id?

polyglot_generated_files:
  source_file_id -> generator_kind?, generation_run_id?,
  declared_by_file_id?, source_blob_hash, generated_status
```

Detection or parsing does not authorize execution. Default imports use
`execution_status = not_executed`; dynamic facts require the existing unsafe
approval and execution provenance. A generated marker never removes the file
from Tier 0 byte capture.

## 8. Validation and Failure Rows

The extension-specific validation tables mirror, but do not replace, V1.1
`capture_gaps` and `validation_errors`:

```text
polyglot_capture_gaps:
  capture_run_id + gap_kind + subject_id? + language_kind?
  + detail_hash -> severity, description, required_backend?,
  parser_version_id?, resolution_status, parent_capture_gap_id?

polyglot_validation_errors:
  capture_run_id + error_code + subject_id? + language_kind?
  + detail_hash -> severity, message, validator, parser_version_id?,
  resolution_status, parent_validation_error_id?
```

`parent_capture_gap_id` and `parent_validation_error_id` link to the existing
generic V1.1 rows when an issue affects whole-run completeness or validity.
This preserves existing doctor/export behavior while allowing language and
tool detail. Unsupported grammar, parser failure, malformed manifests,
conflicting detection, stale blob evidence, broken foreign keys, and
policy-denied execution must be explicit rows.

## 9. Single-binary Export and Proof

```text
single_binary_export_runs:
  export_run_id -> workspace_id, source_capture_run_id, schema_digest,
  table_manifest_hash, blob_manifest_hash, policy_hash,
  generated_crate_id, status

single_binary_embedded_blobs:
  export_run_id + blob_id -> content_hash, byte_length, media_type?,
  inclusion_policy, embedded_path, embedded_hash

single_binary_materialization_proofs:
  proof_id -> export_run_id, subject_id, destination_manifest_hash,
  expected_bytes_hash, materialized_bytes_hash, metadata_hash,
  comparison_policy_hash, status, degraded_reason?
```

These tables extend the existing proof and generated-artifact families. Blob
bytes remain governed by `source_leak_policy`; a withheld blob is represented
by policy and manifest facts, never an empty byte value. Only a successful P9
proof establishes equality for the exact covered bytes and metadata.

## 10. Foreign-key and Integrity Gates

An implementation is conformant only if all of the following hold:

- every source-bearing row resolves to one existing `source_files` row and its
  exact `source_blobs` content hash;
- every span belongs to that same source file and falls within its byte length;
- every parser/indexer-produced row resolves to immutable version and
  configuration provenance;
- every graph edge resolves its source endpoint; nullable targets have an
  explicit non-`resolved` status;
- every Rust projection resolves to an existing specialized Rust/Cargo row and
  a reproducible `projection_hash`;
- deletion or regeneration of all shared Rust projections leaves every V1.1
  Rust and Cargo row byte-for-byte and checksum-for-checksum unchanged;
- V1.1-only export/import round trips retain their original table checksums;
- mixed-repository fixtures cover Rust plus at least one non-Rust language,
  conflicting/unknown detection, parser failure, unresolved edges, malformed
  lockfiles, generated files, and policy-denied execution;
- P2-P6 and P9-P10 evidence is recorded before any source-authority claim.

These gates are the concrete meaning of “planned schema does not break
Rust-first tables.” A later implementation that cannot satisfy them must emit
validation errors and stop schema promotion rather than migrate or reinterpret
V1.1 data silently.

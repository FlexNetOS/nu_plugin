# Security and Secret Policy

Source of truth: PRD sections 13.3 and 15, especially 15.3-15.4.

## Scope and invariants

This policy applies to scans, redb stores, exports, generated artifacts,
CLI/Nushell output, MCP responses, diagnostics, traces, and raw evidence logs.

- Default capture is read-only and deterministic.
- A source path is not authorization to disclose its bytes.
- Secret-looking content must not enter tracked artifacts, prompts, ordinary
  command output, MCP output, or failure messages.
- Hashes, spans, counts, identities, and provenance may be returned only when
  they do not reproduce the sensitive value.
- Redaction is an output control, not permission to retain a raw secret in a
  tracked file.
- A suspected leak is a hard stop. Record a sanitized `validation_errors` row
  and do not echo the matched value.

## Source blob modes

The selected mode and its approval provenance must be recorded for each
capture. Mode changes do not retroactively authorize an existing blob.

| Mode | Raw bytes | Required behavior |
|---|---|---|
| `refuse` | Not stored | Stop when secret-looking content is detected. This is the default for tracked or exported artifacts. |
| `hash-only` | Not stored | Store metadata and a content hash only. Queries and exports must not reconstruct source. |
| `local-only` | Stored only in an explicitly selected, untracked, operator-owned local redb | Export hashes/metadata only. This mode is never implied by a store path and is never exposed through MCP. |
| `allow` | Stored only for controlled fixtures | Require explicit operator approval identifying the fixture and purpose. It is not valid for an ordinary source repository. |

The deprecated labels `metadata-only`, `hashed-blob`, `redacted-export`, and
`raw-local` are not policy modes. Implementations must use the four canonical
mode names above.

Before raw bytes are persisted, the implementation must classify the
destination as tracked/exported, untracked local, or controlled fixture and
apply the mode gate. If classification or secret detection cannot complete,
fail closed and emit sanitized validation evidence.

## Secret detection and disposition

Detection must cover at least configured secret paths, known fixture markers,
credential/token/private-key patterns, and values supplied to the no-leak test
harness. Detection results contain rule identifiers, locations, counts, and
hashes where safe; they never contain the matched bytes.

| Condition | Disposition |
|---|---|
| Match in `refuse` mode | Abort the affected blob/artifact and emit a sanitized `validation_errors` row. |
| Match in `hash-only` mode | Retain metadata/hash only; record that raw persistence was denied. |
| Match in `local-only` mode | Keep bytes only in the approved untracked store; redact all exports and summaries. |
| Match in `allow` mode | Proceed only when fixture-scoped approval is valid and recorded. |
| Match in output, evidence, or a tracked artifact | Stop, quarantine the output without displaying it, and require regeneration after remediation. |

Raw evidence needed for debugging must remain local and access-controlled.
Tracked summaries may include only redacted facts. Logs and errors must use
labels or digests rather than secret-looking values.

## CLI and Nushell output guard

Normal CLI and plugin commands return table facts, summaries, hashes, spans,
counts, and bounded pages. A local CLI command that intentionally exposes
source must be explicit, must reapply the selected blob policy, and must never
be used as an implicit fallback for metadata queries.

Stdout, stderr, tracing, panic messages, and test failure text are all output
surfaces. They must pass the same leak guard. Debug formatting of request
objects, blobs, environment values, or matched detector input is prohibited.

## MCP source-leak guard

V1.1 MCP is read-only, bounded, and source-denying by default. It may expose
schema, paginated table rows, capture gaps, validation errors, summaries,
hashes, spans, counts, and no-mutation proofs.

The following remain blocked:

- raw source/blob reads and full-file dumps;
- unbounded table dumps or pagination bypass;
- unsafe build/proc-macro capture;
- source overwrite, patch application, or Git mutation.

MCP must enforce row and byte limits before serialization and recheck the
serialized response. Truncation must not split into or reveal a sensitive
value. There is no MCP approval that enables dynamic execution or raw source
in V1.1; an explicitly authorized local CLI workflow is required instead.

## Verification and incident behavior

Security acceptance requires tests proving:

- every source blob mode fails or persists exactly as specified;
- known secret-like fixture values are absent from stdout, stderr, MCP
  responses, exports, manifests, and failure messages;
- MCP raw-source and unsafe-execution requests are refused;
- pagination and byte limits cannot be bypassed;
- tracked files and release artifacts pass the secret-hygiene scan.

The plugin transport guard uses the isolated `fixtures/secret_like` fixture and
checks metadata/table surfaces plus MCP tests. Its report contains labels,
output hashes, row counts, and `secret_like_values: absent`, never the fixture
values.

On a leak, stop the operation, preserve only sanitized evidence, identify every
affected output, remove or quarantine it through the operator-approved
recovery process, rotate real credentials when applicable, and rerun the full
no-leak gate. A redacted summary is not evidence that the original leaked
artifact is safe.

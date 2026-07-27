# Security And Execution Policy

## Hard Boundaries

- no package-manager dependency installation or project script execution by default
- no raw source over MCP by default
- no credential dump
- no hidden Git mutation
- no source overwrite
- no dynamic runtime/build execution without an explicit future unsafe gate
- no claim of database-owned source truth until repeated round-trip proof exists

## Operational Rules

- Prefer pure Rust libraries on the default path where feasible.
- External CLIs are allowed only behind optional/gated commands.
- License review is required before code adoption.
- No downloaded runtimes or package caches in default tests.
- No heavyweight fixture blobs.
- Missing evidence must be recorded as QUESTION, GAP, or BLOCKER.

## Default View Policy

Nu, CLI, and MCP use the shared bounded-view pipeline defined in
[whole-repo import architecture](whole-repo-import-architecture.md). Default
views are read-only, offline, metadata-first, and table-shaped. Each request is
restricted to an allowlisted logical view and projection and is subject to
finite server-side row and serialized-byte ceilings. File, input-byte,
diagnostic, nesting/depth, scan, and elapsed-resource ceilings also apply when
the view can consume those resources.

Configured maxima are authoritative. A request may select a smaller limit but
cannot enlarge or disable a maximum. Limits apply to work performed as well as
rows returned, so selective filters, empty projections, counts, aggregation,
pagination, compression, and alternate encodings cannot turn a bounded request
into an unbounded scan. Over-budget work terminates with a typed,
sanitized limit result; it does not silently return a complete-looking answer.

Pages use deterministic total ordering. Continuation cursors are opaque,
integrity-protected capabilities bound to the store identity/version, view,
projection, normalized filter, order, policy identity, and last emitted sort
key. Malformed, replayed against different bindings, expired, or otherwise
invalid cursors fail closed. A response identifies its enforced limits and
reports `complete` or explicit `truncated` state; a truncated collection
provides a continuation cursor when safe. Omitted counts may be `unknown` when
an exact count would exceed a budget.

## Source And Secret Disclosure

- Raw source/blob reads remain blocked through MCP. No request parameter,
  cursor, approval token, alternate tool, diagnostic, or error path changes
  that rule.
- Local Nu/CLI raw-source access, if implemented, must be a separately named
  opt-in command. It must reapply the source-blob mode and approval provenance
  from the repository-wide
  [security and secret policy](../SECURITY_AND_SECRET_POLICY.md); metadata
  commands never fall back to returning source.
- Full-file dumps, environment values, package-manager credentials, runtime
  output, and credential-like values are excluded from default projections.
- Secret-looking values are denied, redacted, or retained only as approved
  local-only evidence. Redaction returns a typed marker and safe metadata, not
  a partial value from which the original can be reconstructed.
- Limit and policy errors contain field/view identifiers and safe counts or
  digests only. They must not echo source, filters containing sensitive input,
  parser context, detector matches, or cursor internals.

Policy and redaction are applied before serialization, and the serialized
payload is checked again before release. Byte truncation must occur at row or
typed-field boundaries and must never split a value, UTF-8 sequence, secret,
or redaction marker. The same guard covers stdout, stderr, structured errors,
diagnostics, traces, and logs.

## Execution Separation

A query observes an existing capture. It must not install dependencies, invoke
package managers, fetch from the network, execute repository scripts,
interpreters, builds, tests, proc macros, build scripts, or external indexers,
write source, mutate Git, or refresh the capture implicitly.

Future runtime/build capture requires a separately named unsafe operation with
explicit approval, pinned executable identity, sandbox/resource policy, and
sanitized evidence. That approval authorizes only that operation; it does not
enable execution through a view or relax source/secret disclosure rules. MCP
has no dynamic execution path.

## Required Evidence

The bounded-view gate must exercise equivalent allowed requests through every
implemented Nu, CLI, and MCP surface. Evidence covers empty, normal, at-limit,
over-limit, malformed-filter, invalid/adversarial-cursor, binary,
secret-shaped, and unauthorized-source cases and records:

- requested and enforced row, byte, file, depth, diagnostic, scan, and time
  limits applicable to the request;
- stable order, cursor binding behavior, completion/truncation state, and
  omitted count or `unknown`;
- selected policy identity, redaction/denial decision, and response digest;
- absence of source/secret values from payloads, errors, diagnostics, and logs;
- absence of repository writes, Git mutation, network access, dependency
  installation, and runtime/build execution.

The hard boundaries are bound into the proof plan as follows:

| Boundary | Required gate evidence |
|---|---|
| Stored/exported data does not disclose credentials or silently promote database facts to source truth | P6 verifies policy-bound manifests, redaction decisions, capture gaps, and the snapshot/policy digest |
| Materialization cannot overwrite source, escape its selected root, leak refused credential-like bytes, or leave partial output | P9 independently compares allowed bytes and metadata and proves refusal, rollback, and unchanged inputs |
| Nu, CLI, and MCP queries remain bounded, read-only, offline, non-executing, and subject to identical source/secret policy | P10 exercises positive and adversarial requests on every implemented surface and records policy and response digests |
| Missing or failed safety evidence remains an explicit delivery blocker and cannot relax policy | P11 maps every unresolved safety case to dependency-ordered work with the applicable gate and negative fixtures |

P6, P9, P10, and P11 must cite the same applicable policy identity and digest.
`unsupported`, missing, skipped, truncated, or inconclusive evidence is not
authorization to weaken a boundary and cannot pass a gate. An intentional
policy change requires a separately reviewed policy revision; a fixture,
adapter, or surface limitation cannot create an exception.

Any limit bypass, silent truncation, unstable pagination, cursor rebinding,
secret/source leak, query-triggered execution or mutation, or materially
different enforcement between Nu, CLI, and MCP fails the gate.

# Unsafe Capture Policy

Source of truth: PRD sections 10.7, 10.8, 13.3, and 15.2.

## Default refusal

Static detection of `build.rs`, proc-macro crates/invocations, include edges,
and native/linker declarations is allowed during a normal read-only scan.
Executing build scripts or procedural macros is not read-only: it may run
arbitrary code, read files or environment values, invoke tools, access the
network, and write generated artifacts.

Consequently:

- `codedb capture build` refuses unless the explicit unsafe gate succeeds;
- trust in first-party source does not bypass the gate;
- missing dynamic facts become `capture_gaps`, not inferred facts;
- no dynamic execution is available through MCP in V1.1;
- unsafe capture must not mutate the scanned source repository.

## Approval gate

The request must deliberately name the unsafe action (for example,
`--unsafe-execute-build`) and bind approval to one capture attempt. Before any
code executes, validate and record:

- approval identifier, approver identity, task ID, and timestamp;
- canonical source repository/workspace path and before-state Git status plus
  file-hash manifest;
- exact command, CodeDB version, toolchain, target, profile, features, and
  relevant lock/config checksums;
- permitted build-script/proc-macro scope;
- isolated working, Cargo target, temporary HOME, and artifact directories;
- environment-variable allowlist and network policy;
- raw stdout/stderr log path outside the source tree;
- output artifact path, retention policy, and cleanup/recovery plan.

Approval is invalid if any field is missing, paths resolve inside the source
tree where prohibited, the request differs from the approved scope, or
before-state cannot be captured. Approval is single-use and does not authorize
later retries with changed inputs.

An illustrative local invocation is:

```text
codedb capture build /repo \
  --unsafe-execute-build \
  --approver operator-name \
  --task-id CDB033 \
  --before-state source-snapshot-recorded \
  --cleanup-plan remove-isolated-sandbox \
  --raw-log /local-evidence/capture.log \
  --store /local-evidence/repo.redb
```

Flag names may evolve, but weakening or inferring any approval field is not
permitted.

## Isolation and execution controls

Run the approved command in a fresh isolated build directory with the source
mounted/readable but not used for build outputs. Use a temporary HOME and
explicit Cargo target/output directories. Pass only the approved environment
allowlist; values must be filtered by the secret policy before persistence.
Network access is denied unless separately declared and bound in the approval.

Do not reuse caches, generated output, credentials, or an earlier sandbox
unless their identities are explicit approved inputs. Do not install
dependencies or alter lockfiles as an incidental capture step. Any attempted
source-tree write, scope escape, undeclared process/tool invocation, or
boundary failure stops capture and becomes a sanitized validation error.

## Evidence and raw logs

Preserve, subject to the secret policy:

- the `unsafe_execution_approval` receipt;
- build-script runs and proc-macro invocations;
- stdout/stderr and Cargo instructions;
- proc-macro input/output token-stream metadata, panics, environment access,
  and file-access observations when safely observable;
- `rerun-if-changed` and `rerun-if-env-changed` facts;
- native libraries, link paths/arguments, linker/pkg-config/cc invocations;
- OUT_DIR/generated-file identities, paths, sizes, and content hashes;
- toolchain/context metadata, exit status, timestamps/durations, and failure
  logs;
- `capture_gaps` and sanitized `validation_errors`.

Raw logs are mandatory evidence but are not automatically safe to publish.
They must be written outside the source tree, treated as local sensitive data,
scanned for secrets, and represented in tracked summaries by path policy,
checksum, and redacted facts only. Missing or unsafe evidence invalidates the
capture; it must not be silently omitted.

## Completion, cleanup, and reproduction

After execution:

1. close and checksum logs and captured artifacts;
2. compare source Git status and the file-hash manifest with before-state;
3. mark the run failed if source mutation occurred or cannot be ruled out;
4. remove the isolated sandbox according to the approved cleanup plan;
5. emit the approval/run receipt, evidence hashes, gaps, errors, and
   `no_mutation_proof`.

Captured OUT_DIR artifacts may be reproduced only into a new, explicitly
declared artifact directory using the bound approval/evidence record. They
must never be restored into the source tree by default.

## Refusal and acceptance tests

The gate must prove that:

- omission, misspelling, or indirect implication of the unsafe flag refuses;
- missing/expired/mismatched approval provenance refuses before execution;
- MCP cannot request or trigger dynamic capture;
- logs and build outputs cannot resolve inside the source repository;
- environment and network restrictions are enforced;
- source writes are detected by the before/after proof;
- secret-like values do not appear in receipts, summaries, or test failures;
- successful fixture capture records required rows, raw-log checksums,
  artifact hashes, cleanup status, and all unobserved facts as gaps.

Any failed gate leaves dynamic facts unclaimed and records only sanitized
failure evidence.

# Unsafe Capture Policy

Source: PRD sections 10.7, 10.8, and 15.2.

## Default refusal

Build scripts and proc macros execute compile-time code and may generate build
artifacts. Therefore dynamic observation remains an explicit operator action,
including for trusted first-party sources:

```text
codedb capture build
```

must refuse by default.

## Required explicit gate

Unsafe capture may run only with a deliberately named flag such as:

```text
codedb capture build /repo \
  --unsafe-execute-build \
  --approver operator-name \
  --task-id CDB078,CDB079,CDB080,CDB082 \
  --before-state source-snapshot-recorded \
  --cleanup-plan remove-isolated-sandbox \
  --raw-log /evidence/capture.log
```

and only after the selected task declares:

- task ID;
- source repo path;
- before-state hash/status;
- raw log path;
- output artifact path;
- cleanup plan;
- operator approval evidence.

The implementation enforces the raw-log destination outside the source tree,
runs Cargo in the isolated build directory, removes the sandbox afterward, and
keeps dynamic execution out of MCP. `--store` optionally persists the exact
row receipt; `codedb reproduce --approval-id ... --artifact-dir ...` restores
captured OUT_DIR artifacts into a new declared artifact directory.

`codedb capture compiler <source.rs>` applies the same explicit flag and named
provenance fields. It additionally requires `--repo-path`, a new
`--evidence-dir` outside that repository, and `--store`. The public approved
front door validates those boundaries, while the request-bound authority,
capability, and raw compiler executor remain private to `codedb-rust-static`.

## Evidence required

Unsafe capture must preserve stdout/stderr, Cargo instructions, native
`linked_libs`/`linked_paths`, OUT_DIR artifact hashes, environment allowlist,
and failure logs. Missing observations become `capture_gaps`.

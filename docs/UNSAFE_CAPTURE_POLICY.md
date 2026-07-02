# Unsafe Capture Policy

Source: PRD sections 10.7, 10.8, and 15.2.

## Default refusal

Build scripts and proc macros may execute arbitrary compile-time code. Therefore:

```text
codedb capture build
```

must refuse by default.

## Required explicit gate

Unsafe capture may run only with a deliberately named flag such as:

```text
codedb capture build --unsafe-execute-build
```

and only after the selected task declares:

- task ID;
- source repo path;
- before-state hash/status;
- raw log path;
- output artifact path;
- cleanup plan;
- operator approval evidence.

## Evidence required

Unsafe capture must preserve stdout/stderr, Cargo instructions, OUT_DIR artifact hashes, environment allowlist, and failure logs. Missing observations become `capture_gaps`.

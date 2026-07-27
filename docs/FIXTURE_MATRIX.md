# CodeDB V1.1 Fixture Matrix

Source of truth: PRD section 18. Names below are logical fixture identities;
their implementation may share a workspace only if each row remains
independently selectable and produces independent evidence.

| ID | Fixture | Required input | Expected rows and proof | Negative, gap, or safety assertion |
|---:|---|---|---|---|
| F01 | Single simple crate | One library package with source, manifest, and lock metadata | Source root, filesystem, Cargo package/target, source-file, module, item, function, and export rows | Repeated scans produce identical identities and table checksums |
| F02 | Workspace with two crates | Workspace with two members and an inter-package dependency | One workspace, two distinct package/member identities, targets, and resolved dependency edge | No member collapse, duplicate identity, or path ambiguity |
| F03 | Feature-gated code | Default and named features guarding Rust items | Feature definitions/selections, cfg expressions, and item context for each tested feature set | Disabled facts are not reported as enabled; static-but-inactive facts retain explicit context |
| F04 | Target-gated dependency | A dependency selected by a target `cfg` | Target-specific dependency declaration and resolved target/context rows | Host resolution is not claimed as universal; unresolved targets produce an explicit gap/error |
| F05 | `macro_rules!` crate | Local macro definition and invocations | Macro definition, invocation, source span, and context rows | Unobserved expansion/hygiene facts produce `capture_gaps`, never inferred compiler facts |
| F06 | Proc-macro consumer | Proc-macro crate plus a consumer invocation | Static proc-macro crate/dependency/invocation identification | Normal scan records a dynamic-expansion gap; execution refuses without valid explicit approval and is unavailable through MCP |
| F07 | Build script crate | Package containing `build.rs` and representative Cargo instructions | Static build-script target/file and declared native/rerun hints when statically observable | Normal scan records dynamic-output gaps; build execution refuses without valid approval |
| F08 | OUT_DIR generator | Approved build script that generates a known file in isolated OUT_DIR | Approved-run receipt, build-script run, generated-artifact identity/path/size/hash, raw-log checksum, and cleanup/no-mutation proof | No output resolves into the fixture; missing or mismatched approval refuses before execution |
| F09 | Native/link fixture | Native library/link search/link argument declarations | Native library, linker instruction, path/argument, provenance, and context rows where observable | Dynamically discovered native facts become gaps unless the approved unsafe phase captured them |
| F10 | include fixture | `include_str!`, `include_bytes!`, and `include!` references | Three typed static include edges with normalized source and destination paths | Missing, escaping, or unreadable targets become sanitized validation errors |
| F11 | Non-Rust asset crate | Rust crate with representative text and binary/non-Rust assets | Asset filesystem/source metadata, crate-envelope membership, size, and content hash under selected blob policy | Assets are not discarded merely because no Rust item references them |
| F12 | Symlink fixture | In-tree link and a declared boundary/unsupported case | Symlink entry, link text/normalized target, and boundary classification | If platform support is unavailable, emit a platform-limitation gap; never silently follow an escaping link |
| F13 | Secret-looking fixture | Controlled markers recognized only by the leak harness | Safe path/type/hash/count metadata according to mode; sanitized policy decision or validation error | `refuse`, `hash-only`, `local-only`, and fixture-approved `allow` behave exactly as specified; marker bytes are absent from every output surface |
| F14 | Dirty Git repository | Repository pre-dirtied by the harness with tracked and untracked state | Before/after Git status and tree checksums plus a no-mutation receipt preserving the exact pre-existing state | Scan neither cleans, stages, expands, nor worsens the dirty state |
| F15 | Generated artifact reproduction | Captured approved OUT_DIR artifact set and bound manifest | Reproduction into a fresh external directory, artifact-tree checksum parity, provenance link, and successful `cargo check` proof | Reproduction never writes into the source fixture; missing artifact or checksum mismatch fails closed |

## Cross-fixture requirements

All fifteen fixtures are mandatory. The clean-state no-mutation proof uses a
fresh F01 copy; the dirty-state proof uses F14. Each row must run through the
common procedure in [TEST_PLAN.md](TEST_PLAN.md), including repeated-scan
determinism, sanitized raw-log references, exported-table checksums, and
before/after source proof.

F06-F09 and F15 have two distinct phases: a normal static scan that must not
execute code, and an operator-approved isolated phase where the row requires
dynamic evidence. A static gap is the correct normal-scan result but does not
substitute for the approved-run evidence required by F08 or F15.

# CodeDB V1.1 Test Plan

Source of truth: PRD sections 18 and 19. Fixture expectations are defined in
[FIXTURE_MATRIX.md](FIXTURE_MATRIX.md). Security and dynamic-execution tests
must also obey [SECURITY_AND_SECRET_POLICY.md](SECURITY_AND_SECRET_POLICY.md)
and [UNSAFE_CAPTURE_POLICY.md](UNSAFE_CAPTURE_POLICY.md).

## Test invariants

Every test run uses an isolated temporary store, HOME, Cargo target directory,
and evidence directory. Network access is disabled. A test may read a fixture
but may not write into it, except that the dirty-repository harness creates its
declared pre-existing state before the scan. Unsafe build or proc-macro
execution is a separate, explicitly approved test phase and never runs through
MCP.

For each fixture, preserve:

- fixture identity and input-tree checksum;
- exact command, binary checksum, schema version, toolchain, target, profile,
  feature set, and source-blob mode;
- exit status and sanitized raw stdout/stderr log references;
- exported-table checksums, `capture_gaps`, and `validation_errors`;
- before/after Git status and input-tree checksums as no-mutation proof.

A pass requires all expected rows, gaps, refusals, and errors to match the
fixture matrix; zero unexpected validation errors; no secret-like value in any
output; and unchanged source status and checksums. Row ordering and generated
identities must be deterministic.

## Test groups

| Group | Required checks | Primary acceptance criteria |
|---|---|---|
| Unit | Schema serialization/versioning; stable identity; path normalization; row ordering; redaction; JSON, NUON, and CSV formatting; pagination and byte-limit boundaries. | 5, 9, 11, 15, 17 |
| Store | redb is the default and only enabled durable backend; initialize/open; schema mismatch; concurrent lock refusal; backup; restore; restored checksum and sample-query parity. | 4, 5, 25 |
| Filesystem scan | File, directory, symlink, and non-Rust asset capture; exact source metadata; unreadable/malformed input becomes a sanitized validation error. | 6, 8 |
| Cargo and context | `cargo metadata --format-version 1`; workspace/package/target/dependency/source provenance; lock, cfg, feature, target, profile, edition, and toolchain contexts. | 6, 10, 11 |
| Rust static | Items, modules, imports, `macro_rules!` definitions/invocations, proc-macro/build-script identification, include edges, and native/link declarations. Unsupported dynamic facts become gaps. | 7, 12, 13 |
| Unsafe capture | Default refusal; missing/mismatched approval refusal; sandbox/path/network/environment controls; approved build/proc-macro evidence; OUT_DIR artifacts; cleanup and no-mutation proof. | 7, 13, 14, 19 |
| Security | Exercise all four blob modes; fail closed on secret-like content; prove absence from stdout, stderr, logs, exports, manifests, errors, Nu output, and MCP responses. | 9, 15 |
| Determinism and no mutation | Scan unchanged fixtures twice into fresh stores and compare canonical table checksums; compare clean and pre-dirtied repositories before/after. | 6, 17, 18, 19 |
| CLI and Nu plugin | Build both binaries against the shared core; validate deterministic CLI JSON; load, filter, and join plugin tables in Nu; check protocol compatibility. | 1, 2, 16, 17 |
| Doctor and integrations | Validate host Nu, optional Yazelix Nu, optional Codex CLI, store path, and plugin protocol; test project ID/path input without meta mutation; validate envctl exports without redb access. | 3, 20, 21, 22, 23 |
| MCP | Read-only schema/query smoke; stable cursors; row and byte bounds; refusal of raw-source, mutation, and dynamic-execution requests; serialized-response leak scan. | 14, 15, 23 |
| Reproduction and release | Reproduce the approved artifact tree outside the source fixture; run `cargo check`; compare artifact/table checksums; then run format, lint, full tests, doctors, fixtures, link, manifest, and secret-hygiene gates. | 19, 24, 25 |

## Fixture execution procedure

For every row in `FIXTURE_MATRIX.md`:

1. Materialize a fresh fixture copy and record its tree checksum and Git state.
2. Run the normal read-only scan with dynamic execution disabled.
3. Assert the required tables, rows, gaps, refusals, and validation errors.
4. Export canonical JSON and the flat review tables; record table checksums.
5. Repeat the scan into a fresh store with identical inputs and compare
   canonical exports and table checksums.
6. Compare before/after fixture checksums and Git state.
7. Run any fixture-specific negative or operator-approved phase named in the
   matrix. Approved dynamic phases use a fresh sandbox and evidence directory.
8. Scan all serialized output and evidence summaries for the test harness's
   secret markers without printing those markers.

Platform-dependent expectations may pass only through the explicit alternative
in the matrix (for example, a sanitized platform-limitation gap for symlinks).
Skipping a required fixture or silently omitting an unavailable fact fails the
fixture gate.

## Acceptance trace

| PRD 19 criterion | Proof |
|---:|---|
| 1-3 | Workspace build plus CLI, Nu plugin, and `doctor` integration tests. |
| 4-5 | Store backend/config assertion, lock tests, and backup/restore parity. |
| 6-8 | Fixture scans, gap assertions, validation-error negative tests, and no-mutation receipts. |
| 9 | Four-mode source-blob suite and end-to-end leak scan. |
| 10-13 | Cargo/context/static and unsafe-capture fixture assertions. |
| 14-15 | MCP refusal, pagination, byte-limit, and post-serialization leak tests. |
| 16-17 | Nu table operations plus repeated canonical CLI JSON/checksum comparison. |
| 18-19 | Clean/dirty before-after receipts and runner evidence-field assertions. |
| 20-23 | envctl, meta, Yazelix/Nu, and Codex bridge contract tests. |
| 24 | Every required row in `FIXTURE_MATRIX.md` passes. |
| 25 | Release manifest contains and verifies binary, schema, table, redb, and artifact checksums. |

## Failure and evidence handling

Tests fail closed on source mutation, secret leakage, nondeterminism, missing
rows or evidence, unsafe execution without valid approval, unbounded MCP
output, or an unexpected dynamic action. Preserve raw failure logs outside the
source tree, subject them to the secret policy, and reference them by path and
checksum from sanitized test output. Never promote a missing fact to inferred
success: emit and assert a `capture_gaps` or `validation_errors` row instead.

The completion gate is **all required fixtures listed and passing**. Release
additionally requires the full PRD section 19 acceptance trace above to have
current-run evidence.

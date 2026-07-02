# Test Plan

Source: PRD sections 18 and 19.

## Test groups

| Group | Required proof |
|---|---|
| Unit | schema, identity, path normalization, redaction, export formatting |
| Store | redb init, schema version, lock behavior, backup/restore |
| Scan | read-only deterministic fixture scans |
| Cargo | metadata/lock/profile/feature capture fixtures |
| Rust static | item/import/macro/build-script/include/native static detection |
| Security | no raw secret leak through default outputs |
| No mutation | clean and dirty fixture before/after proof |
| Nu plugin | structured output and protocol doctor checks |
| MCP | bounded output, pagination, no raw source default |
| Yazelix | runtime Nu compatibility and init/extern bridge smoke |
| Release | fmt/clippy/test/doctor/fixture/manifest/link/secret scans |

## Execution discipline

Tests must preserve raw logs and record checksums. Unsafe build capture tests must verify refusal by default.

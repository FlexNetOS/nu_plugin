# ACCEPTANCE

The package is execution-ready only when:

- Navigation, readiness, drift, and stop gates exist.
- `TASK_GRAPH.csv` has unique task IDs and validation gates.
- `TASK_FILE_MAP.csv` maps every task to must-read and may-update files.
- Command ledger and worklog exist before implementation.
- PRD is canonical and standalone.
- Yazelix/Nushell bridge requirements are integrated.
- Manifest, checksums, link report, and secret hygiene scan pass.
- Any planning-only lane is labeled as planning-only and keeps V1.1 implementation status distinct.

The implementation is acceptable only when:

- scans are read-only and deterministic;
- source blobs obey secret policy;
- capture gaps are emitted for unobserved compiler reality;
- Nu plugin/CLI/MCP outputs are bounded and table-shaped;
- host Nu and Yazelix runtime Nu compatibility are checked;
- generated Yazelix init/extern bridge is state-only;
- envctl consumes exports/checksums, not redb internals;
- runner proof logs and manifests exist.

The polyglot planning package is acceptable only when:

- research, schema, language-surface, package-manager, proof-gate, and security docs exist;
- `execution/POLYGLOT_TASK_GRAPH.csv` and `execution/POLYGLOT_TASK_FILE_MAP.csv` parse cleanly;
- GitHub issue delivery drafts exist for CDB091-CDB105;
- the addendum and navigation surfaces say clearly that V1.2 is planning-only;
- no planning artifact is presented as completed code, runner proof, or release implementation.
The bidirectional roadmap package is acceptable only when:

- CDB070-CDB090 exist in GitKB and in `execution/BIDIRECTIONAL_TASK_GRAPH.csv`;
- all seven phases from issue 212 are represented;
- V1.1 gap closure coverage is explicit in `docs/GAP_CLOSURE_PLAN.md`;
- `docs/MUTATION_POLICY.md` preserves read-only defaults, bounded MCP, no hidden Git mutation, and no source overwrite before controlled apply gates;
- `scripts/validate_bidirectional_package.py` passes.

## Mandatory capability acceptance

All named CodeDB capabilities are mandatory. Release is blocked unless current-head tests positively prove compiler-observed macros, approval-gated proc macros and build scripts, generated artifacts, real Cargo/cfg/feature/target/toolchain contexts, HIR/MIR semantics, rustdoc/API equivalence, database-neutral storage parity, and complete reproduction. GAP, QUESTION, degraded, deferred, optional, planned, or documentation-only evidence cannot satisfy implementation acceptance.

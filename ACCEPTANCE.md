# ACCEPTANCE

The package is execution-ready only when:

- Navigation, readiness, drift, and stop gates exist.
- `TASK_GRAPH.csv` has unique task IDs and validation gates.
- `TASK_FILE_MAP.csv` maps every task to must-read and may-update files.
- Command ledger and worklog exist before implementation.
- PRD is canonical and standalone.
- Yazelix/Nushell bridge requirements are integrated.
- Manifest, checksums, link report, and secret hygiene scan pass.

The implementation is acceptable only when:

- scans are read-only and deterministic;
- source blobs obey secret policy;
- capture gaps are emitted for unobserved compiler reality;
- Nu plugin/CLI/MCP outputs are bounded and table-shaped;
- host Nu and Yazelix runtime Nu compatibility are checked;
- generated Yazelix init/extern bridge is state-only;
- envctl consumes exports/checksums, not redb internals;
- runner proof logs and manifests exist.

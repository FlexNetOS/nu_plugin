#!/usr/bin/env python3
"""One-way migration from optional/GAP closure to mandatory release blockers."""

from __future__ import annotations

import csv
import io
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def replace_required(relative: str, old: str, new: str) -> None:
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"required governance source not found in {relative}: {old!r}")
    path.write_text(text.replace(old, new), encoding="utf-8")


def append_once(relative: str, marker: str, body: str) -> None:
    path = ROOT / relative
    text = path.read_text(encoding="utf-8")
    if marker not in text:
        path.write_text(text.rstrip() + "\n\n" + body.rstrip() + "\n", encoding="utf-8")


def repair_prd() -> None:
    relative = "prd/nu_plugin_codedb_v1_1_full_prd.md"
    replace_required(relative, "rustdoc/API proof where enabled", "rustdoc/API proof")
    replace_required(
        relative,
        "- `cfg`, feature, target, profile, edition, toolchain context capture.\n",
        "- `cfg`, feature, target, profile, edition, toolchain context capture.\n"
        "- Compiler-facing HIR/MIR semantic capture under a pinned toolchain.\n"
        "- rustdoc JSON and public-API equivalence proof under a pinned toolchain.\n"
        "- Compiler-observed declarative-macro expansion, resolution, and hygiene evidence.\n",
    )
    replace_required(
        relative,
        "- Full semantic/HIR/MIR truth as mandatory V1.1 success.\n",
        "",
    )
    replace_required(
        relative,
        "After V1.1 proves capture correctness:\n",
        "After every mandatory V1.1 compiler and reproduction gate is implemented and proven:\n",
    )
    for deferred in (
        "- Rust-analyzer/HIR semantic backend.\n",
        "- rustdoc JSON API-delta backend with pinned nightly where approved.\n",
    ):
        replace_required(relative, deferred, "")
    append_once(
        relative,
        "## 25. Mandatory completion rule",
        """## 25. Mandatory completion rule

Every compiler, Cargo, macro, build, database, artifact, and reproduction capability named by this PRD is a release-blocking V1.1 deliverable. Nothing is optional. A `GAP`, `QUESTION`, degraded observation, missing test, or missing provenance row is required diagnostic evidence but cannot satisfy implementation acceptance or close a task. Approval-gated execution must remain refused by default and must also have a fully implemented, isolated, provenance-preserving approved path.
""",
    )


def repair_backlog() -> None:
    relative = "BACKLOG.md"
    for deferred in (
        "- rust-analyzer/HIR semantic backend\n",
        "- rustdoc JSON API-delta backend with pinned nightly where approved\n",
        "- compiler-observed macro expansion capture beyond static gap rows\n",
    ):
        replace_required(relative, deferred, "")
    append_once(
        relative,
        "## Mandatory V1.1 release blockers",
        """## Mandatory V1.1 release blockers

The following are not backlog candidates and block V1.1 completion until direct tests pass:

- compiler-observed macro expansion, resolution, and hygiene;
- approval-gated proc-macro and build-script capture with complete provenance;
- checksum-bound generated `OUT_DIR` artifacts;
- real cfg/feature/profile/host/target/toolchain/lockfile contexts;
- HIR/MIR semantic capture under a pinned compiler-facing backend;
- rustdoc JSON and public-API equivalence proof;
- materialize/check/test/rustdoc/checksum/provenance reproduction proof.
""",
    )


def repair_gap_docs() -> None:
    relative = "docs/GAP_CLOSURE_PLAN.md"
    replacements = {
        "compiler-observed expansion rail or explicit GAP rows": "compiler-observed expansion, resolution, and hygiene rail",
        "fixture proves dynamic expansion facts or gated refusal": "fixture proves compiler-observed expansion, resolution, and hygiene facts",
        "checksum-bound generated artifacts or GAP": "checksum-bound generated artifacts and environment provenance",
        "native/link rows or GAP": "approved dynamic native/link rows with provenance",
        "## Closed By CDB077": "## CDB077 Interim Evidence - Still Active",
        "## Closed By CDB078": "## CDB078 Interim Evidence - Still Active",
        "## Closed By CDB079": "## CDB079 Interim Evidence - Still Active",
        "## Closed By CDB080": "## CDB080 Interim Evidence - Still Active",
        "## Closed By CDB082": "## CDB082 Interim Evidence - Still Active",
        "## Closed By CDB085": "## CDB085 Interim Evidence - Still Active",
    }
    for old, new in replacements.items():
        replace_required(relative, old, new)
    append_once(
        relative,
        "## Mandatory closure semantics",
        """## Mandatory closure semantics

A GAP proves that CodeDB detected missing truth; it never proves that the capability was delivered. Every task in this plan remains active until its positive implementation path and failure path both have executable, current-head tests. Any remaining GAP blocks CDB090 and release readiness.
""",
    )

    replace_required(
        "docs/ROUND_TRIP_PROOF.md",
        "Round-trip proof must cover or explicitly gap:",
        "Round-trip proof must cover every item below; any explicit gap blocks acceptance:",
    )


def repair_task_graph() -> None:
    relative = "execution/BIDIRECTIONAL_TASK_GRAPH.csv"
    path = ROOT / relative
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        fieldnames = reader.fieldnames
        if fieldnames is None:
            raise SystemExit(f"missing CSV header: {relative}")
        rows = list(reader)

    mandatory_reverify = {f"CDB{number:03d}" for number in range(77, 91)}
    gates = {
        "CDB077": "compiler-observed expansion, resolution, and hygiene fixture proof",
        "CDB078": "default refusal plus approved proc-macro input/output/provenance proof",
        "CDB079": "default refusal plus approved build-script environment/instruction/log proof",
        "CDB080": "generated OUT_DIR artifact checksum and reproduction proof",
        "CDB082": "approved dynamic native/link fact and provenance proof",
        "CDB085": "HIR/MIR semantic and rustdoc public-API equivalence fixtures",
        "CDB090": "all mandatory capability tests, reproduction proof, and full validation",
    }
    for row in rows:
        task_id = row["task_id"]
        if task_id in mandatory_reverify:
            row["status"] = "active"
        if task_id in gates:
            row["validation_gate"] = gates[task_id]

    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=fieldnames, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    path.write_text(output.getvalue(), encoding="utf-8")


def repair_acceptance() -> None:
    mandatory = """## Mandatory capability acceptance

All named CodeDB capabilities are mandatory. Release is blocked unless current-head tests positively prove compiler-observed macros, approval-gated proc macros and build scripts, generated artifacts, real Cargo/cfg/feature/target/toolchain contexts, HIR/MIR semantics, rustdoc/API equivalence, database-neutral storage parity, and complete reproduction. GAP, QUESTION, degraded, deferred, optional, planned, or documentation-only evidence cannot satisfy implementation acceptance.
"""
    append_once("ACCEPTANCE.md", "## Mandatory capability acceptance", mandatory)
    append_once(
        "GOAL.md",
        "## Mandatory completion invariant",
        """## Mandatory completion invariant

Everything named by the product objective is mandatory. Missing observations must still be recorded, but every such row blocks completion until the positive implementation path is proven. Storage and query behavior must remain database-neutral and pass equivalent redb and PostgreSQL contracts.
""",
    )
    append_once(
        "docs/RELEASE_GATE.md",
        "## Mandatory compiler and reproduction gate",
        """## Mandatory compiler and reproduction gate

Release is blocked by any unresolved compiler/Cargo/macro/build/generated-artifact/HIR/MIR/rustdoc/database-parity/reproduction GAP. CDB090 cannot be satisfied by documentation, refusal-only tests, or a GAP-compatible validation gate. Every completed task must identify a current-head executable test and provenance artifact.
""",
    )
    append_once(
        "HANDOFF.md",
        "## Mandatory completion override",
        """## Mandatory completion override

All historical GAP and MVP deferral language is non-terminal. CDB077-CDB090 are active until positive implementation and current-head proof exist. The original task graph, bidirectional task graph, checklist, manifests, and release gate must be reconciled before completion.
""",
    )


def main() -> int:
    repair_prd()
    repair_backlog()
    repair_gap_docs()
    repair_task_graph()
    repair_acceptance()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

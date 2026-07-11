#!/usr/bin/env python3
"""Validate the bidirectional roadmap package required by issue 212."""

from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

from validate_requirement_proof_ledger import audit_ledger


REQUIRED_DOCS = [
    Path("docs/BIDIRECTIONAL_ROADMAP.md"),
    Path("docs/BIDIRECTIONAL_ARCHITECTURE.md"),
    Path("docs/ROUND_TRIP_PROOF.md"),
    Path("docs/CHANGE_PLAN_SCHEMA.md"),
    Path("docs/MUTATION_POLICY.md"),
    Path("docs/GAP_CLOSURE_PLAN.md"),
    Path("execution/BIDIRECTIONAL_TASK_GRAPH.csv"),
    Path("execution/BIDIRECTIONAL_TASK_FILE_MAP.csv"),
]

EXPECTED_TASK_IDS = [f"CDB{idx:03d}" for idx in range(70, 91)]
EXPECTED_PHASES = [
    "Phase 0",
    "Phase 1",
    "Phase 2",
    "Phase 3",
    "Phase 4",
    "Phase 5",
    "Phase 6",
]


def read_rows(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        return list(csv.DictReader(handle))


def audit_package(root: Path) -> list[str]:
    root = root.resolve()
    failures: list[str] = []
    for path in REQUIRED_DOCS:
        if not (root / path).is_file():
            failures.append(f"missing required bidirectional artifact: {path}")

    if failures:
        return failures

    graph_rows = read_rows(root / "execution/BIDIRECTIONAL_TASK_GRAPH.csv")
    file_rows = read_rows(root / "execution/BIDIRECTIONAL_TASK_FILE_MAP.csv")

    graph_ids = [row.get("task_id", "") for row in graph_rows]
    file_ids = [row.get("task_id", "") for row in file_rows]
    if graph_ids != EXPECTED_TASK_IDS:
        failures.append(f"task graph IDs mismatch: {graph_ids}")
    if file_ids != EXPECTED_TASK_IDS:
        failures.append(f"file map IDs mismatch: {file_ids}")

    graph_id_set = set(graph_ids)
    if len(graph_id_set) != len(graph_ids):
        failures.append("duplicate task IDs in bidirectional task graph")

    incomplete = [
        f"{row.get('task_id', '')}:{row.get('status', '')}"
        for row in graph_rows
        if row.get("status", "") != "complete"
    ]
    if incomplete:
        failures.append(f"bidirectional task graph has incomplete rows: {incomplete}")

    graph_text = (root / "execution/BIDIRECTIONAL_TASK_GRAPH.csv").read_text(encoding="utf-8")
    for phase in EXPECTED_PHASES:
        if phase not in graph_text:
            failures.append(f"missing required phase in bidirectional task graph: {phase}")

    safety_text = (root / "docs/MUTATION_POLICY.md").read_text(encoding="utf-8")
    for phrase in [
        "Default commands remain read-only",
        "No hidden Git mutation",
        "No direct source overwrite",
        "MCP remains read-only and bounded by default",
        "Missing evidence is QUESTION or GAP",
    ]:
        if phrase not in safety_text:
            failures.append(f"mutation policy missing phrase: {phrase}")

    gap_text = (root / "docs/GAP_CLOSURE_PLAN.md").read_text(encoding="utf-8")
    for gap in [
        "macro expansion",
        "proc-macro execution gate",
        "build-script execution gate",
        "OUT_DIR",
        "symlink",
        "native/linker",
        "raw source/blob reads through MCP",
        "anonymous/unstable syntax nodes",
        "semantic hashing",
        "store migrations",
        "source drift",
        "failed materialization/apply",
        "operator approvals",
    ]:
        if gap not in gap_text:
            failures.append(f"gap closure plan missing coverage: {gap}")

    ledger_violations = audit_ledger(root, require_all_verified=True)
    failures.extend(
        f"requirement proof ledger: {violation}" for violation in ledger_violations
    )
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()

    failures = audit_package(args.root)
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1

    print(
        "bidirectional package ok: 21 task rows and "
        "140 current-head requirement proofs"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

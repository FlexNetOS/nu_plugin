#!/usr/bin/env python3
"""Fail closed when mandatory CodeDB capabilities are deferred or GAP-closed."""

from __future__ import annotations

import argparse
import csv
import re
from dataclasses import dataclass
from pathlib import Path

from validate_requirement_proof_ledger import audit_ledger


@dataclass(frozen=True)
class Violation:
    path: str
    line: int
    rule: str
    text: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.rule}: {self.text.strip()}"


# These are compiler/reproduction capabilities named by the product objective.
# A GAP is required evidence when observation fails, but never a completion mode.
TEXT_RULES: dict[str, tuple[tuple[str, str], ...]] = {
    "prd/nu_plugin_codedb_v1_1_full_prd.md": (
        (r"Full semantic/HIR/MIR truth as mandatory V1\.1 success", "mandatory HIR/MIR deferred"),
        (r"rustdoc/API proof where enabled", "mandatory rustdoc proof made conditional"),
    ),
    "BACKLOG.md": (
        (r"rust-analyzer/HIR semantic backend", "mandatory HIR backend deferred to backlog"),
        (r"rustdoc JSON API-delta backend", "mandatory rustdoc backend deferred to backlog"),
        (r"compiler-observed macro expansion capture", "mandatory macro expansion deferred to backlog"),
    ),
    "docs/GAP_CLOSURE_PLAN.md": (
        (r"\bor explicit GAP rows\b", "GAP accepted as gap closure"),
        (r"\bor GAP\b", "GAP accepted as implementation proof"),
    ),
    "docs/ROUND_TRIP_PROOF.md": (
        (r"cover or explicitly gap", "missing round-trip coverage accepted"),
    ),
}

MANDATORY_GAP_TASKS = {
    "CDB077",  # macro expansion
    "CDB078",  # proc macros
    "CDB079",  # build scripts
    "CDB080",  # OUT_DIR artifacts
    "CDB082",  # native/linker facts
    "CDB085",  # semantic/API proof
}


def _line_violations(root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for relative_path, rules in TEXT_RULES.items():
        path = root / relative_path
        if not path.is_file():
            violations.append(Violation(relative_path, 0, "required authority missing", "file not found"))
            continue
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            for pattern, rule in rules:
                if re.search(pattern, line, flags=re.IGNORECASE):
                    violations.append(Violation(relative_path, line_number, rule, line))
    return violations


def _task_graph_violations(root: Path) -> list[Violation]:
    relative_path = "execution/BIDIRECTIONAL_TASK_GRAPH.csv"
    path = root / relative_path
    if not path.is_file():
        return [Violation(relative_path, 0, "required authority missing", "file not found")]

    violations: list[Violation] = []
    with path.open(newline="", encoding="utf-8") as handle:
        for line_number, row in enumerate(csv.DictReader(handle), 2):
            task_id = row.get("task_id", "")
            status = row.get("status", "").strip().lower()
            gate = row.get("validation_gate", "")
            if task_id in MANDATORY_GAP_TASKS and status == "complete" and "gap" in gate.lower():
                violations.append(
                    Violation(
                        relative_path,
                        line_number,
                        "mandatory task completed by GAP-compatible gate",
                        f"{task_id}: {gate}",
                    )
                )
    return violations


def audit_repository(root: Path) -> list[Violation]:
    root = root.resolve()
    return sorted(
        [*_line_violations(root), *_task_graph_violations(root)],
        key=lambda item: (item.path, item.line, item.rule),
    )


def audit_release(root: Path, *, require_all_verified: bool) -> list[object]:
    """Combine language-policy checks with direct proof-ledger validation."""

    return [
        *audit_repository(root),
        *audit_ledger(root, require_all_verified=require_all_verified),
    ]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--structure-only",
        action="store_true",
        help="validate policy and ledger structure without asserting release readiness",
    )
    args = parser.parse_args()
    violations = audit_release(
        args.root,
        require_all_verified=not args.structure_only,
    )
    if violations:
        print("mandatory capability policy: FAILED")
        for violation in violations:
            print(violation)
        return 1
    print("mandatory capability policy: PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

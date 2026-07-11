#!/usr/bin/env python3
"""Validate the exhaustive mandatory CodeDB requirement-to-proof ledger.

The release mode is deliberately fail closed: an implementation claim is not
verified unless its source, implementation, executable test, proof artifact,
and exact current Git revision are all present. A GAP/refusal is useful
diagnostic evidence, but it is never completion evidence.
"""

from __future__ import annotations

import argparse
import csv
import glob
import hashlib
import os
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

from requirement_proof_attestation import (
    CheckoutIdentity,
    load_receipt,
    validate_receipt,
)


LEDGER_PATH = Path("execution/REQUIREMENT_PROOF_LEDGER.csv")

CDB_REQUIREMENT_IDS = {
    *(f"CDB{index:03d}" for index in range(13, 64)),
    *(f"CDB{index:03d}" for index in range(77, 91)),
}
CDB106_REQUIREMENT_IDS = {f"CDB106-AC{index:02d}" for index in range(1, 11)}
REQ061_REQUIREMENT_IDS = {
    *(f"REQ-061-NFR{index:02d}" for index in range(1, 11)),
    *(f"REQ-061-ARCH{index:02d}" for index in range(1, 20)),
    *(f"REQ-061-CMD{index:02d}" for index in range(1, 12)),
    *(f"REQ-061-AC{index:02d}" for index in range(1, 13)),
    *(f"REQ-061-MISS{index:02d}" for index in range(1, 14)),
}
EXPECTED_REQUIREMENT_IDS = (
    CDB_REQUIREMENT_IDS | CDB106_REQUIREMENT_IDS | REQ061_REQUIREMENT_IDS
)

REQUIRED_COLUMNS = [
    "requirement_id",
    "parent_id",
    "requirement",
    "authoritative_source",
    "source_ref",
    "implementation_paths",
    "test_paths",
    "verification_command",
    "proof_artifacts",
    "proof_head_sha",
    "evidence_status",
    "task_status",
    "notes",
]

EVIDENCE_STATUSES = {"verified", "partial", "missing", "contradicted", "blocked"}
TASK_STATUSES = {"planned", "active", "complete", "blocked"}
EXECUTABLE_COMMAND = re.compile(
    r"(^|[;&|]\s*)(cargo|python3?|pytest|bash|sh|nu|nix|codedb|envctl)\b"
)
NON_PROOF_PREFIXES = ("docs/", "execution/", "logs/", "manifests/")


@dataclass(frozen=True)
class Violation:
    requirement_id: str
    rule: str
    detail: str

    def __str__(self) -> str:
        return f"{self.requirement_id}: {self.rule}: {self.detail}"


def read_ledger(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames != REQUIRED_COLUMNS:
            raise ValueError(
                f"ledger columns mismatch: expected {REQUIRED_COLUMNS}, got {reader.fieldnames}"
            )
        return list(reader)


def _split_paths(value: str) -> list[str]:
    return [item.strip() for item in value.split(";") if item.strip()]


def _resolve_paths(root: Path, value: str) -> list[Path]:
    resolved: list[Path] = []
    for item in _split_paths(value):
        if item.startswith(("external:", "gitkb:", "https://", "http://")):
            continue
        matches = [Path(match) for match in glob.glob(str(root / item), recursive=True)]
        resolved.extend(matches)
    return resolved


def _git_output(root: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


def _current_head(root: Path) -> str:
    return _git_output(root, "rev-parse", "HEAD")


def _current_tree(root: Path) -> str:
    return _git_output(root, "rev-parse", "HEAD^{tree}")


def _worktree_clean(root: Path) -> bool:
    return not _git_output(
        root,
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
    )


def _sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _graph_statuses(root: Path) -> dict[str, str]:
    statuses: dict[str, str] = {}
    for relative_path in [
        Path("execution/TASK_GRAPH.csv"),
        Path("execution/BIDIRECTIONAL_TASK_GRAPH.csv"),
    ]:
        path = root / relative_path
        if not path.is_file():
            continue
        with path.open(newline="", encoding="utf-8") as handle:
            for row in csv.DictReader(handle):
                task_id = row.get("task_id", "")
                if task_id in CDB_REQUIREMENT_IDS:
                    statuses[task_id] = row.get("status", "")
    return statuses


def validate_rows(
    root: Path,
    rows: list[dict[str, str]],
    *,
    expected_ids: set[str],
    current_head: str,
    require_all_verified: bool,
    graph_statuses: dict[str, str] | None = None,
    receipt_rows: dict[str, dict] | None = None,
) -> list[Violation]:
    violations: list[Violation] = []
    ids = [row.get("requirement_id", "") for row in rows]
    id_set = set(ids)

    for requirement_id in sorted(expected_ids - id_set):
        violations.append(Violation(requirement_id, "missing requirement row", "not in ledger"))
    for requirement_id in sorted(id_set - expected_ids):
        violations.append(
            Violation(requirement_id, "unexpected requirement row", "not in mandatory inventory")
        )
    for requirement_id in sorted({item for item in ids if ids.count(item) > 1}):
        violations.append(Violation(requirement_id, "duplicate requirement row", "must be unique"))

    graph_statuses = graph_statuses or {}
    for row in rows:
        requirement_id = row.get("requirement_id", "<missing>")
        if requirement_id not in expected_ids:
            continue

        for column in [
            "parent_id",
            "requirement",
            "authoritative_source",
            "source_ref",
            "implementation_paths",
            "test_paths",
            "verification_command",
        ]:
            if not row.get(column, "").strip():
                violations.append(Violation(requirement_id, "missing required field", column))

        authority = row.get("authoritative_source", "").strip()
        if authority and not authority.startswith(
            ("gitkb:", "https://", "http://")
        ) and not (root / authority).is_file():
            violations.append(
                Violation(requirement_id, "missing authoritative source", authority)
            )

        evidence_status = row.get("evidence_status", "").strip().lower()
        task_status = row.get("task_status", "").strip().lower()
        if evidence_status not in EVIDENCE_STATUSES:
            violations.append(
                Violation(requirement_id, "invalid evidence status", evidence_status or "<empty>")
            )
        if task_status not in TASK_STATUSES:
            violations.append(
                Violation(requirement_id, "invalid task status", task_status or "<empty>")
            )

        expected_task_status = graph_statuses.get(requirement_id)
        if expected_task_status and task_status != expected_task_status:
            violations.append(
                Violation(
                    requirement_id,
                    "task status contradicts authoritative graph",
                    f"ledger={task_status}, graph={expected_task_status}",
                )
            )

        if task_status == "complete" and evidence_status != "verified":
            violations.append(
                Violation(
                    requirement_id,
                    "task complete without verified proof",
                    f"evidence_status={evidence_status}",
                )
            )

        gap_text = f"{evidence_status} {row.get('notes', '')}".lower()
        if "gap" in gap_text and re.search(
            r"\b(closure|complete|completed|satisfied|proof)\b", gap_text
        ):
            violations.append(
                Violation(
                    requirement_id,
                    "GAP used as completion evidence",
                    row.get("notes", "") or evidence_status,
                )
            )

        if require_all_verified and evidence_status != "verified":
            violations.append(
                Violation(
                    requirement_id,
                    "release-blocking evidence status",
                    evidence_status or "<empty>",
                )
            )

        if evidence_status != "verified":
            continue

        implementation_items = _split_paths(row.get("implementation_paths", ""))
        if implementation_items and all(
            item.startswith(NON_PROOF_PREFIXES) for item in implementation_items
        ):
            violations.append(
                Violation(
                    requirement_id,
                    "documentation-only implementation proof",
                    row.get("implementation_paths", ""),
                )
            )

        for field, rule in [
            ("implementation_paths", "missing implementation path"),
            ("test_paths", "missing test path"),
        ]:
            items = _split_paths(row.get(field, ""))
            paths = _resolve_paths(root, row.get(field, ""))
            if not items or len(paths) < len(items):
                violations.append(
                    Violation(requirement_id, rule, row.get(field, "") or "<empty>")
                )

        command = row.get("verification_command", "").strip()
        if not EXECUTABLE_COMMAND.search(command):
            violations.append(
                Violation(requirement_id, "non-executable verification command", command)
            )

        proof_head = row.get("proof_head_sha", "").strip()
        if proof_head:
            violations.append(
                Violation(
                    requirement_id,
                    "self-referential legacy proof revision",
                    "proof_head_sha must be empty; exact commit identity belongs in the external attestation",
                )
            )

        logical_artifacts = _split_paths(row.get("proof_artifacts", ""))
        if not logical_artifacts:
            violations.append(
                Violation(
                    requirement_id,
                    "missing logical proof artifact",
                    "proof_artifacts must name receipt evidence, not committed generated files",
                )
            )
        receipt_row = (receipt_rows or {}).get(requirement_id)
        if receipt_row is None:
            violations.append(
                Violation(
                    requirement_id,
                    "missing external current-head attestation",
                    current_head,
                )
            )
        else:
            receipt_evidence = {
                item.get("logical_name", "")
                for item in receipt_row.get("evidence", [])
                if isinstance(item, dict)
            }
            for logical_artifact in logical_artifacts:
                if logical_artifact not in receipt_evidence:
                    violations.append(
                        Violation(
                            requirement_id,
                            "receipt missing logical proof artifact",
                            logical_artifact,
                        )
                    )

    return sorted(
        violations,
        key=lambda item: (item.requirement_id, item.rule, item.detail),
    )


def audit_ledger(
    root: Path,
    *,
    require_all_verified: bool,
    receipt_path: Path | None = None,
    require_trusted_ci: bool = True,
) -> list[Violation]:
    root = root.resolve()
    if receipt_path is None:
        configured_receipt = os.environ.get("CODEDB_REQUIREMENT_PROOF_RECEIPT")
        if configured_receipt:
            receipt_path = Path(configured_receipt)
    path = root / LEDGER_PATH
    if not path.is_file():
        return [Violation("*", "missing ledger", str(LEDGER_PATH))]
    try:
        rows = read_ledger(path)
    except (OSError, ValueError) as error:
        return [Violation("*", "invalid ledger", str(error))]

    current_head = _current_head(root)
    receipt_rows: dict[str, dict] = {}
    receipt_violations: list[Violation] = []
    verified_rows = [row for row in rows if row.get("evidence_status") == "verified"]
    if require_all_verified and verified_rows:
        if receipt_path is None:
            receipt_violations.append(
                Violation(
                    "*",
                    "missing external proof receipt",
                    "pass --receipt or CODEDB_REQUIREMENT_PROOF_RECEIPT",
                )
            )
        else:
            resolved_receipt = (
                receipt_path
                if receipt_path.is_absolute()
                else (Path.cwd() / receipt_path).resolve()
            )
            try:
                receipt = load_receipt(resolved_receipt)
                receipt_rows, failures = validate_receipt(
                    receipt,
                    identity=CheckoutIdentity(
                        commit_sha=current_head,
                        tree_sha=_current_tree(root),
                        ledger_sha256=_sha256_file(path),
                        validator_sha256=_sha256_file(Path(__file__).resolve()),
                        clean=_worktree_clean(root),
                    ),
                    ledger_rows=rows,
                    require_trusted_ci=require_trusted_ci,
                )
                receipt_violations.extend(
                    Violation("*", failure.rule, failure.detail)
                    for failure in failures
                )
            except (OSError, ValueError) as error:
                receipt_violations.append(
                    Violation("*", "invalid external proof receipt", str(error))
                )

    return [
        *receipt_violations,
        *validate_rows(
        root,
        rows,
        expected_ids=EXPECTED_REQUIREMENT_IDS,
        current_head=current_head,
        require_all_verified=require_all_verified,
        graph_statuses=_graph_statuses(root),
        receipt_rows=receipt_rows,
        ),
    ]


def _print_violations(violations: Iterable[Violation]) -> None:
    for violation in violations:
        print(violation)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument(
        "--structure-only",
        action="store_true",
        help="Validate inventory/schema/contradictions without asserting release readiness",
    )
    parser.add_argument(
        "--receipt",
        type=Path,
        default=(
            Path(value)
            if (value := os.environ.get("CODEDB_REQUIREMENT_PROOF_RECEIPT"))
            else None
        ),
        help="External receipt generated after checkout; never committed into the attested tree",
    )
    parser.add_argument(
        "--allow-local-receipt",
        action="store_true",
        help="Development only: do not require GitHub Actions generator/attestation fields",
    )
    args = parser.parse_args()

    violations = audit_ledger(
        args.root,
        require_all_verified=not args.structure_only,
        receipt_path=args.receipt,
        require_trusted_ci=not args.allow_local_receipt,
    )
    if violations:
        print("requirement proof ledger: FAILED")
        _print_violations(violations)
        return 1
    print(
        "requirement proof ledger: PASSED "
        f"({len(EXPECTED_REQUIREMENT_IDS)} mandatory requirement rows)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

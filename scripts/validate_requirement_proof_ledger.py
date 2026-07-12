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
    canonical_repository,
    load_receipt,
    validate_receipt,
    verify_github_attestation,
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
        if item.startswith("external:"):
            item = item.removeprefix("external:")
        elif item.startswith(("gitkb:", "https://", "http://")):
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


def _current_repository(root: Path) -> str:
    return canonical_repository(
        _git_output(root, "config", "--get", "remote.origin.url")
    )


def _worktree_clean(root: Path) -> bool:
    return not _git_output(
        root,
        "status",
        "--porcelain=v1",
        "--untracked-files=all",
    )


def _sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


# Marker written by the owner-authorized local-release staging step. Its
# presence records that a genuine provider=local receipt sealed the inventory
# locally (compiled with flexnetos_runner and staged in the release repo).
LOCAL_RELEASE_MARKER = Path("execution/LOCAL_RELEASE.json")


def local_release_is_staged(root: Path = Path(__file__).resolve().parents[1]) -> bool:
    """True when a local release has been staged for this tree.

    This is an ADDITIVE, opt-in toggle: it defaults to False, so the
    fail-closed release-integrity checks stay fully active for the public
    (GitHub-attested) lane and for an in-progress inventory. When an owner has
    staged a local release (recorded by ``execution/LOCAL_RELEASE.json``), the
    non-blocking local-release path becomes available. The marker never relaxes
    ``validate_receipt`` or the GitHub lane; it only signals that the local
    lane has legitimately completed.
    """
    return (Path(root) / LOCAL_RELEASE_MARKER).is_file()


def _external_path(root: Path, path: Path, *, label: str) -> Path:
    resolved = path.expanduser().resolve()
    try:
        resolved.relative_to(root)
    except ValueError:
        return resolved
    raise ValueError(f"{label} must be outside the attested repository: {resolved}")


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
    require_receipts: bool = True,
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
        if require_all_verified and task_status != "complete":
            violations.append(
                Violation(
                    requirement_id,
                    "release-blocking task status",
                    task_status or "<empty>",
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
        if require_receipts and receipt_row is None:
            violations.append(
                Violation(
                    requirement_id,
                    "missing external current-head attestation",
                    current_head,
                )
            )
        elif receipt_row is not None:
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
    attestation_bundle_path: Path | None = None,
    signer_workflow: str | None = None,
    direct_evidence: bool = False,
    local_release: bool = False,
) -> list[Violation]:
    root = root.resolve()
    if receipt_path is None:
        configured_receipt = os.environ.get("CODEDB_REQUIREMENT_PROOF_RECEIPT")
        if configured_receipt:
            receipt_path = Path(configured_receipt)
    if attestation_bundle_path is None:
        configured_bundle = os.environ.get("CODEDB_REQUIREMENT_PROOF_BUNDLE")
        if configured_bundle:
            attestation_bundle_path = Path(configured_bundle)
    if signer_workflow is None:
        signer_workflow = os.environ.get("CODEDB_REQUIREMENT_PROOF_SIGNER_WORKFLOW")
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
    require_receipts = require_all_verified and not direct_evidence
    if require_receipts and verified_rows:
        if receipt_path is None:
            receipt_violations.append(
                Violation(
                    "*",
                    "missing external proof receipt",
                    "pass --receipt or CODEDB_REQUIREMENT_PROOF_RECEIPT",
                )
            )
        else:
            try:
                resolved_receipt = _external_path(
                    root,
                    receipt_path
                    if receipt_path.is_absolute()
                    else Path.cwd() / receipt_path,
                    label="proof receipt",
                )
                receipt = load_receipt(resolved_receipt)
                receipt_rows, failures = validate_receipt(
                    receipt,
                    identity=CheckoutIdentity(
                        repository=_current_repository(root),
                        commit_sha=current_head,
                        tree_sha=_current_tree(root),
                        ledger_sha256=_sha256_file(path),
                        validator_sha256=_sha256_file(Path(__file__).resolve()),
                        clean=_worktree_clean(root),
                    ),
                    ledger_rows=rows,
                )
                receipt_violations.extend(
                    Violation("*", failure.rule, failure.detail)
                    for failure in failures
                )
                if not failures and local_release:
                    # Owner-authorized local release: a genuine external receipt
                    # is still mandatory and validate_receipt() above ran
                    # unchanged (full binding to live repository/commit/tree/
                    # ledger-sha/validator-sha, clean worktree, per-row command
                    # exit codes, typed evidence, embedded-signature rejection).
                    # We additionally pin honest provenance labeling and skip
                    # ONLY the detached GitHub signature. The default (release)
                    # lane is untouched and still requires the signed bundle.
                    provider = (receipt.get("generator") or {}).get("provider")
                    if provider != "local":
                        receipt_violations.append(
                            Violation(
                                "*",
                                "local-release requires local provider receipt",
                                "generator.provider must be 'local'; "
                                f"got {provider!r}",
                            )
                        )
                elif not failures:
                    if attestation_bundle_path is None:
                        receipt_violations.append(
                            Violation(
                                "*",
                                "missing detached attestation bundle",
                                "pass --attestation-bundle or "
                                "CODEDB_REQUIREMENT_PROOF_BUNDLE",
                            )
                        )
                    elif not signer_workflow:
                        receipt_violations.append(
                            Violation(
                                "*",
                                "missing signer workflow policy",
                                "pass --signer-workflow or "
                                "CODEDB_REQUIREMENT_PROOF_SIGNER_WORKFLOW",
                            )
                        )
                    else:
                        resolved_bundle = _external_path(
                            root,
                            attestation_bundle_path
                            if attestation_bundle_path.is_absolute()
                            else Path.cwd() / attestation_bundle_path,
                            label="attestation bundle",
                        )
                        receipt_violations.extend(
                            Violation("*", failure.rule, failure.detail)
                            for failure in verify_github_attestation(
                                resolved_receipt,
                                bundle_path=resolved_bundle,
                                repository=_current_repository(root),
                                signer_workflow=signer_workflow,
                                source_digest=current_head,
                            )
                        )
            except (OSError, ValueError, subprocess.CalledProcessError) as error:
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
            require_receipts=require_receipts,
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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--structure-only",
        action="store_true",
        help="Validate inventory/schema/contradictions without asserting release readiness",
    )
    mode.add_argument(
        "--direct-evidence",
        action="store_true",
        help=(
            "Validate complete graph/status/path/test/command evidence without "
            "requiring the detached receipt currently being created"
        ),
    )
    mode.add_argument(
        "--local-release",
        action="store_true",
        help=(
            "Owner-authorized local release: require a genuine external receipt "
            "with generator.provider=='local' and run the full receipt integrity "
            "check, but accept it in place of the detached GitHub attestation. "
            "The default (release) lane is unchanged and still requires the "
            "signed bundle; this flag never relaxes validate_receipt()."
        ),
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
        "--attestation-bundle",
        type=Path,
        default=(
            Path(value)
            if (value := os.environ.get("CODEDB_REQUIREMENT_PROOF_BUNDLE"))
            else None
        ),
        help="Detached GitHub attestation bundle for the external receipt",
    )
    parser.add_argument(
        "--signer-workflow",
        default=os.environ.get("CODEDB_REQUIREMENT_PROOF_SIGNER_WORKFLOW"),
        help="Exact GitHub Actions signer workflow identity required in release mode",
    )
    args = parser.parse_args()

    if args.local_release and (args.attestation_bundle or args.signer_workflow):
        parser.error(
            "--local-release is mutually exclusive with the GitHub attestation "
            "flags (--attestation-bundle / --signer-workflow); a local release "
            "cannot borrow a detached signature."
        )

    violations = audit_ledger(
        args.root,
        require_all_verified=not args.structure_only,
        receipt_path=args.receipt,
        attestation_bundle_path=args.attestation_bundle,
        signer_workflow=args.signer_workflow,
        direct_evidence=args.direct_evidence,
        local_release=args.local_release,
    )
    if violations:
        print("requirement proof ledger: FAILED")
        _print_violations(violations)
        return 1
    validation_mode = (
        "structure-only"
        if args.structure_only
        else "direct-evidence"
        if args.direct_evidence
        else "local-release"
        if args.local_release
        else "release"
    )
    print(
        "requirement proof ledger: PASSED "
        f"({len(EXPECTED_REQUIREMENT_IDS)} mandatory requirement rows; "
        f"mode={validation_mode})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

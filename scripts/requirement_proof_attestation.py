#!/usr/bin/env python3
"""Validate external, non-self-referential requirement-proof attestations."""

from __future__ import annotations

import hashlib
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 2
ATTESTATION_TYPE = "requirement-proof"
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")


@dataclass(frozen=True)
class CheckoutIdentity:
    commit_sha: str
    tree_sha: str
    ledger_sha256: str
    validator_sha256: str
    clean: bool


@dataclass(frozen=True)
class ReceiptViolation:
    rule: str
    detail: str

    def __str__(self) -> str:
        return f"{self.rule}: {self.detail}"


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_receipt_payload(receipt: dict[str, Any]) -> bytes:
    payload = {
        key: value
        for key, value in receipt.items()
        if key not in {"receipt_sha256", "signature"}
    }
    return (
        json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        + "\n"
    ).encode("utf-8")


def load_receipt(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("receipt root must be a JSON object")
    return value


def validate_receipt(
    receipt: dict[str, Any],
    *,
    identity: CheckoutIdentity,
    ledger_rows: list[dict[str, str]],
    require_trusted_ci: bool,
) -> tuple[dict[str, dict[str, Any]], list[ReceiptViolation]]:
    violations: list[ReceiptViolation] = []

    def mismatch(field: str, expected: object, observed: object) -> None:
        if observed != expected:
            violations.append(
                ReceiptViolation(
                    f"{field} mismatch",
                    f"expected={expected!r}, observed={observed!r}",
                )
            )

    mismatch("schema_version", SCHEMA_VERSION, receipt.get("schema_version"))
    mismatch("attestation_type", ATTESTATION_TYPE, receipt.get("attestation_type"))
    mismatch("commit_sha", identity.commit_sha, receipt.get("commit_sha"))
    mismatch("tree_sha", identity.tree_sha, receipt.get("tree_sha"))

    ledger = receipt.get("ledger")
    if not isinstance(ledger, dict):
        violations.append(ReceiptViolation("invalid ledger identity", "must be an object"))
    else:
        mismatch(
            "ledger.path",
            "execution/REQUIREMENT_PROOF_LEDGER.csv",
            ledger.get("path"),
        )
        mismatch("ledger.sha256", identity.ledger_sha256, ledger.get("sha256"))

    validator = receipt.get("validator")
    if not isinstance(validator, dict):
        violations.append(ReceiptViolation("invalid validator identity", "must be an object"))
    else:
        mismatch(
            "validator.path",
            "scripts/validate_requirement_proof_ledger.py",
            validator.get("path"),
        )
        mismatch(
            "validator.sha256",
            identity.validator_sha256,
            validator.get("sha256"),
        )

    worktree = receipt.get("worktree")
    if not isinstance(worktree, dict):
        violations.append(ReceiptViolation("invalid worktree proof", "must be an object"))
    else:
        for field in ("clean_before", "clean_after"):
            if worktree.get(field) is not True:
                violations.append(
                    ReceiptViolation("dirty proof execution", f"worktree.{field} is not true")
                )
    if not identity.clean:
        violations.append(
            ReceiptViolation(
                "dirty checkout",
                "current tracked or untracked worktree state is not empty",
            )
        )

    expected_receipt_sha = sha256_bytes(canonical_receipt_payload(receipt))
    observed_receipt_sha = receipt.get("receipt_sha256")
    if not isinstance(observed_receipt_sha, str) or not SHA256_PATTERN.fullmatch(
        observed_receipt_sha
    ):
        violations.append(
            ReceiptViolation("invalid receipt digest", repr(observed_receipt_sha))
        )
    elif observed_receipt_sha != expected_receipt_sha:
        violations.append(
            ReceiptViolation(
                "receipt digest mismatch",
                f"expected={expected_receipt_sha}, observed={observed_receipt_sha}",
            )
        )

    generator = receipt.get("generator")
    signature = receipt.get("signature")
    if require_trusted_ci:
        if not isinstance(generator, dict):
            violations.append(
                ReceiptViolation("untrusted receipt generator", "missing generator object")
            )
        else:
            mismatch("generator.provider", "github-actions", generator.get("provider"))
            if not str(generator.get("run_id", "")).strip():
                violations.append(
                    ReceiptViolation("untrusted receipt generator", "missing GitHub run_id")
                )
        if not isinstance(signature, dict):
            violations.append(
                ReceiptViolation("missing external attestation", "signature object is required")
            )
        else:
            mismatch(
                "signature.kind",
                "github-artifact-attestation",
                signature.get("kind"),
            )
            if not str(signature.get("reference", "")).strip():
                violations.append(
                    ReceiptViolation(
                        "missing external attestation",
                        "signature.reference is empty",
                    )
                )

    ledger_by_id = {row["requirement_id"]: row for row in ledger_rows}
    receipt_rows = receipt.get("rows")
    indexed_rows: dict[str, dict[str, Any]] = {}
    if not isinstance(receipt_rows, list):
        violations.append(ReceiptViolation("invalid receipt rows", "must be a list"))
        receipt_rows = []
    for index, row in enumerate(receipt_rows):
        if not isinstance(row, dict):
            violations.append(
                ReceiptViolation("invalid receipt row", f"rows[{index}] is not an object")
            )
            continue
        requirement_id = row.get("requirement_id")
        if not isinstance(requirement_id, str) or requirement_id not in ledger_by_id:
            violations.append(
                ReceiptViolation(
                    "unknown receipt requirement",
                    f"rows[{index}].requirement_id={requirement_id!r}",
                )
            )
            continue
        if requirement_id in indexed_rows:
            violations.append(
                ReceiptViolation("duplicate receipt requirement", requirement_id)
            )
            continue
        indexed_rows[requirement_id] = row

        ledger_row = ledger_by_id[requirement_id]
        mismatch(
            f"{requirement_id}.verification_command",
            ledger_row["verification_command"],
            row.get("verification_command"),
        )
        if row.get("status") != "verified":
            violations.append(
                ReceiptViolation(
                    "unverified receipt row",
                    f"{requirement_id}: status={row.get('status')!r}",
                )
            )
        if row.get("exit_code") != 0:
            violations.append(
                ReceiptViolation(
                    "failed receipt command",
                    f"{requirement_id}: exit_code={row.get('exit_code')!r}",
                )
            )
        for field in ("stdout_sha256", "stderr_sha256"):
            digest = row.get(field)
            if not isinstance(digest, str) or not SHA256_PATTERN.fullmatch(digest):
                violations.append(
                    ReceiptViolation(
                        "invalid command-output digest",
                        f"{requirement_id}.{field}={digest!r}",
                    )
                )

        evidence = row.get("evidence")
        if not isinstance(evidence, list) or not evidence:
            violations.append(
                ReceiptViolation("missing row evidence", requirement_id)
            )
        else:
            for evidence_index, item in enumerate(evidence):
                if not isinstance(item, dict):
                    violations.append(
                        ReceiptViolation(
                            "invalid row evidence",
                            f"{requirement_id}[{evidence_index}] is not an object",
                        )
                    )
                    continue
                digest = item.get("sha256")
                if not isinstance(digest, str) or not SHA256_PATTERN.fullmatch(digest):
                    violations.append(
                        ReceiptViolation(
                            "invalid evidence digest",
                            f"{requirement_id}[{evidence_index}].sha256={digest!r}",
                        )
                    )
                if not str(item.get("logical_name", "")).strip():
                    violations.append(
                        ReceiptViolation(
                            "invalid row evidence",
                            f"{requirement_id}[{evidence_index}] has no logical_name",
                        )
                    )

    return indexed_rows, violations

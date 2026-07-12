#!/usr/bin/env python3
"""Validate external, non-self-referential requirement-proof attestations."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
from typing import Any, Callable
from urllib.parse import urlparse


SCHEMA_VERSION = 3
ATTESTATION_TYPE = "requirement-proof"
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
GIT_SHA_PATTERN = re.compile(r"^[0-9a-f]{40}$")
ARTIFACT_NAME_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")
EMPTY_STATUS_SHA256 = hashlib.sha256(b"").hexdigest()


@dataclass(frozen=True)
class CheckoutIdentity:
    repository: str
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


@dataclass(frozen=True)
class ArtifactDeclaration:
    artifact_type: str
    logical_name: str
    root_name: str | None = None
    relative_path: str | None = None


def parse_artifact_declarations(value: str) -> list[ArtifactDeclaration]:
    """Parse strict typed proof-artifact declarations from a ledger cell."""

    raw_items = [item.strip() for item in value.split(";") if item.strip()]
    if not raw_items:
        raise ValueError("proof_artifacts has no typed artifact declarations")

    declarations: list[ArtifactDeclaration] = []
    logical_names: set[str] = set()
    sources: set[tuple[str, str, str]] = set()
    for raw_item in raw_items:
        parts = raw_item.split(":", 3)
        artifact_type = parts[0]
        if artifact_type in {"stdout", "stderr"} and len(parts) == 2:
            logical_name = parts[1]
            root_name = None
            relative_path = None
            source = (artifact_type, "", "")
        elif artifact_type == "file" and len(parts) == 4:
            logical_name, root_name, raw_path = parts[1:]
            if not ARTIFACT_NAME_PATTERN.fullmatch(root_name):
                raise ValueError(
                    f"invalid artifact root name in declaration: {raw_item!r}"
                )
            if "\\" in raw_path:
                raise ValueError(
                    f"file artifact path must use POSIX separators: {raw_item!r}"
                )
            path = PurePosixPath(raw_path)
            if (
                path.is_absolute()
                or not path.parts
                or any(part in {"", ".", ".."} for part in path.parts)
                or raw_path != path.as_posix()
            ):
                raise ValueError(
                    f"file artifact path must be normalized and relative: {raw_item!r}"
                )
            relative_path = path.as_posix()
            source = ("file", root_name, relative_path)
        else:
            raise ValueError(
                "artifact declaration must be stdout:<name>, stderr:<name>, "
                f"or file:<name>:<approved-root>:<relative-path>: {raw_item!r}"
            )

        if not ARTIFACT_NAME_PATTERN.fullmatch(logical_name):
            raise ValueError(
                f"invalid artifact logical name in declaration: {raw_item!r}"
            )
        if logical_name in logical_names:
            raise ValueError(f"duplicate artifact logical name: {logical_name}")
        if source in sources:
            raise ValueError(f"duplicate artifact source: {raw_item}")
        logical_names.add(logical_name)
        sources.add(source)
        declarations.append(
            ArtifactDeclaration(
                artifact_type=artifact_type,
                logical_name=logical_name,
                root_name=root_name,
                relative_path=relative_path,
            )
        )
    return declarations


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_receipt_payload(receipt: dict[str, Any]) -> bytes:
    payload = {key: value for key, value in receipt.items() if key != "receipt_sha256"}
    return _canonical_json(payload)


def canonical_receipt_row_payload(row: dict[str, Any]) -> bytes:
    return _canonical_json(
        {key: value for key, value in row.items() if key != "row_sha256"}
    )


def canonical_ledger_row_payload(row: dict[str, str]) -> bytes:
    return _canonical_json(row)


def _canonical_json(value: Any) -> bytes:
    return (
        json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        + "\n"
    ).encode("utf-8")


def canonical_repository(remote: str) -> str:
    """Return a GitHub owner/repository identity from SSH or HTTPS remotes."""

    value = remote.strip()
    if value.startswith("git@github.com:"):
        path = value.removeprefix("git@github.com:")
    else:
        parsed = urlparse(value)
        if parsed.scheme not in {"http", "https", "ssh"}:
            raise ValueError(f"unsupported repository remote: {remote!r}")
        if parsed.hostname != "github.com":
            raise ValueError(f"repository remote is not github.com: {remote!r}")
        path = parsed.path.lstrip("/")
    if path.endswith(".git"):
        path = path[:-4]
    parts = path.split("/")
    if len(parts) != 2 or not all(parts):
        raise ValueError(
            f"repository remote has no owner/repository identity: {remote!r}"
        )
    return "/".join(parts)


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
    mismatch("repository", identity.repository, receipt.get("repository"))
    mismatch("commit_sha", identity.commit_sha, receipt.get("commit_sha"))
    mismatch("tree_sha", identity.tree_sha, receipt.get("tree_sha"))
    for field in ("commit_sha", "tree_sha"):
        value = receipt.get(field)
        if not isinstance(value, str) or not GIT_SHA_PATTERN.fullmatch(value):
            violations.append(
                ReceiptViolation("invalid Git identity", f"{field}={value!r}")
            )

    if "signature" in receipt:
        violations.append(
            ReceiptViolation(
                "embedded trust claim",
                "receipt signatures/references are self-asserted; verify a detached attestation bundle",
            )
        )

    generated_at = receipt.get("generated_at_utc")
    try:
        parsed_generated_at = datetime.fromisoformat(str(generated_at))
    except ValueError:
        parsed_generated_at = None
    if (
        parsed_generated_at is None
        or parsed_generated_at.tzinfo is None
        or parsed_generated_at.utcoffset() != timezone.utc.utcoffset(None)
    ):
        violations.append(
            ReceiptViolation(
                "invalid generation timestamp",
                f"generated_at_utc={generated_at!r}",
            )
        )

    ledger = receipt.get("ledger")
    if not isinstance(ledger, dict):
        violations.append(
            ReceiptViolation("invalid ledger identity", "must be an object")
        )
    else:
        mismatch(
            "ledger.path",
            "execution/REQUIREMENT_PROOF_LEDGER.csv",
            ledger.get("path"),
        )
        mismatch("ledger.sha256", identity.ledger_sha256, ledger.get("sha256"))

    validator = receipt.get("validator")
    if not isinstance(validator, dict):
        violations.append(
            ReceiptViolation("invalid validator identity", "must be an object")
        )
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
        violations.append(
            ReceiptViolation("invalid worktree proof", "must be an object")
        )
    else:
        for field in ("clean_before", "clean_after"):
            if worktree.get(field) is not True:
                violations.append(
                    ReceiptViolation(
                        "dirty proof execution", f"worktree.{field} is not true"
                    )
                )
        for field in ("status_before_sha256", "status_after_sha256"):
            if worktree.get(field) != EMPTY_STATUS_SHA256:
                violations.append(
                    ReceiptViolation(
                        "dirty proof status digest",
                        f"worktree.{field}={worktree.get(field)!r}",
                    )
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
    if not isinstance(generator, dict):
        violations.append(
            ReceiptViolation("invalid receipt generator", "missing generator object")
        )
    elif generator.get("provider") not in {"local", "github-actions"}:
        violations.append(
            ReceiptViolation(
                "invalid receipt generator",
                f"provider={generator.get('provider')!r}",
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
                ReceiptViolation(
                    "invalid receipt row", f"rows[{index}] is not an object"
                )
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
        if (
            ledger_row.get("evidence_status", "").strip().lower() != "verified"
            or ledger_row.get("task_status", "").strip().lower() != "complete"
        ):
            violations.append(
                ReceiptViolation(
                    "receipt attests unverified ledger row",
                    f"{requirement_id}: evidence_status="
                    f"{ledger_row.get('evidence_status')!r}, "
                    f"task_status={ledger_row.get('task_status')!r}",
                )
            )
        expected_ledger_row_sha = sha256_bytes(canonical_ledger_row_payload(ledger_row))
        if row.get("ledger_row_sha256") != expected_ledger_row_sha:
            violations.append(
                ReceiptViolation(
                    "ledger row digest mismatch",
                    f"{requirement_id}: expected={expected_ledger_row_sha}, "
                    f"observed={row.get('ledger_row_sha256')!r}",
                )
            )
        expected_row_sha = sha256_bytes(canonical_receipt_row_payload(row))
        row_sha = row.get("row_sha256")
        if not isinstance(row_sha, str) or not SHA256_PATTERN.fullmatch(row_sha):
            violations.append(
                ReceiptViolation(
                    "invalid receipt row digest",
                    f"{requirement_id}: row_sha256={row_sha!r}",
                )
            )
        elif row_sha != expected_row_sha:
            violations.append(
                ReceiptViolation(
                    "receipt row digest mismatch",
                    f"{requirement_id}: expected={expected_row_sha}, observed={row_sha}",
                )
            )
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
            violations.append(ReceiptViolation("missing row evidence", requirement_id))
        else:
            try:
                declarations = parse_artifact_declarations(
                    ledger_row.get("proof_artifacts", "")
                )
            except ValueError as error:
                declarations = []
                violations.append(
                    ReceiptViolation(
                        "invalid artifact declaration",
                        f"{requirement_id}: {error}",
                    )
                )
            expected_by_name = {
                declaration.logical_name: declaration for declaration in declarations
            }
            observed_names: set[str] = set()
            for evidence_index, item in enumerate(evidence):
                if not isinstance(item, dict):
                    violations.append(
                        ReceiptViolation(
                            "invalid row evidence",
                            f"{requirement_id}[{evidence_index}] is not an object",
                        )
                    )
                    continue
                logical_name = item.get("logical_name")
                if logical_name in observed_names:
                    violations.append(
                        ReceiptViolation(
                            "duplicate row evidence",
                            f"{requirement_id}: logical_name={logical_name!r}",
                        )
                    )
                elif isinstance(logical_name, str):
                    observed_names.add(logical_name)
                digest = item.get("sha256")
                if not isinstance(digest, str) or not SHA256_PATTERN.fullmatch(digest):
                    violations.append(
                        ReceiptViolation(
                            "invalid evidence digest",
                            f"{requirement_id}[{evidence_index}].sha256={digest!r}",
                        )
                    )
                if not isinstance(logical_name, str) or not logical_name.strip():
                    violations.append(
                        ReceiptViolation(
                            "invalid row evidence",
                            f"{requirement_id}[{evidence_index}] has no logical_name",
                        )
                    )
                size_bytes = item.get("size_bytes")
                if (
                    not isinstance(size_bytes, int)
                    or isinstance(size_bytes, bool)
                    or size_bytes < 0
                ):
                    violations.append(
                        ReceiptViolation(
                            "invalid row evidence",
                            f"{requirement_id}[{evidence_index}].size_bytes="
                            f"{size_bytes!r}",
                        )
                    )
                declaration = expected_by_name.get(logical_name)
                if declaration is None:
                    violations.append(
                        ReceiptViolation(
                            "artifact declaration mismatch",
                            f"{requirement_id}[{evidence_index}] has undeclared "
                            f"logical_name={logical_name!r}",
                        )
                    )
                    continue
                if item.get("type") != declaration.artifact_type:
                    violations.append(
                        ReceiptViolation(
                            "artifact declaration mismatch",
                            f"{requirement_id}.{logical_name}.type="
                            f"{item.get('type')!r}, expected="
                            f"{declaration.artifact_type!r}",
                        )
                    )
                if declaration.artifact_type == "file":
                    if (
                        item.get("root") != declaration.root_name
                        or item.get("path") != declaration.relative_path
                    ):
                        violations.append(
                            ReceiptViolation(
                                "artifact declaration mismatch",
                                f"{requirement_id}.{logical_name} file identity "
                                "does not match the ledger declaration",
                            )
                        )
                elif "root" in item or "path" in item:
                    violations.append(
                        ReceiptViolation(
                            "artifact declaration mismatch",
                            f"{requirement_id}.{logical_name} stream evidence "
                            "must not claim a file path",
                        )
                    )
            missing_evidence = sorted(set(expected_by_name) - observed_names)
            if missing_evidence:
                violations.append(
                    ReceiptViolation(
                        "artifact declaration mismatch",
                        f"{requirement_id}: missing declared evidence "
                        f"{missing_evidence}",
                    )
                )

    return indexed_rows, violations


def verify_github_attestation(
    receipt_path: Path,
    *,
    bundle_path: Path,
    repository: str,
    signer_workflow: str,
    source_digest: str,
    runner: Callable[..., subprocess.CompletedProcess[str]] = subprocess.run,
) -> list[ReceiptViolation]:
    """Cryptographically verify a detached GitHub artifact attestation."""

    if not signer_workflow.strip():
        return [
            ReceiptViolation(
                "missing attestation policy",
                "an exact GitHub signer workflow is required",
            )
        ]
    command = [
        "gh",
        "attestation",
        "verify",
        str(receipt_path),
        "--bundle",
        str(bundle_path),
        "--repo",
        repository,
        "--signer-workflow",
        signer_workflow,
        "--source-digest",
        source_digest,
        "--deny-self-hosted-runners",
        "--format",
        "json",
    ]
    try:
        completed = runner(
            command,
            check=False,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        return [
            ReceiptViolation(
                "external attestation verification failed",
                str(error),
            )
        ]
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        return [
            ReceiptViolation(
                "external attestation verification failed",
                f"gh exit={completed.returncode}: {detail[:1000]}",
            )
        ]
    try:
        verification_results = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return [
            ReceiptViolation(
                "invalid attestation verification output",
                str(error),
            )
        ]
    if not isinstance(verification_results, list) or not verification_results:
        return [
            ReceiptViolation(
                "missing verified external attestation",
                "gh returned no verified attestation results",
            )
        ]
    for index, result in enumerate(verification_results):
        if not isinstance(result, dict):
            return [
                ReceiptViolation(
                    "invalid attestation verification result",
                    f"result[{index}] is not an object",
                )
            ]
        verification = result.get("verificationResult")
        signature = (
            verification.get("signature") if isinstance(verification, dict) else None
        )
        statement = (
            verification.get("statement") if isinstance(verification, dict) else None
        )
        subjects = statement.get("subject") if isinstance(statement, dict) else None
        if (
            not isinstance(result.get("attestation"), dict)
            or not isinstance(verification, dict)
            or not isinstance(signature, dict)
            or not isinstance(signature.get("certificate"), dict)
            or not isinstance(statement, dict)
            or not isinstance(subjects, list)
            or not subjects
        ):
            return [
                ReceiptViolation(
                    "invalid attestation verification result",
                    f"result[{index}] lacks the verified bundle, certificate, "
                    "statement, or artifact subject",
                )
            ]
    return []

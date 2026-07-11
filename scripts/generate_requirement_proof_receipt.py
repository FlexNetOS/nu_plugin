#!/usr/bin/env python3
"""Run selected ledger commands and write an external current-head receipt."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from requirement_proof_attestation import (
    ATTESTATION_TYPE,
    SCHEMA_VERSION,
    canonical_receipt_payload,
    sha256_bytes,
)
from validate_requirement_proof_ledger import LEDGER_PATH, read_ledger


VALIDATOR_PATH = Path("scripts/validate_requirement_proof_ledger.py")


def git_output(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def worktree_status(root: Path) -> str:
    return git_output(root, "status", "--porcelain=v1", "--untracked-files=all")


def ensure_external_output(root: Path, output: Path) -> Path:
    root = root.resolve()
    output = output.resolve()
    try:
        output.relative_to(root)
    except ValueError:
        return output
    raise ValueError(
        f"receipt output must be outside the attested repository: {output}"
    )


def run_requirement(root: Path, row: dict[str, str]) -> dict:
    requirement_id = row["requirement_id"]
    command = row["verification_command"]
    before = worktree_status(root)
    if before:
        raise RuntimeError(
            f"{requirement_id}: checkout became dirty before proof command: {before}"
        )
    completed = subprocess.run(
        ["bash", "-euo", "pipefail", "-c", command],
        cwd=root,
        capture_output=True,
    )
    after = worktree_status(root)
    if after != before:
        raise RuntimeError(
            f"{requirement_id}: proof command mutated checkout: before={before!r}, after={after!r}"
        )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{requirement_id}: verification command failed with exit code "
            f"{completed.returncode}"
        )

    logical_names = [
        name.strip()
        for name in row.get("proof_artifacts", "").split(";")
        if name.strip()
    ]
    if not logical_names:
        logical_names = [f"{requirement_id.lower()}-command-output"]
    combined_digest = sha256_bytes(
        completed.stdout + b"\0stderr\0" + completed.stderr
    )
    return {
        "requirement_id": requirement_id,
        "status": "verified",
        "verification_command": command,
        "exit_code": completed.returncode,
        "stdout_sha256": sha256_bytes(completed.stdout),
        "stderr_sha256": sha256_bytes(completed.stderr),
        "evidence": [
            {
                "logical_name": logical_name,
                "sha256": combined_digest,
                "kind": "command-output",
            }
            for logical_name in logical_names
        ],
    }


def build_receipt(
    root: Path,
    selected_ids: set[str],
    *,
    provider: str,
    run_id: str,
    signature_reference: str | None,
) -> dict:
    root = root.resolve()
    before = worktree_status(root)
    if before:
        raise RuntimeError(f"proof checkout is not clean: {before}")

    ledger_path = root / LEDGER_PATH
    validator_path = root / VALIDATOR_PATH
    rows = read_ledger(ledger_path)
    rows_by_id = {row["requirement_id"]: row for row in rows}
    missing = sorted(selected_ids - rows_by_id.keys())
    if missing:
        raise ValueError(f"unknown requirement IDs: {missing}")
    receipt_rows = [
        run_requirement(root, rows_by_id[requirement_id])
        for requirement_id in sorted(selected_ids)
    ]

    after = worktree_status(root)
    if after != before:
        raise RuntimeError(
            f"proof execution mutated checkout: before={before!r}, after={after!r}"
        )

    remote = git_output(root, "config", "--get", "remote.origin.url")
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "attestation_type": ATTESTATION_TYPE,
        "repository": remote,
        "commit_sha": git_output(root, "rev-parse", "HEAD"),
        "tree_sha": git_output(root, "rev-parse", "HEAD^{tree}"),
        "ledger": {
            "path": LEDGER_PATH.as_posix(),
            "sha256": sha256_bytes(ledger_path.read_bytes()),
        },
        "validator": {
            "path": VALIDATOR_PATH.as_posix(),
            "sha256": sha256_bytes(validator_path.read_bytes()),
        },
        "generated_at_utc": datetime.now(timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "generator": {
            "provider": provider,
            "run_id": run_id,
        },
        "worktree": {
            "clean_before": not before,
            "clean_after": not after,
            "status_before_sha256": sha256_bytes(before.encode()),
            "status_after_sha256": sha256_bytes(after.encode()),
        },
        "rows": receipt_rows,
    }
    if signature_reference:
        receipt["signature"] = {
            "kind": "github-artifact-attestation",
            "reference": signature_reference,
        }
    receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1]
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--requirement",
        action="append",
        dest="requirements",
        required=True,
        help="Requirement ID to prove; repeat for each selected row",
    )
    parser.add_argument(
        "--provider",
        default=os.environ.get("GITHUB_ACTIONS") and "github-actions" or "local",
    )
    parser.add_argument("--run-id", default=os.environ.get("GITHUB_RUN_ID", ""))
    parser.add_argument(
        "--signature-reference",
        default=os.environ.get("CODEDB_ATTESTATION_REFERENCE"),
    )
    args = parser.parse_args()

    try:
        output = ensure_external_output(args.root, args.output)
        receipt = build_receipt(
            args.root,
            set(args.requirements),
            provider=args.provider,
            run_id=args.run_id,
            signature_reference=args.signature_reference,
        )
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(
            json.dumps(receipt, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
    except (OSError, RuntimeError, ValueError, subprocess.CalledProcessError) as error:
        print(f"requirement proof receipt generation failed: {error}", file=sys.stderr)
        return 1

    print(f"requirement proof receipt written outside checkout: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

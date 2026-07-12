#!/usr/bin/env python3
"""Run selected ledger commands and write an external current-head receipt."""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import stat
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

from requirement_proof_attestation import (
    ATTESTATION_TYPE,
    SCHEMA_VERSION,
    canonical_ledger_row_payload,
    canonical_receipt_payload,
    canonical_receipt_row_payload,
    canonical_repository,
    parse_artifact_declarations,
    sha256_bytes,
)
from validate_requirement_proof_ledger import (
    EXPECTED_REQUIREMENT_IDS,
    LEDGER_PATH,
    read_ledger,
)


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


def ensure_attestable_row(root: Path, row: dict[str, str]) -> None:
    requirement_id = row["requirement_id"]
    evidence_status = row.get("evidence_status", "").strip().lower()
    if evidence_status != "verified":
        raise RuntimeError(
            f"{requirement_id}: ledger evidence_status is not verified: "
            f"{evidence_status or '<empty>'}"
        )
    task_status = row.get("task_status", "").strip().lower()
    if task_status != "complete":
        raise RuntimeError(
            f"{requirement_id}: ledger task_status is not complete: "
            f"{task_status or '<empty>'}"
        )
    try:
        parse_artifact_declarations(row.get("proof_artifacts", ""))
    except ValueError as error:
        raise RuntimeError(
            f"{requirement_id}: invalid typed proof artifacts: {error}"
        ) from error

    test_items = [
        item.strip() for item in row.get("test_paths", "").split(";") if item.strip()
    ]
    if not test_items or any(item == "<missing-direct-test>" for item in test_items):
        raise RuntimeError(f"{requirement_id}: ledger has no direct test path")
    for item in test_items:
        if item.startswith("external:"):
            item = item.removeprefix("external:")
        elif item.startswith(("https://", "http://")):
            raise RuntimeError(
                f"{requirement_id}: direct test path is not locally executable: {item}"
            )
        if not glob.glob(str(root / item), recursive=True):
            raise RuntimeError(
                f"{requirement_id}: direct test path does not exist: {item}"
            )


def _stable_identity(file_stat: os.stat_result) -> tuple[int, int, int, int, int]:
    return (
        file_stat.st_dev,
        file_stat.st_ino,
        file_stat.st_size,
        file_stat.st_mtime_ns,
        file_stat.st_ctime_ns,
    )


def hash_file_artifact(
    approved_root: Path,
    relative_path: str,
    *,
    requirement_id: str,
) -> tuple[int, str]:
    """Hash one regular file without following symlinks and reject path races."""

    approved_root = approved_root.resolve(strict=True)
    parts = PurePosixPath(relative_path).parts
    directory_fds: list[int] = []
    file_fd: int | None = None
    try:
        current_fd = os.open(
            approved_root,
            os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC,
        )
        directory_fds.append(current_fd)
        for component in parts[:-1]:
            current_fd = os.open(
                component,
                os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW,
                dir_fd=current_fd,
            )
            directory_fds.append(current_fd)
        file_fd = os.open(
            parts[-1],
            os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW,
            dir_fd=current_fd,
        )
        before = os.fstat(file_fd)
        if not stat.S_ISREG(before.st_mode):
            raise RuntimeError(
                f"{requirement_id}: file artifact is not a regular file: "
                f"{relative_path}"
            )
        digest = hashlib.sha256()
        while chunk := os.read(file_fd, 1024 * 1024):
            digest.update(chunk)
        after = os.fstat(file_fd)
        path_after = os.stat(
            parts[-1],
            dir_fd=current_fd,
            follow_symlinks=False,
        )
        if _stable_identity(before) != _stable_identity(after) or _stable_identity(
            after
        ) != _stable_identity(path_after):
            raise RuntimeError(
                f"{requirement_id}: file artifact raced during hashing: {relative_path}"
            )
        return after.st_size, digest.hexdigest()
    except FileNotFoundError as error:
        raise RuntimeError(
            f"{requirement_id}: missing file artifact: {relative_path}"
        ) from error
    except OSError as error:
        raise RuntimeError(
            f"{requirement_id}: unsafe or unreadable file artifact "
            f"{relative_path}: {error}"
        ) from error
    finally:
        if file_fd is not None:
            os.close(file_fd)
        for directory_fd in reversed(directory_fds):
            os.close(directory_fd)


def run_requirement(
    root: Path,
    row: dict[str, str],
    *,
    approved_artifact_roots: dict[str, Path] | None = None,
) -> dict:
    requirement_id = row["requirement_id"]
    command = row["verification_command"]
    ensure_attestable_row(root, row)
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

    declarations = parse_artifact_declarations(row["proof_artifacts"])
    approved_roots = {
        name: path.resolve()
        for name, path in (approved_artifact_roots or {"repository": root}).items()
    }
    evidence: list[dict[str, object]] = []
    for declaration in declarations:
        if declaration.artifact_type == "stdout":
            evidence.append(
                {
                    "logical_name": declaration.logical_name,
                    "type": "stdout",
                    "size_bytes": len(completed.stdout),
                    "sha256": sha256_bytes(completed.stdout),
                }
            )
        elif declaration.artifact_type == "stderr":
            evidence.append(
                {
                    "logical_name": declaration.logical_name,
                    "type": "stderr",
                    "size_bytes": len(completed.stderr),
                    "sha256": sha256_bytes(completed.stderr),
                }
            )
        else:
            approved_root = approved_roots.get(declaration.root_name or "")
            if approved_root is None:
                raise RuntimeError(
                    f"{requirement_id}: unapproved artifact root: "
                    f"{declaration.root_name}"
                )
            size_bytes, digest = hash_file_artifact(
                approved_root,
                declaration.relative_path or "",
                requirement_id=requirement_id,
            )
            evidence.append(
                {
                    "logical_name": declaration.logical_name,
                    "type": "file",
                    "root": declaration.root_name,
                    "path": declaration.relative_path,
                    "size_bytes": size_bytes,
                    "sha256": digest,
                }
            )
    receipt_row = {
        "requirement_id": requirement_id,
        "status": "verified",
        "verification_command": command,
        "exit_code": completed.returncode,
        "stdout_sha256": sha256_bytes(completed.stdout),
        "stderr_sha256": sha256_bytes(completed.stderr),
        "evidence": evidence,
        "ledger_row_sha256": sha256_bytes(canonical_ledger_row_payload(row)),
    }
    receipt_row["row_sha256"] = sha256_bytes(canonical_receipt_row_payload(receipt_row))
    return receipt_row


def build_receipt(
    root: Path,
    selected_ids: set[str] | None,
    *,
    provider: str,
    run_id: str,
) -> dict:
    root = root.resolve()
    before = worktree_status(root)
    if before:
        raise RuntimeError(f"proof checkout is not clean: {before}")

    ledger_path = root / LEDGER_PATH
    validator_path = root / VALIDATOR_PATH
    rows = read_ledger(ledger_path)
    ledger_ids = [row["requirement_id"] for row in rows]
    duplicate_ids = sorted(
        requirement_id
        for requirement_id in set(ledger_ids)
        if ledger_ids.count(requirement_id) > 1
    )
    if duplicate_ids:
        raise ValueError(f"duplicate requirement IDs in ledger: {duplicate_ids}")
    rows_by_id = {row["requirement_id"]: row for row in rows}

    if selected_ids is None:
        ledger_id_set = set(ledger_ids)
        missing = sorted(EXPECTED_REQUIREMENT_IDS - ledger_id_set)
        unexpected = sorted(ledger_id_set - EXPECTED_REQUIREMENT_IDS)
        if missing or unexpected:
            raise ValueError(
                "all-requirements inventory mismatch: "
                f"missing={missing}, unexpected={unexpected}"
            )
        selected_ids = ledger_id_set
    else:
        unknown = sorted(selected_ids - rows_by_id.keys())
        if unknown:
            raise ValueError(f"unknown requirement IDs: {unknown}")

    selected_rows = [
        rows_by_id[requirement_id] for requirement_id in sorted(selected_ids)
    ]
    # Preflight every selected row before running any command. In all-row mode,
    # one unresolved row rejects the whole receipt instead of producing a
    # misleading partial artifact after earlier commands have already run.
    for row in selected_rows:
        ensure_attestable_row(root, row)

    receipt_rows = [run_requirement(root, row) for row in selected_rows]

    after = worktree_status(root)
    if after != before:
        raise RuntimeError(
            f"proof execution mutated checkout: before={before!r}, after={after!r}"
        )

    remote = git_output(root, "config", "--get", "remote.origin.url")
    receipt = {
        "schema_version": SCHEMA_VERSION,
        "attestation_type": ATTESTATION_TYPE,
        "repository": canonical_repository(remote),
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
    receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--root", type=Path, default=Path(__file__).resolve().parents[1]
    )
    parser.add_argument("--output", type=Path, required=True)
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument(
        "--requirement",
        action="append",
        dest="requirements",
        help="Requirement ID to prove; repeat for each selected row",
    )
    selection.add_argument(
        "--all-requirements",
        action="store_true",
        help=(
            "Prove the complete mandatory ledger; reject before execution if "
            "any row is missing, unexpected, unresolved, or incomplete"
        ),
    )
    parser.add_argument(
        "--provider",
        default=os.environ.get("GITHUB_ACTIONS") and "github-actions" or "local",
    )
    parser.add_argument("--run-id", default=os.environ.get("GITHUB_RUN_ID", ""))
    args = parser.parse_args()

    try:
        output = ensure_external_output(args.root, args.output)
        receipt = build_receipt(
            args.root,
            None if args.all_requirements else set(args.requirements or []),
            provider=args.provider,
            run_id=args.run_id,
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

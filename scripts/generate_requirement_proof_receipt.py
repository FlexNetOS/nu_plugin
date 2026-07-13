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
import tempfile
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath

from requirement_proof_attestation import (
    ATTESTATION_TYPE,
    EXTERNAL_SOURCE_PIN_PATH,
    SCHEMA_VERSION,
    ExternalSourceIdentity,
    canonical_command_execution_payload,
    canonical_ledger_row_payload,
    canonical_receipt_payload,
    canonical_receipt_row_payload,
    canonical_repository,
    external_source_receipt_identity,
    load_external_source_pins,
    parse_artifact_declarations,
    sha256_bytes,
)
from validate_requirement_proof_ledger import (
    EXPECTED_REQUIREMENT_IDS,
    LEDGER_PATH,
    read_ledger,
)


VALIDATOR_PATH = Path("scripts/validate_requirement_proof_ledger.py")
EXTERNAL_WORKSPACE_TEMPLATE_PATH = Path("external-sources/Cargo.toml")
EXTERNAL_WORKSPACE_PATH = Path("../Cargo.toml")


def checkout_snapshot(root: Path) -> dict[str, str]:
    return {
        "commit_sha": git_output(root, "rev-parse", "HEAD"),
        "tree_sha": git_output(root, "rev-parse", "HEAD^{tree}"),
        "status": worktree_status(root),
    }


def external_checkout_snapshot(
    root: Path,
    source: ExternalSourceIdentity,
) -> tuple[Path, dict[str, str]]:
    checkout = (root / source.checkout_path).resolve()
    try:
        checkout.relative_to(root)
    except ValueError:
        pass
    else:
        raise RuntimeError(
            f"{source.name}: external checkout must remain outside the "
            f"attested repository: {checkout}"
        )
    if not checkout.is_dir():
        raise RuntimeError(
            f"{source.name}: required external checkout is absent: {checkout}"
        )
    try:
        repository = canonical_repository(
            git_output(checkout, "config", "--get", "remote.origin.url")
        )
        commit_sha = git_output(checkout, "rev-parse", "HEAD")
        tree_sha = git_output(checkout, "rev-parse", "HEAD^{tree}")
        status = worktree_status(checkout)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        raise RuntimeError(
            f"{source.name}: invalid external checkout {checkout}: {error}"
        ) from error
    if repository != source.repository:
        raise RuntimeError(
            f"{source.name}: external remote mismatch: "
            f"expected={source.repository}, observed={repository}"
        )
    if commit_sha != source.commit_sha:
        raise RuntimeError(
            f"{source.name}: external HEAD mismatch: "
            f"expected={source.commit_sha}, observed={commit_sha}"
        )
    if tree_sha != source.tree_sha:
        raise RuntimeError(
            f"{source.name}: external tree mismatch: "
            f"expected={source.tree_sha}, observed={tree_sha}"
        )
    if status:
        raise RuntimeError(
            f"{source.name}: external checkout is dirty: {status}"
        )
    return checkout, {
        "repository": repository,
        "commit_sha": commit_sha,
        "tree_sha": tree_sha,
        "status": status,
    }


def load_external_checkouts(
    root: Path,
) -> dict[str, tuple[ExternalSourceIdentity, Path, dict[str, str]]]:
    sources = load_external_source_pins(root / EXTERNAL_SOURCE_PIN_PATH)
    checkouts: dict[
        str, tuple[ExternalSourceIdentity, Path, dict[str, str]]
    ] = {}
    for name, source in sources.items():
        checkout, snapshot = external_checkout_snapshot(root, source)
        checkouts[name] = (source, checkout, snapshot)
    return checkouts


def external_workspace_snapshot(root: Path) -> str:
    template = (root / EXTERNAL_WORKSPACE_TEMPLATE_PATH).resolve(strict=True)
    workspace = (root / EXTERNAL_WORKSPACE_PATH).resolve(strict=True)
    try:
        workspace.relative_to(root)
    except ValueError:
        pass
    else:
        raise RuntimeError(
            f"external Cargo workspace must remain outside the attested "
            f"repository: {workspace}"
        )
    template_bytes = template.read_bytes()
    workspace_bytes = workspace.read_bytes()
    if workspace_bytes != template_bytes:
        raise RuntimeError(
            "external Cargo workspace does not match the tracked template: "
            f"{workspace} != {template}"
        )
    return sha256_bytes(workspace_bytes)


def git_output(root: Path, *args: str) -> str:
    return subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


# Declared runtime side-effect paths that verification commands legitimately
# write (test run-logs, gitkb runtime store/cache/workspaces). These are never
# part of the attested tracked tree (the receipt binds commit/tree SHAs), so an
# UNTRACKED file under one of these prefixes must not count as a dirty checkout.
# The full untracked-file check remains in force for every other path — a
# command that generates an unexpected source/config/manifest file still fails.
RUNTIME_SIDE_EFFECT_PREFIXES = (
    "logs/",
    ".kb/store/",
    ".kb/.cache/",
    ".kb/workspaces/",
)


def _is_runtime_side_effect(status_line: str) -> bool:
    # Only untracked entries ("?? path") are tolerated; any tracked-file change
    # (M/A/D/R) under these prefixes is still reported so nothing attested drifts.
    if not status_line.startswith("?? "):
        return False
    path = status_line[3:].strip().strip('"')
    return path.startswith(RUNTIME_SIDE_EFFECT_PREFIXES)


def worktree_status(root: Path) -> str:
    raw = git_output(root, "status", "--porcelain=v1", "--untracked-files=all")
    lines = [line for line in raw.splitlines() if line and not _is_runtime_side_effect(line)]
    return "\n".join(lines)


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


def execute_verification_command(
    root: Path,
    command: str,
    *,
    requirement_ids: list[str],
    external_checkouts: dict[
        str, tuple[ExternalSourceIdentity, Path, dict[str, str]]
    ]
    | None = None,
    external_workspace_sha256: str | None = None,
    proof_environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[bytes]:
    execution_label = ",".join(requirement_ids)
    before_snapshot = checkout_snapshot(root)
    before = before_snapshot["status"]
    if before:
        raise RuntimeError(
            f"{execution_label}: checkout became dirty before proof command: {before}"
        )
    external_before: dict[str, dict[str, str]] = {}
    for name, (source, expected_path, expected_snapshot) in (
        external_checkouts or {}
    ).items():
        observed_path, observed_snapshot = external_checkout_snapshot(root, source)
        if observed_path != expected_path or observed_snapshot != expected_snapshot:
            raise RuntimeError(
                f"{execution_label}: external checkout {name} changed before "
                "proof command"
            )
        external_before[name] = observed_snapshot
    if external_workspace_sha256 is not None:
        observed_workspace_sha256 = external_workspace_snapshot(root)
        if observed_workspace_sha256 != external_workspace_sha256:
            raise RuntimeError(
                f"{execution_label}: external Cargo workspace changed before "
                "proof command"
            )
    completed = subprocess.run(
        ["bash", "-euo", "pipefail", "-c", command],
        cwd=root,
        capture_output=True,
        env=proof_environment,
    )
    after_snapshot = checkout_snapshot(root)
    if after_snapshot != before_snapshot:
        raise RuntimeError(
            f"{execution_label}: proof command mutated checkout: "
            f"before={before_snapshot!r}, after={after_snapshot!r}"
        )
    for name, (source, expected_path, _) in (external_checkouts or {}).items():
        try:
            observed_path, observed_snapshot = external_checkout_snapshot(root, source)
        except RuntimeError as error:
            raise RuntimeError(
                f"{execution_label}: proof command mutated external checkout "
                f"{name}: {error}"
            ) from error
        if (
            observed_path != expected_path
            or observed_snapshot != external_before[name]
        ):
            raise RuntimeError(
                f"{execution_label}: proof command mutated external checkout "
                f"{name}: before={external_before[name]!r}, "
                f"after={observed_snapshot!r}"
            )
    if external_workspace_sha256 is not None:
        try:
            observed_workspace_sha256 = external_workspace_snapshot(root)
        except (OSError, RuntimeError) as error:
            raise RuntimeError(
                f"{execution_label}: proof command mutated external Cargo "
                f"workspace: {error}"
            ) from error
        if observed_workspace_sha256 != external_workspace_sha256:
            raise RuntimeError(
                f"{execution_label}: proof command mutated external Cargo "
                "workspace"
            )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{execution_label}: verification command failed with exit code "
            f"{completed.returncode}"
        )
    return completed


def build_command_execution(
    command: str,
    completed: subprocess.CompletedProcess[bytes],
) -> dict[str, object]:
    execution: dict[str, object] = {
        "verification_command": command,
        "exit_code": completed.returncode,
        "stdout_size_bytes": len(completed.stdout),
        "stderr_size_bytes": len(completed.stderr),
        "stdout_sha256": sha256_bytes(completed.stdout),
        "stderr_sha256": sha256_bytes(completed.stderr),
    }
    execution["execution_sha256"] = sha256_bytes(
        canonical_command_execution_payload(execution)
    )
    return execution


def derive_receipt_row(
    root: Path,
    row: dict[str, str],
    completed: subprocess.CompletedProcess[bytes],
    command_execution_sha256: str,
    *,
    approved_artifact_roots: dict[str, Path] | None = None,
) -> dict:
    requirement_id = row["requirement_id"]
    command = row["verification_command"]
    ensure_attestable_row(root, row)

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
        "command_execution_sha256": command_execution_sha256,
        "evidence": evidence,
        "ledger_row_sha256": sha256_bytes(canonical_ledger_row_payload(row)),
    }
    receipt_row["row_sha256"] = sha256_bytes(canonical_receipt_row_payload(receipt_row))
    return receipt_row


def run_requirement(
    root: Path,
    row: dict[str, str],
    *,
    approved_artifact_roots: dict[str, Path] | None = None,
    external_checkouts: dict[
        str, tuple[ExternalSourceIdentity, Path, dict[str, str]]
    ]
    | None = None,
    external_workspace_sha256: str | None = None,
    proof_environment: dict[str, str] | None = None,
) -> dict:
    ensure_attestable_row(root, row)
    completed = execute_verification_command(
        root,
        row["verification_command"],
        requirement_ids=[row["requirement_id"]],
        external_checkouts=external_checkouts,
        external_workspace_sha256=external_workspace_sha256,
        proof_environment=proof_environment,
    )
    execution = build_command_execution(row["verification_command"], completed)
    return derive_receipt_row(
        root,
        row,
        completed,
        str(execution["execution_sha256"]),
        approved_artifact_roots=approved_artifact_roots,
    )


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
    external_checkouts = load_external_checkouts(root)
    external_workspace_sha256 = external_workspace_snapshot(root)

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

    with tempfile.TemporaryDirectory(
        prefix="codedb-requirement-proof-target-", dir="/tmp"
    ) as target:
        proof_environment = os.environ.copy()
        proof_environment["CARGO_TARGET_DIR"] = target
        command_groups: dict[str, list[dict[str, str]]] = {}
        for row in selected_rows:
            command_groups.setdefault(row["verification_command"], []).append(row)
        command_executions: list[dict[str, object]] = []
        receipt_rows_by_id: dict[str, dict] = {}
        for command, command_rows in command_groups.items():
            completed = execute_verification_command(
                root,
                command,
                requirement_ids=[row["requirement_id"] for row in command_rows],
                external_checkouts=external_checkouts,
                external_workspace_sha256=external_workspace_sha256,
                proof_environment=proof_environment,
            )
            command_execution = build_command_execution(command, completed)
            command_executions.append(command_execution)
            execution_sha256 = str(command_execution["execution_sha256"])
            for row in command_rows:
                receipt_rows_by_id[row["requirement_id"]] = derive_receipt_row(
                    root,
                    row,
                    completed,
                    execution_sha256,
                )
        receipt_rows = [
            receipt_rows_by_id[row["requirement_id"]] for row in selected_rows
        ]

    after = worktree_status(root)
    if after != before:
        raise RuntimeError(
            f"proof execution mutated checkout: before={before!r}, after={after!r}"
        )

    external_receipts: dict[str, dict[str, object]] = {}
    for name, (source, expected_path, before_snapshot) in external_checkouts.items():
        after_path, after_snapshot = external_checkout_snapshot(root, source)
        if after_path != expected_path or after_snapshot != before_snapshot:
            raise RuntimeError(
                f"proof execution mutated external checkout {name}: "
                f"before={before_snapshot!r}, after={after_snapshot!r}"
            )
        external_receipt = external_source_receipt_identity(source)
        external_receipt["worktree"] = {
            "clean_before": not before_snapshot["status"],
            "clean_after": not after_snapshot["status"],
            "status_before_sha256": sha256_bytes(
                before_snapshot["status"].encode()
            ),
            "status_after_sha256": sha256_bytes(
                after_snapshot["status"].encode()
            ),
        }
        external_receipts[name] = external_receipt
    if external_workspace_snapshot(root) != external_workspace_sha256:
        raise RuntimeError("proof execution mutated external Cargo workspace")

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
        "external_sources": external_receipts,
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
        "command_executions": command_executions,
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

#!/usr/bin/env python3
"""Generate or validate repo-native truth-surface manifests."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


CHECKSUMS_PATH = Path("manifests/CHECKSUMS.sha256")
MANIFEST_PATH = Path("manifests/PACK_MANIFEST.json")
VALIDATION_PATH = Path("manifests/PACKAGE_VALIDATION.json")

GENERATED_PATHS = {
    CHECKSUMS_PATH.as_posix(),
    MANIFEST_PATH.as_posix(),
    VALIDATION_PATH.as_posix(),
}


def git_ls_files(repo: Path, include_untracked: bool = False) -> list[str]:
    command = ["git", "ls-files"]
    if include_untracked:
        command.extend(["--cached", "--others", "--exclude-standard"])
    result = subprocess.run(
        command,
        cwd=repo,
        text=True,
        check=True,
        stdout=subprocess.PIPE,
    )
    return [line for line in result.stdout.splitlines() if line]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def included_files(repo: Path) -> list[str]:
    return [
        path
        for path in git_ls_files(repo, include_untracked=True)
        if path not in GENERATED_PATHS and not path.startswith("target/")
    ]


def build_surfaces(repo: Path, generated_at: str) -> dict[str, str]:
    files = []
    for rel_path in included_files(repo):
        full_path = repo / rel_path
        files.append(
            {
                "path": rel_path,
                "bytes": full_path.stat().st_size,
                "sha256": sha256_file(full_path),
            }
        )

    checksum_lines = [
        f"{entry['sha256']}  {entry['path']}" for entry in files
    ]
    checksums = "\n".join(checksum_lines) + "\n"

    checksum_scope_hash = sha256_text(checksums)
    manifest = {
        "package": "nu_plugin",
        "surface": "repo_native_truth_surface",
        "generated_at_utc": generated_at,
        "entrypoint": "CODEDB_START_HERE.md",
        "canonical_prd": "prd/nu_plugin_codedb_v1_1_full_prd.md",
        "task_graph_source_of_truth": "execution/TASK_GRAPH.csv",
        "checksum_scope": "tracked repository files excluding generated truth-surface files",
        "checksum_excluded_paths": sorted(GENERATED_PATHS),
        "file_count_in_checksum_scope": len(files),
        "checksum_scope_files_sha256": checksum_scope_hash,
        "files": files,
    }
    manifest_text = json.dumps(manifest, indent=2, sort_keys=True) + "\n"

    validation = {
        "generated_at_utc": generated_at,
        "status": "passed",
        "checks": {
            "manifest_path": MANIFEST_PATH.as_posix(),
            "checksums_path": CHECKSUMS_PATH.as_posix(),
            "checksum_scope_count": len(files),
            "checksum_scope_files_sha256": checksum_scope_hash,
            "generated_paths": sorted(GENERATED_PATHS),
            "mode": "repo_native",
        },
        "warnings": [
            "This validates the current Git repository, not the sealed Downloads execution package.",
            "Generated truth-surface files are excluded to avoid self-referential checksums.",
        ],
    }
    validation_text = json.dumps(validation, indent=2, sort_keys=True) + "\n"

    return {
        CHECKSUMS_PATH.as_posix(): checksums,
        MANIFEST_PATH.as_posix(): manifest_text,
        VALIDATION_PATH.as_posix(): validation_text,
    }


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def write_surfaces(repo: Path) -> None:
    generated_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat()
    surfaces = build_surfaces(repo, generated_at)
    for rel_path, content in surfaces.items():
        path = repo / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def check_surfaces(repo: Path) -> int:
    existing_validation = read_text(repo / VALIDATION_PATH)
    generated_at = "1970-01-01T00:00:00+00:00"
    try:
        generated_at = json.loads(existing_validation).get(
            "generated_at_utc", generated_at
        )
    except json.JSONDecodeError:
        pass

    expected = build_surfaces(repo, generated_at)
    mismatches = []
    for rel_path, content in expected.items():
        actual = read_text(repo / rel_path)
        if actual != content:
            mismatches.append(rel_path)

    if mismatches:
        print("truth surface is stale:", file=sys.stderr)
        for rel_path in mismatches:
            print(f"  {rel_path}", file=sys.stderr)
        print("run: scripts/truth_surface.py --write", file=sys.stderr)
        return 1

    print(
        f"truth surface ok: {len(included_files(repo))} tracked files in checksum scope"
    )
    return 0


def load_json(path: Path) -> dict:
    try:
        with path.open(encoding="utf-8") as handle:
            value = json.load(handle)
    except (FileNotFoundError, json.JSONDecodeError) as source:
        print(f"failed to load {path}: {source}", file=sys.stderr)
        return {}
    if not isinstance(value, dict):
        print(f"{path} must contain a JSON object", file=sys.stderr)
        return {}
    return value


def check_source_surfaces(repo: Path) -> int:
    manifest = load_json(repo / MANIFEST_PATH)
    validation = load_json(repo / VALIDATION_PATH)
    checksums = read_text(repo / CHECKSUMS_PATH)
    if not manifest or not validation or not checksums:
        return 1

    files = manifest.get("files")
    if not isinstance(files, list):
        print("manifest files must be a list", file=sys.stderr)
        return 1

    expected_lines = []
    failures = []
    seen_paths = set()
    for entry in files:
        if not isinstance(entry, dict):
            failures.append("manifest file entry is not an object")
            continue
        rel_path = entry.get("path")
        expected_hash = entry.get("sha256")
        expected_bytes = entry.get("bytes")
        if not isinstance(rel_path, str):
            failures.append("manifest file entry is missing string path")
            continue
        if rel_path in seen_paths:
            failures.append(f"duplicate manifest path: {rel_path}")
        seen_paths.add(rel_path)
        if rel_path in GENERATED_PATHS:
            failures.append(f"generated file must not be in checksum scope: {rel_path}")
        path = repo / rel_path
        if not path.is_file():
            failures.append(f"manifest path is missing: {rel_path}")
            continue
        actual_hash = sha256_file(path)
        actual_bytes = path.stat().st_size
        if actual_hash != expected_hash:
            failures.append(f"sha256 mismatch: {rel_path}")
        if actual_bytes != expected_bytes:
            failures.append(f"byte count mismatch: {rel_path}")
        expected_lines.append(f"{expected_hash}  {rel_path}")

    expected_checksums = "\n".join(expected_lines) + "\n"
    if checksums != expected_checksums:
        failures.append("CHECKSUMS.sha256 does not match manifest file list")

    checksum_scope_hash = sha256_text(expected_checksums)
    if manifest.get("file_count_in_checksum_scope") != len(files):
        failures.append("manifest file_count_in_checksum_scope is stale")
    if manifest.get("checksum_scope_files_sha256") != checksum_scope_hash:
        failures.append("manifest checksum_scope_files_sha256 is stale")
    if sorted(manifest.get("checksum_excluded_paths", [])) != sorted(GENERATED_PATHS):
        failures.append("manifest checksum_excluded_paths is stale")

    validation_checks = validation.get("checks")
    if not isinstance(validation_checks, dict):
        failures.append("validation checks must be an object")
    else:
        expected_checks = {
            "manifest_path": MANIFEST_PATH.as_posix(),
            "checksums_path": CHECKSUMS_PATH.as_posix(),
            "checksum_scope_count": len(files),
            "checksum_scope_files_sha256": checksum_scope_hash,
            "generated_paths": sorted(GENERATED_PATHS),
            "mode": "repo_native",
        }
        for key, expected_value in expected_checks.items():
            if validation_checks.get(key) != expected_value:
                failures.append(f"validation checks.{key} is stale")
    if validation.get("status") != "passed":
        failures.append("validation status must be passed")

    if failures:
        print("truth source validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"  {failure}", file=sys.stderr)
        return 1

    print(f"truth source ok: {len(files)} manifest files validated")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--check-source", action="store_true")
    args = parser.parse_args()

    repo = Path.cwd()
    if args.write:
        write_surfaces(repo)
        return 0
    if args.check_source:
        return check_source_surfaces(repo)
    return check_surfaces(repo)


if __name__ == "__main__":
    raise SystemExit(main())

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


def git_ls_files(repo: Path) -> list[str]:
    result = subprocess.run(
        ["git", "ls-files"],
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
        for path in git_ls_files(repo)
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


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()

    repo = Path.cwd()
    if args.write:
        write_surfaces(repo)
        return 0
    return check_surfaces(repo)


if __name__ == "__main__":
    raise SystemExit(main())

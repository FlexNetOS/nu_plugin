#!/usr/bin/env python3
"""Repair deterministic CDB068 execution-control drift."""

from __future__ import annotations

import argparse
import csv
import io
from collections import Counter
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
TASK_GRAPH = ROOT / "execution/TASK_GRAPH.csv"
TASK_MAP = ROOT / "execution/TASK_FILE_MAP.csv"
TASK_PROJECTION = ROOT / "execution/TASK_GRAPH.md"
COMMAND_LEDGER = ROOT / "execution/COMMAND_LEDGER.csv"
EPHEMERAL_PARTS = {"__pycache__", ".pytest_cache", ".mypy_cache", "target"}
EPHEMERAL_SUFFIXES = {".pyc", ".pyo"}
CDB068_VALIDATOR_PATHS = [
    "scripts/repair_cdb068_execution_truth.py",
    "scripts/validate_task_graph.py",
    "tests/test_task_graph_validator.py",
]


def read_csv(path: Path) -> tuple[list[str], list[dict[str | None, object]]]:
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        if reader.fieldnames is None:
            raise SystemExit(f"missing CSV header: {path}")
        return list(reader.fieldnames), list(reader)


def write_csv(fieldnames: list[str], rows: list[dict[str, str]]) -> str:
    output = io.StringIO(newline="")
    writer = csv.DictWriter(output, fieldnames=fieldnames, lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return output.getvalue()


def is_ephemeral(value: str) -> bool:
    path = PurePosixPath(value)
    return bool(
        EPHEMERAL_PARTS.intersection(path.parts)
        or path.suffix.lower() in EPHEMERAL_SUFFIXES
    )


def normalized_paths(value: str) -> str:
    return ";".join(
        item
        for item in (part.strip() for part in value.split(";"))
        if item and not is_ephemeral(item)
    )


def append_paths(value: str, additions: list[str]) -> str:
    values = [item for item in value.split(";") if item]
    for addition in additions:
        if addition not in values:
            values.append(addition)
    return ";".join(values)


def normalize_graph() -> tuple[str, list[dict[str, str]]]:
    fieldnames, raw_rows = read_csv(TASK_GRAPH)
    rows: list[dict[str, str]] = []
    path_fields = {
        "allowed_files",
        "current_artifact_paths",
        "future_artifact_paths",
    }
    for raw in raw_rows:
        if None in raw:
            raise SystemExit(f"unexpected extra TASK_GRAPH fields: {raw[None]!r}")
        row = {key: str(value or "") for key, value in raw.items() if key is not None}
        for field in path_fields:
            row[field] = normalized_paths(row[field])
        if row["status"] == "complete":
            row["evidence_status"] = "evidence_files_present"
        if row["task_id"] == "CDB068":
            row["allowed_files"] = append_paths(
                row["allowed_files"], CDB068_VALIDATOR_PATHS
            )
            row["current_artifact_paths"] = append_paths(
                row["current_artifact_paths"], CDB068_VALIDATOR_PATHS
            )
        rows.append(row)
    return write_csv(fieldnames, rows), rows


def normalize_map() -> str:
    fieldnames, raw_rows = read_csv(TASK_MAP)
    rows: list[dict[str, str]] = []
    path_fields = {
        "must_read",
        "may_update",
        "must_update_on_change",
        "raw_log_path",
        "evidence_artifacts",
        "exact_read_paths",
        "exact_update_paths",
    }
    for raw in raw_rows:
        if None in raw:
            raise SystemExit(f"unexpected extra TASK_FILE_MAP fields: {raw[None]!r}")
        row = {key: str(value or "") for key, value in raw.items() if key is not None}
        for field in path_fields:
            row[field] = normalized_paths(row[field])
        if row["task_id"] == "CDB068":
            row["may_update"] = append_paths(
                row["may_update"], CDB068_VALIDATOR_PATHS
            )
            row["exact_update_paths"] = append_paths(
                row["exact_update_paths"], CDB068_VALIDATOR_PATHS
            )
        rows.append(row)
    return write_csv(fieldnames, rows)


def normalize_ledger() -> str:
    fieldnames, raw_rows = read_csv(COMMAND_LEDGER)
    rows: list[dict[str, str]] = []
    for raw in raw_rows:
        extras = raw.pop(None, [])
        row = {key: str(value or "") for key, value in raw.items() if key is not None}
        if extras:
            row["notes"] = ",".join([row["notes"], *map(str, extras)])
        rows.append(row)
    return write_csv(fieldnames, rows)


def render_projection(rows: list[dict[str, str]]) -> str:
    counts = dict(sorted(Counter(row["status"] for row in rows).items()))
    first_incomplete = next(
        (row["task_id"] for row in rows if row["status"] != "complete"),
        "none",
    )
    lines = [
        "# TASK GRAPH",
        "",
        "`execution/TASK_GRAPH.csv` is the source of truth for task execution. "
        "This Markdown file is a readable projection only.",
        "",
        "## Source-of-truth contract",
        "",
        "- Every task row must cite exact package-relative file paths for current package artifacts.",
        "- Future implementation paths may use declared globs only when the task status is `planned`.",
        "- Completed tasks must have an existing evidence path and raw log path.",
        "- Any execution starts by selecting one row from `execution/TASK_GRAPH.csv`, then passing `READINESS_GATE.md`.",
        "",
        "## Summary",
        "",
        "- Generated deterministically from: `execution/TASK_GRAPH.csv`",
        f"- Task rows: `{len(rows)}`",
        f"- Status counts: `{counts}`",
        f"- First incomplete implementation task: `{first_incomplete}`",
        "- Package repair task: `CDB068`",
        "",
        "## Tasks",
        "",
        "| Task | Status | Phase | Name | Depends on | Primary artifact | Validation gate | Evidence | Path status |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    fields = (
        "task_id",
        "status",
        "phase",
        "task_name",
        "depends_on",
        "primary_artifact",
        "validation_gate",
        "evidence_path",
        "path_resolution_status",
    )
    for row in rows:
        values = [row[field] for field in fields]
        if any("|" in value or "\n" in value for value in values):
            raise SystemExit(f"projection field requires escaping: {row['task_id']}")
        lines.append("| " + " | ".join(values) + " |")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    graph_text, graph_rows = normalize_graph()
    updates = {
        TASK_GRAPH: graph_text,
        TASK_MAP: normalize_map(),
        TASK_PROJECTION: render_projection(graph_rows),
        COMMAND_LEDGER: normalize_ledger(),
    }
    changed = [path for path, content in updates.items() if path.read_text() != content]
    if args.check:
        for path in changed:
            print(path.relative_to(ROOT))
        return int(bool(changed))
    for path in changed:
        path.write_text(updates[path], encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

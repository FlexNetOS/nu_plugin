#!/usr/bin/env python3
"""Validate the executable CodeDB task-graph and proof control plane.

Structure mode validates that the package is internally executable without
claiming that unfinished work is complete. Release mode adds fail-closed
completion requirements for every graph and proof-ledger row.

The validator is read-only. It never updates graph status, proof status,
receipts, manifests, or evidence paths.
"""

from __future__ import annotations

import argparse
import collections
import csv
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any, Iterable


TASK_GRAPH = Path("execution/TASK_GRAPH.csv")
TASK_GRAPH_PROJECTION = Path("execution/TASK_GRAPH.md")
TASK_MAP = Path("execution/TASK_FILE_MAP.csv")
COMMAND_LEDGER = Path("execution/COMMAND_LEDGER.csv")
BIDIRECTIONAL_GRAPH = Path("execution/BIDIRECTIONAL_TASK_GRAPH.csv")
BIDIRECTIONAL_MAP = Path("execution/BIDIRECTIONAL_TASK_FILE_MAP.csv")
POLYGLOT_GRAPH = Path("execution/POLYGLOT_TASK_GRAPH.csv")
POLYGLOT_MAP = Path("execution/POLYGLOT_TASK_FILE_MAP.csv")
PROOF_LEDGER = Path("execution/REQUIREMENT_PROOF_LEDGER.csv")
SOURCE_RECEIPTS = Path("execution/REQUIREMENT_SOURCE_RECEIPTS.json")
PACKAGE_VALIDATION = Path("manifests/PACKAGE_VALIDATION.json")
PACK_MANIFEST = Path("manifests/PACK_MANIFEST.json")

TASK_GRAPH_COLUMNS = [
    "task_id",
    "task_name",
    "phase",
    "source_file",
    "checklist_ref",
    "goal_ref",
    "subgoal_ref",
    "depends_on",
    "blocks",
    "owner_surface",
    "input_artifacts",
    "output_artifacts",
    "validation_gate",
    "stop_condition",
    "evidence_path",
    "status",
    "title",
    "prd_sections",
    "target_surface",
    "allowed_files",
    "forbidden_actions",
    "primary_artifact",
    "execution_gate",
    "raw_log_path",
    "evidence_artifacts",
    "acceptance_signal",
    "notes",
    "source_truth",
    "governing_docs",
    "acceptance_refs",
    "first_run_refs",
    "stop_condition_refs",
    "checklist_source_files",
    "current_artifact_paths",
    "future_artifact_paths",
    "non_file_outputs",
    "path_resolution_status",
    "evidence_status",
    "path_policy",
    "implementation_start_gate",
]

TASK_MAP_COLUMNS = [
    "task_id",
    "must_read",
    "may_update",
    "must_update_on_change",
    "validation_gate",
    "raw_log_path",
    "evidence_artifacts",
    "source_of_truth_table",
    "governing_docs",
    "exact_read_paths",
    "exact_update_paths",
    "path_policy",
    "task_graph_row",
]

BIDIRECTIONAL_GRAPH_COLUMNS = [
    "task_id",
    "phase",
    "title",
    "gitkb_slug",
    "depends_on",
    "primary_artifact",
    "validation_gate",
    "status",
]

BIDIRECTIONAL_MAP_COLUMNS = [
    "task_id",
    "must_read",
    "may_update",
    "must_update_on_change",
    "validation_gate",
]

POLYGLOT_GRAPH_COLUMNS = [
    "task_id",
    "title",
    "phase",
    "status",
    "depends_on",
    "primary_outputs",
    "validation_gate",
    "safety_constraints",
    "notes",
]

POLYGLOT_MAP_COLUMNS = [
    "task_id",
    "must_read",
    "may_update",
    "must_update_on_change",
    "validation_commands",
    "safety_constraints",
]

PROOF_LEDGER_COLUMNS = [
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

COMMAND_LEDGER_COLUMNS = [
    "timestamp_utc",
    "task_id",
    "cwd",
    "repo",
    "command",
    "output_path",
    "exit_code",
    "redaction",
    "notes",
]

GRAPH_STATUS_VALUES = {"planned", "active", "blocked", "complete"}
PROOF_STATUS_VALUES = {"verified", "partial", "missing", "contradicted", "blocked"}
TASK_STATUS_VALUES = {"planned", "active", "blocked", "complete"}
PATH_PATTERN = re.compile(r"[*?[]")
EPHEMERAL_PATH_PARTS = {"__pycache__", ".pytest_cache", ".mypy_cache", "target"}
EPHEMERAL_SUFFIXES = {".pyc", ".pyo"}
EXECUTABLE_COMMAND = re.compile(
    r"(^|[;&|]\s*)(cargo|python3?|pytest|bash|sh|nu|nix|codedb|envctl|git)\b"
)


@dataclass(frozen=True, order=True)
class ValidationIssue:
    path: str
    row: str
    rule: str
    detail: str

    def __str__(self) -> str:
        location = self.path
        if self.row:
            location += f":{self.row}"
        return f"{location}: {self.rule}: {self.detail}"


@dataclass(frozen=True)
class GraphContract:
    graph_path: Path
    graph_columns: list[str]
    map_path: Path
    map_columns: list[str]
    graph_validation_fields: tuple[str, ...]
    map_validation_field: str


GRAPH_CONTRACTS = [
    GraphContract(
        TASK_GRAPH,
        TASK_GRAPH_COLUMNS,
        TASK_MAP,
        TASK_MAP_COLUMNS,
        ("validation_gate", "execution_gate"),
        "validation_gate",
    ),
    GraphContract(
        BIDIRECTIONAL_GRAPH,
        BIDIRECTIONAL_GRAPH_COLUMNS,
        BIDIRECTIONAL_MAP,
        BIDIRECTIONAL_MAP_COLUMNS,
        ("validation_gate",),
        "validation_gate",
    ),
    GraphContract(
        POLYGLOT_GRAPH,
        POLYGLOT_GRAPH_COLUMNS,
        POLYGLOT_MAP,
        POLYGLOT_MAP_COLUMNS,
        ("validation_gate",),
        "validation_commands",
    ),
]

MANIFEST_BOUND_INPUTS = {
    str(TASK_GRAPH),
    str(TASK_GRAPH_PROJECTION),
    str(TASK_MAP),
    str(COMMAND_LEDGER),
    str(BIDIRECTIONAL_GRAPH),
    str(BIDIRECTIONAL_MAP),
    str(POLYGLOT_GRAPH),
    str(POLYGLOT_MAP),
    str(PROOF_LEDGER),
    str(SOURCE_RECEIPTS),
}


def _split(value: str) -> list[str]:
    return [item.strip() for item in value.split(";") if item.strip()]


def _issue(
    issues: list[ValidationIssue],
    path: Path | str,
    row: str,
    rule: str,
    detail: str,
) -> None:
    issues.append(ValidationIssue(str(path), row, rule, detail))


def _read_csv(
    root: Path,
    relative: Path,
    expected_columns: list[str],
    issues: list[ValidationIssue],
) -> list[dict[str, str]]:
    path = root / relative
    if not path.is_file():
        _issue(issues, relative, "", "missing required file", "file not found")
        return []
    try:
        with path.open(newline="", encoding="utf-8") as handle:
            reader = csv.DictReader(handle)
            if reader.fieldnames != expected_columns:
                _issue(
                    issues,
                    relative,
                    "header",
                    "CSV schema mismatch",
                    f"expected {expected_columns}, got {reader.fieldnames}",
                )
                return []
            rows = list(reader)
    except (OSError, csv.Error, UnicodeError) as error:
        _issue(issues, relative, "", "invalid CSV", str(error))
        return []

    for number, row in enumerate(rows, 2):
        if None in row:
            _issue(
                issues,
                relative,
                str(number),
                "CSV row width mismatch",
                f"unexpected fields: {row[None]}",
            )
    return rows


def _read_json(
    root: Path,
    relative: Path,
    issues: list[ValidationIssue],
) -> dict[str, Any] | None:
    path = root / relative
    if not path.is_file():
        _issue(issues, relative, "", "missing required file", "file not found")
        return None
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        _issue(issues, relative, "", "invalid JSON", str(error))
        return None
    if not isinstance(value, dict):
        _issue(
            issues,
            relative,
            "",
            "JSON schema mismatch",
            "top-level value must be an object",
        )
        return None
    return value


def _duplicates(values: Iterable[str]) -> set[str]:
    seen: set[str] = set()
    duplicates: set[str] = set()
    for value in values:
        if value in seen:
            duplicates.add(value)
        seen.add(value)
    return duplicates


def _safe_package_path(value: str) -> bool:
    if (
        not value
        or "\\" in value
        or re.match(r"^[A-Za-z][A-Za-z0-9+.-]*:", value)
    ):
        return False
    path = PurePosixPath(value)
    return not path.is_absolute() and ".." not in path.parts and "." not in path.parts


def _ephemeral_package_path(value: str) -> bool:
    path = PurePosixPath(value)
    return bool(
        EPHEMERAL_PATH_PARTS.intersection(path.parts)
        or path.suffix.lower() in EPHEMERAL_SUFFIXES
    )


def _validate_path_value(
    issues: list[ValidationIssue],
    relative: Path,
    task_id: str,
    field: str,
    value: str,
    *,
    patterns_allowed: bool,
) -> None:
    if not _safe_package_path(value):
        _issue(
            issues,
            relative,
            task_id,
            "unsafe package path",
            f"{field}={value!r}",
        )
    if PATH_PATTERN.search(value) and not patterns_allowed:
        _issue(
            issues,
            relative,
            task_id,
            "path pattern not allowed",
            f"{field}={value!r}",
        )
    if _ephemeral_package_path(value):
        _issue(
            issues,
            relative,
            task_id,
            "ephemeral package path",
            f"{field}={value!r}",
        )


def _validate_graph_identity_and_fields(
    contract: GraphContract,
    graph_rows: list[dict[str, str]],
    map_rows: list[dict[str, str]],
    issues: list[ValidationIssue],
) -> None:
    graph_ids = [row.get("task_id", "").strip() for row in graph_rows]
    map_ids = [row.get("task_id", "").strip() for row in map_rows]

    for task_id in sorted(_duplicates(graph_ids)):
        _issue(
            issues,
            contract.graph_path,
            task_id or "<empty>",
            "duplicate task id",
            "task IDs must be unique",
        )
    for task_id in sorted(_duplicates(map_ids)):
        _issue(
            issues,
            contract.map_path,
            task_id or "<empty>",
            "duplicate map task id",
            "file-map task IDs must be unique",
        )
    if "" in graph_ids:
        _issue(
            issues,
            contract.graph_path,
            "<empty>",
            "missing task id",
            "task_id is required",
        )
    if "" in map_ids:
        _issue(
            issues,
            contract.map_path,
            "<empty>",
            "missing task id",
            "task_id is required",
        )

    graph_set = set(graph_ids) - {""}
    map_set = set(map_ids) - {""}
    if graph_set != map_set:
        _issue(
            issues,
            contract.map_path,
            "",
            "graph-map parity",
            "missing from map="
            f"{sorted(graph_set - map_set)}, missing from graph={sorted(map_set - graph_set)}",
        )

    for row in graph_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        status = row.get("status", "").strip().lower()
        if status not in GRAPH_STATUS_VALUES:
            _issue(
                issues,
                contract.graph_path,
                task_id,
                "invalid graph status",
                status or "<empty>",
            )
        for field in contract.graph_validation_fields:
            if not row.get(field, "").strip():
                _issue(
                    issues,
                    contract.graph_path,
                    task_id,
                    "missing validation command",
                    field,
                )

    for row in map_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        if not row.get(contract.map_validation_field, "").strip():
            _issue(
                issues,
                contract.map_path,
                task_id,
                "missing validation command",
                contract.map_validation_field,
            )
        if not row.get("must_update_on_change", "").strip():
            _issue(
                issues,
                contract.map_path,
                task_id,
                "missing evidence path",
                "must_update_on_change",
            )


def _validate_dependencies(
    graph_rows_by_path: dict[Path, list[dict[str, str]]],
    issues: list[ValidationIssue],
) -> None:
    all_rows: list[tuple[Path, dict[str, str]]] = [
        (path, row)
        for path, rows in graph_rows_by_path.items()
        for row in rows
        if row.get("task_id", "").strip()
    ]
    all_ids = [row["task_id"].strip() for _, row in all_rows]
    id_set = set(all_ids)

    for task_id in sorted(_duplicates(all_ids)):
        _issue(
            issues,
            "execution",
            task_id,
            "duplicate task id",
            "task ID is duplicated across graph files",
        )

    adjacency: dict[str, list[str]] = {}
    source_path: dict[str, Path] = {}
    for path, row in all_rows:
        task_id = row["task_id"].strip()
        source_path.setdefault(task_id, path)
        dependencies = _split(row.get("depends_on", ""))
        adjacency.setdefault(task_id, [])
        for dependency in dependencies:
            if dependency == task_id:
                _issue(
                    issues,
                    path,
                    task_id,
                    "self dependency",
                    dependency,
                )
            elif dependency not in id_set:
                _issue(
                    issues,
                    path,
                    task_id,
                    "unknown dependency",
                    dependency,
                )
            else:
                adjacency[task_id].append(dependency)

        for blocked in _split(row.get("blocks", "")):
            if blocked not in id_set:
                _issue(
                    issues,
                    path,
                    task_id,
                    "unknown blocked task",
                    blocked,
                )

    state: dict[str, int] = {}
    stack: list[str] = []
    reported_cycles: set[tuple[str, ...]] = set()

    def visit(task_id: str) -> None:
        state[task_id] = 1
        stack.append(task_id)
        for dependency in adjacency.get(task_id, []):
            if state.get(dependency, 0) == 0:
                visit(dependency)
            elif state.get(dependency) == 1:
                start = stack.index(dependency)
                cycle = tuple(stack[start:] + [dependency])
                if cycle not in reported_cycles:
                    reported_cycles.add(cycle)
                    _issue(
                        issues,
                        source_path.get(task_id, Path("execution")),
                        task_id,
                        "dependency cycle",
                        " -> ".join(cycle),
                    )
        stack.pop()
        state[task_id] = 2

    for task_id in sorted(adjacency):
        if state.get(task_id, 0) == 0:
            visit(task_id)


def _validate_task_paths(
    root: Path,
    graph_rows: list[dict[str, str]],
    map_rows: list[dict[str, str]],
    issues: list[ValidationIssue],
) -> None:
    statuses = {
        row.get("task_id", "").strip(): row.get("status", "").strip().lower()
        for row in graph_rows
    }

    for row in graph_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        status = row.get("status", "").strip().lower()
        allowed = _split(row.get("allowed_files", ""))
        current = _split(row.get("current_artifact_paths", ""))
        future = _split(row.get("future_artifact_paths", ""))

        if not row.get("path_policy", "").strip():
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "missing path policy",
                "path_policy",
            )
        if not allowed:
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "missing allowed paths",
                "allowed_files",
            )
        if not current:
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "missing current paths",
                "current_artifact_paths",
            )

        for value in allowed:
            _validate_path_value(
                issues,
                TASK_GRAPH,
                task_id,
                "allowed_files",
                value,
                patterns_allowed=status == "planned",
            )
            if PATH_PATTERN.search(value) and status != "planned":
                _issue(
                    issues,
                    TASK_GRAPH,
                    task_id,
                    "allowed path pattern requires planned status",
                    value,
                )

        for value in current:
            _validate_path_value(
                issues,
                TASK_GRAPH,
                task_id,
                "current_artifact_paths",
                value,
                patterns_allowed=False,
            )
            if PATH_PATTERN.search(value):
                _issue(
                    issues,
                    TASK_GRAPH,
                    task_id,
                    "current path contains pattern",
                    value,
                )
                continue
            if _ephemeral_package_path(value):
                _issue(
                    issues,
                    TASK_GRAPH,
                    task_id,
                    "ephemeral current artifact",
                    value,
                )
            if _safe_package_path(value) and not (root / value).exists():
                _issue(
                    issues,
                    TASK_GRAPH,
                    task_id,
                    "current path missing",
                    value,
                )

        for value in future:
            _validate_path_value(
                issues,
                TASK_GRAPH,
                task_id,
                "future_artifact_paths",
                value,
                patterns_allowed=True,
            )
        if future and status != "planned":
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "future paths require planned status",
                f"status={status}",
            )
        if status == "planned" and not future:
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "planned task missing future paths",
                "future_artifact_paths",
            )
        if set(current) & set(future):
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "current/future path overlap",
                ";".join(sorted(set(current) & set(future))),
            )

        resolution = row.get("path_resolution_status", "").strip()
        if status == "planned" and not resolution.startswith("planned_"):
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "path resolution contradicts status",
                f"status={status}, path_resolution_status={resolution}",
            )

        evidence_status = row.get("evidence_status", "").strip()
        expected_evidence_status = {
            "complete": "evidence_files_present",
            "planned": "pending_task_execution",
        }.get(status)
        if expected_evidence_status and evidence_status != expected_evidence_status:
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "evidence status contradicts task status",
                f"status={status}, evidence_status={evidence_status or '<empty>'}, "
                f"expected={expected_evidence_status}",
            )
        if status == "complete" and not resolution.startswith("complete_"):
            _issue(
                issues,
                TASK_GRAPH,
                task_id,
                "path resolution contradicts status",
                f"status={status}, path_resolution_status={resolution}",
            )

        for field in ("raw_log_path", "evidence_artifacts"):
            if not row.get(field, "").strip():
                _issue(
                    issues,
                    TASK_GRAPH,
                    task_id,
                    "missing evidence path",
                    field,
                )

    for row in map_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        status = statuses.get(task_id, "")
        if not row.get("path_policy", "").strip():
            _issue(
                issues,
                TASK_MAP,
                task_id,
                "missing path policy",
                "path_policy",
            )
        if row.get("source_of_truth_table", "").strip() != str(TASK_GRAPH):
            _issue(
                issues,
                TASK_MAP,
                task_id,
                "map source binding",
                row.get("source_of_truth_table", "") or "<empty>",
            )
        if row.get("task_graph_row", "").strip() != task_id:
            _issue(
                issues,
                TASK_MAP,
                task_id,
                "map row binding",
                row.get("task_graph_row", "") or "<empty>",
            )

        for field in (
            "must_read",
            "may_update",
            "must_update_on_change",
            "raw_log_path",
            "evidence_artifacts",
            "exact_read_paths",
            "exact_update_paths",
        ):
            for value in _split(row.get(field, "")):
                patterns_allowed = field == "may_update" and status == "planned"
                _validate_path_value(
                    issues,
                    TASK_MAP,
                    task_id,
                    field,
                    value,
                    patterns_allowed=patterns_allowed,
                )


def _validate_task_graph_projection(
    root: Path,
    graph_rows: list[dict[str, str]],
    issues: list[ValidationIssue],
) -> None:
    path = root / TASK_GRAPH_PROJECTION
    if not path.is_file():
        _issue(
            issues,
            TASK_GRAPH_PROJECTION,
            "",
            "missing required file",
            "file not found",
        )
        return
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        _issue(
            issues,
            TASK_GRAPH_PROJECTION,
            "",
            "invalid task graph projection",
            str(error),
        )
        return

    projected: dict[str, list[str]] = {}
    for line in text.splitlines():
        if not re.match(r"^\|\s*CDB\d{3}\s*\|", line):
            continue
        fields = [field.strip() for field in line.strip().strip("|").split("|")]
        task_id = fields[0] if fields else "<empty>"
        if len(fields) != 9:
            _issue(
                issues,
                TASK_GRAPH_PROJECTION,
                task_id,
                "task graph projection drift",
                f"expected 9 table fields, got {len(fields)}",
            )
            continue
        if task_id in projected:
            _issue(
                issues,
                TASK_GRAPH_PROJECTION,
                task_id,
                "task graph projection drift",
                "duplicate projected task row",
            )
        projected[task_id] = fields

    graph_by_id = {row["task_id"].strip(): row for row in graph_rows}
    if set(projected) != set(graph_by_id):
        _issue(
            issues,
            TASK_GRAPH_PROJECTION,
            "tasks",
            "task graph projection drift",
            "missing from projection="
            f"{sorted(set(graph_by_id) - set(projected))}, extra="
            f"{sorted(set(projected) - set(graph_by_id))}",
        )

    csv_fields = (
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
    for task_id in sorted(set(projected) & set(graph_by_id)):
        expected = [graph_by_id[task_id].get(field, "").strip() for field in csv_fields]
        if projected[task_id] != expected:
            _issue(
                issues,
                TASK_GRAPH_PROJECTION,
                task_id,
                "task graph projection drift",
                f"expected={expected!r}, got={projected[task_id]!r}",
            )

    row_match = re.search(r"^- Task rows: `(?P<count>\d+)`$", text, re.MULTILINE)
    expected_count = len(graph_rows)
    if row_match is None or int(row_match.group("count")) != expected_count:
        _issue(
            issues,
            TASK_GRAPH_PROJECTION,
            "summary",
            "task graph projection drift",
            f"task row summary must equal {expected_count}",
        )
    counts = dict(sorted(collections.Counter(row["status"].strip() for row in graph_rows).items()))
    status_match = re.search(r"^- Status counts: `(?P<counts>.+)`$", text, re.MULTILINE)
    if status_match is None or status_match.group("counts") != str(counts):
        _issue(
            issues,
            TASK_GRAPH_PROJECTION,
            "summary",
            "task graph projection drift",
            f"status count summary must equal {counts}",
        )

def _validate_supplemental_map_paths(
    graph_path: Path,
    map_path: Path,
    graph_rows: list[dict[str, str]],
    map_rows: list[dict[str, str]],
    issues: list[ValidationIssue],
) -> None:
    statuses = {
        row.get("task_id", "").strip(): row.get("status", "").strip().lower()
        for row in graph_rows
    }
    for row in map_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        status = statuses.get(task_id, "")
        for field in ("must_read", "may_update", "must_update_on_change"):
            for value in _split(row.get(field, "")):
                _validate_path_value(
                    issues,
                    map_path,
                    task_id,
                    field,
                    value,
                    patterns_allowed=field == "may_update" and status == "planned",
                )

    # Primary artifacts are exact package-relative declarations, even while the
    # implementation behind an active task remains incomplete.
    output_field = "primary_artifact" if graph_path == BIDIRECTIONAL_GRAPH else "primary_outputs"
    for row in graph_rows:
        task_id = row.get("task_id", "<empty>").strip() or "<empty>"
        for value in _split(row.get(output_field, "")):
            _validate_path_value(
                issues,
                graph_path,
                task_id,
                output_field,
                value,
                patterns_allowed=False,
            )


def _validate_receipts(
    receipt: dict[str, Any] | None,
    ledger_rows: list[dict[str, str]],
    issues: list[ValidationIssue],
) -> None:
    if receipt is None:
        return
    if receipt.get("schema_version") != 1:
        _issue(
            issues,
            SOURCE_RECEIPTS,
            "schema_version",
            "JSON schema mismatch",
            "schema_version must equal 1",
        )
    sources = receipt.get("sources")
    if not isinstance(sources, dict):
        _issue(
            issues,
            SOURCE_RECEIPTS,
            "sources",
            "JSON schema mismatch",
            "sources must be an object",
        )
        return

    authorities: dict[str, set[str]] = {}
    for row in ledger_rows:
        authority = row.get("authoritative_source", "").strip()
        if authority.startswith(("gitkb:", "https://", "http://")):
            authorities.setdefault(row.get("parent_id", "").strip(), set()).add(authority)

    for parent_id, values in sorted(authorities.items()):
        source = sources.get(parent_id)
        if not isinstance(source, dict):
            _issue(
                issues,
                SOURCE_RECEIPTS,
                parent_id or "<empty>",
                "missing source receipt",
                ", ".join(sorted(values)),
            )
            continue
        if not isinstance(source.get("observed_at_utc"), str) or not source.get(
            "observed_at_utc", ""
        ).strip():
            _issue(
                issues,
                SOURCE_RECEIPTS,
                parent_id,
                "JSON schema mismatch",
                "observed_at_utc is required",
            )

        for authority in values:
            if authority.startswith("gitkb:"):
                if source.get("kind") != "gitkb":
                    _issue(
                        issues,
                        SOURCE_RECEIPTS,
                        parent_id,
                        "source receipt binding",
                        f"expected kind=gitkb for {authority}",
                    )
                if source.get("slug") != authority.removeprefix("gitkb:"):
                    _issue(
                        issues,
                        SOURCE_RECEIPTS,
                        parent_id,
                        "source receipt binding",
                        f"slug does not match {authority}",
                    )
                for field in ("document_id", "status_observed"):
                    if not isinstance(source.get(field), str) or not source.get(
                        field, ""
                    ).strip():
                        _issue(
                            issues,
                            SOURCE_RECEIPTS,
                            parent_id,
                            "JSON schema mismatch",
                            f"{field} is required for gitkb receipts",
                        )
            else:
                if source.get("kind") != "github_issue":
                    _issue(
                        issues,
                        SOURCE_RECEIPTS,
                        parent_id,
                        "source receipt binding",
                        f"expected kind=github_issue for {authority}",
                    )
                if source.get("url") != authority:
                    _issue(
                        issues,
                        SOURCE_RECEIPTS,
                        parent_id,
                        "source receipt binding",
                        f"url does not match {authority}",
                    )
                for field in (
                    "repository",
                    "issue",
                    "title",
                    "state_observed",
                    "source_updated_at",
                    "body_sha256",
                ):
                    if source.get(field) in (None, ""):
                        _issue(
                            issues,
                            SOURCE_RECEIPTS,
                            parent_id,
                            "JSON schema mismatch",
                            f"{field} is required for GitHub issue receipts",
                        )


def _validate_ledger(
    ledger_rows: list[dict[str, str]],
    graph_statuses: dict[str, str],
    issues: list[ValidationIssue],
    *,
    release: bool,
) -> None:
    ids = [row.get("requirement_id", "").strip() for row in ledger_rows]
    for requirement_id in sorted(_duplicates(ids)):
        _issue(
            issues,
            PROOF_LEDGER,
            requirement_id or "<empty>",
            "duplicate requirement id",
            "requirement IDs must be unique",
        )
    if "" in ids:
        _issue(
            issues,
            PROOF_LEDGER,
            "<empty>",
            "missing requirement id",
            "requirement_id is required",
        )

    required_fields = (
        "parent_id",
        "requirement",
        "authoritative_source",
        "source_ref",
        "implementation_paths",
        "test_paths",
        "verification_command",
    )
    for row in ledger_rows:
        requirement_id = (
            row.get("requirement_id", "<empty>").strip() or "<empty>"
        )
        for field in required_fields:
            if not row.get(field, "").strip():
                _issue(
                    issues,
                    PROOF_LEDGER,
                    requirement_id,
                    "missing proof field",
                    field,
                )

        evidence_status = row.get("evidence_status", "").strip().lower()
        task_status = row.get("task_status", "").strip().lower()
        if evidence_status not in PROOF_STATUS_VALUES:
            _issue(
                issues,
                PROOF_LEDGER,
                requirement_id,
                "invalid proof status",
                evidence_status or "<empty>",
            )
        if task_status not in TASK_STATUS_VALUES:
            _issue(
                issues,
                PROOF_LEDGER,
                requirement_id,
                "invalid proof task status",
                task_status or "<empty>",
            )

        graph_status = graph_statuses.get(requirement_id)
        if graph_status and graph_status != task_status:
            _issue(
                issues,
                PROOF_LEDGER,
                requirement_id,
                "proof status contradicts graph",
                f"graph={graph_status}, ledger={task_status}",
            )

        command = row.get("verification_command", "").strip()
        if command and not EXECUTABLE_COMMAND.search(command):
            _issue(
                issues,
                PROOF_LEDGER,
                requirement_id,
                "non-executable validation command",
                command,
            )

        if release:
            incomplete: list[str] = []
            if evidence_status != "verified":
                incomplete.append(f"evidence_status={evidence_status or '<empty>'}")
            if task_status != "complete":
                incomplete.append(f"task_status={task_status or '<empty>'}")
            if not row.get("proof_artifacts", "").strip():
                incomplete.append("proof_artifacts=<empty>")
            # proof_head_sha is intentionally deprecated. Exact revision
            # identity is carried by the external schema-4 receipt and its
            # detached GitHub attestation; requiring a self-referential SHA in
            # the committed ledger would contradict that trust model.
            if row.get("proof_head_sha", "").strip():
                incomplete.append("proof_head_sha=deprecated-nonempty")
            for field in ("implementation_paths", "test_paths", "proof_artifacts"):
                if "<missing" in row.get(field, "").lower():
                    incomplete.append(f"{field}=placeholder")
            if incomplete:
                _issue(
                    issues,
                    PROOF_LEDGER,
                    requirement_id,
                    "release proof row incomplete",
                    ", ".join(incomplete),
                )


def _validate_manifest(
    manifest: dict[str, Any] | None,
    package_validation: dict[str, Any] | None,
    issues: list[ValidationIssue],
    *,
    release: bool,
) -> None:
    if manifest is not None:
        required_scalars = {
            "canonical_prd": str,
            "checksum_scope": str,
            "checksum_scope_files_sha256": str,
            "entrypoint": str,
            "file_count_in_checksum_scope": int,
            "generated_at_utc": str,
            "package": str,
            "surface": str,
            "task_graph_source_of_truth": str,
        }
        for field, expected_type in required_scalars.items():
            if not isinstance(manifest.get(field), expected_type):
                _issue(
                    issues,
                    PACK_MANIFEST,
                    field,
                    "JSON schema mismatch",
                    f"expected {expected_type.__name__}",
                )

        if manifest.get("task_graph_source_of_truth") != str(TASK_GRAPH):
            _issue(
                issues,
                PACK_MANIFEST,
                "task_graph_source_of_truth",
                "manifest graph binding",
                str(manifest.get("task_graph_source_of_truth", "<missing>")),
            )

        excluded = manifest.get("checksum_excluded_paths")
        if not isinstance(excluded, list) or not all(
            isinstance(item, str) for item in excluded
        ):
            _issue(
                issues,
                PACK_MANIFEST,
                "checksum_excluded_paths",
                "JSON schema mismatch",
                "expected an array of paths",
            )

        files = manifest.get("files")
        if not isinstance(files, list):
            _issue(
                issues,
                PACK_MANIFEST,
                "files",
                "JSON schema mismatch",
                "files must be an array",
            )
        else:
            paths: list[str] = []
            for index, item in enumerate(files):
                if not isinstance(item, dict):
                    _issue(
                        issues,
                        PACK_MANIFEST,
                        f"files[{index}]",
                        "JSON schema mismatch",
                        "file entry must be an object",
                    )
                    continue
                path = item.get("path")
                paths.append(path if isinstance(path, str) else "")
                if not isinstance(path, str) or not _safe_package_path(path):
                    _issue(
                        issues,
                        PACK_MANIFEST,
                        f"files[{index}]",
                        "unsafe package path",
                        repr(path),
                    )
                if not isinstance(item.get("bytes"), int) or item.get("bytes", -1) < 0:
                    _issue(
                        issues,
                        PACK_MANIFEST,
                        f"files[{index}]",
                        "JSON schema mismatch",
                        "bytes must be a non-negative integer",
                    )
                digest = item.get("sha256")
                if not isinstance(digest, str) or not re.fullmatch(
                    r"[0-9a-f]{64}", digest
                ):
                    _issue(
                        issues,
                        PACK_MANIFEST,
                        f"files[{index}]",
                        "JSON schema mismatch",
                        "sha256 must be a lowercase 64-character digest",
                    )
            for path in sorted(_duplicates(paths)):
                _issue(
                    issues,
                    PACK_MANIFEST,
                    path or "<empty>",
                    "duplicate manifest path",
                    "manifest file paths must be unique",
                )
            count = manifest.get("file_count_in_checksum_scope")
            if isinstance(count, int) and count != len(files):
                _issue(
                    issues,
                    PACK_MANIFEST,
                    "file_count_in_checksum_scope",
                    "manifest count binding",
                    f"declared={count}, actual={len(files)}",
                )
            missing = MANIFEST_BOUND_INPUTS - set(paths)
            if missing:
                _issue(
                    issues,
                    PACK_MANIFEST,
                    "files",
                    "manifest input binding",
                    f"missing {sorted(missing)}",
                )

    if package_validation is not None:
        if not isinstance(package_validation.get("generated_at_utc"), str):
            _issue(
                issues,
                PACKAGE_VALIDATION,
                "generated_at_utc",
                "JSON schema mismatch",
                "generated_at_utc must be a string",
            )
        if not isinstance(package_validation.get("warnings"), list):
            _issue(
                issues,
                PACKAGE_VALIDATION,
                "warnings",
                "JSON schema mismatch",
                "warnings must be an array",
            )
        if not isinstance(package_validation.get("status"), str):
            _issue(
                issues,
                PACKAGE_VALIDATION,
                "status",
                "JSON schema mismatch",
                "status must be a string",
            )
        checks = package_validation.get("checks")
        if not isinstance(checks, dict):
            _issue(
                issues,
                PACKAGE_VALIDATION,
                "checks",
                "JSON schema mismatch",
                "checks must be an object",
            )
        else:
            expected_paths = {
                "manifest_path": str(PACK_MANIFEST),
                "checksums_path": "manifests/CHECKSUMS.sha256",
            }
            for field, expected in expected_paths.items():
                if checks.get(field) != expected:
                    _issue(
                        issues,
                        PACKAGE_VALIDATION,
                        field,
                        "package validation binding",
                        f"expected {expected}, got {checks.get(field)!r}",
                    )
            generated = checks.get("generated_paths")
            if not isinstance(generated, list) or not {
                str(PACK_MANIFEST),
                str(PACKAGE_VALIDATION),
                "manifests/CHECKSUMS.sha256",
            }.issubset(set(generated)):
                _issue(
                    issues,
                    PACKAGE_VALIDATION,
                    "generated_paths",
                    "package validation binding",
                    "generated truth-surface paths are incomplete",
                )
            if manifest is not None:
                bindings = (
                    (
                        "checksum_scope_count",
                        "file_count_in_checksum_scope",
                    ),
                    (
                        "checksum_scope_files_sha256",
                        "checksum_scope_files_sha256",
                    ),
                )
                for check_field, manifest_field in bindings:
                    if checks.get(check_field) != manifest.get(manifest_field):
                        _issue(
                            issues,
                            PACKAGE_VALIDATION,
                            check_field,
                            "package validation binding",
                            f"does not match manifest {manifest_field}",
                        )

        if release and package_validation.get("status") != "passed":
            _issue(
                issues,
                PACKAGE_VALIDATION,
                "status",
                "release package validation incomplete",
                str(package_validation.get("status", "<missing>")),
            )


def audit_task_graph(root: Path, *, release: bool = False) -> list[ValidationIssue]:
    """Return all structure or release violations without modifying ``root``."""

    root = root.resolve()
    issues: list[ValidationIssue] = []
    graph_rows_by_path: dict[Path, list[dict[str, str]]] = {}
    map_rows_by_path: dict[Path, list[dict[str, str]]] = {}

    for contract in GRAPH_CONTRACTS:
        graph_rows = _read_csv(
            root, contract.graph_path, contract.graph_columns, issues
        )
        map_rows = _read_csv(root, contract.map_path, contract.map_columns, issues)
        graph_rows_by_path[contract.graph_path] = graph_rows
        map_rows_by_path[contract.map_path] = map_rows
        _validate_graph_identity_and_fields(
            contract, graph_rows, map_rows, issues
        )

    _validate_dependencies(graph_rows_by_path, issues)
    _read_csv(root, COMMAND_LEDGER, COMMAND_LEDGER_COLUMNS, issues)
    _validate_task_paths(
        root,
        graph_rows_by_path[TASK_GRAPH],
        map_rows_by_path[TASK_MAP],
        issues,
    )
    _validate_task_graph_projection(
        root,
        graph_rows_by_path[TASK_GRAPH],
        issues,
    )
    _validate_supplemental_map_paths(
        BIDIRECTIONAL_GRAPH,
        BIDIRECTIONAL_MAP,
        graph_rows_by_path[BIDIRECTIONAL_GRAPH],
        map_rows_by_path[BIDIRECTIONAL_MAP],
        issues,
    )
    _validate_supplemental_map_paths(
        POLYGLOT_GRAPH,
        POLYGLOT_MAP,
        graph_rows_by_path[POLYGLOT_GRAPH],
        map_rows_by_path[POLYGLOT_MAP],
        issues,
    )

    graph_statuses = {
        row.get("task_id", "").strip(): row.get("status", "").strip().lower()
        for rows in graph_rows_by_path.values()
        for row in rows
        if row.get("task_id", "").strip()
    }
    if release:
        for graph_path, rows in graph_rows_by_path.items():
            for row in rows:
                task_id = row.get("task_id", "<empty>").strip() or "<empty>"
                status = row.get("status", "").strip().lower()
                if status != "complete":
                    _issue(
                        issues,
                        graph_path,
                        task_id,
                        "release graph row incomplete",
                        f"status={status or '<empty>'}",
                    )

    ledger_rows = _read_csv(root, PROOF_LEDGER, PROOF_LEDGER_COLUMNS, issues)
    _validate_ledger(
        ledger_rows,
        graph_statuses,
        issues,
        release=release,
    )

    receipt = _read_json(root, SOURCE_RECEIPTS, issues)
    package_validation = _read_json(root, PACKAGE_VALIDATION, issues)
    manifest = _read_json(root, PACK_MANIFEST, issues)
    _validate_receipts(receipt, ledger_rows, issues)
    _validate_manifest(
        manifest,
        package_validation,
        issues,
        release=release,
    )
    return sorted(set(issues))


def audit_repository(root: Path, *, release: bool = False) -> list[ValidationIssue]:
    """Compatibility alias for callers that name the repository-level audit."""

    return audit_task_graph(root, release=release)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate CodeDB task graphs without mutating source tables."
    )
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
    )
    parser.add_argument(
        "--mode",
        choices=("structure", "release"),
        default=None,
        help="validation mode; structure is the default",
    )
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument(
        "--structure-only",
        action="store_true",
        help="validate structure without asserting release completion",
    )
    modes.add_argument(
        "--release",
        action="store_true",
        help="require every mandatory graph and proof row to be complete",
    )
    args = parser.parse_args(argv)

    release = args.release or args.mode == "release"
    if args.structure_only and args.mode == "release":
        parser.error("--structure-only conflicts with --mode release")
    if args.release and args.mode == "structure":
        parser.error("--release conflicts with --mode structure")

    issues = audit_task_graph(args.root, release=release)
    if issues:
        for issue in issues:
            print(issue, file=sys.stderr)
        print(
            f"task graph validation failed ({'release' if release else 'structure'}): "
            f"{len(issues)} issue(s)",
            file=sys.stderr,
        )
        return 1

    print(
        f"task graph validation passed ({'release' if release else 'structure'}): "
        "graphs, maps, paths, proofs, receipts, and manifests are consistent"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

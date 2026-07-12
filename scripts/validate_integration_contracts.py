#!/usr/bin/env python3
"""Validate the governed CodeDB integration contracts without guessing."""

from __future__ import annotations

import argparse
import re
from dataclasses import dataclass
from pathlib import Path


SURFACES = ("GitKB", "RTK", "Kache", "wild", "Fenix")

BOUNDARIES = {
    "GitKB": ("GitKB Boundary", ("durable explanations", "Rust/crate truth")),
    "RTK": ("RTK Boundary", ("raw failure logs", "source of truth")),
    "Kache": (
        "Kache, wild, and Fenix Boundary",
        ("environment/toolchain facts", "not CodeDB-owned installs"),
    ),
    "wild": (
        "Kache, wild, and Fenix Boundary",
        ("environment/toolchain facts", "not CodeDB-owned installs"),
    ),
    "Fenix": (
        "Kache, wild, and Fenix Boundary",
        ("environment/toolchain facts", "not CodeDB-owned installs"),
    ),
}

OWNERS = {
    "GitKB": "GitKB project workflow",
    "RTK": "RTK summarization/compression workflow",
    "Kache": "Host/toolchain owner",
    "wild": "Host/toolchain owner",
    "Fenix": "Host/toolchain owner",
}

FORBIDDEN_CROSSINGS = {
    "GitKB": ("raw source blobs", "replace runner proof"),
    "RTK": ("failure evidence", "stderr/root cause"),
    "Kache": ("install", "cache state"),
    "wild": ("linker optimization", "crate truth"),
    "Fenix": ("install", "mutate toolchains"),
}

EXPORT_FACTS = {
    "GitKB": (
        "codedb_doctrine_summary",
        "capture_gaps",
        "validation_errors",
        "meta_repo_selection",
        "runner_proof_manifest",
        "command-ledger references",
    ),
    "RTK": (
        "raw_log_paths",
        "runner_proof_manifest.raw_log_path",
        "capture_gaps",
        "validation_errors",
    ),
    "Kache": (
        "kache_status",
        "wrapper path",
        "cache root",
        "enabled/disabled state",
        "provenance source",
    ),
    "wild": ("linker path", "opt-in feature state", "target triple", "link args source"),
    "Fenix": (
        "toolchain channel",
        "component list",
        "target list",
        "rustc/cargo/rustfmt/clippy/rust-analyzer paths and versions",
    ),
}


@dataclass(frozen=True)
class Violation:
    category: str
    surface: str
    detail: str

    def __str__(self) -> str:
        return f"{self.category} [{self.surface}]: {self.detail}"


def _normalize(value: str) -> str:
    return " ".join(value.replace("`", "").split()).casefold()


def _missing_fragments(text: str, required: tuple[str, ...]) -> list[str]:
    normalized = _normalize(text)
    return [fragment for fragment in required if _normalize(fragment) not in normalized]


def _sections(document: str) -> tuple[dict[str, str], set[str]]:
    headings = list(re.finditer(r"^##[ \t]+(.+?)[ \t]*$", document, re.MULTILINE))
    sections: dict[str, str] = {}
    duplicates: set[str] = set()
    for index, match in enumerate(headings):
        title = match.group(1).strip()
        key = _normalize(title)
        start = match.end()
        end = headings[index + 1].start() if index + 1 < len(headings) else len(document)
        if key in sections:
            duplicates.add(title)
        else:
            sections[key] = document[start:end]
    return sections, duplicates


def _ownership_rows(section: str) -> tuple[dict[str, tuple[str, str, str]], bool]:
    rows: dict[str, tuple[str, str, str]] = {}
    header_valid = False
    for line in section.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) != 4 or all(set(cell) <= {"-", ":", " "} for cell in cells):
            continue
        if [_normalize(cell) for cell in cells] == [
            "surface",
            "owner",
            "codedb responsibility",
            "forbidden crossing",
        ]:
            header_valid = True
            continue
        surface = next(
            (candidate for candidate in SURFACES if _normalize(candidate) == _normalize(cells[0])),
            None,
        )
        if surface is not None and surface not in rows:
            rows[surface] = (cells[1], cells[2], cells[3])
    return rows, header_valid


def audit_text(document: str) -> list[Violation]:
    sections, duplicate_headings = _sections(document)
    violations: list[Violation] = []

    for heading in sorted(duplicate_headings):
        violations.append(Violation("document", "all", f"duplicate level-two heading: {heading}"))

    acceptance = sections.get(_normalize("Integration acceptance"), "")
    acceptance_shape = ("owner", "input rows", "output rows", "forbidden actions")
    for missing in _missing_fragments(acceptance, acceptance_shape):
        violations.append(
            Violation("contract-shape", "all", f"integration acceptance omits {missing!r}")
        )

    has_validation_gate = not _missing_fragments(acceptance, ("validation gate",))
    has_evidence_path = not _missing_fragments(acceptance, ("raw-log/evidence path",))
    for surface in SURFACES:
        if not has_validation_gate:
            violations.append(
                Violation("validation-gate", surface, "universal acceptance gate is absent")
            )
        if not has_evidence_path:
            violations.append(
                Violation("evidence-path", surface, "universal raw-log/evidence path is absent")
            )

    ownership_section = sections.get(_normalize("Ownership Matrix"), "")
    ownership_rows, ownership_header_valid = _ownership_rows(ownership_section)
    if not ownership_header_valid:
        violations.append(
            Violation("document", "all", "ownership matrix header is missing or malformed")
        )

    for surface in SURFACES:
        boundary_title, boundary_facts = BOUNDARIES[surface]
        boundary = sections.get(_normalize(boundary_title), "")
        missing_boundary = _missing_fragments(boundary, boundary_facts)
        if missing_boundary:
            violations.append(
                Violation(
                    "boundary",
                    surface,
                    "missing " + ", ".join(repr(item) for item in missing_boundary),
                )
            )

        row = ownership_rows.get(surface)
        if row is None:
            violations.append(
                Violation("integration", surface, "ownership matrix row is absent")
            )
        else:
            owner, responsibility, forbidden = row
            if _missing_fragments(owner, (OWNERS[surface],)):
                violations.append(
                    Violation("owner", surface, f"expected owner {OWNERS[surface]!r}")
                )
            if not responsibility.strip():
                violations.append(
                    Violation("boundary", surface, "CodeDB responsibility is empty")
                )
            missing_forbidden = _missing_fragments(
                forbidden, FORBIDDEN_CROSSINGS[surface]
            )
            if missing_forbidden:
                violations.append(
                    Violation(
                        "forbidden-crossing",
                        surface,
                        "missing " + ", ".join(repr(item) for item in missing_forbidden),
                    )
                )

        missing_exports = _missing_fragments(boundary, EXPORT_FACTS[surface])
        if missing_exports:
            violations.append(
                Violation(
                    "export-facts",
                    surface,
                    "missing " + ", ".join(repr(item) for item in missing_exports),
                )
            )

    return sorted(violations, key=lambda item: (item.category, item.surface, item.detail))


def audit_document(path: Path) -> list[Violation]:
    try:
        document = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        return [Violation("document", "all", f"cannot read {path}: {error}")]
    return audit_text(document)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--document",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "docs/INTEGRATION_CONTRACTS.md",
    )
    args = parser.parse_args()

    violations = audit_document(args.document)
    if violations:
        print("integration contract validation: FAILED")
        for violation in violations:
            print(violation)
        return 1
    print("integration contract validation: PASSED")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

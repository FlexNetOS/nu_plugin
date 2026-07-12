#!/usr/bin/env python3

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from validate_integration_contracts import audit_document  # noqa: E402


VALID_DOCUMENT = """\
# CodeDB Integration Contracts

## Integration acceptance

Every governed integration is valid only when it has: owner, input rows,
output rows, validation gate, forbidden actions, and raw-log/evidence path.

## GitKB Boundary

GitKB stores durable explanations, decisions, and handoffs. It is not the
source of Rust/crate truth and must not store raw source blobs.

| Row/export | Purpose |
|---|---|
| `codedb_doctrine_summary` | Authority boundary summary |
| `capture_gaps` | Incomplete facts |
| `validation_errors` | Invalid facts |
| `meta_repo_selection` | Repo and scan-root facts |
| `runner_proof_manifest` | Runner proof and raw log paths |
| command-ledger references | Raw command/log evidence and proof artifacts |

## RTK Boundary

RTK summarizes outputs while raw failure logs remain the source of truth.

| Row/export | Purpose |
|---|---|
| `raw_log_paths` | Uncompressed failure evidence |
| `runner_proof_manifest.raw_log_path` | Release-gate evidence path |
| `capture_gaps` summary | Bounded summary |
| `validation_errors` summary | Bounded summary |

## Kache, wild, and Fenix Boundary

Kache, wild, and Fenix are environment/toolchain facts, not CodeDB-owned
installs and not substitutes for Cargo/rustc evidence.

| Tooling surface | Fact rows |
|---|---|
| Kache | `kache_status`, wrapper path, cache root, enabled/disabled state, provenance source |
| wild | linker path, opt-in feature state, target triple, link args source |
| Fenix | toolchain channel, component list, target list, rustc/cargo/rustfmt/clippy/rust-analyzer paths and versions |

## Ownership Matrix

| Surface | Owner | CodeDB responsibility | Forbidden crossing |
|---|---|---|---|
| GitKB | GitKB project workflow | Store handoffs and proof links | Store raw source blobs or replace runner proof |
| RTK | RTK summarization/compression workflow | Preserve raw log paths | Replace failure evidence or erase stderr/root cause |
| Kache | Host/toolchain owner | Capture cache facts | Install or tune cache state |
| wild | Host/toolchain owner | Capture linker facts | Treat linker optimization as crate truth |
| Fenix | Host/toolchain owner | Capture toolchain facts | Install or mutate toolchains |
"""


class IntegrationContractValidatorTest(unittest.TestCase):
    def audit(self, document: str):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "INTEGRATION_CONTRACTS.md"
            path.write_text(document, encoding="utf-8")
            return audit_document(path)

    def assert_category_fails(self, document: str, category: str) -> None:
        violations = self.audit(document)
        self.assertTrue(
            any(violation.category == category for violation in violations),
            "expected category " + category + ":\n" + "\n".join(map(str, violations)),
        )

    def test_complete_contract_passes(self) -> None:
        self.assertEqual([], self.audit(VALID_DOCUMENT))

    def test_missing_document_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            violations = audit_document(Path(temp) / "missing.md")
        self.assertTrue(any(item.category == "document" for item in violations))

    def test_each_governed_integration_is_required(self) -> None:
        ownership_rows = {
            "GitKB": "| GitKB | GitKB project workflow | Store handoffs and proof links | Store raw source blobs or replace runner proof |\n",
            "RTK": "| RTK | RTK summarization/compression workflow | Preserve raw log paths | Replace failure evidence or erase stderr/root cause |\n",
            "Kache": "| Kache | Host/toolchain owner | Capture cache facts | Install or tune cache state |\n",
            "wild": "| wild | Host/toolchain owner | Capture linker facts | Treat linker optimization as crate truth |\n",
            "Fenix": "| Fenix | Host/toolchain owner | Capture toolchain facts | Install or mutate toolchains |\n",
        }
        for surface, row in ownership_rows.items():
            with self.subTest(surface=surface):
                violations = self.audit(VALID_DOCUMENT.replace(row, ""))
                self.assertTrue(
                    any(
                        item.category == "integration" and item.surface == surface
                        for item in violations
                    ),
                    "\n".join(map(str, violations)),
                )

    def test_missing_boundary_fails(self) -> None:
        self.assert_category_fails(
            VALID_DOCUMENT.replace("durable explanations", "temporary notes"),
            "boundary",
        )

    def test_missing_owner_fails(self) -> None:
        self.assert_category_fails(
            VALID_DOCUMENT.replace("GitKB project workflow", ""),
            "owner",
        )

    def test_missing_forbidden_crossing_fails(self) -> None:
        self.assert_category_fails(
            VALID_DOCUMENT.replace("Install or tune cache state", ""),
            "forbidden-crossing",
        )

    def test_missing_export_fact_fails(self) -> None:
        self.assert_category_fails(
            VALID_DOCUMENT.replace("`kache_status`", "cache observation"),
            "export-facts",
        )

    def test_missing_validation_gate_fails_for_every_integration(self) -> None:
        violations = self.audit(VALID_DOCUMENT.replace("validation gate", "review step"))
        failed_surfaces = {
            item.surface for item in violations if item.category == "validation-gate"
        }
        self.assertEqual({"GitKB", "RTK", "Kache", "wild", "Fenix"}, failed_surfaces)

    def test_missing_evidence_path_fails_for_every_integration(self) -> None:
        violations = self.audit(
            VALID_DOCUMENT.replace("raw-log/evidence path", "review output")
        )
        failed_surfaces = {
            item.surface for item in violations if item.category == "evidence-path"
        }
        self.assertEqual({"GitKB", "RTK", "Kache", "wild", "Fenix"}, failed_surfaces)


if __name__ == "__main__":
    unittest.main()

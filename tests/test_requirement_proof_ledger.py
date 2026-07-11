#!/usr/bin/env python3

from __future__ import annotations

import csv
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from validate_requirement_proof_ledger import (  # noqa: E402
    EXPECTED_REQUIREMENT_IDS,
    audit_ledger,
    read_ledger,
    validate_rows,
)


REQUIRED_COLUMNS = [
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


def complete_row(requirement_id: str, head: str = "") -> dict[str, str]:
    return {
        "requirement_id": requirement_id,
        "parent_id": requirement_id.split("-", 1)[0],
        "requirement": "A directly testable requirement",
        "authoritative_source": "execution/source.md",
        "source_ref": "section 1",
        "implementation_paths": "src/implementation.rs",
        "test_paths": "tests/proof_test.py",
        "verification_command": "python3 tests/proof_test.py",
        "proof_artifacts": "cargo-metadata-output",
        "proof_head_sha": head,
        "evidence_status": "verified",
        "task_status": "complete",
        "notes": "",
    }


def receipt_row(requirement_id: str) -> dict:
    digest = "1" * 64
    return {
        "requirement_id": requirement_id,
        "status": "verified",
        "verification_command": "python3 tests/proof_test.py",
        "exit_code": 0,
        "stdout_sha256": digest,
        "stderr_sha256": digest,
        "evidence": [
            {
                "logical_name": "cargo-metadata-output",
                "sha256": digest,
                "kind": "command-output",
            }
        ],
    }


class RequirementProofLedgerUnitTest(unittest.TestCase):
    def test_verified_row_requires_direct_external_current_head_attestation(self) -> None:
        head = "a" * 40
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for path in [
                "execution/source.md",
                "src/implementation.rs",
                "tests/proof_test.py",
            ]:
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("proof\n", encoding="utf-8")

            violations = validate_rows(
                root,
                [complete_row("CDB013")],
                expected_ids={"CDB013"},
                current_head=head,
                require_all_verified=True,
                receipt_rows={"CDB013": receipt_row("CDB013")},
            )
            self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))

    def test_missing_expected_requirement_fails_closed(self) -> None:
        violations = validate_rows(
            Path("."),
            [],
            expected_ids={"CDB013"},
            current_head="a" * 40,
            require_all_verified=False,
        )
        self.assertTrue(any(v.rule == "missing requirement row" for v in violations))

    def test_self_referential_legacy_head_and_documentation_only_proof_are_rejected(
        self,
    ) -> None:
        head = "a" * 40
        row = complete_row("CDB013", "b" * 40)
        row["implementation_paths"] = "docs/implementation.md"
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head=head,
            require_all_verified=True,
        )
        rules = {v.rule for v in violations}
        self.assertIn("self-referential legacy proof revision", rules)
        self.assertIn("documentation-only implementation proof", rules)
        self.assertIn("missing external current-head attestation", rules)

    def test_missing_files_and_non_executable_command_are_rejected(self) -> None:
        head = "a" * 40
        row = complete_row("CDB013", head)
        row["verification_command"] = "see docs/RELEASE_GATE.md"
        row["proof_artifacts"] = ""
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head=head,
            require_all_verified=True,
        )
        rules = {v.rule for v in violations}
        self.assertIn("missing implementation path", rules)
        self.assertIn("missing test path", rules)
        self.assertIn("missing logical proof artifact", rules)
        self.assertIn("non-executable verification command", rules)

    def test_missing_local_authoritative_source_is_rejected(self) -> None:
        row = complete_row("CDB013", "a" * 40)
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head="a" * 40,
            require_all_verified=False,
        )
        self.assertTrue(any(v.rule == "missing authoritative source" for v in violations))

    def test_unverified_or_gap_closed_row_blocks_release(self) -> None:
        head = "a" * 40
        row = complete_row("CDB077", head)
        row["evidence_status"] = "gap"
        row["task_status"] = "active"
        row["notes"] = "GAP accepted as closure"
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB077"},
            current_head=head,
            require_all_verified=True,
        )
        rules = {v.rule for v in violations}
        self.assertIn("release-blocking evidence status", rules)
        self.assertIn("GAP used as completion evidence", rules)

    def test_complete_task_without_verified_evidence_is_contradictory(self) -> None:
        row = complete_row("CDB013", "")
        row["evidence_status"] = "missing"
        row["task_status"] = "complete"
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head="a" * 40,
            require_all_verified=False,
        )
        self.assertTrue(any(v.rule == "task complete without verified proof" for v in violations))

    def test_receipt_evidence_is_bound_to_the_specific_requirement_row(self) -> None:
        head = "a" * 40
        row = complete_row("CDB013")
        wrong = receipt_row("CDB013")
        wrong["evidence"][0]["logical_name"] = "different-proof"
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head=head,
            require_all_verified=True,
            receipt_rows={"CDB013": wrong},
        )
        self.assertTrue(
            any(v.rule == "receipt missing logical proof artifact" for v in violations)
        )


class RepositoryRequirementProofLedgerTest(unittest.TestCase):
    def test_repository_ledger_is_exhaustive_and_structurally_valid(self) -> None:
        rows = read_ledger(ROOT / "execution/REQUIREMENT_PROOF_LEDGER.csv")
        self.assertEqual(EXPECTED_REQUIREMENT_IDS, {row["requirement_id"] for row in rows})
        violations = audit_ledger(ROOT, require_all_verified=False)
        self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))

    def test_release_mode_fails_closed_while_any_requirement_lacks_proof(self) -> None:
        violations = audit_ledger(ROOT, require_all_verified=True)
        self.assertTrue(
            any(v.rule == "release-blocking evidence status" for v in violations),
            "the current ledger must block release until every row has current-head proof",
        )

    def test_csv_header_is_stable(self) -> None:
        with (ROOT / "execution/REQUIREMENT_PROOF_LEDGER.csv").open(
            newline="", encoding="utf-8"
        ) as handle:
            self.assertEqual(REQUIRED_COLUMNS, csv.DictReader(handle).fieldnames)


if __name__ == "__main__":
    unittest.main()

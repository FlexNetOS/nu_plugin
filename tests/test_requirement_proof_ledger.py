#!/usr/bin/env python3

from __future__ import annotations

import csv
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import validate_requirement_proof_ledger as ledger_validator  # noqa: E402
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



def _local_release_row() -> dict[str, str]:
    row = complete_row("CDB013")
    row["authoritative_source"] = "execution/TASK_GRAPH.csv"
    row["implementation_paths"] = "scripts/validate_requirement_proof_ledger.py"
    row["test_paths"] = "tests/test_requirement_proof_ledger.py"
    return row

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

    def test_direct_evidence_validates_complete_row_without_recursive_receipt(
        self,
    ) -> None:
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
                require_receipts=False,
                graph_statuses={"CDB013": "complete"},
            )
            self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))

    def test_direct_evidence_rejects_incomplete_task_and_graph_status(self) -> None:
        row = complete_row("CDB013")
        row["task_status"] = "active"
        violations = validate_rows(
            Path("."),
            [row],
            expected_ids={"CDB013"},
            current_head="a" * 40,
            require_all_verified=True,
            require_receipts=False,
            graph_statuses={"CDB013": "active"},
        )
        self.assertTrue(
            any(v.rule == "release-blocking task status" for v in violations),
            "\n" + "\n".join(map(str, violations)),
        )

    def test_verified_external_sibling_paths_must_exist(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            envctl = workspace / "envctl"
            for path in [
                root / "execution/source.md",
                envctl / "README.md",
                envctl / "tests/db_docs_contract.rs",
            ]:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("proof\n", encoding="utf-8")

            row = complete_row("CDB013")
            row["implementation_paths"] = "external:../envctl/README.md"
            row["test_paths"] = "external:../envctl/tests/db_docs_contract.rs"
            violations = validate_rows(
                root,
                [row],
                expected_ids={"CDB013"},
                current_head="a" * 40,
                require_all_verified=True,
                require_receipts=False,
                graph_statuses={"CDB013": "complete"},
            )
            self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))

            row["test_paths"] = "external:../envctl/tests/missing.rs"
            violations = validate_rows(
                root,
                [row],
                expected_ids={"CDB013"},
                current_head="a" * 40,
                require_all_verified=True,
                require_receipts=False,
                graph_statuses={"CDB013": "complete"},
            )
            self.assertTrue(
                any(v.rule == "missing test path" for v in violations),
                "\n" + "\n".join(map(str, violations)),
            )

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

    def test_release_mode_requires_attestation_after_direct_evidence_completes(self) -> None:
        violations = audit_ledger(ROOT, require_all_verified=True)
        self.assertTrue(
            any(v.rule == "missing external proof receipt" for v in violations),
            "complete direct evidence must not self-authorize a release",
        )
        local_violations = audit_ledger(
            ROOT, require_all_verified=True, local_release=True
        )
        self.assertTrue(
            any(v.rule == "missing external proof receipt" for v in local_violations),
            "local-release still requires a genuine detached local receipt",
        )

        # Always enforced, independent of the live tree: a deliberately-
        # incomplete row blocks release in BOTH release and local-release mode.
        # local-release only swaps the GitHub signature for a local receipt; it
        # never relaxes the verified/complete floor.
        incomplete = complete_row("CDB013")
        incomplete["evidence_status"] = "partial"
        incomplete["task_status"] = "planned"
        floor = validate_rows(
            Path("."),
            [incomplete],
            expected_ids={"CDB013"},
            current_head="a" * 40,
            require_all_verified=True,
            require_receipts=False,
            graph_statuses={"CDB013": "planned"},
        )
        self.assertTrue(
            any(v.rule == "release-blocking evidence status" for v in floor),
            "\n" + "\n".join(map(str, floor)),
        )

    def test_full_mode_always_requires_detached_cryptographic_verification(
        self,
    ) -> None:
        row = complete_row("CDB013")
        row["authoritative_source"] = "execution/TASK_GRAPH.csv"
        row["implementation_paths"] = "scripts/validate_requirement_proof_ledger.py"
        row["test_paths"] = "tests/test_requirement_proof_ledger.py"
        receipt = Path("/outside/requirement-proof.json")
        bundle = Path("/outside/requirement-proof.bundle.jsonl")
        crypto = Mock(return_value=[])

        with (
            patch.object(ledger_validator, "EXPECTED_REQUIREMENT_IDS", {"CDB013"}),
            patch.object(ledger_validator, "read_ledger", return_value=[row]),
            patch.object(ledger_validator, "_current_head", return_value="a" * 40),
            patch.object(ledger_validator, "_current_tree", return_value="b" * 40),
            patch.object(
                ledger_validator,
                "_current_repository",
                return_value="FlexNetOS/nu_plugin",
            ),
            patch.object(ledger_validator, "_worktree_clean", return_value=True),
            patch.object(ledger_validator, "_sha256_file", return_value="1" * 64),
            patch.object(ledger_validator, "_graph_statuses", return_value={}),
            patch.object(ledger_validator, "_external_path", side_effect=lambda _, p, **__: p),
            patch.object(ledger_validator, "load_receipt", return_value={}),
            patch.object(
                ledger_validator,
                "validate_receipt",
                return_value=({"CDB013": receipt_row("CDB013")}, []),
            ),
            patch.object(
                ledger_validator,
                "verify_github_attestation",
                crypto,
            ),
        ):
            missing_bundle = audit_ledger(
                ROOT,
                require_all_verified=True,
                receipt_path=receipt,
                signer_workflow="FlexNetOS/nu_plugin/.github/workflows/ci.yml",
            )
            self.assertTrue(
                any(v.rule == "missing detached attestation bundle" for v in missing_bundle)
            )
            crypto.assert_not_called()

            verified = audit_ledger(
                ROOT,
                require_all_verified=True,
                receipt_path=receipt,
                attestation_bundle_path=bundle,
                signer_workflow="FlexNetOS/nu_plugin/.github/workflows/ci.yml",
            )
            self.assertEqual([], verified, "\n" + "\n".join(map(str, verified)))
            crypto.assert_called_once()

    def test_cli_supports_explicit_local_release_but_still_requires_local_attestation(
        self,
    ) -> None:
        script = ROOT / "scripts/validate_requirement_proof_ledger.py"
        direct = subprocess.run(
            [sys.executable, str(script), "--root", str(ROOT), "--direct-evidence"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotIn("unrecognized arguments", direct.stderr)
        self.assertNotIn("missing external proof receipt", direct.stdout)
        self.assertEqual(0, direct.returncode, direct.stdout + direct.stderr)
        self.assertIn("mode=direct-evidence", direct.stdout)

        # The dishonest "bypass" spelling stays unrecognized: there is no flag
        # that lets an already-generated in-tree receipt self-authorize.
        bypass = subprocess.run(
            [sys.executable, str(script), "--allow-local-receipt"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(0, bypass.returncode)
        self.assertIn("unrecognized arguments: --allow-local-receipt", bypass.stderr)

        # The honest, owner-authorized local release is spelled --local-release.
        # It is recognized, but it NEVER grants zero-provenance completion: a
        # genuine external receipt remains mandatory, so with no --receipt it
        # still fails closed with "missing external proof receipt".
        local = subprocess.run(
            [sys.executable, str(script), "--root", str(ROOT), "--local-release"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotIn("unrecognized arguments", local.stderr)
        self.assertNotEqual(0, local.returncode)
        self.assertIn("missing external proof receipt", local.stdout)

        # A local release cannot borrow the detached GitHub signature.
        combined = subprocess.run(
            [
                sys.executable,
                str(script),
                "--root",
                str(ROOT),
                "--local-release",
                "--attestation-bundle",
                "/outside/x.bundle.jsonl",
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(0, combined.returncode)
        self.assertIn("mutually exclusive", combined.stderr)

    def test_local_release_passes_with_local_receipt_and_never_calls_github(
        self,
    ) -> None:
        receipt = Path("/outside/requirement-proof.json")
        crypto = Mock(return_value=[])
        with (
            patch.object(ledger_validator, "EXPECTED_REQUIREMENT_IDS", {"CDB013"}),
            patch.object(
                ledger_validator, "read_ledger", return_value=[_local_release_row()]
            ),
            patch.object(ledger_validator, "_current_head", return_value="a" * 40),
            patch.object(ledger_validator, "_current_tree", return_value="b" * 40),
            patch.object(
                ledger_validator, "_current_repository", return_value="FlexNetOS/nu_plugin"
            ),
            patch.object(ledger_validator, "_worktree_clean", return_value=True),
            patch.object(ledger_validator, "_sha256_file", return_value="1" * 64),
            patch.object(ledger_validator, "_graph_statuses", return_value={}),
            patch.object(ledger_validator, "_external_path", side_effect=lambda _, p, **__: p),
            patch.object(
                ledger_validator,
                "load_receipt",
                return_value={"generator": {"provider": "local"}},
            ),
            patch.object(
                ledger_validator,
                "validate_receipt",
                return_value=({"CDB013": receipt_row("CDB013")}, []),
            ),
            patch.object(ledger_validator, "verify_github_attestation", crypto),
        ):
            violations = audit_ledger(
                ROOT,
                require_all_verified=True,
                receipt_path=receipt,
                local_release=True,
            )
            self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))
            crypto.assert_not_called()

    def test_local_release_rejects_github_provider_receipt(self) -> None:
        receipt = Path("/outside/requirement-proof.json")
        crypto = Mock(return_value=[])
        with (
            patch.object(ledger_validator, "EXPECTED_REQUIREMENT_IDS", {"CDB013"}),
            patch.object(
                ledger_validator, "read_ledger", return_value=[_local_release_row()]
            ),
            patch.object(ledger_validator, "_current_head", return_value="a" * 40),
            patch.object(ledger_validator, "_current_tree", return_value="b" * 40),
            patch.object(
                ledger_validator, "_current_repository", return_value="FlexNetOS/nu_plugin"
            ),
            patch.object(ledger_validator, "_worktree_clean", return_value=True),
            patch.object(ledger_validator, "_sha256_file", return_value="1" * 64),
            patch.object(ledger_validator, "_graph_statuses", return_value={}),
            patch.object(ledger_validator, "_external_path", side_effect=lambda _, p, **__: p),
            patch.object(
                ledger_validator,
                "load_receipt",
                return_value={"generator": {"provider": "github-actions"}},
            ),
            patch.object(
                ledger_validator,
                "validate_receipt",
                return_value=({"CDB013": receipt_row("CDB013")}, []),
            ),
            patch.object(ledger_validator, "verify_github_attestation", crypto),
        ):
            violations = audit_ledger(
                ROOT,
                require_all_verified=True,
                receipt_path=receipt,
                local_release=True,
            )
            self.assertTrue(
                any(
                    v.rule == "local-release requires local provider receipt"
                    for v in violations
                ),
                "\n" + "\n".join(map(str, violations)),
            )
            crypto.assert_not_called()

    def test_local_receipt_cannot_satisfy_default_github_release(self) -> None:
        receipt = Path("/outside/requirement-proof.json")
        crypto = Mock(return_value=[])
        with (
            patch.object(ledger_validator, "EXPECTED_REQUIREMENT_IDS", {"CDB013"}),
            patch.object(
                ledger_validator, "read_ledger", return_value=[_local_release_row()]
            ),
            patch.object(ledger_validator, "_current_head", return_value="a" * 40),
            patch.object(ledger_validator, "_current_tree", return_value="b" * 40),
            patch.object(
                ledger_validator, "_current_repository", return_value="FlexNetOS/nu_plugin"
            ),
            patch.object(ledger_validator, "_worktree_clean", return_value=True),
            patch.object(ledger_validator, "_sha256_file", return_value="1" * 64),
            patch.object(ledger_validator, "_graph_statuses", return_value={}),
            patch.object(ledger_validator, "_external_path", side_effect=lambda _, p, **__: p),
            patch.object(
                ledger_validator,
                "load_receipt",
                return_value={"generator": {"provider": "local"}},
            ),
            patch.object(
                ledger_validator,
                "validate_receipt",
                return_value=({"CDB013": receipt_row("CDB013")}, []),
            ),
            patch.object(ledger_validator, "verify_github_attestation", crypto),
        ):
            # Default (public/GitHub) release lane, local_release omitted: even a
            # perfectly valid local receipt cannot substitute for the detached
            # GitHub attestation bundle.
            violations = audit_ledger(
                ROOT,
                require_all_verified=True,
                receipt_path=receipt,
            )
            self.assertTrue(
                any(v.rule == "missing detached attestation bundle" for v in violations),
                "\n" + "\n".join(map(str, violations)),
            )
            crypto.assert_not_called()

    def test_every_full_validator_row_uses_nonrecursive_evidence_mode(self) -> None:
        rows = {
            row["requirement_id"]: row
            for row in read_ledger(ROOT / "execution/REQUIREMENT_PROOF_LEDGER.csv")
        }
        expected = {
            "CDB090": "python3 scripts/validate_bidirectional_package.py --direct-evidence",
            "CDB106-AC10": "python3 scripts/validate_requirement_proof_ledger.py --direct-evidence",
        }
        for requirement_id, command in expected.items():
            with self.subTest(requirement_id=requirement_id):
                self.assertEqual(command, rows[requirement_id]["verification_command"])

        self.assertEqual(
            "python3 -m unittest tests.test_integration_contracts && "
            "python3 scripts/validate_integration_contracts.py",
            rows["CDB040"]["verification_command"],
        )

        self.assertFalse(
            any(
                row["verification_command"]
                == "python3 scripts/validate_requirement_proof_ledger.py"
                for row in rows.values()
            ),
            "an all-row receipt cannot execute a full release validator recursively",
        )

        self.assertFalse(
            any("--local-release" in row["verification_command"] for row in rows.values()),
            "no ledger row may invoke the local-release mode as its own gate",
        )

        self.assertEqual(
            "python3 -m unittest tests.test_truth_surface && "
            "python3 scripts/truth_surface.py --check && "
            "python3 scripts/truth_surface.py --check-source",
            rows["CDB047"]["verification_command"],
        )
        self.assertEqual(
            "cargo test --manifest-path ../envctl/Cargo.toml -p envctl "
            "--test db_docs_contract",
            rows["REQ-061-ARCH18"]["verification_command"],
        )

        completed = {
            "CDB013",
            "CDB040",
            "CDB046",
            "CDB047",
            "CDB050",
            *(f"CDB{index:03d}" for index in range(77, 90)),
            "CDB090",
            "CDB106-AC10",
            "REQ-061-ARCH18",
        }
        for requirement_id in completed:
            with self.subTest(requirement_id=requirement_id):
                self.assertEqual("verified", rows[requirement_id]["evidence_status"])
                self.assertEqual("complete", rows[requirement_id]["task_status"])

    def test_csv_header_is_stable(self) -> None:
        with (ROOT / "execution/REQUIREMENT_PROOF_LEDGER.csv").open(
            newline="", encoding="utf-8"
        ) as handle:
            self.assertEqual(REQUIRED_COLUMNS, csv.DictReader(handle).fieldnames)


if __name__ == "__main__":
    unittest.main()

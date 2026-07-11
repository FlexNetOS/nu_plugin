#!/usr/bin/env python3

from __future__ import annotations

import copy
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from requirement_proof_attestation import (  # noqa: E402
    CheckoutIdentity,
    canonical_receipt_payload,
    sha256_bytes,
    validate_receipt,
)
from generate_requirement_proof_receipt import ensure_external_output  # noqa: E402


DIGEST = "1" * 64
COMMIT = "a" * 40
TREE = "b" * 40


def ledger_row(requirement_id: str = "CDB013") -> dict[str, str]:
    return {
        "requirement_id": requirement_id,
        "verification_command": "cargo metadata --format-version 1 --no-deps",
    }


def valid_receipt() -> dict:
    receipt = {
        "schema_version": 2,
        "attestation_type": "requirement-proof",
        "repository": "FlexNetOS/nu_plugin",
        "commit_sha": COMMIT,
        "tree_sha": TREE,
        "ledger": {
            "path": "execution/REQUIREMENT_PROOF_LEDGER.csv",
            "sha256": DIGEST,
        },
        "validator": {
            "path": "scripts/validate_requirement_proof_ledger.py",
            "sha256": DIGEST,
        },
        "generator": {"provider": "github-actions", "run_id": "1234"},
        "worktree": {"clean_before": True, "clean_after": True},
        "rows": [
            {
                "requirement_id": "CDB013",
                "status": "verified",
                "verification_command": "cargo metadata --format-version 1 --no-deps",
                "exit_code": 0,
                "stdout_sha256": DIGEST,
                "stderr_sha256": DIGEST,
                "evidence": [
                    {
                        "logical_name": "cargo-metadata-output",
                        "sha256": DIGEST,
                        "kind": "command-output",
                    }
                ],
            }
        ],
        "signature": {
            "kind": "github-artifact-attestation",
            "reference": "https://github.example/attestations/1234",
        },
    }
    receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
    return receipt


def identity(*, clean: bool = True) -> CheckoutIdentity:
    return CheckoutIdentity(
        commit_sha=COMMIT,
        tree_sha=TREE,
        ledger_sha256=DIGEST,
        validator_sha256=DIGEST,
        clean=clean,
    )


class RequirementProofAttestationTest(unittest.TestCase):
    def test_receipt_output_must_remain_outside_attested_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            with self.assertRaises(ValueError):
                ensure_external_output(root, root / "receipt.json")
            outside = Path(temp) / "receipt.json"
            self.assertEqual(outside.resolve(), ensure_external_output(root, outside))

    def test_valid_external_current_head_receipt_passes(self) -> None:
        rows, violations = validate_receipt(
            valid_receipt(),
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))
        self.assertEqual({"CDB013"}, set(rows))

    def test_parent_commit_tree_ledger_and_validator_drift_are_rejected(self) -> None:
        receipt = valid_receipt()
        receipt["commit_sha"] = "c" * 40
        receipt["tree_sha"] = "d" * 40
        receipt["ledger"]["sha256"] = "2" * 64
        receipt["validator"]["sha256"] = "3" * 64
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("commit_sha mismatch", rules)
        self.assertIn("tree_sha mismatch", rules)
        self.assertIn("ledger.sha256 mismatch", rules)
        self.assertIn("validator.sha256 mismatch", rules)

    def test_arbitrary_current_sha_text_cannot_replace_structured_receipt(self) -> None:
        receipt = {"note": f"proof for {COMMIT}"}
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("schema_version mismatch", rules)
        self.assertIn("invalid receipt digest", rules)
        self.assertIn("invalid receipt rows", rules)

    def test_dirty_checkout_or_dirty_proof_execution_is_rejected(self) -> None:
        receipt = valid_receipt()
        receipt["worktree"]["clean_after"] = False
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(clean=False),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("dirty checkout", rules)
        self.assertIn("dirty proof execution", rules)

    def test_row_command_exit_and_evidence_are_requirement_bound(self) -> None:
        receipt = valid_receipt()
        row = receipt["rows"][0]
        row["verification_command"] = "true"
        row["exit_code"] = 1
        row["evidence"] = []
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("CDB013.verification_command mismatch", rules)
        self.assertIn("failed receipt command", rules)
        self.assertIn("missing row evidence", rules)

    def test_tampered_receipt_digest_is_rejected(self) -> None:
        receipt = valid_receipt()
        tampered = copy.deepcopy(receipt)
        tampered["rows"][0]["stdout_sha256"] = "9" * 64
        _, violations = validate_receipt(
            tampered,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        self.assertTrue(
            any(violation.rule == "receipt digest mismatch" for violation in violations)
        )

    def test_untrusted_local_receipt_requires_explicit_development_mode(self) -> None:
        receipt = valid_receipt()
        receipt["generator"] = {"provider": "local", "run_id": ""}
        receipt.pop("signature")
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, trusted_violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=True,
        )
        _, local_violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
            require_trusted_ci=False,
        )
        self.assertTrue(trusted_violations)
        self.assertEqual([], local_violations)


if __name__ == "__main__":
    unittest.main()

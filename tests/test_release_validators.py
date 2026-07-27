#!/usr/bin/env python3

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import validate_bidirectional_package as bidirectional_validator  # noqa: E402
from validate_bidirectional_package import audit_package  # noqa: E402
from validate_mandatory_capabilities import audit_release  # noqa: E402


class ReleaseValidatorIntegrationTest(unittest.TestCase):
    def test_mandatory_release_audit_requires_detached_receipt(self) -> None:
        violations = audit_release(ROOT, require_all_verified=True)
        self.assertTrue(
            any("missing external proof receipt" in str(item) for item in violations)
        )

    def test_bidirectional_release_cannot_pass_on_graph_status_alone(self) -> None:
        violations = audit_package(ROOT)
        self.assertTrue(
            any("requirement proof ledger" in item for item in violations),
            "\n" + "\n".join(violations),
        )

    def test_bidirectional_direct_evidence_mode_skips_only_receipt_recursion(
        self,
    ) -> None:
        violations = audit_package(ROOT, direct_evidence=True)
        self.assertEqual([], violations, "\n" + "\n".join(violations))

    def test_bidirectional_runner_environment_explicitly_selects_local_release(
        self,
    ) -> None:
        clean_environment = {
            "CODEDB_REQUIREMENT_PROOF_LOCAL_RELEASE": "1",
            "CODEDB_REQUIREMENT_PROOF_BUNDLE": "",
            "CODEDB_REQUIREMENT_PROOF_SIGNER_WORKFLOW": "",
        }
        with (
            patch.dict(os.environ, clean_environment, clear=False),
            patch.object(sys, "argv", ["validate_bidirectional_package.py"]),
            patch.object(
                bidirectional_validator,
                "audit_package",
                return_value=[],
            ) as audit,
        ):
            self.assertEqual(0, bidirectional_validator.main())
        audit.assert_called_once_with(
            ROOT,
            direct_evidence=False,
            local_release=True,
        )

    def test_bidirectional_local_release_rejects_github_attestation_environment(
        self,
    ) -> None:
        with (
            patch.dict(
                os.environ,
                {
                    "CODEDB_REQUIREMENT_PROOF_LOCAL_RELEASE": "1",
                    "CODEDB_REQUIREMENT_PROOF_BUNDLE": "/outside/bundle.jsonl",
                },
                clear=False,
            ),
            patch.object(sys, "argv", ["validate_bidirectional_package.py"]),
            self.assertRaises(SystemExit),
        ):
            bidirectional_validator.main()


if __name__ == "__main__":
    unittest.main()

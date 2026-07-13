#!/usr/bin/env python3

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

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


if __name__ == "__main__":
    unittest.main()

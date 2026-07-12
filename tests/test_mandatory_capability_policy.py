#!/usr/bin/env python3

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from validate_mandatory_capabilities import audit_repository  # noqa: E402


class MandatoryCapabilityPolicyTest(unittest.TestCase):
    def test_repository_has_no_optional_or_gap_closed_capabilities(self) -> None:
        violations = audit_repository(ROOT)
        self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))


if __name__ == "__main__":
    unittest.main()

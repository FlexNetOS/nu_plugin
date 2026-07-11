from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class MandatoryPostgresParityTest(unittest.TestCase):
    def test_pg_parity_is_a_required_service_lane_not_an_ignored_test(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        parity = (ROOT / "crates/codedb_store_pg/tests/blobstore_parity.rs").read_text()
        manifest = (ROOT / "crates/codedb_store_pg/Cargo.toml").read_text()

        self.assertIn("postgres_parity:", workflow)
        self.assertIn("services:", workflow)
        self.assertIn("postgres:", workflow)
        self.assertIn("CODEDB_PG_CONN", workflow)
        self.assertIn("--features pg-integration", workflow)
        self.assertIn("pg-integration", manifest)

        self.assertNotIn("#[ignore", parity)
        self.assertNotIn("CODEDB_PG_TEST", parity)
        self.assertNotIn("SKIP:", parity)
        self.assertIn("CODEDB_PG_CONN", parity)


if __name__ == "__main__":
    unittest.main()

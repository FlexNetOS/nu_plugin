from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class PostgresOpenPolicyTest(unittest.TestCase):
    def test_read_open_never_runs_schema_ddl_and_bare_pg_needs_explicit_dsn(self):
        pg = (ROOT / "crates/codedb_store_pg/src/lib.rs").read_text()
        cli = (ROOT / "crates/codedb/src/main.rs").read_text()

        self.assertIn("pub fn initialize", pg)
        self.assertIn("pub fn open_existing", pg)
        open_existing = pg.split("pub fn open_existing", 1)[1].split("pub fn", 1)[0]
        self.assertNotIn("CREATE TABLE", open_existing)
        self.assertNotIn("ALTER TABLE", open_existing)
        self.assertNotIn("batch_execute", open_existing)

        self.assertNotIn("/home/flexnetos/", pg)
        self.assertIn("PgStore::initialize", cli)
        self.assertIn("PgStore::open_existing", cli)
        self.assertIn("PostgreSQL DSN is required", cli)
        self.assertNotIn("codedb_store_pg::DEFAULT_CONN", cli)


if __name__ == "__main__":
    unittest.main()

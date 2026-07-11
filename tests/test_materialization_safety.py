from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class MaterializationSafetyTest(unittest.TestCase):
    def test_cli_validates_untrusted_stored_paths_before_backend_write(self):
        source = (ROOT / "crates/codedb/src/main.rs").read_text()
        materialize = source.split("fn materialize_rows", 1)[1].split("fn scan_rows", 1)[0]
        self.assertIn("prepare_materialization_path", materialize)
        self.assertNotIn("out_dir.join(&file.relative_path)", materialize)


if __name__ == "__main__":
    unittest.main()

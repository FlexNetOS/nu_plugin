from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class LockedContextIntegrationTest(unittest.TestCase):
    def test_context_is_the_only_cargo_execution_and_identity_authority(self):
        cargo_source = (ROOT / "crates/codedb_cargo/src/lib.rs").read_text()
        self.assertIn("pub fn capture_cargo_metadata_json", cargo_source)
        self.assertNotIn('Command::new("cargo")', cargo_source)
        self.assertNotIn("pub fn capture_cargo_metadata(", cargo_source)
        self.assertNotIn("pub fn build_context_rows(", cargo_source)

        frontdoors = [
            "crates/codedb",
            "crates/nu_plugin_codedb",
            "crates/codedb_mcp",
        ]
        for crate in frontdoors:
            manifest = (ROOT / crate / "Cargo.toml").read_text()
            source = (ROOT / crate / "src/lib.rs").read_text() if crate.endswith("mcp") else (ROOT / crate / "src/main.rs").read_text()
            self.assertIn("codedb-context.workspace = true", manifest, crate)
            self.assertIn("capture_context", source, crate)
            self.assertIn("capture_cargo_metadata_json", source, crate)
            self.assertNotIn("capture_cargo_metadata(", source, crate)

        nu_source = (ROOT / "crates/nu_plugin_codedb/src/main.rs").read_text()
        rust_cfg = nu_source.split("fn rust_cfg_rows", 1)[1].split("fn build_script_rows", 1)[0]
        self.assertNotIn('"unknown".to_string()', rust_cfg)
        self.assertNotIn("cargo_lock_hash: None", rust_cfg)
        self.assertIn("cargo_lock_sha256", rust_cfg)


if __name__ == "__main__":
    unittest.main()

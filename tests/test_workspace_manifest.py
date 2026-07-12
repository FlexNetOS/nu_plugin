import json
import subprocess
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "Cargo.toml"
EXPECTED_MEMBERS = frozenset(
    {
        "crates/codedb",
        "crates/codedb_build_capture",
        "crates/codedb_cargo",
        "crates/codedb_context",
        "crates/codedb_core",
        "crates/codedb_fixtures",
        "crates/codedb_mcp",
        "crates/codedb_rust_static",
        "crates/codedb_store_pg",
        "crates/codedb_store_redb",
        "crates/nu_plugin_codedb",
    }
)
EXPECTED_PACKAGE = {
    "edition": "2024",
    "license": "MIT",
    "rust-version": "1.93.1",
    "version": "0.1.0",
}


def validate_workspace_manifest(manifest: dict[str, object]) -> frozenset[str]:
    """Validate the governed root workspace contract."""

    workspace = manifest.get("workspace")
    assert isinstance(workspace, dict), "workspace table is missing"

    members = workspace.get("members")
    assert isinstance(members, list), "workspace members must be a list"
    member_set = frozenset(members)
    assert member_set == EXPECTED_MEMBERS, "workspace members do not match"
    assert len(members) == len(EXPECTED_MEMBERS), "workspace members contain duplicates"
    assert workspace.get("resolver") == "3", "workspace resolver does not match"

    package = workspace.get("package")
    assert isinstance(package, dict), "workspace package table is missing"
    for field, expected in EXPECTED_PACKAGE.items():
        assert package.get(field) == expected, f"workspace package {field} does not match"

    return member_set


class WorkspaceManifestTests(unittest.TestCase):
    def test_root_manifest_matches_the_governed_workspace_contract(self):
        manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))

        self.assertEqual(validate_workspace_manifest(manifest), EXPECTED_MEMBERS)

    def test_validator_rejects_a_mutated_workspace_member_set(self):
        manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
        workspace = manifest["workspace"]
        workspace["members"] = workspace["members"][:-1]

        with self.assertRaisesRegex(AssertionError, "workspace members"):
            validate_workspace_manifest(manifest)

    def test_cargo_metadata_parses_and_reports_the_same_workspace_members(self):
        completed = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
        metadata = json.loads(completed.stdout)
        packages = {package["id"]: package for package in metadata["packages"]}
        metadata_members = frozenset(
            Path(packages[package_id]["manifest_path"])
            .parent.resolve()
            .relative_to(ROOT.resolve())
            .as_posix()
            for package_id in metadata["workspace_members"]
        )

        self.assertEqual(metadata_members, EXPECTED_MEMBERS)


if __name__ == "__main__":
    unittest.main()

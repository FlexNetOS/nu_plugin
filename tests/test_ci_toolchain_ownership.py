import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


class CiToolchainOwnershipTests(unittest.TestCase):
    def test_all_external_actions_are_pinned_to_full_commit_shas(self):
        action_ref = re.compile(
            r"^\s*(?:-\s*)?uses:\s*(\S+)\s*$", re.MULTILINE
        )
        workflows = sorted((ROOT / ".github/workflows").glob("*.y*ml"))

        self.assertTrue(workflows)
        for workflow_path in workflows:
            workflow = workflow_path.read_text(encoding="utf-8")
            refs = action_ref.findall(workflow)
            self.assertTrue(refs, workflow_path)
            for ref in refs:
                if ref.startswith("./"):
                    continue
                revision = ref.rpartition("@")[2]
                self.assertRegex(
                    revision,
                    r"\A[0-9a-f]{40}\Z",
                    f"{workflow_path}: external action is not SHA-pinned: {ref}",
                )

    def test_self_hosted_rust_job_uses_the_profile_owned_toolchain(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        rust_job = workflow.split("\n  rust:\n", 1)[1].split("\n  nu:\n", 1)[0]

        self.assertIn("if: runner.environment == 'github-hosted'", rust_job)
        self.assertIn("if: runner.environment == 'self-hosted'", rust_job)
        self.assertIn("/home/flexnetos/.nix-profile/toolbin", rust_job)
        self.assertIn('"$toolbin/rustc" -Vv', rust_job)

    def test_hosted_rust_job_uses_the_flake_locked_nightly_toolchain(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        rust_job = workflow.split("\n  rust:\n", 1)[1].split("\n  nu:\n", 1)[0]

        self.assertIn("nix develop .#ci", rust_job)
        self.assertIn("/nix/store/*/bin", rust_job)
        self.assertIn('"$toolbin/rustc" -Vv', rust_job)
        self.assertIn('"$toolbin/rustdoc" -Vv', rust_job)
        self.assertNotIn("dtolnay/rust-toolchain", rust_job)


if __name__ == "__main__":
    unittest.main()

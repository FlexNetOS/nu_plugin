import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "truth_surface.py"
SPEC = importlib.util.spec_from_file_location("truth_surface", SCRIPT_PATH)
assert SPEC and SPEC.loader
truth_surface = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(truth_surface)


class TruthSurfaceTests(unittest.TestCase):
    def test_repo_truth_surface_nix_check_has_git_on_path(self):
        flake = (SCRIPT_PATH.parents[1] / "flake.nix").read_text(encoding="utf-8")
        check = flake.split("repo_truth_surface =", 1)[1].split(
            "import_rows_smoke =", 1
        )[0]

        self.assertIn("nativeBuildInputs = [ pkgs.git ];", check)
        self.assertIn("git init --quiet", check)
        self.assertIn("git add --all", check)

    def test_checksum_scope_excludes_untracked_files(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            repo = Path(temporary_directory)
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
            (repo / "tracked.txt").write_text("tracked\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)
            (repo / "host-local.txt").write_text("host local\n", encoding="utf-8")

            self.assertEqual(truth_surface.included_files(repo), ["tracked.txt"])

    def test_cli_round_trip_detects_tracked_mutation_and_reseal_restores_truth(self):
        with tempfile.TemporaryDirectory() as temporary_directory:
            repo = Path(temporary_directory)
            subprocess.run(["git", "init"], cwd=repo, check=True, capture_output=True)
            tracked = repo / "tracked.txt"
            tracked.write_text("original\n", encoding="utf-8")
            subprocess.run(["git", "add", "tracked.txt"], cwd=repo, check=True)

            def run_truth_surface(mode: str) -> subprocess.CompletedProcess[str]:
                return subprocess.run(
                    [sys.executable, str(SCRIPT_PATH), mode],
                    cwd=repo,
                    text=True,
                    capture_output=True,
                    check=False,
                )

            self.assertEqual(run_truth_surface("--write").returncode, 0)
            subprocess.run(["git", "add", "manifests"], cwd=repo, check=True)
            self.assertEqual(run_truth_surface("--check").returncode, 0)
            self.assertEqual(run_truth_surface("--check-source").returncode, 0)

            tracked.write_text("mutated\n", encoding="utf-8")
            stale = run_truth_surface("--check-source")
            self.assertEqual(stale.returncode, 1)
            self.assertIn("sha256 mismatch: tracked.txt", stale.stderr)

            self.assertEqual(run_truth_surface("--write").returncode, 0)
            self.assertEqual(run_truth_surface("--check").returncode, 0)
            self.assertEqual(run_truth_surface("--check-source").returncode, 0)


if __name__ == "__main__":
    unittest.main()

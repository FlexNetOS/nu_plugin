#!/usr/bin/env python3
"""Static policy tests for the detached requirement-proof CI lane."""

from __future__ import annotations

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
FLAKE = ROOT / "flake.nix"
SANDBOX_SETUP = ROOT / "scripts/ci/require_bwrap_userns.sh"
PINNED_ACTION = re.compile(r"^[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+@[0-9a-f]{40}$")


def workflow_job(workflow: str, name: str) -> str:
    match = re.search(
        rf"(?ms)^  {re.escape(name)}:\n(?P<body>.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)",
        workflow,
    )
    if match is None:
        raise AssertionError(f"ci.yml has no {name} job")
    return match.group(0)


class DetachedRequirementProofWorkflowTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")
        cls.verify_job = workflow_job(cls.workflow, "requirement_proof_verification")
        cls.sign_job = workflow_job(cls.workflow, "requirement_proof_signer")

    def test_every_action_reference_is_pinned_to_a_full_commit(self) -> None:
        action_refs = re.findall(
            r"(?m)^\s*-\s+uses:\s+(\S+?)(?:\s+#.*)?$", self.workflow
        )
        self.assertTrue(action_refs)
        self.assertEqual(
            [],
            [
                reference
                for reference in action_refs
                if not PINNED_ACTION.fullmatch(reference)
            ],
        )

    def test_untrusted_events_verify_without_oidc_or_repository_write(self) -> None:
        self.assertRegex(self.workflow, r"(?m)^  pull_request:$")
        self.assertRegex(
            self.workflow,
            r"(?ms)^  merge_group:\n    types:\n      - checks_requested$",
        )
        self.assertRegex(self.workflow, r"(?m)^permissions:\n  contents: read$")
        self.assertIn("runs-on: ubuntu-latest", self.verify_job)
        for forbidden in (
            "id-token: write",
            "attestations: write",
            "artifact-metadata: write",
            "contents: write",
            "write-all",
        ):
            self.assertNotIn(forbidden, self.verify_job)

    def test_checkout_and_receipt_are_bound_to_the_submitted_sha(self) -> None:
        self.assertIn(
            "github.event.pull_request.head.sha || github.sha",
            self.verify_job,
        )
        self.assertIn("ref: ${{ env.CODEDB_REVIEWED_SHA }}", self.verify_job)
        self.assertIn("persist-credentials: false", self.verify_job)
        self.assertIn(
            'test "$(git rev-parse HEAD)" = "$CODEDB_REVIEWED_SHA"',
            self.verify_job,
        )
        self.assertIn(
            '"commit_sha"] == os.environ["CODEDB_REVIEWED_SHA"]',
            self.verify_job,
        )
        self.assertIn(
            "CODEDB_PROOF_RECEIPT: /tmp/codedb-requirement-proof-",
            self.verify_job,
        )

    def test_job_environment_uses_only_admission_time_contexts(self) -> None:
        for job in (self.verify_job, self.sign_job):
            self.assertIn(
                "CODEDB_PROOF_RECEIPT: /tmp/codedb-requirement-proof-",
                job,
            )
            self.assertNotRegex(
                job,
                r"CODEDB_PROOF_RECEIPT:\s+\$\{\{\s*runner\.temp\s*\}\}",
            )

    def test_receipt_targets_every_mandatory_requirement_without_subset(self) -> None:
        self.assertIn("--all-requirements", self.verify_job)
        self.assertNotRegex(self.verify_job, r"--requirement\s+(?:CDB|REQ-)")
        self.assertIn("scripts/generate_requirement_proof_receipt.py", self.verify_job)
        self.assertIn("assert len(expected_requirements) == 140", self.verify_job)
        self.assertIn('assert len(receipt["rows"]) == 140', self.verify_job)
        self.assertIn('len(receipt["command_executions"])', self.verify_job)
        self.assertIn("}) == 61", self.verify_job)
        self.assertIn("execution/REQUIREMENT_PROOF_LEDGER.csv", self.verify_job)
        self.assertIn('"schema_version"] == 4', self.verify_job)

    def test_verifier_provides_non_skipping_verified_tls_postgres(self) -> None:
        for expected in (
            "services:",
            "image: postgres:17",
            "POSTGRES_CONTAINER: ${{ job.services.postgres.id }}",
            "CODEDB_PG_CONN=postgresql://codedb:codedb@localhost:5432/codedb",
            "sslmode=verify-full",
            "sslrootcert=$encoded_ca",
            "openssl x509 -req",
            "pg_isready -U codedb -d codedb",
        ):
            self.assertIn(expected, self.verify_job)
        self.assertNotIn("CODEDB_PG_CONN", self.sign_job)

    def test_trusted_post_check_signer_is_protected_and_exact_sha_bound(self) -> None:
        self.assertIn("needs: requirement_proof_verification", self.sign_job)
        self.assertIn("environment: requirement-proof-signer", self.sign_job)
        self.assertIn("github.repository == 'FlexNetOS/nu_plugin'", self.sign_job)
        self.assertIn(
            "github.event.pull_request.head.repo.full_name == github.repository",
            self.sign_job,
        )
        self.assertIn("id-token: write", self.sign_job)
        self.assertIn("attestations: write", self.sign_job)
        self.assertIn("contents: read", self.sign_job)
        self.assertIn("CODEDB_REVIEWED_SHA", self.sign_job)
        self.assertIn("path: /tmp", self.sign_job)
        self.assertIn(
            '"commit_sha"] == os.environ["CODEDB_REVIEWED_SHA"]', self.sign_job
        )
        self.assertNotRegex(self.sign_job, r"uses: actions/checkout@")
        self.assertNotIn("scripts/generate_requirement_proof_receipt.py", self.sign_job)

    def test_receipt_and_detached_bundle_are_attested_and_retained(self) -> None:
        self.assertRegex(self.sign_job, r"uses: actions/attest@[0-9a-f]{40}")
        self.assertIn("subject-path: ${{ env.CODEDB_PROOF_RECEIPT }}", self.sign_job)
        self.assertRegex(self.sign_job, r"uses: actions/upload-artifact@[0-9a-f]{40}")
        self.assertIn("${{ steps.attest.outputs.bundle-path }}", self.sign_job)
        self.assertIn("retention-days: 90", self.sign_job)
        self.assertIn("if-no-files-found: error", self.sign_job)

    def test_lane_cannot_push_or_leave_source_mutations(self) -> None:
        jobs = self.verify_job + self.sign_job
        self.assertNotRegex(jobs, r"(?m)^\s*git\s+push\b")
        self.assertNotRegex(jobs, r"(?m)^\s*git\s+(add|commit|checkout|reset)\b")
        self.assertGreaterEqual(
            self.verify_job.count(
                'test -z "$(git status --porcelain=v1 --untracked-files=all)"'
            ),
            2,
        )
        self.assertNotIn("HY3", jobs)

    def test_nushell_binary_consumers_honor_external_cargo_target_dir(self) -> None:
        for relative_path in (
            "tests/test_dynamic_store_plugin.nu",
            "tests/test_yazelix_enabled_smoke.nu",
            "tests/test_transient_plugin.nu",
            "tests/test_plugin_secret_guard.nu",
            "tests/test_no_real_home_plugin_registration.nu",
            "tests/test_plugin_registry.nu",
        ):
            with self.subTest(path=relative_path):
                source = (ROOT / relative_path).read_text(encoding="utf-8")
                self.assertIn("$env.CARGO_TARGET_DIR?", source)
                self.assertNotIn("[$repo_root target debug", source)

    def test_execution_lanes_provision_the_mandatory_bubblewrap_sandbox(self) -> None:
        flake = FLAKE.read_text(encoding="utf-8")
        sandbox_setup = SANDBOX_SETUP.read_text(encoding="utf-8")
        self.assertIn("pkgs.bubblewrap", flake)
        self.assertIn("pkgs.lib.optionals pkgs.stdenv.isLinux", flake)
        for job_name in (
            "mandatory_capabilities",
            "postgres_parity",
            "rust",
            "nu",
            "requirement_proof_verification",
        ):
            with self.subTest(job=job_name):
                self.assertIn(
                    "bash scripts/ci/require_bwrap_userns.sh",
                    workflow_job(self.workflow, job_name),
                )
        for expected in (
            "kernel/apparmor_restrict_unprivileged_userns",
            'sudo tee "$restriction"',
            "/nix/store/*/bin/bwrap",
            "--unshare-all",
            "--ro-bind / /",
            "--proc /proc",
            "--dev /dev",
            'dirname "$bwrap_path" >>"$GITHUB_PATH"',
        ):
            self.assertIn(expected, sandbox_setup)
        self.assertIn(
            "nix develop .#ci -c python3 "
            "scripts/generate_requirement_proof_receipt.py",
            self.verify_job,
        )


if __name__ == "__main__":
    unittest.main()

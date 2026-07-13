#!/usr/bin/env python3

from __future__ import annotations

import copy
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from requirement_proof_attestation import (  # noqa: E402
    EXTERNAL_SOURCE_PIN_PATH,
    CheckoutIdentity,
    ExternalSourceIdentity,
    canonical_command_execution_payload,
    canonical_ledger_row_payload,
    canonical_receipt_payload,
    canonical_receipt_row_payload,
    canonical_repository,
    external_source_receipt_identity,
    load_external_source_pins,
    parse_artifact_declarations,
    sha256_bytes,
    validate_receipt,
    verify_github_attestation,
)
import generate_requirement_proof_receipt as receipt_generator  # noqa: E402
from generate_requirement_proof_receipt import (  # noqa: E402
    build_receipt,
    ensure_attestable_row,
    ensure_external_output,
    run_requirement,
)


DIGEST = "1" * 64
COMMIT = "a" * 40
TREE = "b" * 40
REPOSITORY = "FlexNetOS/nu_plugin"
PIN_DIGEST = "4" * 64


def external_identities() -> dict[str, ExternalSourceIdentity]:
    return {
        "envctl": ExternalSourceIdentity(
            name="envctl",
            repository="FlexNetOS/envctl",
            source_ref="refs/heads/envctl-db-automation-2026-07-12",
            commit_sha="c" * 40,
            tree_sha="d" * 40,
            checkout_path="../envctl",
            pin_path=EXTERNAL_SOURCE_PIN_PATH.as_posix(),
            pin_sha256=PIN_DIGEST,
        ),
        "loop_lib": ExternalSourceIdentity(
            name="loop_lib",
            repository="FlexNetOS/loop_lib",
            source_ref="refs/heads/main",
            commit_sha="e" * 40,
            tree_sha="f" * 40,
            checkout_path="../loop_lib",
            pin_path=EXTERNAL_SOURCE_PIN_PATH.as_posix(),
            pin_sha256=PIN_DIGEST,
        ),
        "meta_plugin_protocol": ExternalSourceIdentity(
            name="meta_plugin_protocol",
            repository="FlexNetOS/meta_plugin_protocol",
            source_ref="refs/heads/main",
            commit_sha="1" * 40,
            tree_sha="2" * 40,
            checkout_path="../meta_plugin_protocol",
            pin_path=EXTERNAL_SOURCE_PIN_PATH.as_posix(),
            pin_sha256=PIN_DIGEST,
        ),
    }


def ledger_row(requirement_id: str = "CDB013") -> dict[str, str]:
    return {
        "requirement_id": requirement_id,
        "verification_command": "cargo metadata --format-version 1 --no-deps",
        "evidence_status": "verified",
        "task_status": "complete",
        "proof_artifacts": (
            "stdout:cargo-metadata-stdout;"
            "stderr:cargo-metadata-stderr;"
            "file:cargo-manifest:repository:Cargo.toml"
        ),
        "test_paths": "tests/test_requirement_proof_attestation.py",
    }


def valid_receipt() -> dict:
    source_row = ledger_row()
    command_execution = {
        "verification_command": source_row["verification_command"],
        "exit_code": 0,
        "stdout_size_bytes": 7,
        "stderr_size_bytes": 0,
        "stdout_sha256": DIGEST,
        "stderr_sha256": DIGEST,
    }
    command_execution["execution_sha256"] = sha256_bytes(
        canonical_command_execution_payload(command_execution)
    )
    receipt_row = {
        "requirement_id": "CDB013",
        "status": "verified",
        "verification_command": "cargo metadata --format-version 1 --no-deps",
        "exit_code": 0,
        "stdout_sha256": DIGEST,
        "stderr_sha256": DIGEST,
        "command_execution_sha256": command_execution["execution_sha256"],
        "evidence": [
            {
                "logical_name": "cargo-metadata-stdout",
                "sha256": DIGEST,
                "size_bytes": 7,
                "type": "stdout",
            },
            {
                "logical_name": "cargo-metadata-stderr",
                "sha256": DIGEST,
                "size_bytes": 0,
                "type": "stderr",
            },
            {
                "logical_name": "cargo-manifest",
                "root": "repository",
                "path": "Cargo.toml",
                "sha256": DIGEST,
                "size_bytes": 123,
                "type": "file",
            },
        ],
        "ledger_row_sha256": sha256_bytes(canonical_ledger_row_payload(source_row)),
    }
    receipt_row["row_sha256"] = sha256_bytes(canonical_receipt_row_payload(receipt_row))
    receipt = {
        "schema_version": 4,
        "attestation_type": "requirement-proof",
        "repository": REPOSITORY,
        "commit_sha": COMMIT,
        "tree_sha": TREE,
        "ledger": {
            "path": "execution/REQUIREMENT_PROOF_LEDGER.csv",
            "sha256": DIGEST,
        },
        "validator": {
            "path": "scripts/validate_requirement_proof_ledger.py",
            "sha256": DIGEST,
        },
        "external_sources": {
            name: {
                **external_source_receipt_identity(source),
                "worktree": {
                    "clean_before": True,
                    "clean_after": True,
                    "status_before_sha256": sha256_bytes(b""),
                    "status_after_sha256": sha256_bytes(b""),
                },
            }
            for name, source in external_identities().items()
        },
        "generated_at_utc": "2026-07-11T19:00:00+00:00",
        "generator": {"provider": "github-actions", "run_id": "1234"},
        "worktree": {
            "clean_before": True,
            "clean_after": True,
            "status_before_sha256": sha256_bytes(b""),
            "status_after_sha256": sha256_bytes(b""),
        },
        "command_executions": [command_execution],
        "rows": [receipt_row],
    }
    receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
    return receipt


def identity(*, clean: bool = True) -> CheckoutIdentity:
    return CheckoutIdentity(
        repository=REPOSITORY,
        commit_sha=COMMIT,
        tree_sha=TREE,
        ledger_sha256=DIGEST,
        validator_sha256=DIGEST,
        clean=clean,
        external_sources=external_identities(),
    )


def init_git_checkout(path: Path, repository: str) -> tuple[str, str]:
    path.mkdir(parents=True)
    subprocess.run(["git", "init", "-q"], cwd=path, check=True)
    subprocess.run(
        ["git", "remote", "add", "origin", f"https://github.com/{repository}.git"],
        cwd=path,
        check=True,
    )
    (path / "tracked.txt").write_text("pinned\n", encoding="utf-8")
    subprocess.run(["git", "add", "tracked.txt"], cwd=path, check=True)
    subprocess.run(
        [
            "git",
            "-c",
            "user.name=CodeDB Test",
            "-c",
            "user.email=codedb-test@example.invalid",
            "commit",
            "-q",
            "-m",
            "pinned",
        ],
        cwd=path,
        check=True,
    )
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=path,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    tree = subprocess.run(
        ["git", "rev-parse", "HEAD^{tree}"],
        cwd=path,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    return commit, tree


def make_external_identity(
    name: str,
    repository: str,
    checkout_path: str,
    commit_sha: str,
    tree_sha: str,
) -> ExternalSourceIdentity:
    return ExternalSourceIdentity(
        name=name,
        repository=repository,
        source_ref="refs/heads/test",
        commit_sha=commit_sha,
        tree_sha=tree_sha,
        checkout_path=checkout_path,
        pin_path=EXTERNAL_SOURCE_PIN_PATH.as_posix(),
        pin_sha256=PIN_DIGEST,
    )


class RequirementProofAttestationTest(unittest.TestCase):
    def test_tracked_external_source_pin_is_exact_and_complete(self) -> None:
        sources = load_external_source_pins(ROOT / EXTERNAL_SOURCE_PIN_PATH)
        self.assertEqual(
            {"envctl", "loop_lib", "meta_plugin_protocol"}, set(sources)
        )
        self.assertEqual(
            "b62669c4e32c8de0407aa51ca3add94d529b50b6",
            sources["envctl"].commit_sha,
        )
        self.assertEqual(
            "6e79836387d15ac5849e73e7bb869c6077953d90",
            sources["loop_lib"].commit_sha,
        )
        self.assertEqual(
            "7d65eeac3bba8e9702eb0590ba9476e4e420bfb3",
            sources["meta_plugin_protocol"].commit_sha,
        )
        with tempfile.TemporaryDirectory() as temp:
            tampered_pin = Path(temp) / "sources.json"
            pin_text = (ROOT / EXTERNAL_SOURCE_PIN_PATH).read_text(encoding="utf-8")
            tampered_pin.write_text(
                pin_text.replace(
                    "b62669c4e32c8de0407aa51ca3add94d529b50b6",
                    "9" * 40,
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "commit_sha mismatch"):
                load_external_source_pins(tampered_pin)

    def test_external_checkout_rejects_absent_wrong_or_dirty_sources(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            root.mkdir()
            envctl = workspace / "envctl"
            missing_source = make_external_identity(
                "envctl", "FlexNetOS/envctl", "../envctl", "a" * 40, "b" * 40
            )
            with self.assertRaisesRegex(RuntimeError, "checkout is absent"):
                receipt_generator.external_checkout_snapshot(root, missing_source)

            commit, tree = init_git_checkout(envctl, "attacker/envctl")
            source = make_external_identity(
                "envctl", "FlexNetOS/envctl", "../envctl", commit, tree
            )
            with self.assertRaisesRegex(RuntimeError, "remote mismatch"):
                receipt_generator.external_checkout_snapshot(root, source)

            subprocess.run(
                [
                    "git",
                    "remote",
                    "set-url",
                    "origin",
                    "https://github.com/FlexNetOS/envctl.git",
                ],
                cwd=envctl,
                check=True,
            )
            (envctl / "untracked.txt").write_text("dirty\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "checkout is dirty"):
                receipt_generator.external_checkout_snapshot(root, source)
            (envctl / "untracked.txt").unlink()

            wrong_head = ExternalSourceIdentity(
                **{**source.__dict__, "commit_sha": "9" * 40}
            )
            with self.assertRaisesRegex(RuntimeError, "HEAD mismatch"):
                receipt_generator.external_checkout_snapshot(root, wrong_head)
            wrong_tree = ExternalSourceIdentity(
                **{**source.__dict__, "tree_sha": "8" * 40}
            )
            with self.assertRaisesRegex(RuntimeError, "tree mismatch"):
                receipt_generator.external_checkout_snapshot(root, wrong_tree)

    def test_all_external_dependencies_are_mandatory(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            root.mkdir()
            envctl = workspace / "envctl"
            commit, tree = init_git_checkout(envctl, "FlexNetOS/envctl")
            sources = {
                "envctl": make_external_identity(
                    "envctl", "FlexNetOS/envctl", "../envctl", commit, tree
                ),
                "loop_lib": make_external_identity(
                    "loop_lib",
                    "FlexNetOS/loop_lib",
                    "../loop_lib",
                    "a" * 40,
                    "b" * 40,
                ),
                "meta_plugin_protocol": make_external_identity(
                    "meta_plugin_protocol",
                    "FlexNetOS/meta_plugin_protocol",
                    "../meta_plugin_protocol",
                    "c" * 40,
                    "d" * 40,
                ),
            }
            with (
                patch.object(
                    receipt_generator,
                    "load_external_source_pins",
                    return_value=sources,
                ),
                self.assertRaisesRegex(RuntimeError, "loop_lib.*checkout is absent"),
            ):
                receipt_generator.load_external_checkouts(root)

    def test_external_cargo_workspace_must_match_tracked_template(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            template = root / receipt_generator.EXTERNAL_WORKSPACE_TEMPLATE_PATH
            template.parent.mkdir(parents=True)
            template.write_text("[workspace]\nmembers = []\n", encoding="utf-8")
            with self.assertRaises(FileNotFoundError):
                receipt_generator.external_workspace_snapshot(root)

            external_workspace = workspace / "Cargo.toml"
            external_workspace.write_text("[workspace]\n", encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "does not match"):
                receipt_generator.external_workspace_snapshot(root)
            external_workspace.write_bytes(template.read_bytes())
            self.assertEqual(
                sha256_bytes(template.read_bytes()),
                receipt_generator.external_workspace_snapshot(root),
            )

    def test_proof_command_cannot_mutate_attested_or_external_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            envctl = workspace / "envctl"
            init_git_checkout(root, "FlexNetOS/nu_plugin")
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (root / "proof.py").write_text("# proof\n", encoding="utf-8")
            subprocess.run(["git", "add", "Cargo.toml", "proof.py"], cwd=root, check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=CodeDB Test",
                    "-c",
                    "user.email=codedb-test@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "proof inputs",
                ],
                cwd=root,
                check=True,
            )
            commit, tree = init_git_checkout(envctl, "FlexNetOS/envctl")
            source = make_external_identity(
                "envctl", "FlexNetOS/envctl", "../envctl", commit, tree
            )
            path, snapshot = receipt_generator.external_checkout_snapshot(root, source)
            row = ledger_row()
            row["test_paths"] = "proof.py"
            row["verification_command"] = "printf mutation > mutated-root.txt"
            with self.assertRaisesRegex(RuntimeError, "mutated checkout"):
                run_requirement(
                    root,
                    row,
                    external_checkouts={"envctl": (source, path, snapshot)},
                )
            (root / "mutated-root.txt").unlink()

            row["verification_command"] = "printf mutation > ../envctl/mutated.txt"
            with self.assertRaisesRegex(RuntimeError, "mutated external checkout"):
                run_requirement(
                    root,
                    row,
                    external_checkouts={"envctl": (source, path, snapshot)},
                )

    def test_generator_requires_external_sibling_test_to_exist(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            workspace = Path(temp)
            root = workspace / "nu_plugin"
            external_test = workspace / "envctl/tests/db_docs_contract.rs"
            root.mkdir()
            external_test.parent.mkdir(parents=True)
            external_test.write_text("proof\n", encoding="utf-8")

            row = ledger_row()
            row["test_paths"] = "external:../envctl/tests/db_docs_contract.rs"
            ensure_attestable_row(root, row)

            row["test_paths"] = "external:../envctl/tests/missing.rs"
            with self.assertRaisesRegex(
                RuntimeError, "direct test path does not exist"
            ):
                ensure_attestable_row(root, row)

    def test_artifact_declarations_are_typed_exact_and_unique(self) -> None:
        declarations = parse_artifact_declarations(ledger_row()["proof_artifacts"])
        self.assertEqual(
            ["stdout", "stderr", "file"],
            [declaration.artifact_type for declaration in declarations],
        )
        self.assertEqual(
            ("repository", "Cargo.toml"),
            (declarations[2].root_name, declarations[2].relative_path),
        )
        for invalid in (
            "cargo-metadata-output",
            "stdout:duplicate;stderr:duplicate",
            "file:one:repository:Cargo.toml;file:two:repository:Cargo.toml",
            "file:escape:repository:../outside",
            "file:absolute:repository:/tmp/outside",
            "file:unnormalized:repository:a//b",
        ):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                parse_artifact_declarations(invalid)

    def test_generator_hashes_stdout_stderr_and_exact_file_independently(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            artifact = root / "Cargo.toml"
            artifact.write_bytes(b'{"proof":true}\n')
            row = ledger_row()
            row["test_paths"] = "Cargo.toml"
            completed = subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout=b"stdout bytes\n",
                stderr=b"stderr bytes\n",
            )
            with (
                patch.object(receipt_generator, "worktree_status", return_value=""),
                patch.object(
                    receipt_generator.subprocess,
                    "run",
                    return_value=completed,
                ),
            ):
                receipt_row = run_requirement(
                    root,
                    row,
                    approved_artifact_roots={"repository": root},
                )

        evidence = {item["logical_name"]: item for item in receipt_row["evidence"]}
        self.assertEqual(
            sha256_bytes(completed.stdout),
            evidence["cargo-metadata-stdout"]["sha256"],
        )
        self.assertEqual(
            sha256_bytes(completed.stderr),
            evidence["cargo-metadata-stderr"]["sha256"],
        )
        self.assertNotEqual(
            evidence["cargo-metadata-stdout"]["sha256"],
            evidence["cargo-metadata-stderr"]["sha256"],
        )
        self.assertEqual(
            {
                "logical_name": "cargo-manifest",
                "type": "file",
                "root": "repository",
                "path": "Cargo.toml",
                "size_bytes": len(b'{"proof":true}\n'),
                "sha256": sha256_bytes(b'{"proof":true}\n'),
            },
            evidence["cargo-manifest"],
        )

    def test_generator_rejects_missing_or_raced_file_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            row = ledger_row()
            row["test_paths"] = "proof.py"
            (root / "proof.py").write_text("# direct test\n", encoding="utf-8")
            completed = subprocess.CompletedProcess([], 0, b"", b"")
            with (
                patch.object(receipt_generator, "worktree_status", return_value=""),
                patch.object(
                    receipt_generator.subprocess,
                    "run",
                    return_value=completed,
                ),
                self.assertRaisesRegex(RuntimeError, "missing file artifact"),
            ):
                run_requirement(
                    root,
                    row,
                    approved_artifact_roots={"repository": root},
                )

            (root / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
            with (
                patch.object(receipt_generator, "worktree_status", return_value=""),
                patch.object(
                    receipt_generator.subprocess,
                    "run",
                    return_value=completed,
                ),
                patch.object(
                    receipt_generator,
                    "hash_file_artifact",
                    side_effect=RuntimeError("file artifact raced during hashing"),
                ),
                self.assertRaisesRegex(RuntimeError, "raced"),
            ):
                run_requirement(
                    root,
                    row,
                    approved_artifact_roots={"repository": root},
                )

    def test_generator_refuses_to_label_partial_or_missing_rows_verified(
        self,
    ) -> None:
        for status in ("partial", "missing", "blocked", "contradicted"):
            with self.subTest(status=status):
                row = {
                    "requirement_id": "CDB013",
                    "verification_command": "python3 -c 'print(1)'",
                    "proof_artifacts": "stdout:command-output",
                    "test_paths": "tests/proof.py",
                    "evidence_status": status,
                    "task_status": "active",
                }
                with (
                    patch.object(receipt_generator, "worktree_status", return_value=""),
                    patch.object(receipt_generator.subprocess, "run") as command_runner,
                    self.assertRaisesRegex(
                        RuntimeError, "ledger evidence_status is not verified"
                    ),
                ):
                    run_requirement(ROOT, row)
                command_runner.assert_not_called()

    def test_all_requirements_preflights_every_row_before_running_commands(
        self,
    ) -> None:
        first = ledger_row("CDB013")
        second = ledger_row("CDB014")
        second["evidence_status"] = "partial"
        second["task_status"] = "active"
        command_runner = Mock()
        with (
            patch.object(
                receipt_generator,
                "EXPECTED_REQUIREMENT_IDS",
                {"CDB013", "CDB014"},
            ),
            patch.object(
                receipt_generator,
                "read_ledger",
                return_value=[first, second],
            ),
            patch.object(
                receipt_generator, "load_external_checkouts", return_value={}
            ),
            patch.object(receipt_generator, "worktree_status", return_value=""),
            patch.object(
                receipt_generator, "execute_verification_command", command_runner
            ),
            self.assertRaisesRegex(
                RuntimeError,
                "CDB014: ledger evidence_status is not verified",
            ),
        ):
            build_receipt(ROOT, None, provider="github-actions", run_id="1234")
        command_runner.assert_not_called()

    def test_stateful_external_commands_use_fresh_temporary_targets(self) -> None:
        rows = {
            row["requirement_id"]: row
            for row in receipt_generator.read_ledger(
                ROOT / receipt_generator.LEDGER_PATH
            )
        }
        stateful_ids = {
            "REQ-061-CMD01",
            "REQ-061-CMD06",
            "REQ-061-CMD07",
            "REQ-061-CMD11",
            "REQ-061-AC01",
            "REQ-061-AC05",
        }
        for requirement_id in stateful_ids:
            with self.subTest(requirement_id=requirement_id):
                command = rows[requirement_id]["verification_command"]
                self.assertIn("mktemp -d", command)
                self.assertIn("trap 'rm -rf", command)
                self.assertNotIn("/tmp/claude-1001", command)
        for requirement_id in {
            "REQ-061-CMD01",
            "REQ-061-CMD11",
            "REQ-061-AC01",
        }:
            self.assertIn(
                "git -C ../envctl archive HEAD",
                rows[requirement_id]["verification_command"],
            )

    def test_identical_commands_execute_once_and_validate_both_rows(self) -> None:
        first = ledger_row("CDB013")
        second = ledger_row("CDB014")
        completed = subprocess.CompletedProcess([], 0, b"shared stdout\n", b"")
        executor = Mock(return_value=completed)

        def git_value(_: Path, *args: str) -> str:
            if args == ("config", "--get", "remote.origin.url"):
                return "git@github.com:FlexNetOS/nu_plugin.git"
            if args == ("rev-parse", "HEAD"):
                return COMMIT
            if args == ("rev-parse", "HEAD^{tree}"):
                return TREE
            raise AssertionError(f"unexpected git args: {args}")

        with (
            patch.object(
                receipt_generator,
                "EXPECTED_REQUIREMENT_IDS",
                {"CDB013", "CDB014"},
            ),
            patch.object(
                receipt_generator,
                "read_ledger",
                return_value=[first, second],
            ),
            patch.object(
                receipt_generator, "load_external_checkouts", return_value={}
            ),
            patch.object(receipt_generator, "worktree_status", return_value=""),
            patch.object(receipt_generator, "git_output", side_effect=git_value),
            patch.object(
                receipt_generator,
                "execute_verification_command",
                executor,
            ),
        ):
            receipt = build_receipt(
                ROOT,
                None,
                provider="github-actions",
                run_id="1234",
            )

        executor.assert_called_once()
        self.assertEqual(
            ["CDB013", "CDB014"],
            executor.call_args.kwargs["requirement_ids"],
        )
        self.assertEqual(
            {"CDB013", "CDB014"},
            {row["requirement_id"] for row in receipt["rows"]},
        )
        self.assertEqual(1, len(receipt["command_executions"]))
        execution_sha = receipt["command_executions"][0]["execution_sha256"]
        self.assertEqual(
            {execution_sha},
            {row["command_execution_sha256"] for row in receipt["rows"]},
        )
        proof_environment = executor.call_args.kwargs["proof_environment"]
        self.assertTrue(proof_environment["CARGO_TARGET_DIR"].startswith("/tmp/"))
        _, violations = validate_receipt(
            receipt,
            identity=CheckoutIdentity(
                repository=REPOSITORY,
                commit_sha=COMMIT,
                tree_sha=TREE,
                ledger_sha256=receipt["ledger"]["sha256"],
                validator_sha256=receipt["validator"]["sha256"],
                clean=True,
                external_sources={},
            ),
            ledger_rows=[first, second],
        )
        self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))

    def test_different_commands_execute_separately(self) -> None:
        first = ledger_row("CDB013")
        second = ledger_row("CDB014")
        second["verification_command"] += " --offline"
        executor = Mock(
            side_effect=[
                subprocess.CompletedProcess([], 0, b"first\n", b""),
                subprocess.CompletedProcess([], 0, b"second\n", b""),
            ]
        )

        def git_value(_: Path, *args: str) -> str:
            if args == ("config", "--get", "remote.origin.url"):
                return "git@github.com:FlexNetOS/nu_plugin.git"
            if args == ("rev-parse", "HEAD"):
                return COMMIT
            if args == ("rev-parse", "HEAD^{tree}"):
                return TREE
            raise AssertionError(f"unexpected git args: {args}")

        with (
            patch.object(
                receipt_generator,
                "EXPECTED_REQUIREMENT_IDS",
                {"CDB013", "CDB014"},
            ),
            patch.object(
                receipt_generator,
                "read_ledger",
                return_value=[first, second],
            ),
            patch.object(
                receipt_generator, "load_external_checkouts", return_value={}
            ),
            patch.object(receipt_generator, "worktree_status", return_value=""),
            patch.object(receipt_generator, "git_output", side_effect=git_value),
            patch.object(
                receipt_generator,
                "execute_verification_command",
                executor,
            ),
        ):
            receipt = build_receipt(
                ROOT,
                None,
                provider="github-actions",
                run_id="1234",
            )

        self.assertEqual(2, executor.call_count)
        self.assertEqual(
            [first["verification_command"], second["verification_command"]],
            [call.args[1] for call in executor.call_args_list],
        )
        self.assertEqual(2, len(receipt["command_executions"]))
        self.assertEqual(
            2,
            len(
                {
                    row["command_execution_sha256"]
                    for row in receipt["rows"]
                }
            ),
        )

    def test_shared_command_failure_executes_once_and_aborts_receipt(self) -> None:
        first = ledger_row("CDB013")
        second = ledger_row("CDB014")
        executor = Mock(side_effect=RuntimeError("shared command failed"))
        with (
            patch.object(
                receipt_generator,
                "EXPECTED_REQUIREMENT_IDS",
                {"CDB013", "CDB014"},
            ),
            patch.object(
                receipt_generator,
                "read_ledger",
                return_value=[first, second],
            ),
            patch.object(
                receipt_generator, "load_external_checkouts", return_value={}
            ),
            patch.object(receipt_generator, "worktree_status", return_value=""),
            patch.object(
                receipt_generator,
                "execute_verification_command",
                executor,
            ),
            self.assertRaisesRegex(RuntimeError, "shared command failed"),
        ):
            build_receipt(ROOT, None, provider="github-actions", run_id="1234")
        executor.assert_called_once()
        self.assertEqual(
            ["CDB013", "CDB014"],
            executor.call_args.kwargs["requirement_ids"],
        )

    def test_all_requirements_rejects_incomplete_inventory(self) -> None:
        row = ledger_row("CDB013")
        command_runner = Mock()
        with (
            patch.object(
                receipt_generator,
                "EXPECTED_REQUIREMENT_IDS",
                {"CDB013", "CDB014"},
            ),
            patch.object(receipt_generator, "read_ledger", return_value=[row]),
            patch.object(
                receipt_generator, "load_external_checkouts", return_value={}
            ),
            patch.object(receipt_generator, "worktree_status", return_value=""),
            patch.object(receipt_generator, "run_requirement", command_runner),
            self.assertRaisesRegex(
                ValueError,
                "all-requirements inventory mismatch",
            ),
        ):
            build_receipt(ROOT, None, provider="github-actions", run_id="1234")
        command_runner.assert_not_called()

    def test_repository_urls_are_canonicalized_without_weakening_owner_binding(
        self,
    ) -> None:
        self.assertEqual(
            REPOSITORY,
            canonical_repository("git@github.com:FlexNetOS/nu_plugin.git"),
        )
        self.assertEqual(
            REPOSITORY,
            canonical_repository("https://github.com/FlexNetOS/nu_plugin.git"),
        )
        with self.assertRaises(ValueError):
            canonical_repository("file:///tmp/nu_plugin")

    def test_receipt_output_must_remain_outside_attested_checkout(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / "repo"
            root.mkdir()
            with self.assertRaises(ValueError):
                ensure_external_output(root, root / "receipt.json")
            outside = Path(temp) / "receipt.json"
            self.assertEqual(outside.resolve(), ensure_external_output(root, outside))

    def test_valid_external_current_head_receipt_passes(self) -> None:
        rows, violations = validate_receipt(
            valid_receipt(),
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        self.assertEqual([], violations, "\n" + "\n".join(map(str, violations)))
        self.assertEqual({"CDB013"}, set(rows))

    def test_parent_commit_tree_ledger_and_validator_drift_are_rejected(self) -> None:
        receipt = valid_receipt()
        receipt["commit_sha"] = "c" * 40
        receipt["tree_sha"] = "d" * 40
        receipt["ledger"]["sha256"] = "2" * 64
        receipt["validator"]["sha256"] = "3" * 64
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("commit_sha mismatch", rules)
        self.assertIn("tree_sha mismatch", rules)
        self.assertIn("ledger.sha256 mismatch", rules)
        self.assertIn("validator.sha256 mismatch", rules)

    def test_external_source_receipt_identity_is_pin_bound_without_checkout(self) -> None:
        pinned_sources = load_external_source_pins(ROOT / EXTERNAL_SOURCE_PIN_PATH)
        receipt = valid_receipt()
        receipt["external_sources"] = {
            name: {
                **external_source_receipt_identity(source),
                "worktree": {
                    "clean_before": True,
                    "clean_after": True,
                    "status_before_sha256": sha256_bytes(b""),
                    "status_after_sha256": sha256_bytes(b""),
                },
            }
            for name, source in pinned_sources.items()
        }
        pinned_identity = CheckoutIdentity(
            repository=REPOSITORY,
            commit_sha=COMMIT,
            tree_sha=TREE,
            ledger_sha256=DIGEST,
            validator_sha256=DIGEST,
            clean=True,
            external_sources=pinned_sources,
        )
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, baseline_violations = validate_receipt(
            receipt,
            identity=pinned_identity,
            ledger_rows=[ledger_row()],
        )
        self.assertEqual([], baseline_violations)

        receipt["external_sources"]["envctl"]["commit_sha"] = "7" * 40
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=pinned_identity,
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(
                violation.rule
                == "external_sources.envctl.commit_sha mismatch"
                for violation in violations
            ),
            "\n" + "\n".join(map(str, violations)),
        )

        receipt["external_sources"]["envctl"]["commit_sha"] = pinned_sources[
            "envctl"
        ].commit_sha
        receipt["external_sources"].pop("loop_lib")
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=pinned_identity,
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(v.rule == "missing external-source identity" for v in violations)
        )

    def test_arbitrary_current_sha_text_cannot_replace_structured_receipt(self) -> None:
        receipt = {"note": f"proof for {COMMIT}"}
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("schema_version mismatch", rules)
        self.assertIn("invalid receipt digest", rules)
        self.assertIn("invalid receipt rows", rules)

    def test_dirty_checkout_or_dirty_proof_execution_is_rejected(self) -> None:
        receipt = valid_receipt()
        receipt["worktree"]["clean_after"] = False
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(clean=False),
            ledger_rows=[ledger_row()],
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("dirty checkout", rules)
        self.assertIn("dirty proof execution", rules)

    def test_row_command_exit_and_evidence_are_requirement_bound(self) -> None:
        receipt = valid_receipt()
        row = receipt["rows"][0]
        row["verification_command"] = "true"
        row["exit_code"] = 1
        row["evidence"] = []
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("CDB013.verification_command mismatch", rules)
        self.assertIn("failed receipt command", rules)
        self.assertIn("missing row evidence", rules)

    def test_tampered_receipt_digest_is_rejected(self) -> None:
        receipt = valid_receipt()
        tampered = copy.deepcopy(receipt)
        tampered["rows"][0]["stdout_sha256"] = "9" * 64
        _, violations = validate_receipt(
            tampered,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(violation.rule == "receipt digest mismatch" for violation in violations)
        )

    def test_shared_execution_cannot_substitute_stream_evidence(self) -> None:
        receipt = valid_receipt()
        execution = receipt["command_executions"][0]
        execution["stdout_size_bytes"] = 8
        execution["execution_sha256"] = sha256_bytes(
            canonical_command_execution_payload(execution)
        )
        receipt_row = receipt["rows"][0]
        receipt_row["command_execution_sha256"] = execution["execution_sha256"]
        receipt_row["row_sha256"] = sha256_bytes(
            canonical_receipt_row_payload(receipt_row)
        )
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(
                violation.rule
                == "CDB013.cargo-metadata-stdout.size_bytes mismatch"
                for violation in violations
            ),
            "\n" + "\n".join(map(str, violations)),
        )

    def test_embedded_signature_claim_cannot_substitute_for_detached_verification(
        self,
    ) -> None:
        receipt = valid_receipt()
        receipt["signature"] = {
            "kind": "github-artifact-attestation",
            "reference": "https://attacker.invalid/self-asserted",
        }
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(v.rule == "embedded trust claim" for v in violations),
            "\n" + "\n".join(map(str, violations)),
        )

    def test_repository_and_entire_ledger_row_are_bound(self) -> None:
        receipt = valid_receipt()
        receipt["repository"] = "attacker/fork"
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, repository_violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        self.assertTrue(
            any(v.rule == "repository mismatch" for v in repository_violations)
        )

        changed_ledger_row = ledger_row()
        changed_ledger_row["new_policy_field"] = "changed"
        _, row_violations = validate_receipt(
            valid_receipt(),
            identity=identity(),
            ledger_rows=[changed_ledger_row],
        )
        self.assertTrue(
            any(v.rule == "ledger row digest mismatch" for v in row_violations)
        )

    def test_receipt_cannot_attest_a_nonverified_ledger_row(self) -> None:
        source_row = ledger_row()
        source_row["evidence_status"] = "partial"
        source_row["task_status"] = "active"
        receipt = valid_receipt()
        receipt_row = receipt["rows"][0]
        receipt_row["ledger_row_sha256"] = sha256_bytes(
            canonical_ledger_row_payload(source_row)
        )
        receipt_row["row_sha256"] = sha256_bytes(
            canonical_receipt_row_payload(receipt_row)
        )
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[source_row],
        )
        self.assertTrue(
            any(v.rule == "receipt attests unverified ledger row" for v in violations),
            "\n" + "\n".join(map(str, violations)),
        )

    def test_receipt_rejects_duplicate_or_declaration_mismatched_artifacts(
        self,
    ) -> None:
        for mutate in (
            lambda evidence: evidence.append(copy.deepcopy(evidence[0])),
            lambda evidence: evidence[0].update(type="stderr"),
            lambda evidence: evidence[2].update(path="pyproject.toml"),
            lambda evidence: evidence[2].pop("size_bytes"),
        ):
            with self.subTest(mutate=mutate):
                receipt = valid_receipt()
                mutate(receipt["rows"][0]["evidence"])
                receipt["rows"][0]["row_sha256"] = sha256_bytes(
                    canonical_receipt_row_payload(receipt["rows"][0])
                )
                receipt["receipt_sha256"] = sha256_bytes(
                    canonical_receipt_payload(receipt)
                )
                _, violations = validate_receipt(
                    receipt,
                    identity=identity(),
                    ledger_rows=[ledger_row()],
                )
                self.assertTrue(
                    any(
                        violation.rule
                        in {
                            "duplicate row evidence",
                            "artifact declaration mismatch",
                            "invalid row evidence",
                        }
                        for violation in violations
                    ),
                    "\n" + "\n".join(map(str, violations)),
                )

    def test_row_digest_and_clean_status_digests_are_enforced(self) -> None:
        receipt = valid_receipt()
        receipt["rows"][0]["stdout_sha256"] = "9" * 64
        receipt["worktree"]["status_after_sha256"] = DIGEST
        receipt["receipt_sha256"] = sha256_bytes(canonical_receipt_payload(receipt))
        _, violations = validate_receipt(
            receipt,
            identity=identity(),
            ledger_rows=[ledger_row()],
        )
        rules = {violation.rule for violation in violations}
        self.assertIn("receipt row digest mismatch", rules)
        self.assertIn("dirty proof status digest", rules)

    def test_detached_github_attestation_verification_is_policy_bound(self) -> None:
        runner = Mock(
            return_value=subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout=(
                    '[{"attestation": {}, "verificationResult": '
                    '{"signature": {"certificate": {}}, '
                    '"statement": {"subject": [{"digest": {"sha256": "abc"}}]}}}]'
                ),
                stderr="",
            )
        )
        violations = verify_github_attestation(
            Path("/tmp/receipt.json"),
            bundle_path=Path("/tmp/receipt.bundle.jsonl"),
            repository=REPOSITORY,
            signer_workflow="FlexNetOS/nu_plugin/.github/workflows/proof.yml",
            source_digest=COMMIT,
            runner=runner,
        )
        self.assertEqual([], violations)
        command = runner.call_args.args[0]
        self.assertEqual("gh", command[0])
        self.assertIn("--bundle", command)
        self.assertIn("--repo", command)
        self.assertIn("--signer-workflow", command)
        self.assertIn("--source-digest", command)
        self.assertIn("--deny-self-hosted-runners", command)

    def test_failed_or_empty_detached_attestation_fails_closed(self) -> None:
        for completed in [
            subprocess.CompletedProcess([], 1, "", "signature rejected"),
            subprocess.CompletedProcess([], 0, "[]", ""),
            subprocess.CompletedProcess([], 0, "not-json", ""),
            subprocess.CompletedProcess([], 0, "[{}]", ""),
        ]:
            with self.subTest(completed=completed):
                violations = verify_github_attestation(
                    Path("/tmp/receipt.json"),
                    bundle_path=Path("/tmp/receipt.bundle.jsonl"),
                    repository=REPOSITORY,
                    signer_workflow="FlexNetOS/nu_plugin/.github/workflows/proof.yml",
                    source_digest=COMMIT,
                    runner=Mock(return_value=completed),
                )
                self.assertTrue(violations)


if __name__ == "__main__":
    unittest.main()

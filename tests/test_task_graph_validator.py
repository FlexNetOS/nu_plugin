#!/usr/bin/env python3

from __future__ import annotations

import csv
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

from validate_task_graph import audit_task_graph  # noqa: E402


CONTRACT_FILES = [
    "execution/TASK_GRAPH.csv",
    "execution/TASK_FILE_MAP.csv",
    "execution/BIDIRECTIONAL_TASK_GRAPH.csv",
    "execution/BIDIRECTIONAL_TASK_FILE_MAP.csv",
    "execution/POLYGLOT_TASK_GRAPH.csv",
    "execution/POLYGLOT_TASK_FILE_MAP.csv",
    "execution/REQUIREMENT_PROOF_LEDGER.csv",
    "execution/REQUIREMENT_SOURCE_RECEIPTS.json",
    "manifests/PACKAGE_VALIDATION.json",
    "manifests/PACK_MANIFEST.json",
]


def copy_contract(root: Path) -> None:
    for relative in CONTRACT_FILES:
        source = ROOT / relative
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)

    # Structure mode requires every declared current artifact to exist. Copying
    # the repository is unnecessary; empty sentinels preserve the path contract.
    with (root / "execution/TASK_GRAPH.csv").open(
        newline="", encoding="utf-8"
    ) as handle:
        rows = list(csv.DictReader(handle))
    for row in rows:
        for value in row["current_artifact_paths"].split(";"):
            relative = value.strip()
            if not relative:
                continue
            target = root / relative
            if relative.endswith("/"):
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.touch(exist_ok=True)


def mutate_csv(
    root: Path,
    relative: str,
    task_id: str,
    updates: dict[str, str],
) -> None:
    path = root / relative
    with path.open(newline="", encoding="utf-8") as handle:
        reader = csv.DictReader(handle)
        fieldnames = list(reader.fieldnames or [])
        rows = list(reader)
    for row in rows:
        if row.get("task_id") == task_id:
            row.update(updates)
            break
    else:
        raise AssertionError(f"missing fixture row: {task_id}")
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def rules(issues: list[object]) -> set[str]:
    return {getattr(issue, "rule") for issue in issues}


class TaskGraphValidatorTest(unittest.TestCase):
    def test_repository_structure_mode_passes(self) -> None:
        issues = audit_task_graph(ROOT, release=False)
        self.assertEqual([], issues, "\n" + "\n".join(map(str, issues)))

    def test_both_modes_are_read_only(self) -> None:
        before = {
            relative: (ROOT / relative).read_bytes() for relative in CONTRACT_FILES
        }
        audit_task_graph(ROOT, release=False)
        audit_task_graph(ROOT, release=True)
        after = {
            relative: (ROOT / relative).read_bytes() for relative in CONTRACT_FILES
        }
        self.assertEqual(before, after)

    def test_release_mode_fails_closed_on_graph_and_proof_rows(self) -> None:
        issues = audit_task_graph(ROOT, release=True)
        self.assertIn("release graph row incomplete", rules(issues))
        self.assertIn("release proof row incomplete", rules(issues))
        proof_details = [
            issue.detail
            for issue in issues
            if issue.rule == "release proof row incomplete"
        ]
        self.assertTrue(proof_details)
        self.assertFalse(
            any("proof_head_sha=<empty>" in detail for detail in proof_details),
            "empty deprecated proof_head_sha must not block detached-attestation release",
        )

    def test_duplicate_ids_and_graph_map_drift_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)

            graph = root / "execution/POLYGLOT_TASK_GRAPH.csv"
            with graph.open(newline="", encoding="utf-8") as handle:
                reader = csv.DictReader(handle)
                fieldnames = list(reader.fieldnames or [])
                rows = list(reader)
            rows[-1]["task_id"] = rows[0]["task_id"]
            with graph.open("w", newline="", encoding="utf-8") as handle:
                writer = csv.DictWriter(
                    handle, fieldnames=fieldnames, lineterminator="\n"
                )
                writer.writeheader()
                writer.writerows(rows)

            issue_rules = rules(audit_task_graph(root, release=False))
            self.assertIn("duplicate task id", issue_rules)
            self.assertIn("graph-map parity", issue_rules)

    def test_unknown_dependency_and_cycle_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)
            mutate_csv(
                root,
                "execution/TASK_GRAPH.csv",
                "CDB000",
                {"depends_on": "CDB001;CDB999"},
            )

            issue_rules = rules(audit_task_graph(root, release=False))
            self.assertIn("unknown dependency", issue_rules)
            self.assertIn("dependency cycle", issue_rules)

    def test_current_and_future_path_policy_is_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)
            mutate_csv(
                root,
                "execution/TASK_GRAPH.csv",
                "CDB003",
                {
                    "current_artifact_paths": "execution/*.csv",
                    "future_artifact_paths": "future/output.json",
                },
            )
            mutate_csv(
                root,
                "execution/TASK_GRAPH.csv",
                "CDB013",
                {"future_artifact_paths": ""},
            )

            issue_rules = rules(audit_task_graph(root, release=False))
            self.assertIn("current path contains pattern", issue_rules)
            self.assertIn("future paths require planned status", issue_rules)
            self.assertIn("planned task missing future paths", issue_rules)

    def test_missing_current_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)
            mutate_csv(
                root,
                "execution/TASK_GRAPH.csv",
                "CDB003",
                {"current_artifact_paths": "missing/current-proof.log"},
            )
            self.assertIn(
                "current path missing",
                rules(audit_task_graph(root, release=False)),
            )

    def test_required_json_schema_and_bindings_are_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)

            receipts_path = root / "execution/REQUIREMENT_SOURCE_RECEIPTS.json"
            receipts = json.loads(receipts_path.read_text(encoding="utf-8"))
            del receipts["sources"]["CDB106"]
            receipts_path.write_text(
                json.dumps(receipts, indent=2) + "\n", encoding="utf-8"
            )

            manifest_path = root / "manifests/PACK_MANIFEST.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["task_graph_source_of_truth"] = "execution/not-canonical.csv"
            manifest_path.write_text(
                json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
            )

            issue_rules = rules(audit_task_graph(root, release=False))
            self.assertIn("missing source receipt", issue_rules)
            self.assertIn("manifest graph binding", issue_rules)

            receipts_path.write_text("{not-json\n", encoding="utf-8")
            self.assertIn(
                "invalid JSON",
                rules(audit_task_graph(root, release=False)),
            )

    def test_validation_commands_and_evidence_paths_are_required(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            copy_contract(root)
            mutate_csv(
                root,
                "execution/TASK_GRAPH.csv",
                "CDB003",
                {
                    "validation_gate": "",
                    "execution_gate": "",
                    "raw_log_path": "",
                    "evidence_artifacts": "",
                },
            )
            mutate_csv(
                root,
                "execution/POLYGLOT_TASK_FILE_MAP.csv",
                "CDB091",
                {
                    "validation_commands": "",
                    "must_update_on_change": "",
                },
            )

            issue_rules = rules(audit_task_graph(root, release=False))
            self.assertIn("missing validation command", issue_rules)
            self.assertIn("missing evidence path", issue_rules)

    def test_cli_exposes_structure_and_release_modes(self) -> None:
        script = ROOT / "scripts/validate_task_graph.py"
        structure = subprocess.run(
            [sys.executable, str(script), "--root", str(ROOT), "--structure-only"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(0, structure.returncode, structure.stdout + structure.stderr)

        release = subprocess.run(
            [sys.executable, str(script), "--root", str(ROOT), "--release"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertNotEqual(0, release.returncode)
        self.assertIn("release graph row incomplete", release.stderr)
        self.assertIn("release proof row incomplete", release.stderr)


if __name__ == "__main__":
    unittest.main()

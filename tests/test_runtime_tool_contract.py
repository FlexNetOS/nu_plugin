#!/usr/bin/env python3

from __future__ import annotations

import json
import re
import shutil
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONTRACT_FILES = (
    "Cargo.toml",
    "flake.nix",
    "packaging/codedb_runtime_tool.nix",
    "crates/codedb/Cargo.toml",
    "crates/nu_plugin_codedb/Cargo.toml",
)
PACKAGE_EXPORTS = (
    ("default", "codedbRuntimeTools"),
    ("codedb_runtime_tools", "codedbRuntimeTools"),
    ("codedb", "codedbRuntimeTools"),
    ("nu_plugin_codedb", "codedbRuntimeTools"),
)
RUNTIME_COMMANDS = ("codedb", "nu_plugin_codedb")


def _copy_contract(root: Path) -> None:
    for relative in CONTRACT_FILES:
        source = ROOT / relative
        target = root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)


def _read_toml(path: Path) -> dict[str, object]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def _has_nix_assignment(source: str, name: str, value: str) -> bool:
    return (
        re.search(
            rf"(?m)^\s*{re.escape(name)}\s*=\s*{re.escape(value)};\s*$",
            source,
        )
        is not None
    )


def _quoted_nix_list(source: str, name: str) -> tuple[str, ...] | None:
    match = re.search(
        rf"(?ms)^\s*{re.escape(name)}\s*=\s*\[(?P<body>.*?)^\s*\];\s*$",
        source,
    )
    if match is None:
        return None
    return tuple(re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', match["body"]))


def _runtime_metadata(source: str, version: str) -> dict[str, object] | None:
    match = re.search(
        r'(?ms)^\s*cat > "\$out/share/codedb/runtime-tool-metadata\.json" '
        r"<<JSON\s*$\n(?P<body>.*?)^\s*JSON\s*$",
        source,
    )
    if match is None:
        return None
    rendered = textwrap.dedent(match["body"]).replace("${packageVersion}", version)
    return json.loads(rendered)


def audit_runtime_tool_contract(root: Path) -> list[str]:
    issues: list[str] = []
    try:
        workspace = _read_toml(root / "Cargo.toml")
        workspace_version = str(workspace["workspace"]["package"]["version"])
        flake = (root / "flake.nix").read_text(encoding="utf-8")
        package_nix = (root / "packaging/codedb_runtime_tool.nix").read_text(
            encoding="utf-8"
        )
    except (KeyError, OSError, tomllib.TOMLDecodeError) as error:
        return [f"cannot load runtime contract: {error}"]

    for export, value in PACKAGE_EXPORTS:
        if not _has_nix_assignment(flake, export, value):
            issues.append(f"missing flake package export: {export}")

    if not _has_nix_assignment(
        flake,
        "runtimeTools",
        "self.packages.${system}.codedb_runtime_tools",
    ):
        issues.append("runtime smoke does not consume codedb_runtime_tools")
    if not re.search(
        r"(?m)^\s*codedb_runtime_tool_smoke\s*=\s*pkgs\.runCommand\b",
        flake,
    ):
        issues.append("missing flake check export: codedb_runtime_tool_smoke")
    for expected in (
        "${runtimeTools}/bin/codedb --version",
        "test -x ${runtimeTools}/bin/nu_plugin_codedb",
        "${runtimeTools}/share/codedb/runtime-tool-metadata.json",
    ):
        if expected not in flake:
            issues.append(f"runtime smoke is missing assertion: {expected}")

    package_versions = re.findall(
        r'(?m)^\s*packageVersion\s*=\s*"([^"]+)";\s*$', package_nix
    )
    if len(package_versions) != 1:
        issues.append("runtime package must declare exactly one packageVersion")
        package_version = ""
    else:
        package_version = package_versions[0]
        if package_version != workspace_version:
            issues.append(
                "runtime package version "
                f"{package_version!r} does not match workspace version "
                f"{workspace_version!r}"
            )
    if not _has_nix_assignment(package_nix, "version", "packageVersion"):
        issues.append("runtime derivation version does not reference packageVersion")

    package_flags = _quoted_nix_list(package_nix, "cargoPackageFlags")
    if package_flags != ("-p", "codedb", "-p", "nu_plugin_codedb"):
        issues.append(
            "cargoPackageFlags must build exactly codedb and nu_plugin_codedb"
        )

    for command in RUNTIME_COMMANDS:
        if f'"$out/bin/{command}"' not in package_nix:
            issues.append(f"runtime package does not install bin/{command}")

    try:
        metadata = _runtime_metadata(package_nix, package_version)
    except json.JSONDecodeError as error:
        issues.append(f"runtime metadata template is not valid JSON: {error}")
        metadata = None
    expected_metadata = {
        "schema_version": 1,
        "package_name": "codedb-runtime-tools",
        "version": workspace_version,
        "commands": list(RUNTIME_COMMANDS),
        "runtime_tool_source": "bundled",
        "codedb_bin": "$out/bin/codedb",
        "codedb_nu_plugin_bin": "$out/bin/nu_plugin_codedb",
    }
    if metadata is None:
        issues.append("runtime metadata template is missing")
    elif metadata != expected_metadata:
        issues.append(
            "runtime metadata template does not match the package command/version contract"
        )

    passthru_match = re.search(
        r"(?ms)^\s*passthru\.runtimeToolMetadata\s*=\s*\{"
        r"(?P<body>.*?)^\s*\};\s*$",
        package_nix,
    )
    if passthru_match is None:
        issues.append("passthru.runtimeToolMetadata is missing")
    else:
        passthru = passthru_match["body"]
        if _quoted_nix_list(passthru, "commands") != RUNTIME_COMMANDS:
            issues.append("passthru runtime command names do not match installed binaries")
        for name, value in (
            ("YAZELIX_CODEDB_BIN", '"bin/codedb"'),
            ("YAZELIX_CODEDB_PLUGIN_BIN", '"bin/nu_plugin_codedb"'),
        ):
            if not _has_nix_assignment(passthru, name, value):
                issues.append(f"passthru runtime metadata is missing {name}")

    for relative, package_name, binary_name in (
        ("crates/codedb/Cargo.toml", "codedb", "codedb"),
        (
            "crates/nu_plugin_codedb/Cargo.toml",
            "nu_plugin_codedb",
            "nu_plugin_codedb",
        ),
    ):
        try:
            manifest = _read_toml(root / relative)
            package = manifest["package"]
            binaries = manifest["bin"]
            if package["name"] != package_name:
                issues.append(f"{relative} package name must be {package_name}")
            if package.get("version") != {"workspace": True}:
                issues.append(f"{relative} must inherit the workspace version")
            if [binary.get("name") for binary in binaries] != [binary_name]:
                issues.append(f"{relative} must expose the {binary_name} binary")
        except (KeyError, OSError, TypeError, tomllib.TOMLDecodeError) as error:
            issues.append(f"cannot load {relative}: {error}")

    return issues


class RuntimeToolContractTest(unittest.TestCase):
    def test_repository_runtime_tool_contract(self) -> None:
        issues = audit_runtime_tool_contract(ROOT)
        self.assertEqual([], issues, "\n" + "\n".join(issues))

    def test_missing_package_or_check_export_is_rejected(self) -> None:
        mutations = (
            (
                "          codedb_runtime_tools = codedbRuntimeTools;\n",
                "",
                "missing flake package export: codedb_runtime_tools",
            ),
            (
                "          codedb_runtime_tool_smoke = pkgs.runCommand",
                "          removed_runtime_tool_smoke = pkgs.runCommand",
                "missing flake check export: codedb_runtime_tool_smoke",
            ),
        )
        for old, new, expected_issue in mutations:
            with self.subTest(expected_issue=expected_issue):
                with tempfile.TemporaryDirectory() as temp:
                    root = Path(temp)
                    _copy_contract(root)
                    path = root / "flake.nix"
                    source = path.read_text(encoding="utf-8")
                    self.assertEqual(1, source.count(old))
                    path.write_text(source.replace(old, new, 1), encoding="utf-8")
                    self.assertIn(expected_issue, audit_runtime_tool_contract(root))

    def test_runtime_package_version_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            _copy_contract(root)
            path = root / "packaging/codedb_runtime_tool.nix"
            source = path.read_text(encoding="utf-8")
            match = re.search(
                r'(?m)^\s*packageVersion\s*=\s*"(?P<version>[^"]+)";\s*$',
                source,
            )
            self.assertIsNotNone(match)
            assert match is not None
            path.write_text(
                source[: match.start("version")]
                + match["version"]
                + ".mismatch"
                + source[match.end("version") :],
                encoding="utf-8",
            )
            issues = audit_runtime_tool_contract(root)
            self.assertTrue(
                any("does not match workspace version" in issue for issue in issues),
                issues,
            )


if __name__ == "__main__":
    unittest.main()

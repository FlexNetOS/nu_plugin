#!/usr/bin/env python3
"""One-way, idempotent migration for the runtime-context integration slice.

This exists because the host command launcher is currently broken before any
repository process can start. CI applies the exact asserted edit, tests it, and
commits the resulting product source. Delete this migration once the integration
slice is fully landed and its final exact-SHA checks are green.
"""

from pathlib import Path

LIB = Path("crates/codedb_context/src/lib.rs")
MARKER = "pub fn detect_host_triple_with_runner"
APPEND = r'''

/// Detect the active rustc host triple for callers that want a host-target
/// context without inventing or hard-coding a target.
pub fn detect_host_triple() -> Result<String, ContextError> {
    detect_host_triple_with_runner(&SystemCommandRunner, Path::new("."))
}

/// Testable host-triple detector using the same command boundary as full
/// context capture.
pub fn detect_host_triple_with_runner<R: CommandRunner + ?Sized>(
    runner: &R,
    current_dir: &Path,
) -> Result<String, ContextError> {
    let rustc_verbose = checked_output(runner, "rustc", &["-vV".to_string()], current_dir)?;
    rustc_verbose
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty())
        .map(ToOwned::to_owned)
        .ok_or(ContextError::MissingHostTriple)
}
'''


def main() -> None:
    text = LIB.read_text()
    if MARKER in text:
        return
    anchor = "fn sha256_hex(bytes: &[u8]) -> String {\n    format!(\"{:x}\", Sha256::digest(bytes))\n}\n"
    if text.count(anchor) != 1:
        raise SystemExit("codedb_context anchor drifted; refusing migration")
    LIB.write_text(text + APPEND)


if __name__ == "__main__":
    main()

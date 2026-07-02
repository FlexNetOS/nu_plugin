# GOAL

Deliver `nu_plugin_codedb` V1.1: a Rust-native Nushell plugin plus `codedb` CLI/MCP surface that captures the full compiler-observable Rust crate envelope into redb-backed tables/blobs/proof rows.

Nushell is the table cockpit. redb is the first embedded Rust store. Git/source files remain authoritative input for V1.1. Generated crates/files are artifacts. Cargo/rustc/runner prove correctness. Missing observations become `capture_gaps` or `validation_errors`; silent omission is failure.

# GOAL

Deliver `nu_plugin_codedb` V1.1 as the first integrated CodeDB surface: a Rust-native Nushell plugin plus `codedb` CLI/MCP surface that turns files into reproducible database-shaped tables, with the Rust/crate envelope captured into redb-backed tables/blobs/proof rows as a primary specialization.

The plugin is general-purpose infrastructure, not an envctl-only tool. envctl is a downstream consumer and materialization target for CodeDB exports. CodeDB owns file-to-datatable conversion, blob semantics, table checksums, and reproducibility metadata; envctl consumes those rows when it needs to recreate files.

Nushell is the table cockpit. redb is the first embedded Rust store, with room for additional database backends. Git/source files remain authoritative input for V1.1. Generated crates/files are artifacts. Cargo/rustc/runner prove correctness. Missing observations become `capture_gaps` or `validation_errors`; silent omission is failure.

## Mandatory completion invariant

Everything named by the product objective is mandatory. Missing observations must still be recorded, but every such row blocks completion until the positive implementation path is proven. Storage and query behavior must remain database-neutral and pass equivalent redb and PostgreSQL contracts.

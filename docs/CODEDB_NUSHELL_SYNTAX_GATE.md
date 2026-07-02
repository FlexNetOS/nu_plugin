# CodeDB Nushell Syntax Gate

Task `CDB056` validates CodeDB's Nushell-facing files with `nu --no-config-file --ide-check` under an isolated temporary HOME.

The gate is implemented by `tests/test_nushell_syntax_gate.nu` and covers:

- `tests/*.nu`
- `templates/nushell/*.nu`
- `examples/nushell/*.nu`
- `fixtures/nushell_syntax/*.nu`

The test sets `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `YAZELIX_CODEDB_BIN`, and `YAZELIX_CODEDB_PLUGIN_BIN` to temporary fixture paths before each syntax check. It also passes an isolated `--plugin-config` path to avoid any dependency on the operator's real Nushell plugin registry.

`fixtures/nushell_syntax/stub_initializer.nu` is intentionally inert. It mirrors the environment variables used by the Yazelix bridge while proving that syntax validation does not need a real CodeDB binary, a real Nu plugin binary, or real HOME state.

Generated bridge files are not source truth. The source truth for the runtime bridge remains the checked-in templates plus CodeDB's generated manifest/provenance rows.

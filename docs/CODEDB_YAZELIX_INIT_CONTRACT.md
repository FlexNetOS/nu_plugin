# CodeDB Yazelix Init Contract

Generated: 2026-07-02
Task: CDB054

## Contract

`codedb generate-yazelix-bridge --out-dir <dir>` writes generated Nushell bridge artifacts for Yazelix-owned startup state. It does not edit tracked Yazelix `nushell/config/config.nu` and it does not run `plugin add`.

Generated files:

- `codedb_init.nu`
- `codedb_extern.nu`
- `codedb_bridge_manifest.json`

The initializer reads `YAZELIX_CODEDB_BIN` and `YAZELIX_CODEDB_PLUGIN_BIN`, validates that the paths exist, and exports CodeDB status variables for the current Nu session.

The extern file declares lightweight external command surfaces for `codedb` and `nu_plugin_codedb`. Nu plugin registration and `plugin use codedb` remain explicit registration flows proven by CDB052 and CDB053.

## Provenance

The generator emits rows with:

- artifact name
- generated path
- SHA-256 checksum
- `generated = true`
- `manual_edits_allowed = false`
- `mutates_plugin_registry = false`
- source truth `templates`

The JSON manifest repeats artifact checksums and names the source templates.

## Safety Rules

- Generated files are not source truth
- Do not edit tracked Yazelix `config.nu`
- Do not run `plugin add` during generation
- Do not assume CodeDB is required for Yazelix launch
- Re-run generation when package paths or templates change

# CodeDB Yazelix Runtime Tool Package

Generated: 2026-07-02
Task: CDB050

## Package Outputs

The package-local flake exposes `codedb_runtime_tools` as the default package. It installs two commands:

- `bin/codedb`
- `bin/nu_plugin_codedb`

The compatibility package names `codedb` and `nu_plugin_codedb` point at the same runtime-tool output so Yazelix can consume one package while still discovering both binaries.

## Runtime Metadata

The Nix derivation writes:

```text
share/codedb/runtime-tool-metadata.json
```

The metadata declares:

- `schema_version = 1`
- package name `codedb-runtime-tools`
- commands `codedb` and `nu_plugin_codedb`
- source mode `bundled`
- absolute package paths for both binaries

The derivation also exposes `passthru.runtimeToolMetadata` with future Yazelix environment names:

- `YAZELIX_CODEDB_BIN` -> `bin/codedb`
- `YAZELIX_CODEDB_PLUGIN_BIN` -> `bin/nu_plugin_codedb`

## Smoke Contract

The package-level smoke gate is:

```bash
nix build .#checks.$(nix eval --raw --impure --expr builtins.currentSystem).codedb_runtime_tool_smoke --no-link --no-write-lock-file
```

The check verifies:

- `codedb --version` runs from the package output
- `nu_plugin_codedb` is present and executable in the package output
- runtime metadata names the plugin binary
- `codedb --version` reports the package version

Direct package proof:

```bash
runtime_out="$(nix build .#codedb_runtime_tools \
  --no-link --print-out-paths --no-write-lock-file)"
"$runtime_out/bin/codedb" --version
test -x "$runtime_out/bin/nu_plugin_codedb"
```

The Nu plugin protocol smoke belongs to CDB051/CDB052. Running `nu_plugin_codedb` directly outside Nushell is not a valid protocol test.

## Yazelix Integration Boundary

Yazelix should treat this as a bundled runtime tool package. CodeDB remains optional for Yazelix launch until the generated initializer and disabled/enabled smoke tasks prove the runtime path.

Do not use a global install as package proof. Do not mutate user profiles, Home Manager generations, or the user's Nu plugin registry during CDB050.

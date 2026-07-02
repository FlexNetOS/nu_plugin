# Yazelix Placement

Source: PRD sections 16.3 and 24.

## Placement stance

Yazelix hosts CodeDB as a runtime tool. CodeDB is not a Yazelix plugin, not a
Yazelix Zellij plugin owner, not a replacement for Yazelix generated shell
initializers, and not a second owner for Nushell startup configuration.

CodeDB fits into Yazelix as:

- a Nu plugin registered inside Yazelix runtime Nushell;
- a CLI executable reachable from Yazelix shells and popups;
- a Codex sidecar tool launched inside the Yazelix terminal workflow;
- a status/report source if a later Yazelix widget consumes capture status.

Yazelix owns operator flow: Nix runtime closure, Nushell shell surface, Zellij
workspace, Yazi/Helix, popups/status, and Codex terminal workflow. CodeDB must
not bypass Yazelix pane/session ownership.

## Host Nu vs Yazelix runtime Nu

Host Nushell and Yazelix runtime Nushell are separate runtimes. They can have
different binary paths, versions, plugin protocol compatibility, registry files,
environment variables, and HOME/XDG roots.

CodeDB must therefore treat plugin registration as runtime-specific:

| Runtime | Verification |
|---|---|
| Host Nu | `codedb doctor --nu --format json` reports host `nu` path, version, protocol compatibility, and registration guidance. |
| Yazelix runtime Nu | `codedb doctor --yazelix --format json` reports the Yazelix runtime `nu` when discoverable from explicit Yazelix environment variables. |

The CLI remains the fallback bridge when either runtime plugin registry is missing,
degraded, or incompatible. CodeDB must not assume that a host `nu plugin add`
registration proves Yazelix runtime registration, or the reverse.

Plugin registry smoke tests must use a temporary HOME/XDG root. They must not use
the operator's real HOME and must not mutate the tracked Yazelix runtime tree.

## Generated bridge

Allowed artifact class:

```text
CodeDB generated init/extern bridge under Yazelix state/cache/config output paths
```

Forbidden:

```text
tracked nushell/config/config.nu direct mutation
```

Generated init/extern bridge files must be declared outputs with checksums and
provenance. They may live under Yazelix state/cache/config output paths only when
Yazelix or envctl owns that materialization step. CodeDB may publish the rows and
artifacts needed for that step, but the package must not directly edit tracked
Yazelix config as an install side effect.

## Ownership boundaries

| Surface | Owner | CodeDB role |
|---|---|---|
| Yazelix runtime closure | Yazelix | Packaged tool input only. |
| Yazelix Nushell startup config | Yazelix/envctl | Publish generated bridge rows; do not edit tracked config. |
| CodeDB Rust/crate facts | CodeDB | Authoritative datatable export. |
| CodeDB Nu plugin registry entry | Runtime-specific Nu registry | Register only in explicit temp/runtime scopes. |
| Codex terminal workflow | Yazelix/Codex | Use bounded CLI/MCP sidecar. |
| Zellij pane/session layout | Yazelix | No CodeDB ownership. |

## Validation gates

- Host/runtime Nu protocol checks.
- Temporary plugin registry smoke.
- No real HOME mutation.
- Yazelix launch disabled/enabled smoke.
- Generated initializer checksum/provenance.

## Disabled Launch Smoke

CodeDB must be optional for Yazelix launch. `tests/test_yazelix_disabled_smoke.nu`
models a Yazelix-managed Nu startup with `IN_YAZELIX_SHELL` and
`YAZELIX_RUNTIME_DIR` set, generates the CodeDB bridge under a temporary
Yazelix-like state directory, then sources the bridge with `YAZELIX_CODEDB_BIN`
and `YAZELIX_CODEDB_PLUGIN_BIN` unset.

The expected disabled-mode behavior is explicit:

- the Nu launch probe reaches `status: ready`
- `CODEDB_CLI_STATUS` is `missing:YAZELIX_CODEDB_BIN`
- `CODEDB_NU_PLUGIN_STATUS` is `missing:YAZELIX_CODEDB_PLUGIN_BIN`
- no `CODEDB_BIN` or `CODEDB_NU_PLUGIN_BIN` path is exported
- no plugin registration is attempted
- no real HOME or tracked Yazelix runtime config is mutated

## Enabled Launch Smoke

`tests/test_yazelix_enabled_smoke.nu` models the complementary enabled mode. It
builds the CodeDB CLI and Nu plugin, points `YAZELIX_CODEDB_BIN` and
`YAZELIX_CODEDB_PLUGIN_BIN` at those binaries, sources the generated bridge in a
temporary Yazelix-like Nu launch, and verifies startup remains light.

The expected enabled-mode behavior is:

- the Nu launch probe reaches `status: ready`
- `CODEDB_CLI_STATUS` is `available`
- `CODEDB_NU_PLUGIN_STATUS` is `available`
- `CODEDB_BIN` and `CODEDB_NU_PLUGIN_BIN` are exported to the explicit runtime paths
- no `plugin add`, `plugin use`, or registry creation happens during launch
- no tracked Yazelix `config.nu` edit is needed

# Nushell Deep Research + FlexNetOS/yazelix Cross-Reference

Generated: 2026-07-01
Scope: current Nushell behavior, plugin integration, and live `FlexNetOS/yazelix` repository surfaces under `nushell/`, packaging, runtime, Home Manager, and validators.

## Executive verdict

Yazelix already treats Nushell as a curated runtime surface, not as the core runtime owner. The tracked `nushell/config/config.nu` is intentionally small, guarded, generated-initializer based, and extern-oriented. The right CodeDB path is therefore:

```text
nu_plugin_codedb binary
+ codedb CLI/MCP sidecar
+ generated Yazelix Nushell initializer/extern bridge
+ packaged runtime tool integration
+ clean-shell proof through runner
```

Do not patch `nushell/config/config.nu` directly as the durable fix. Add CodeDB support through generated initializer/extern files and package/runtime manifests.

## Researched Nushell facts that matter

- Nushell plugins communicate through the versioned `nu-plugin` protocol and must match the Nushell-provided plugin version.
- Plugin binaries must be added to the plugin registry with `plugin add`; plugin file names must start with `nu_plugin_`.
- `plugin use` imports the plugin into the current session; previously registered plugins are auto-loaded at startup.
- For controlled execution without persistent registry state, Nu supports `nu --plugins '[./path/to/plugin]'`.
- Plugins are separate executables launched by Nu over stdin/stdout or local sockets, with JSON or MessagePack encoding.
- Nu can directly load tables from formats including CSV, JSON, NUON, TOML, XML, Excel, and SQLite databases.
- Nu startup order is `env.nu`, `config.nu`, vendor autoload, user autoload, then `login.nu`.

## FlexNetOS/yazelix facts found

- The repository packaging includes the entire `nushell` root as a runtime input and links it into the built runtime.
- The runtime tool registry bundles Nushell as a first-class runtime tool with command `nu`.
- The generated `yzx` wrapper prepends the packaged Nix Nushell to `PATH`.
- Runtime environment resolution sets `YAZELIX_NU_BIN` to the bundled runtime `nu` if available, otherwise to host `nu`.
- The tracked `nushell/config/config.nu` is minimal, guarded by `IN_YAZELIX_SHELL` or `YAZELIX_RUNTIME_DIR`, imports standard modules, disables banner, sources generated initializer files, clears right prompt, defines a tiny alias/function surface, and uses a generated extern bridge instead of loading the full Yazelix Nushell command implementation path.
- `.gitignore` excludes `nushell/initializers`, confirming generated/local initializer state is intentionally untracked.
- The maintainer validator has `validate-nushell-syntax`; it recursively collects `.nu` files under `nushell/`, runs `nu --no-config-file --ide-check 100`, sets a temp HOME and `IN_YAZELIX_SHELL=1`, and stubs the generated initializer files so syntax checks do not depend on a real user home.
- `.nu-lint.toml` exists and intentionally disables noisy/high-churn lint groups while preserving higher-signal safety/correctness checks.
- Home Manager activation runs `yzx_control generate_shell_initializers` after runtime materialization and terminal generation.

## CodeDB integration conclusion

### Best fit

CodeDB should fit as:

```text
Yazelix package/runtime tool -> nu_plugin_codedb + codedb CLI
Yazelix generated initializer -> optional plugin path/export/extern wiring
Nushell plugin -> interactive table cockpit
Codex/MCP -> bounded non-interactive agent interface
runner -> proof gates
```

### CDB049 current-source verification

Verified on 2026-07-02 against `/home/flexnetos/FlexNetOS/src/yazelix`:

- `packaging/runtime_tool_registry.nix` declares `nushell` as a bundled runtime tool with command `nu`.
- `packaging/mk_runtime_tree.nix` links the repo `nushell/` tree into the runtime and makes the generated `yzx` wrapper put packaged `nu` on `PATH`.
- `shells/posix/runtime_env.sh` exports `YAZELIX_NU_BIN` from `$runtime_dir/libexec/nu` when present, then falls back to host `nu`.
- `shells/posix/yazelix_nu.sh` writes `$YAZELIX_STATE_DIR/generated/nushell/config.nu`, sources the managed config, optional user `shell_nu.nu`, and stack prompt guard, then executes `"$YAZELIX_NU_BIN" --login --env-config /dev/null --config "$generated_config"`.
- `nushell/config/config.nu` sources generated `yazelix_init.nu` and `yazelix_extern.nu`; it remains the thin runtime startup bridge and should not become CodeDB's durable install surface.
- `rust_core/yazelix_core/src/initializer_commands.rs` generates `~/.local/share/yazelix/initializers/nushell/yazelix_init.nu` and tool initializer files, including Nu-specific `carapace` and `zoxide` shell names.
- `home_manager/runtime_integration.nix` runs `yzx_control generate_shell_initializers` during activation after runtime materialization.
- `docs/contracts/rust_nushell_bridge_contract.md` defines generated extern ownership as Rust metadata/startup glue, not public command business logic.

This confirms the safe bridge shape: CodeDB should be packaged beside Yazelix runtime tools, exposed to Nu through generated or transient plugin wiring, and tested without mutating tracked Yazelix config or the user's persistent Nu registry.

### Not a fit

CodeDB should not be implemented as:

- a direct edit to `nushell/config/config.nu`,
- a Zellij wasm plugin,
- a meta plugin first,
- an envctl submodule that owns code truth,
- a Nu script-only implementation pretending to capture compiler truth.

## Execution-package checklist additions

1. Add `YAZELIX_NUSHELL_RUNTIME.md` explaining runtime `nu`, `YAZELIX_NU_BIN`, generated initializers, and extern bridge.
2. Add `CODEDB_NU_PLUGIN_REGISTRATION.md` with three modes:
   - transient: `nu --plugins '[.../nu_plugin_codedb]'`
   - user registry: `plugin add`, then `plugin use codedb`
   - Yazelix generated initializer/extern bridge.
3. Add `CODEDB_YAZELIX_RUNTIME_TOOL.md` defining bundled/host/off policy, package path, `YAZELIX_CODEDB_BIN`, and `YAZELIX_CODEDB_PLUGIN_BIN`.
4. Add task rows for:
   - Nu version/protocol compatibility check,
   - host Nu vs Yazelix runtime Nu compatibility,
   - plugin registry isolation under temp HOME,
   - generated initializer checksum/provenance,
   - syntax validator extension,
   - linter alignment with `.nu-lint.toml`,
   - clean-shell `nu --plugins` smoke,
   - Yazelix launch smoke with CodeDB disabled and enabled.
5. Add runner gates:
   - `nu --version`,
   - `plugin add` in temp HOME,
   - `plugin use codedb`,
   - `codedb doctor --format nuon`,
   - `codedb scan --read-only`,
   - before/after Git status unchanged.

## Risks

- Nu plugin protocol mismatch between host Nu and Yazelix runtime Nu.
- Plugin registry pollution if tests use the real user HOME.
- redb locks if the plugin runs long-lived under Nu plugin GC.
- Source/secret leakage through plugin stderr, traces, or MCP.
- Overloading `config.nu` and slowing all Yazelix shells.

## Recommended next task

Update the CodeDB execution-package checklist and task graph with a `Yazelix Nushell Bridge` section and add dedicated task IDs for plugin registration, transient plugin smoke, generated initializer contract, runtime Nu compatibility, and no-mutation proof.

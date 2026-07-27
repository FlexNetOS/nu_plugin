# Nushell/Yazelix Cross-Reference Report

Generated: 2026-07-27
Task: CDB049
Source checked: `/home/flexnetos/meta/src/yazelix` at `6762324414c5e7515f8d424cde6d4c3bd44e7270`

## Executive verdict

Yazelix owns Nushell process launch, packaged runtime selection, layered startup
configuration, and profile state directories. Nushell remains the interactive
table and plugin host; CodeDB should integrate as an optional packaged CLI and
`nu_plugin_codedb` executable. The bridge must not make CodeDB the owner of
Yazelix startup configuration or profile state.

## Runtime `nu` boundary

The packaged `runtime/yzx-nu.rs` launcher is the runtime boundary:

- it resolves `YAZELIX_CONFIG_HOME` (then `XDG_CONFIG_HOME`/`HOME`) for user Nu
  files (`runtime/yzx-nu.rs:27-30,75-81`);
- it resolves `YAZELIX_STATE_DIR` (then `XDG_RUNTIME_DIR/yazelix`) and creates a
  runtime-local `nu/` directory (`runtime/yzx-nu.rs:31-35,83-95`);
- it layers packaged and user `env.nu`/`config.nu` files into that directory,
  using `source-env` for environment setup and `source` for config
  (`runtime/yzx-nu.rs:47-61,97-115`);
- it execs the substituted packaged `nu` with those explicit config paths and a
  runtime-prefixed `PATH` (`runtime/yzx-nu.rs:63-71,159-169`).

The Nix package substitutes the Nushell executable, packaged config, and PATH
prefix into this launcher (`flake.nix:276-281`). The LifeOS foundation adds its
tool bundle through `extraPathPrefix` and selects its layered Nu config
(`flake.nix:1401-1410`). CodeDB should therefore discover the runtime through
the explicit Yazelix package/environment contract rather than assuming that
host `nu` and runtime `nu` share a registry or protocol version.

## Config and initializer boundary

The current checkout does not use the older `yazelix_init.nu` or
`yazelix_extern.nu` generated-initializer paths. Its durable source files are:

- `defaults/nu/config.nu`, which sources Nix-generated carapace/zoxide snippets
  and configures the prompt/banner (`defaults/nu/config.nu:1-10`);
- `nushell/config/config.nu`, which is Nix-substituted with the tracked
  `stack_prompt_guard.nu` and `scripts/flexnetos_init.nu`, then establishes the
  profile shell and runtime/cache environment (`nushell/config/config.nu:1-45`);
- `nushell/system/profile_environment_frontdoor.nu`, which sets
  `YAZELIX_CONFIG_HOME`, XDG roots, `YAZELIX_STATE_DIR`, and `SHELL` only for
  the real profile home (`nushell/system/profile_environment_frontdoor.nu:1-49`);
- `nushell/scripts/flexnetos_init.nu`, the tracked FlexNetOS initialization
  layer referenced by the substituted config.

The Nix build materializes these sources into the foundation and checks their
presence and source relationship (`flake.nix:1419-1424,2217-2225`). This is the
initializer boundary for the current source: generated runtime config is
launch-local and Nix-owned, while the tracked Nu files remain source inputs.
CodeDB must not patch `nushell/config/config.nu`, `defaults/nu/config.nu`, or
the generated runtime files as an installation side effect.

## CodeDB integration shape

The safe placement is:

```text
Yazelix packaged runtime
  -> packaged nu + layered runtime config
  -> optional CodeDB CLI/plugin on the runtime PATH
  -> transient or explicitly isolated Nu registration
  -> Nushell tables and pipelines
```

Recommended registration modes remain:

1. Transient proof mode: use `nu --plugins` with a temporary HOME/XDG state
   root when supported by the selected Nushell version.
2. Explicit user registry mode: require the user to run `plugin add`/`plugin
   use`; never perform this against the real HOME in package tests.
3. Yazelix package mode: expose the CodeDB binaries through the package PATH
   or an explicitly generated, provenance-bearing bridge owned by Yazelix.

The host-Nu and packaged-runtime-Nu versions must be checked separately. A
protocol mismatch should degrade to the CodeDB CLI, with no startup-config or
registry mutation.

## Ownership and safety

- CodeDB owns typed source capture, compiler/Cargo evidence, blobs, and query
  semantics.
- Nushell owns table display, pipelines, filtering, joins, and interactive
  composition.
- Yazelix owns the packaged runtime, launcher, layered config, PATH, and
  profile state roots.
- envctl may consume CodeDB exports and materialize environment/config outputs;
  it does not own CodeDB source truth.

For CDB049, no runtime session was started, no real HOME plugin registry was
modified, and no Yazelix source was changed. Follow-on work must use temporary
state and prove that the tracked config and profile-owned state are unchanged.

## Conclusion

The Yazelix/Nu bridge is understood: the runtime `nu` boundary is
`runtime/yzx-nu.rs`, the config boundary is the layered Nix-substituted
`env.nu`/`config.nu` pair, and the initializer boundary is the tracked
`flexnetos_init.nu` plus generated launch-local config. CodeDB belongs beside
that boundary as an optional runtime tool/plugin, not inside the durable
Yazelix startup implementation.

# Nushell Plugin Compatibility

Source: PRD section 14 and research report.

## Boundary

`nu_plugin_codedb` must not assume one global Nu registry. Host Nushell and Yazelix runtime Nushell may differ by version, path, plugin protocol, and registry location.

## Required checks

| Check | Evidence |
|---|---|
| Host Nu path/version | `codedb doctor --nu` |
| Yazelix runtime Nu path/version | `codedb doctor --yazelix`, preferring `YAZELIX_NU_BIN` |
| Plugin protocol compatibility | doctor row with compatible/degraded/refused status |
| Transient plugin smoke | `nu --plugins` or equivalent temp invocation |
| Temp-HOME registry smoke | proves no real HOME mutation |

## Failure behavior

Incompatibility must degrade clearly: CLI remains usable, plugin registration is refused or marked degraded, and no tracked Yazelix config is mutated.

## CDB051 behavior

`codedb doctor --nu --yazelix` reports host and Yazelix Nu separately. Host Nu is discovered from `PATH`. Yazelix Nu is discovered from explicit runtime variables in this order:

1. `YAZELIX_NU_BIN`
2. `YAZELIX_NU_PATH`
3. `YAZELIX_RUNTIME_NU`
4. `YZX_NU`
5. `YAZELIX_TOOLBIN/nu`

Each discovered Nu path gets rows for path, version, plugin protocol compatibility, plugin binary path, and registration status. Missing runtime Nu or version mismatch is reported as `degraded`; the doctor command does not mutate the user's plugin registry.

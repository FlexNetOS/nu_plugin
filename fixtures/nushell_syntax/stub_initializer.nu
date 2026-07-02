# CodeDB syntax-gate fixture.
#
# This file is intentionally inert: it gives the syntax gate a Yazelix-like
# initializer surface without touching the operator's HOME, plugin registry, or
# runtime config.

export-env {
    $env.CODEDB_BIN = ($env.YAZELIX_CODEDB_BIN? | default "/tmp/codedb-syntax-stub/codedb")
    $env.CODEDB_NU_PLUGIN_BIN = ($env.YAZELIX_CODEDB_PLUGIN_BIN? | default "/tmp/codedb-syntax-stub/nu_plugin_codedb")
    $env.CODEDB_CLI_STATUS = "syntax_stub"
    $env.CODEDB_NU_PLUGIN_STATUS = "syntax_stub"
    $env.CODEDB_YAZELIX_BRIDGE_MODE = "syntax_only"
}

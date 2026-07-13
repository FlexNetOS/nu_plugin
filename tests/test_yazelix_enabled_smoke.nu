# Test lane: default
# Defends: Yazelix-like Nu launch remains ready when CodeDB bridge paths are present without plugin registration.

def fail [message: string] {
    error make { msg: $message }
}

def run_checked [command: string, args: list<string>] {
    let result = (^$command ...$args | complete)
    if $result.exit_code != 0 {
        fail $"command failed: ($command) ($args | str join ' ')\nstdout:\n($result.stdout)\nstderr:\n($result.stderr)"
    }
    $result.stdout
}

def main [] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    let cargo_dir = ($env.CODEDB_TEST_CARGO_DIR? | default "")
    let path = if $cargo_dir == "" {
        $env.PATH
    } else {
        $env.PATH | prepend $cargo_dir
    }

    cd $repo_root

    with-env { PATH: $path } {
        run_checked cargo [build --quiet -p codedb -p nu_plugin_codedb] | ignore

        let target_dir = ($env.CARGO_TARGET_DIR? | default ([$repo_root target] | path join))
        let codedb_bin = ([$target_dir debug codedb] | path join)
        let plugin_bin = ([$target_dir debug nu_plugin_codedb] | path join)
        if not ($codedb_bin | path exists) {
            fail $"expected codedb binary was not built: ($codedb_bin)"
        }
        if not ($plugin_bin | path exists) {
            fail $"expected nu_plugin_codedb binary was not built: ($plugin_bin)"
        }

        let temp_home = (mktemp -d)
        let state_dir = ([$temp_home .local share yazelix] | path join)
        let generated_dir = ([$state_dir initializers nushell] | path join)
        mkdir $generated_dir

        let bridge_rows = (
            run_checked cargo [
                run
                --quiet
                -p
                codedb
                --
                generate-yazelix-bridge
                --out-dir
                $generated_dir
                --format
                json
            ]
            | from json
        )

        let init_path = ([$generated_dir codedb_init.nu] | path join)
        if not ($init_path | path exists) {
            fail $"generated enabled-mode init path missing: ($init_path)"
        }

        let launch_probe = ([$temp_home enabled_launch_probe.nu] | path join)
        [
            $"source '($init_path)'"
            ""
            "{"
            "    status: ready,"
            "    codedb_cli_status: ($env.CODEDB_CLI_STATUS? | default ''),"
            "    codedb_plugin_status: ($env.CODEDB_NU_PLUGIN_STATUS? | default ''),"
            "    codedb_bin: ($env.CODEDB_BIN? | default ''),"
            "    codedb_plugin_bin: ($env.CODEDB_NU_PLUGIN_BIN? | default ''),"
            "    bridge_mode: ($env.CODEDB_YAZELIX_BRIDGE_MODE? | default ''),"
            "    plugin_registry_exists: (([$env.HOME .config nushell] | path join) | path exists),"
            "}"
            "| to json"
        ]
        | str join "\n"
        | save -f $launch_probe

        let output = (
            with-env {
                HOME: $temp_home,
                XDG_CONFIG_HOME: ([$temp_home .config] | path join),
                XDG_DATA_HOME: ([$temp_home .local share] | path join),
                XDG_CACHE_HOME: ([$temp_home .cache] | path join),
                IN_YAZELIX_SHELL: "1",
                YAZELIX_RUNTIME_DIR: ([$temp_home runtime] | path join),
                YAZELIX_CODEDB_BIN: $codedb_bin,
                YAZELIX_CODEDB_PLUGIN_BIN: $plugin_bin,
            } {
                hide-env --ignore-errors CODEDB_BIN CODEDB_NU_PLUGIN_BIN
                run_checked nu [--no-config-file $launch_probe]
            }
        )

        let launch = ($output | from json)
        if $launch.status != "ready" {
            fail $"enabled launch did not reach ready: ($output)"
        }
        if $launch.codedb_cli_status != "available" {
            fail $"unexpected enabled CLI status: ($launch.codedb_cli_status)"
        }
        if $launch.codedb_plugin_status != "available" {
            fail $"unexpected enabled plugin status: ($launch.codedb_plugin_status)"
        }
        if $launch.codedb_bin != $codedb_bin {
            fail $"unexpected enabled CODEDB_BIN: ($launch.codedb_bin)"
        }
        if $launch.codedb_plugin_bin != $plugin_bin {
            fail $"unexpected enabled CODEDB_NU_PLUGIN_BIN: ($launch.codedb_plugin_bin)"
        }
        if $launch.bridge_mode != "generated-state" {
            fail $"unexpected bridge mode: ($launch.bridge_mode)"
        }
        if $launch.plugin_registry_exists != false {
            fail "enabled launch smoke created a Nushell plugin registry"
        }

        {
            status: passed,
            enabled_mode_safe: true,
            launch_status: $launch.status,
            codedb_cli_status: $launch.codedb_cli_status,
            codedb_plugin_status: $launch.codedb_plugin_status,
            bridge_rows: ($bridge_rows | length),
            plugin_registry_created: $launch.plugin_registry_exists,
            generated_dir: $generated_dir,
        }
    }
}

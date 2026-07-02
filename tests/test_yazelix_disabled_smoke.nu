# Test lane: default
# Defends: Yazelix-like Nu launch remains ready when CodeDB is disabled or absent.

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
        run_checked cargo [build --quiet -p codedb] | ignore

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
            fail $"generated disabled-mode init path missing: ($init_path)"
        }

        let launch_probe = ([$temp_home disabled_launch_probe.nu] | path join)
        [
            $"source '($init_path)'"
            ""
            "{"
            "    status: ready,"
            "    in_yazelix_shell: ($env.IN_YAZELIX_SHELL? | default ''),"
            "    yazelix_runtime_dir: ($env.YAZELIX_RUNTIME_DIR? | default ''),"
            "    codedb_cli_status: ($env.CODEDB_CLI_STATUS? | default ''),"
            "    codedb_plugin_status: ($env.CODEDB_NU_PLUGIN_STATUS? | default ''),"
            "    codedb_bin_present: ('CODEDB_BIN' in $env),"
            "    codedb_plugin_bin_present: ('CODEDB_NU_PLUGIN_BIN' in $env),"
            "    bridge_mode: ($env.CODEDB_YAZELIX_BRIDGE_MODE? | default ''),"
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
            } {
                hide-env --ignore-errors YAZELIX_CODEDB_BIN YAZELIX_CODEDB_PLUGIN_BIN CODEDB_BIN CODEDB_NU_PLUGIN_BIN
                run_checked nu [--no-config-file $launch_probe]
            }
        )

        let launch = ($output | from json)
        if $launch.status != "ready" {
            fail $"disabled launch did not reach ready: ($output)"
        }
        if $launch.codedb_cli_status != "missing:YAZELIX_CODEDB_BIN" {
            fail $"unexpected disabled CLI status: ($launch.codedb_cli_status)"
        }
        if $launch.codedb_plugin_status != "missing:YAZELIX_CODEDB_PLUGIN_BIN" {
            fail $"unexpected disabled plugin status: ($launch.codedb_plugin_status)"
        }
        if $launch.codedb_bin_present != false {
            fail "disabled launch exported CODEDB_BIN"
        }
        if $launch.codedb_plugin_bin_present != false {
            fail "disabled launch exported CODEDB_NU_PLUGIN_BIN"
        }
        if $launch.bridge_mode != "generated-state" {
            fail $"unexpected bridge mode: ($launch.bridge_mode)"
        }

        {
            status: passed,
            disabled_mode_safe: true,
            launch_status: $launch.status,
            codedb_cli_status: $launch.codedb_cli_status,
            codedb_plugin_status: $launch.codedb_plugin_status,
            bridge_rows: ($bridge_rows | length),
            generated_dir: $generated_dir,
        }
    }
}

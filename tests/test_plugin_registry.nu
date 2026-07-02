# Test lane: default
# Defends: `plugin add` and `plugin use` work in an isolated HOME without touching the user's registry.

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

def plugin_registry_snapshot [home: string] {
    let nushell_dir = ([$home .config nushell] | path join)
    if not ($nushell_dir | path exists) {
        return []
    }

    glob ([$nushell_dir "*plugin*"] | path join)
    | sort
    | each {|path|
        let raw = (open --raw $path)
        {
            path: $path,
            sha256: ($raw | hash sha256),
        }
    }
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
        run_checked cargo [build --quiet -p nu_plugin_codedb] | ignore

        let plugin = ([$repo_root target debug nu_plugin_codedb] | path join)
        if not ($plugin | path exists) {
            fail $"expected plugin binary was not built: ($plugin)"
        }

        let real_home = ($env.HOME? | default "")
        let real_before = if $real_home == "" { [] } else { plugin_registry_snapshot $real_home }
        let temp_home = (mktemp -d)
        let temp_plugin_config = ([$temp_home plugins.msgpackz] | path join)
        let temp_config_dir = ([$temp_home .config] | path join)
        let temp_data_dir = ([$temp_home .local share] | path join)
        let temp_cache_dir = ([$temp_home .cache] | path join)

        with-env {
            HOME: $temp_home,
            XDG_CONFIG_HOME: $temp_config_dir,
            XDG_DATA_HOME: $temp_data_dir,
            XDG_CACHE_HOME: $temp_cache_dir,
        } {
            run_checked nu [
                --no-config-file
                -c
                $"plugin add --plugin-config '($temp_plugin_config)' '($plugin)'"
            ] | ignore
        }

        if not ($temp_plugin_config | path exists) {
            fail $"plugin add did not create temp plugin registry: ($temp_plugin_config)"
        }

        let registry_hash = (open --raw $temp_plugin_config | hash sha256)
        let output = (
            with-env {
                HOME: $temp_home,
                XDG_CONFIG_HOME: $temp_config_dir,
                XDG_DATA_HOME: $temp_data_dir,
                XDG_CACHE_HOME: $temp_cache_dir,
            } {
                run_checked nu [
                    --no-config-file
                    --plugin-config
                    $temp_plugin_config
                    -c
                    $"plugin use --plugin-config '($temp_plugin_config)' codedb; codedb tables | to json"
                ]
            }
        )

        let rows = ($output | from json)
        if ($rows | length) == 0 {
            fail "plugin registry smoke returned no rows"
        }
        if not (($rows | where table == source_files | length) > 0) {
            fail "plugin registry output did not include source_files table row"
        }

        let real_after = if $real_home == "" { [] } else { plugin_registry_snapshot $real_home }
        if ($real_before | to json --raw) != ($real_after | to json --raw) {
            fail "temp-HOME plugin registry smoke changed real HOME Nushell plugin registry files"
        }

        {
            status: passed,
            row_count: ($rows | length),
            first_table: ($rows | first | get table),
            temp_plugin_config: $temp_plugin_config,
            temp_plugin_config_sha256: $registry_hash,
            plugin: $plugin,
        }
    }
}

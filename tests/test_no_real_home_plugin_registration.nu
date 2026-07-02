# Test lane: default
# Defends: CodeDB plugin registration uses temp HOME/plugin config and leaves the operator's real HOME registry unchanged.

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

def registry_snapshot [home: string] {
    let nushell_dir = ([$home .config nushell] | path join)
    if ($home | str trim | is-empty) or not ($nushell_dir | path exists) {
        return []
    }

    glob ([$nushell_dir "*plugin*"] | path join)
    | sort
    | each {|path|
        let raw = (open --raw $path)
        {
            path_sha256: ($path | hash sha256),
            bytes: ($raw | bytes length),
            sha256: ($raw | hash sha256),
        }
    }
}

def snapshot_report [snapshot: list<any>] {
    {
        file_count: ($snapshot | length),
        snapshot_sha256: ($snapshot | to json --raw | hash sha256),
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
        let real_before = (registry_snapshot $real_home)
        let real_before_report = (snapshot_report $real_before)

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
            fail $"plugin add did not create isolated plugin registry: ($temp_plugin_config)"
        }

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
        if not (($rows | where table == source_files | length) > 0) {
            fail "isolated plugin registration did not expose the source_files table"
        }

        let real_after = (registry_snapshot $real_home)
        let real_after_report = (snapshot_report $real_after)

        if ($real_before | to json --raw) != ($real_after | to json --raw) {
            fail $"isolated plugin registration changed real HOME registry\nbefore: ($real_before_report | to json --raw)\nafter: ($real_after_report | to json --raw)"
        }

        {
            status: passed,
            real_home_registry_unchanged: true,
            real_before: $real_before_report,
            real_after: $real_after_report,
            temp_plugin_config: $temp_plugin_config,
            temp_plugin_config_sha256: (open --raw $temp_plugin_config | hash sha256),
            plugin_table_rows: ($rows | length),
        }
    }
}

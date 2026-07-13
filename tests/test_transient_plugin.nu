# Test lane: default
# Defends: transient `nu --plugins` loads CodeDB without mutating the user's plugin registry.

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

        let target_dir = ($env.CARGO_TARGET_DIR? | default ([$repo_root target] | path join))
        let plugin = ([$target_dir debug nu_plugin_codedb] | path join)
        if not ($plugin | path exists) {
            fail $"expected plugin binary was not built: ($plugin)"
        }

        let real_home = ($env.HOME? | default "")
        let real_before = if $real_home == "" { [] } else { plugin_registry_snapshot $real_home }
        let temp_home = (mktemp -d)
        let temp_plugin_config = ([$temp_home plugins.msgpackz] | path join)

        let output = (
            with-env {
                HOME: $temp_home,
                XDG_CONFIG_HOME: ([$temp_home .config] | path join),
                XDG_DATA_HOME: ([$temp_home .local share] | path join),
                XDG_CACHE_HOME: ([$temp_home .cache] | path join),
            } {
                run_checked nu [
                    --no-config-file
                    --plugin-config
                    $temp_plugin_config
                    --plugins
                    $plugin
                    -c
                    "codedb tables | to json"
                ]
            }
        )

        let rows = ($output | from json)
        if ($rows | length) == 0 {
            fail "transient plugin returned no rows"
        }
        for column in [table status rows note] {
            if not (($rows | first | columns) | any {|name| $name == $column }) {
                fail $"transient plugin first row did not include column: ($column)"
            }
        }
        if not (($rows | where table == source_files | length) > 0) {
            fail "transient plugin output did not include source_files table row"
        }

        let relative_root = (mktemp -d)
        let relative_fixture = ([$relative_root fixture] | path join)
        let relative_cwd = ([$relative_root nested] | path join)
        cp -r ([$repo_root fixtures single_simple_crate] | path join) $relative_fixture
        mkdir $relative_cwd
        run_checked cargo [
            generate-lockfile
            --manifest-path
            ([$relative_fixture Cargo.toml] | path join)
            --offline
        ] | ignore
        let relative_output = (
            with-env {
                HOME: $temp_home,
                XDG_CONFIG_HOME: ([$temp_home .config] | path join),
                XDG_DATA_HOME: ([$temp_home .local share] | path join),
                XDG_CACHE_HOME: ([$temp_home .cache] | path join),
            } {
                run_checked nu [
                    --no-config-file
                    --plugin-config
                    $temp_plugin_config
                    --plugins
                    $plugin
                    -c
                    $"cd '($relative_cwd)'; codedb scan '../fixture' | to json"
                ]
            }
        )
        let relative_rows = ($relative_output | from json)
        if not (($relative_rows | where table == cargo_packages | length) > 0) {
            fail "transient plugin relative scan omitted Cargo package evidence"
        }

        let real_after = if $real_home == "" { [] } else { plugin_registry_snapshot $real_home }
        if ($real_before | to json --raw) != ($real_after | to json --raw) {
            fail "transient plugin smoke changed real HOME Nushell plugin registry files"
        }

        {
            status: passed,
            row_count: ($rows | length),
            relative_row_count: ($relative_rows | length),
            first_table: ($rows | first | get table),
            temp_plugin_config: $temp_plugin_config,
            plugin: $plugin,
        }
    }
}

# Test lane: default
# Defends: CodeDB Nu plugin stderr/stdout and MCP default surfaces do not leak secret-looking fixture values.

def fail [message: string] {
    error make { msg: $message }
}

def forbidden_values [] {
    [
        "sk-placeholder-redacted-not-a-real-key",
        "ghp_placeholder_redacted_not_a_real_token",
    ]
}

def assert_no_forbidden_values [label: string, output: string] {
    let leaked = (forbidden_values | where {|secret| $output | str contains $secret })
    if ($leaked | length) > 0 {
        fail $"($label) leaked secret-looking fixture values"
    }

    {
        label: $label,
        sha256: ($output | hash sha256),
        secret_like_values: absent,
    }
}

def run_cargo_checked [args: list<string>] {
    let result = (^cargo ...$args | complete)
    if $result.exit_code != 0 {
        let combined = $"stdout:\n($result.stdout)\nstderr:\n($result.stderr)"
        assert_no_forbidden_values cargo_failure $combined | ignore
        fail $"cargo command failed: cargo ($args | str join ' ')"
    }
    $result
}

def run_nu_plugin_checked [plugin: string, command: string, home: string] {
    with-env {
        HOME: $home,
        XDG_CONFIG_HOME: ([$home .config] | path join),
        XDG_DATA_HOME: ([$home .local share] | path join),
        XDG_CACHE_HOME: ([$home .cache] | path join),
    } {
        let result = (^nu --no-config-file --plugins $plugin -c $command | complete)
        let combined = $"stdout:\n($result.stdout)\nstderr:\n($result.stderr)"
        if $result.exit_code != 0 {
            assert_no_forbidden_values plugin_failure $combined | ignore
            fail $"nu plugin command failed: ($command)"
        }
        {
            stdout: $result.stdout,
            stderr: $result.stderr,
            combined: $combined,
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
        run_cargo_checked [build --quiet -p nu_plugin_codedb] | ignore

        let plugin = ([$repo_root target debug nu_plugin_codedb] | path join)
        if not ($plugin | path exists) {
            fail $"expected plugin binary was not built: ($plugin)"
        }

        let source_fixture = ([$repo_root fixtures secret_like] | path join)
        let temp_root = (mktemp -d)
        let fixture = ([$temp_root secret_like] | path join)
        cp -r $source_fixture $fixture

        let temp_home = (mktemp -d)
        let scan = (run_nu_plugin_checked $plugin $"codedb scan '($fixture)' | to json" $temp_home)
        let source_files = (run_nu_plugin_checked $plugin $"codedb source files --repo '($fixture)' | to json" $temp_home)
        let rust_items = (run_nu_plugin_checked $plugin $"codedb rust items --repo '($fixture)' | to json" $temp_home)
        let validation_errors = (run_nu_plugin_checked $plugin $"codedb validation errors | to json" $temp_home)

        let mcp = (run_cargo_checked [test -p codedb-mcp --quiet])
        let mcp_combined = $"stdout:\n($mcp.stdout)\nstderr:\n($mcp.stderr)"

        let source_lock = ([$source_fixture Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "plugin secret guard mutated the source fixture Cargo.lock"
        }

        [
            (assert_no_forbidden_values plugin_scan $scan.combined),
            (assert_no_forbidden_values plugin_source_files $source_files.combined),
            (assert_no_forbidden_values plugin_rust_items $rust_items.combined),
            (assert_no_forbidden_values plugin_validation_errors $validation_errors.combined),
            (assert_no_forbidden_values mcp_tests $mcp_combined),
            {
                label: plugin_transport,
                status: passed,
                plugin_path_sha256: ($plugin | hash sha256),
                temp_home: $temp_home,
                scan_rows: (($scan.stdout | from json) | length),
                source_file_rows: (($source_files.stdout | from json) | length),
                rust_item_rows: (($rust_items.stdout | from json) | length),
                validation_error_rows: (($validation_errors.stdout | from json) | length),
            },
        ]
    }
}

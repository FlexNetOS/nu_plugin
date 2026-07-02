# Test lane: default
# Defends: CodeDB default CLI and MCP surfaces do not emit raw secret-looking source values.

def fail [message: string] {
    error make { msg: $message }
}

def run_checked [args: list<string>] {
    let result = (^cargo ...$args | complete)
    if $result.exit_code != 0 {
        fail $"cargo command failed: cargo ($args | str join ' ')\n($result.stderr)"
    }
    $result.stdout
}

def run_codedb [args: list<string>] {
    run_checked ([run --quiet -p codedb --] | append $args)
}

def assert_no_raw_secret_values [label: string, output: string] {
    let forbidden = [
        "sk-placeholder-redacted-not-a-real-key",
        "ghp_placeholder_redacted_not_a_real_token",
    ]

    let leaked = ($forbidden | where {|secret| $output | str contains $secret })
    if ($leaked | length) > 0 {
        fail $"($label) leaked raw secret-looking values: ($leaked | str join ', ')"
    }

    { label: $label, sha256: ($output | hash sha256), raw_secret_values: "absent" }
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
        let source_fixture = ([$repo_root fixtures secret_like] | path join)
        let temp_root = (mktemp -d)
        let fixture = ([$temp_root secret_like] | path join)

        cp -r $source_fixture $fixture

        let mcp_tests = (run_checked [
            test
            -p
            codedb-mcp
            --quiet
        ])

        let scan_output = (run_codedb [scan $fixture --format json])
        let rust_items_output = (run_codedb [export rust_items --repo-path $fixture --format json])
        let checksum_output = (run_codedb [export codedb_table_checksums --repo-path $fixture --format json])
        let envctl_output = (run_codedb [export envctl --repo-path $fixture --format json])

        let source_lock = ([$source_fixture Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "security no-leak test mutated the source fixture Cargo.lock"
        }

        [
            {
                label: mcp_security_tests,
                status: "passed",
                sha256: ($mcp_tests | hash sha256),
            },
            (assert_no_raw_secret_values scan_summary $scan_output),
            (assert_no_raw_secret_values rust_items $rust_items_output),
            (assert_no_raw_secret_values table_checksums $checksum_output),
            (assert_no_raw_secret_values envctl_export $envctl_output),
        ]
    }
}

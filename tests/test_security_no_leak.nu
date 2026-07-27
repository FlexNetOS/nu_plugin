# Test lane: default
# Defends: CodeDB default CLI and MCP surfaces do not emit raw secret-looking source values.

def fail [message: string] {
    error make { msg: $message }
}

def assert_no_raw_secret_values [label: string, output: string] {
    let forbidden = [
        "sk-placeholder-redacted-not-a-real-key",
        "ghp_placeholder_redacted_not_a_real_token",
    ]

    let leaked = ($forbidden | where {|secret| $output | str contains $secret })
    if ($leaked | length) > 0 {
        fail $"($label) leaked raw secret-looking values"
    }

    { label: $label, sha256: ($output | hash sha256), raw_secret_values: "absent" }
}

def run_checked [label: string, args: list<string>] {
    let result = (^cargo ...$args | complete)
    let combined = $"stdout:\n($result.stdout)\nstderr:\n($result.stderr)"
    let proof = (assert_no_raw_secret_values $label $combined)
    if $result.exit_code != 0 {
        fail $"cargo command failed: cargo ($args | str join ' ')"
    }
    {
        stdout: $result.stdout,
        stderr: $result.stderr,
        combined: $combined,
        proof: $proof,
    }
}

def run_codedb [label: string, args: list<string>] {
    run_checked $label ([run --quiet -p codedb --] | append $args)
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
        run_checked cargo_generate_lockfile [
            generate-lockfile
            --manifest-path
            ([$fixture Cargo.toml] | path join)
            --offline
        ] | ignore

        let mcp_tests = (run_checked mcp_security_tests [
            test
            -p
            codedb-mcp
            --quiet
        ])

        let scan = (run_codedb scan_summary [scan $fixture --format json])
        let rust_items = (run_codedb rust_items [export rust_items --repo-path $fixture --format json])
        let table_checksums = (run_codedb table_checksums [export codedb_table_checksums --repo-path $fixture --format json])
        let envctl_export = (run_codedb envctl_export [export envctl --repo-path $fixture --format json])

        let source_lock = ([$source_fixture Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "security no-leak test mutated the source fixture Cargo.lock"
        }

        [
            {
                label: mcp_security_tests,
                status: "passed",
                sha256: ($mcp_tests.combined | hash sha256),
                raw_secret_values: "absent",
            },
            $scan.proof,
            $rust_items.proof,
            $table_checksums.proof,
            $envctl_export.proof,
        ]
    }
}

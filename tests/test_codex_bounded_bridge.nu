# Test lane: default
# Defends: Codex-facing CodeDB CLI and MCP bridge samples stay bounded and raw-source-free by default.

def fail [message: string] {
    error make { msg: $message }
}

def forbidden_values [] {
    [
        "sk-placeholder-redacted-not-a-real-key",
        "ghp_placeholder_redacted_not_a_real_token",
        "PLACEHOLDER_OPENAI_KEY",
        "PLACEHOLDER_GITHUB_TOKEN",
    ]
}

def assert_no_forbidden_values [label: string, output: string] {
    let leaked = (forbidden_values | where {|secret| $output | str contains $secret })
    if ($leaked | length) > 0 {
        fail $"($label) leaked raw source or secret-looking fixture values"
    }
}

def run_cargo_checked [args: list<string>] {
    let result = (^cargo ...$args | complete)
    let combined = $"stdout:\n($result.stdout)\nstderr:\n($result.stderr)"
    assert_no_forbidden_values cargo_output $combined
    if $result.exit_code != 0 {
        fail $"cargo command failed: cargo ($args | str join ' ')"
    }
    $result
}

def run_codedb_json [args: list<string>] {
    let result = (run_cargo_checked ([run --quiet -p codedb --] | append $args))
    $result.stdout | from json
}

def assert_bounded_output [label: string, output: string, max_rows: int, max_bytes: int] {
    assert_no_forbidden_values $label $output
    let bytes = ($output | encode utf-8 | bytes length)
    if $bytes > $max_bytes {
        fail $"($label) exceeded max bytes: ($bytes) > ($max_bytes)"
    }
    let rows = ($output | from json)
    if ($rows | length) > $max_rows {
        fail $"($label) exceeded max rows: (($rows | length)) > ($max_rows)"
    }
    {
        label: $label,
        rows: ($rows | length),
        bytes: $bytes,
        sha256: ($output | hash sha256),
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
        let config_path = ([$repo_root examples codex codedb_mcp_config.json] | path join)
        let config = (open $config_path)
        let server = $config.mcpServers.codedb
        let policy = $config.codedbPolicy

        if $server.command != "/absolute/path/to/codedb" {
            fail "Codex MCP sample command must remain a placeholder path"
        }
        if not (($server.args | to json --raw) | str contains "--default-limit") {
            fail "Codex MCP sample must include --default-limit"
        }
        if not (($server.args | to json --raw) | str contains "--max-bytes") {
            fail "Codex MCP sample must include --max-bytes"
        }
        if ($server.args | any {|arg| ($arg | str contains "auth") or ($arg | str contains "token") or ($arg | str contains "session") }) {
            fail "Codex MCP sample args must not contain auth/session/token fields"
        }
        if ($server.env | columns | length) != 0 {
            fail "Codex MCP sample env must not contain credentials or session material"
        }
        if $policy.bounded != true {
            fail "Codex policy must be bounded"
        }
        if $policy.defaultRowLimit != 50 {
            fail $"Codex policy defaultRowLimit changed: ($policy.defaultRowLimit)"
        }
        if $policy.maxBytes != 65536 {
            fail $"Codex policy maxBytes changed: ($policy.maxBytes)"
        }
        if $policy.rawSourceDefault != "disabled" {
            fail $"Codex policy rawSourceDefault changed: ($policy.rawSourceDefault)"
        }
        if $policy.mutation != "forbidden" {
            fail $"Codex policy mutation changed: ($policy.mutation)"
        }
        if $policy.auth != "external_official_codex_auth_only" {
            fail $"Codex policy auth changed: ($policy.auth)"
        }

        let source_fixture = ([$repo_root fixtures secret_like] | path join)
        let temp_root = (mktemp -d)
        let fixture = ([$temp_root secret_like] | path join)
        cp -r $source_fixture $fixture
        let fixture_manifest = ([$fixture Cargo.toml] | path join)
        run_cargo_checked [
            generate-lockfile
            --manifest-path
            $fixture_manifest
            --offline
        ] | ignore

        let doctor_result = (run_cargo_checked [run --quiet -p codedb -- doctor --codex --format json])
        let scan_result = (run_cargo_checked [run --quiet -p codedb -- scan $fixture --format json])
        let mcp_result = (run_cargo_checked [test -p codedb-mcp --quiet])
        let mcp_combined = $"stdout:\n($mcp_result.stdout)\nstderr:\n($mcp_result.stderr)"
        assert_no_forbidden_values mcp_tests $mcp_combined

        let source_lock = ([$repo_root fixtures secret_like Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "Codex bounded smoke mutated the source fixture Cargo.lock"
        }

        [
            {
                label: codex_mcp_config,
                status: passed,
                command_placeholder: $server.command,
                default_limit: $policy.defaultRowLimit,
                max_bytes: $policy.maxBytes,
                raw_source_default: $policy.rawSourceDefault,
                auth: $policy.auth,
            },
            (assert_bounded_output codex_doctor_cli $doctor_result.stdout 50 65536),
            (assert_bounded_output codex_scan_cli $scan_result.stdout 50 65536),
            {
                label: codex_mcp_tests,
                status: passed,
                stdout_sha256: ($mcp_result.stdout | hash sha256),
                stderr_sha256: ($mcp_result.stderr | hash sha256),
                raw_source_default: disabled,
                bounded: true,
            },
        ]
    }
}

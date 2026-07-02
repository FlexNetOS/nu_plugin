# Test lane: default
# Defends: dynamic build/proc-macro capture is gated and refuses without explicit unsafe approval.

def fail [message: string] {
    error make { msg: $message }
}

def run_checked [command: string, args: list<string>] {
    let result = (^$command ...$args | complete)
    if $result.exit_code != 0 {
        fail $"command failed: ($command) ($args | str join ' ')\n($result.stderr)"
    }
    $result.stdout
}

def run_codedb [args: list<string>] {
    run_checked cargo ([run --quiet -p codedb --] | append $args)
}

def assert_runner_unsafe_gate [repo: string] {
    let output = (run_codedb [export runner_proof_manifest --repo-path $repo --format json])
    let gate = ($output | from json | where gate_id == unsafe_capture_default | first)

    if $gate.status != "satisfied" {
        fail $"unsafe_capture_default status was ($gate.status), expected satisfied"
    }
    if $gate.default_policy != "refuse_without_unsafe_flag" {
        fail $"default_policy was ($gate.default_policy), expected refuse_without_unsafe_flag"
    }
    if $gate.mcp_dynamic_execution != "blocked" {
        fail $"mcp_dynamic_execution was ($gate.mcp_dynamic_execution), expected blocked"
    }

    {
        label: runner_unsafe_gate,
        status: $gate.status,
        default_policy: $gate.default_policy,
        mcp_dynamic_execution: $gate.mcp_dynamic_execution,
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
        let capture_tests = (run_checked cargo [
            test
            -p
            codedb-build-capture
            --quiet
        ])

        let source_fixture = ([$repo_root fixtures build_script] | path join)
        let temp_root = (mktemp -d)
        let fixture = ([$temp_root build_script] | path join)
        cp -r $source_fixture $fixture

        let source_lock = ([$source_fixture Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "unsafe capture test source fixture started with Cargo.lock"
        }

        [
            {
                label: build_capture_crate_tests,
                status: "passed",
                sha256: ($capture_tests | hash sha256),
            },
            (assert_runner_unsafe_gate $fixture),
        ]
    }
}

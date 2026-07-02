# Test lane: default
# Defends: redb lock contention and plugin-like lifecycle release stay documented and safe.

def fail [message: string] {
    error make { msg: $message }
}

def run_checked [args: list<string>] {
    let result = (^cargo ...$args | complete)
    if $result.exit_code != 0 {
        fail $"cargo command failed: cargo ($args | str join ' ')\nstdout:\n($result.stdout)\nstderr:\n($result.stderr)"
    }
    $result
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
        let focused = (run_checked [
            test
            -p
            codedb-store-redb
            lock_contention_blocks_until_writer_lifecycle_release
            --quiet
        ])
        let full = (run_checked [
            test
            -p
            codedb-store-redb
            --quiet
        ])

        let fixture_locks = (glob fixtures/**/Cargo.lock)
        if ($fixture_locks | length) > 0 {
            fail $"redb lifecycle test generated fixture Cargo.lock files: ($fixture_locks | str join ', ')"
        }

        [
            {
                status: passed,
                gate: redb_lock_contention,
                behavior: single_writer_blocks_until_release,
                lifecycle_release: drop_releases_write_lock,
                stdout_sha256: ($focused.stdout | hash sha256),
                stderr_sha256: ($focused.stderr | hash sha256),
            },
            {
                status: passed,
                gate: redb_store_crate_tests,
                behavior: backup_restore_and_lifecycle_tests_pass,
                stdout_sha256: ($full.stdout | hash sha256),
                stderr_sha256: ($full.stderr | hash sha256),
            },
        ]
    }
}

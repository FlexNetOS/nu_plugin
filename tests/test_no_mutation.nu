# Test lane: default
# Defends: CodeDB no-mutation proof preserves clean and pre-existing dirty Git states.

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

def git_cmd [repo: string, args: list<string>] {
    run_checked git ([-C $repo] | append $args)
}

def init_committed_repo [source_fixture: string, name: string] {
    let temp_root = (mktemp -d)
    let repo = ([$temp_root $name] | path join)

    cp -r $source_fixture $repo
    run_checked cargo [
        generate-lockfile
        --manifest-path
        ([$repo Cargo.toml] | path join)
        --offline
    ] | ignore
    run_codedb [scan $repo --format json] | ignore

    git_cmd $repo [init] | ignore
    git_cmd $repo [add .] | ignore
    git_cmd $repo [-c user.name=CodeDB -c user.email=codedb@example.invalid commit -m "initial fixture"] | ignore

    $repo
}

def no_mutation_row [repo: string] {
    let output = (run_codedb [export runner_proof_manifest --repo-path $repo --format json])
    let rows = ($output | from json)
    $rows | where gate_id == no_mutation_proof | first
}

def assert_no_mutation [label: string, repo: string, expect_dirty: bool] {
    let before = (git_cmd $repo [status --porcelain=v1])
    let proof = (no_mutation_row $repo)
    let after = (git_cmd $repo [status --porcelain=v1])

    if $before != $after {
        fail $"($label) changed git status\nbefore:\n($before)\nafter:\n($after)"
    }
    if $proof.proof_status != "proven" {
        fail $"($label) proof_status was ($proof.proof_status), expected proven"
    }
    if $proof.mutation_detected != "false" {
        fail $"($label) mutation_detected was ($proof.mutation_detected), expected false"
    }
    if $proof.pre_existing_dirty != ($expect_dirty | into string) {
        fail $"($label) pre_existing_dirty was ($proof.pre_existing_dirty), expected ($expect_dirty)"
    }

    {
        label: $label,
        proof_status: $proof.proof_status,
        pre_existing_dirty: $proof.pre_existing_dirty,
        mutation_detected: $proof.mutation_detected,
        status_sha256: ($after | hash sha256),
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
        let clean_source = ([$repo_root fixtures clean_repo] | path join)
        let source_lock = ([$clean_source Cargo.lock] | path join)
        if ($source_lock | path exists) {
            fail "no-mutation source fixture started with Cargo.lock"
        }

        let clean_repo = (init_committed_repo $clean_source clean_repo)

        let dirty_source = ([$repo_root fixtures clean_repo] | path join)
        let dirty_repo = (init_committed_repo $dirty_source dirty_repo)
        "pub fn clean_marker() -> &'static str {\n    \"dirty-but-pre-existing\"\n}\n" | save -f ([$dirty_repo src lib.rs] | path join)
        "pre-existing dirty note\n" | save -f ([$dirty_repo dirty_note.txt] | path join)

        if ($source_lock | path exists) {
            fail "no-mutation test mutated the source fixture Cargo.lock"
        }

        [
            (assert_no_mutation clean_repo $clean_repo false),
            (assert_no_mutation dirty_repo $dirty_repo true),
        ]
    }
}

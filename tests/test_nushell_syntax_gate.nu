# Test lane: default
# Defends: CodeDB Nu fixtures, tests, templates, and examples parse under temp-HOME syntax validation.

def fail [message: string] {
    error make { msg: $message }
}

def run_syntax_check [repo_root: string, path: string, home: string, plugin_config: string] {
    let rel_path = ($path | path relative-to $repo_root)
    let result = (
        with-env {
            HOME: $home,
            XDG_CONFIG_HOME: ([$home .config] | path join),
            XDG_DATA_HOME: ([$home .local share] | path join),
            XDG_CACHE_HOME: ([$home .cache] | path join),
            YAZELIX_CODEDB_BIN: ([$home stubs codedb] | path join),
            YAZELIX_CODEDB_PLUGIN_BIN: ([$home stubs nu_plugin_codedb] | path join),
        } {
            nu --no-config-file --plugin-config $plugin_config --ide-check 100 $path | complete
        }
    )

    let hint_count = if ($result.stdout | str trim | is-empty) {
        0
    } else {
        $result.stdout
        | lines
        | where {|line| not ($line | str trim | is-empty) }
        | length
    }

    {
        path: $rel_path,
        status: (if $result.exit_code == 0 { "passed" } else { "failed" }),
        exit_code: $result.exit_code,
        hint_count: $hint_count,
        stderr_sha256: ($result.stderr | hash sha256),
    }
}

def main [] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    cd $repo_root

    let temp_home = (mktemp -d)
    mkdir ([$temp_home .config] | path join)
    mkdir ([$temp_home .local share] | path join)
    mkdir ([$temp_home .cache] | path join)
    mkdir ([$temp_home stubs] | path join)
    let temp_plugin_config = ([$temp_home plugins.msgpackz] | path join)

    let files = (
        glob tests/*.nu
        | append (glob templates/nushell/*.nu)
        | append (glob examples/nushell/*.nu)
        | append (glob fixtures/nushell_syntax/*.nu)
        | sort
    )

    if ($files | length) == 0 {
        fail "syntax gate found no Nu files"
    }

    let rows = ($files | each {|path| run_syntax_check $repo_root $path $temp_home $temp_plugin_config })
    let failures = ($rows | where status == failed)
    if ($failures | length) > 0 {
        fail $"Nu syntax gate failed:\n($failures | to json)"
    }

    {
        status: passed,
        checked_files: ($rows | length),
        temp_home: $temp_home,
        plugin_config: $temp_plugin_config,
        rows: $rows,
    }
}

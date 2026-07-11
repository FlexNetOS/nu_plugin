# Test lane: PostgreSQL service when CODEDB_PG_CONN is set; redb always.
# Defends: the Nu plugin's store commands use the same dynamic selector contract
# as the codedb CLI and round-trip exact bytes through both supported backends.

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

def run_backend [
    nu_bin: string,
    plugin: string,
    codedb: string,
    plugin_config: string,
    repo: string,
    out_dir: string,
    store_args: string,
] {
    let command = (
        "let capture = (codedb capture "
        + ($repo | to nuon)
        + " "
        + $store_args
        + "); let report = (codedb store-report "
        + $store_args
        + "); let materialized = (codedb materialize "
        + ($out_dir | to nuon)
        + " "
        + $store_args
        + "); "
        + "{capture_rows: ($capture | length), report_rows: ($report | length), materialize_rows: ($materialized | length)} | to json"
    )
    let output = (
        with-env { CODEDB_BIN: $codedb } {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config
                $plugin_config
                --plugins
                $plugin
                -c
                $command
            ]
        }
    )
    $output | from json
}

def main [] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    cd $repo_root

    run_checked cargo [build --quiet -p codedb -p nu_plugin_codedb] | ignore

    let nu_bin = (which nu | first | get path)
    let codedb = ([$repo_root target debug codedb] | path join)
    let plugin = ([$repo_root target debug nu_plugin_codedb] | path join)
    let root = (mktemp -d)
    let home = ([$root home] | path join)
    let repo = ([$root repo] | path join)
    let redb_out = ([$root redb-out] | path join)
    let redb_store = ([$root codedb.redb] | path join)
    let plugin_config = ([$root plugins.msgpackz] | path join)
    mkdir $home
    mkdir ([$repo sub] | path join)
    "exact plugin bytes\n" | save --raw ([$repo sub file.txt] | path join)

    let redb = (
        with-env {
            HOME: $home,
            XDG_CONFIG_HOME: ([$home .config] | path join),
            XDG_DATA_HOME: ([$home .local share] | path join),
            XDG_CACHE_HOME: ([$home .cache] | path join),
        } {
            let store_args = ("--store " + ($redb_store | to nuon))
            run_backend $nu_bin $plugin $codedb $plugin_config $repo $redb_out $store_args
        }
    )
    if $redb.capture_rows == 0 or $redb.report_rows == 0 or $redb.materialize_rows == 0 {
        fail $"redb plugin round trip returned empty rows: ($redb | to json --raw)"
    }
    let expected = (open --raw ([$repo sub file.txt] | path join))
    let redb_actual = (open --raw ([$redb_out sub file.txt] | path join))
    if $expected != $redb_actual {
        fail "redb plugin materialization was not byte-exact"
    }

    let pg_conn = ($env.CODEDB_PG_CONN? | default "")
    let postgres = if ($pg_conn | is-empty) {
        {status: "not_run_no_explicit_dsn"}
    } else {
        let pg_out = ([$root pg-out] | path join)
        let pg_table = "codedb_nu_plugin_dynamic"
        let result = (
            with-env {
                HOME: $home,
                XDG_CONFIG_HOME: ([$home .config] | path join),
                XDG_DATA_HOME: ([$home .local share] | path join),
                XDG_CACHE_HOME: ([$home .cache] | path join),
                CODEDB_PG_CONN: $pg_conn,
            } {
                let store_args = (
                    "--store pg --pg-table "
                    + ($pg_table | to nuon)
                )
                run_backend $nu_bin $plugin $codedb $plugin_config $repo $pg_out $store_args
            }
        )
        if $result.capture_rows == 0 or $result.report_rows == 0 or $result.materialize_rows == 0 {
            fail $"PostgreSQL plugin round trip returned empty rows: ($result | to json --raw)"
        }
        let pg_actual = (open --raw ([$pg_out sub file.txt] | path join))
        if $expected != $pg_actual {
            fail "PostgreSQL plugin materialization was not byte-exact"
        }
        {status: "passed", result: $result}
    }

    {
        status: "passed",
        redb: $redb,
        postgres: $postgres,
        plugin: $plugin,
        codedb: $codedb,
    }
}

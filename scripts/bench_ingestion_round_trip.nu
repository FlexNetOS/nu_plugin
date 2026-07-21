# Protocol benchmark for `codedb ingest-envelope` (ARCHBP-001).
#
# Measures full typed round trips (typed Nu envelope -> MessagePack plugin
# child -> codedb CLI -> redb store -> typed receipt) and writes the RAW
# per-run wall-clock samples to logs/ingestion-benchmark/samples.json.
# All performance wording anywhere in this repo must be qualified by these
# raw samples; no zero-copy or vectorization claims are made or implied.

def run_checked [command: string, args: list<string>] {
    let result = (^$command ...$args | complete)
    if $result.exit_code != 0 {
        error make { msg: $"command failed: ($command)\n($result.stderr)" }
    }
    $result.stdout
}

def main [--runs: int = 30] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    cd $repo_root
    run_checked cargo [build --quiet -p codedb -p nu_plugin_codedb] | ignore

    let nu_bin = (which nu | first | get path)
    let target_dir = ($env.CARGO_TARGET_DIR? | default ([$repo_root target] | path join))
    let codedb = ([$target_dir debug codedb] | path join)
    let plugin = ([$target_dir debug nu_plugin_codedb] | path join)
    let fixture_root = ([$repo_root fixtures ingestion_round_trip] | path join)

    let root = (mktemp -d)
    let envelope_path = ([$root envelope.json] | path join)
    let plugin_config = ([$root plugins.msgpackz] | path join)

    let files = (
        ["mod.nu" "src/lib.rs" "Cargo.toml" "scripts/build.nu" "dup/copy_one.nu" "dup/copy_two.nu"]
        | each {|p|
            let absolute = ([$fixture_root $p] | path join)
            let bytes = (open --raw $absolute)
            {
                path: $p,
                module_path: ($p | str replace --all "/" "::" | str replace --regex '\.(nu|rs|toml)$' ""),
                unix_mode: (^stat -c "%a" $absolute | str trim),
                content_base64: ($bytes | encode base64),
                sha256: ($bytes | hash sha256),
                ast: [],
            }
        }
    )
    {schema_version: "codedb.ingest-envelope.v0", files: $files} | to json --raw | save --raw $envelope_path

    let samples = (
        1..$runs | each {|run|
            let store = ([$root $"bench-($run).redb"] | path join)
            let command = (
                "open " + ($envelope_path | to nuon)
                + " | codedb ingest-envelope --store " + ($store | to nuon)
                + " | to json --raw"
            )
            let started = (date now)
            with-env { CODEDB_BIN: $codedb } {
                run_checked $nu_bin [
                    --no-config-file
                    --plugin-config $plugin_config
                    --plugins $plugin
                    -c $command
                ]
            } | ignore
            let elapsed = ((date now) - $started)
            {run: $run, wall_clock_ns: ($elapsed | into int)}
        }
    )

    let ns = ($samples | get wall_clock_ns)
    let report = {
        schema_version: "codedb.ingestion-benchmark.v0",
        measured_at: (date now | format date "%+"),
        host_kernel: (^uname -sr | str trim),
        runs: $runs,
        envelope_file_count: ($files | length),
        envelope_bytes: (ls $envelope_path | first | get size | into int),
        includes_process_startup: true,
        note: "Each sample is one full typed round trip: nu startup + plugin registration + MessagePack envelope transfer + codedb CLI validation + redb persist + typed receipt return, against a fresh store. Raw samples qualify every performance statement; no zero-copy or vectorization behavior is claimed or measured.",
        wall_clock_ns_samples: $ns,
        min_ns: ($ns | math min),
        median_ns: ($ns | math median),
        max_ns: ($ns | math max),
    }
    mkdir logs/ingestion-benchmark
    $report | to json --indent 2 | save --raw --force logs/ingestion-benchmark/samples.json
    print ($report | reject wall_clock_ns_samples | to json --raw)
}

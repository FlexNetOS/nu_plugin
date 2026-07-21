# Test lane: redb only (no PostgreSQL mutation; leave CODEDB_PG_CONN unset).
# Defends ARCHBP-001: bounded .nu/.rs/.toml fixtures round-trip exact bytes,
# relative paths, unix permissions, Nushell AST metadata, and hashes through
# the native MessagePack plugin via the typed `codedb ingest-envelope`
# command, and duplicate content is content-addressed exactly once (BLAKE3).

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

# Nushell AST rows for one source string, normalized to a stable shape.
def nu_ast_rows [source: string] {
    ast $source --json --flatten
    | from json
    | each {|row|
        {
            content: $row.content,
            shape: $row.shape,
            span_start: ($row.span.start? | default 0),
            span_end: ($row.span.end? | default 0),
        }
    }
}

def fixture_entry [fixture_root: string, relative_path: string] {
    let absolute = ([$fixture_root $relative_path] | path join)
    let bytes = (open --raw $absolute)
    let is_nu = ($relative_path | str ends-with ".nu")
    let ast_rows = if $is_nu {
        let text = if ($bytes | describe) == "binary" { $bytes | decode utf-8 } else { $bytes }
        nu_ast_rows $text
    } else {
        []
    }
    {
        path: $relative_path,
        module_path: (
            $relative_path
            | str replace --all "/" "::"
            | str replace --regex '\.(nu|rs|toml)$' ""
        ),
        unix_mode: (^stat -c "%a" $absolute | str trim),
        content_base64: ($bytes | encode base64),
        sha256: ($bytes | hash sha256),
        ast: $ast_rows,
    }
}

def main [] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    cd $repo_root

    run_checked cargo [build --quiet -p codedb -p nu_plugin_codedb] | ignore

    let nu_bin = (which nu | first | get path)
    let target_dir = ($env.CARGO_TARGET_DIR? | default ([$repo_root target] | path join))
    let codedb = ([$target_dir debug codedb] | path join)
    let plugin = ([$target_dir debug nu_plugin_codedb] | path join)
    let fixture_root = ([$repo_root fixtures ingestion_round_trip] | path join)

    let root = (mktemp -d)
    let home = ([$root home] | path join)
    let store = ([$root codedb.redb] | path join)
    let out_dir = ([$root materialized] | path join)
    let envelope_path = ([$root envelope.json] | path join)
    let plugin_config = ([$root plugins.msgpackz] | path join)
    mkdir $home

    # dup/copy_one.nu and dup/copy_two.nu are byte-identical on purpose.
    let relative_paths = [
        "mod.nu"
        "src/lib.rs"
        "Cargo.toml"
        "scripts/build.nu"
        "dup/copy_one.nu"
        "dup/copy_two.nu"
    ]
    let files = ($relative_paths | each {|p| fixture_entry $fixture_root $p })
    {
        schema_version: "codedb.ingest-envelope.v0",
        files: $files,
    } | to json --raw | save --raw $envelope_path

    let env_block = {
        HOME: $home,
        XDG_CONFIG_HOME: ([$home .config] | path join),
        XDG_DATA_HOME: ([$home .local share] | path join),
        XDG_CACHE_HOME: ([$home .cache] | path join),
        CODEDB_BIN: $codedb,
    }

    # Typed envelope enters the plugin as a native Nu record over MessagePack.
    let ingest_command = (
        "open " + ($envelope_path | to nuon)
        + " | codedb ingest-envelope --store " + ($store | to nuon)
        + " | to json --raw"
    )
    let receipt = (
        with-env $env_block {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config $plugin_config
                --plugins $plugin
                -c $ingest_command
            ]
        } | from json
    )

    if $receipt.schema_version != "codedb.ingest-receipt.v0" {
        fail $"unexpected receipt schema: ($receipt.schema_version?)"
    }
    if $receipt.summary.file_count != 6 {
        fail $"expected 6 ingested files, got ($receipt.summary.file_count)"
    }
    if $receipt.summary.unique_blob_count != 5 {
        fail $"duplicate content was not content-addressed once: ($receipt.summary | to json --raw)"
    }
    if $receipt.summary.dedup_hit_count != 1 {
        fail $"expected exactly one dedup hit: ($receipt.summary | to json --raw)"
    }
    for submitted in $files {
        let row = ($receipt.files | where path == $submitted.path | first)
        if $row.sha256 != $submitted.sha256 {
            fail $"sha256 mismatch for ($submitted.path): ($row.sha256) != ($submitted.sha256)"
        }
        if not ($row.blake3 =~ '^[0-9a-f]{64}$') {
            fail $"missing or malformed blake3 for ($submitted.path): ($row.blake3?)"
        }
    }
    let dup_one = ($receipt.files | where path == "dup/copy_one.nu" | first)
    let dup_two = ($receipt.files | where path == "dup/copy_two.nu" | first)
    if $dup_one.blake3 != $dup_two.blake3 {
        fail "identical bytes produced different BLAKE3 identities"
    }
    if not $dup_two.deduplicated {
        fail "second copy of identical content was not reported as deduplicated"
    }

    # AST metadata, module paths, and permissions round-trip via ingest-report.
    let report_command = (
        "codedb ingest-report --store " + ($store | to nuon) + " | to json --raw"
    )
    let report = (
        with-env $env_block {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config $plugin_config
                --plugins $plugin
                -c $report_command
            ]
        } | from json
    )
    for submitted in $files {
        let stored = ($report | where path == $submitted.path | first)
        if $stored.module_path != $submitted.module_path {
            fail $"module_path did not round-trip for ($submitted.path): ($stored.module_path)"
        }
        if $stored.unix_mode != $submitted.unix_mode {
            fail $"unix_mode did not round-trip for ($submitted.path): ($stored.unix_mode) != ($submitted.unix_mode)"
        }
        # Field order over the plugin boundary is cosmetic (serde maps sort
        # keys); compare rows with a normalized column order.
        let stored_ast = ($stored.ast | each {|r|
            {content: $r.content, shape: $r.shape, span_start: $r.span_start, span_end: $r.span_end}
        })
        if ($stored_ast | to json --raw) != ($submitted.ast | to json --raw) {
            fail $"AST metadata did not round-trip for ($submitted.path)"
        }
    }

    # Exact-byte and permission materialization round trip.
    let materialize_command = (
        "codedb materialize " + ($out_dir | to nuon)
        + " --store " + ($store | to nuon)
        + " | to json --raw"
    )
    with-env $env_block {
        run_checked $nu_bin [
            --no-config-file
            --plugin-config $plugin_config
            --plugins $plugin
            -c $materialize_command
        ]
    } | ignore
    for submitted in $files {
        let original = ([$fixture_root $submitted.path] | path join)
        let restored = ([$out_dir $submitted.path] | path join)
        if (open --raw $original) != (open --raw $restored) {
            fail $"materialized bytes differ for ($submitted.path)"
        }
        let restored_mode = (^stat -c "%a" $restored | str trim)
        if $restored_mode != $submitted.unix_mode {
            fail $"materialized unix_mode differs for ($submitted.path): ($restored_mode) != ($submitted.unix_mode)"
        }
    }

    {status: "passed", summary: $receipt.summary} | to json --raw
}

# Test lane: redb only (no PostgreSQL mutation; leave CODEDB_PG_CONN unset).
# Defends ARCHBP-041: real rtk_nu envelopes (JSON aggregate, JSONL event
# stream, and native Nu record/list over the MessagePack plugin protocol)
# validate fail-closed, reassemble byte-exact streams, and receive canonical
# content-addressed raw_object_id values idempotently — with the same ids
# across every input format and a typed receipt at each step. Protocol
# traces are preserved under logs/raw-envelope-protocol/.

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

def main [] {
    let repo_root = ($env.CODEDB_TEST_REPO_ROOT? | default (pwd))
    cd $repo_root

    run_checked cargo [build --quiet -p codedb -p nu_plugin_codedb] | ignore

    let nu_bin = (which nu | first | get path)
    let rtk_nu = (which rtk_nu | first | get path)
    let target_dir = ($env.CARGO_TARGET_DIR? | default ([$repo_root target] | path join))
    let codedb = ([$target_dir debug codedb] | path join)
    let plugin = ([$target_dir debug nu_plugin_codedb] | path join)

    let root = (mktemp -d)
    let home = ([$root home] | path join)
    let plugin_config = ([$root plugins.msgpackz] | path join)
    mkdir $home
    let env_block = {
        HOME: $home,
        XDG_CONFIG_HOME: ([$home .config] | path join),
        XDG_DATA_HOME: ([$home .local share] | path join),
        XDG_CACHE_HOME: ([$home .cache] | path join),
        CODEDB_BIN: $codedb,
    }

    # Deterministic child with both streams. Per-stream reassembly:
    # stdout = "hello world" (11 bytes), stderr = "warn: disk" (10 bytes).
    let child = ["sh" "-c" `printf 'hello '; printf 'warn:' 1>&2; printf 'world'; printf ' disk' 1>&2`]
    let stdout_id = ("hello world" | hash sha256 | $"sha256:($in)")
    let stderr_id = ("warn: disk" | hash sha256 | $"sha256:($in)")

    # 1. JSON aggregate envelope from the real adapter.
    let envelope_json = (run_checked $rtk_nu ([--format json --] ++ $child))
    let envelope_path = ([$root envelope.json] | path join)
    $envelope_json | save --raw $envelope_path

    # 2. JSONL event stream from a second real execution.
    let events_text = (run_checked $rtk_nu ([--format jsonl --] ++ $child))
    let events_path = ([$root events.jsonl] | path join)
    $events_text | save --raw $events_path

    # 3. Native record over the MessagePack plugin protocol.
    let store_record = ([$root record.redb] | path join)
    let ingest_record_command = (
        "open " + ($envelope_path | to nuon) + " | codedb ingest-envelope --store "
        + ($store_record | to nuon) + " | to json --raw"
    )
    let receipt_record = (
        with-env $env_block {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config $plugin_config
                --plugins $plugin
                -c $ingest_record_command
            ]
        } | from json
    )
    if $receipt_record.schema_version != "codedb.raw-ingest-receipt.v0" {
        fail $"unexpected receipt schema: ($receipt_record.schema_version)"
    }
    let record_ids = ($receipt_record.raw_objects | sort-by stream | get raw_object_id)
    if $record_ids != ([$stderr_id $stdout_id]) {
        fail $"record-path canonical ids diverged: ($record_ids)"
    }
    if ($receipt_record.raw_objects | any {|o| $o.deduplicated }) {
        fail "first ingest must not report dedup"
    }

    # 4. Idempotent re-ingest over the protocol: same ids, dedup marked.
    let receipt_again = (
        with-env $env_block {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config $plugin_config
                --plugins $plugin
                -c $ingest_record_command
            ]
        } | from json
    )
    if not ($receipt_again.raw_objects | all {|o| $o.deduplicated }) {
        fail "re-ingest must dedup every stream"
    }
    if ($receipt_again.raw_objects | sort-by stream | get raw_object_id) != $record_ids {
        fail "idempotent re-ingest changed canonical ids"
    }

    # 5. Native list input (JSONL events as Nu records) over the protocol.
    let store_list = ([$root list.redb] | path join)
    let ingest_list_command = (
        "open --raw " + ($events_path | to nuon) + " | lines | each { from json }"
        + " | codedb ingest-envelope --store " + ($store_list | to nuon)
        + " | to json --raw"
    )
    let receipt_list = (
        with-env $env_block {
            run_checked $nu_bin [
                --no-config-file
                --plugin-config $plugin_config
                --plugins $plugin
                -c $ingest_list_command
            ]
        } | from json
    )
    if ($receipt_list.raw_objects | sort-by stream | get raw_object_id) != $record_ids {
        fail "list-path canonical ids diverged from record-path ids"
    }

    # 6. CLI JSONL file path: identical canonical ids again.
    let store_jsonl = ([$root jsonl.redb] | path join)
    let receipt_jsonl = (
        run_checked $codedb [
            ingest-envelope --input $events_path --store $store_jsonl --format json
        ] | from json
    )
    if ($receipt_jsonl.raw_objects | sort-by stream | get raw_object_id) != $record_ids {
        fail "JSONL-path canonical ids diverged from record-path ids"
    }

    # 7. Raw report reads back stream metadata and exact digests.
    let report = (
        run_checked $codedb [raw-report --store $store_record --format json] | from json
    )
    if ($report | length) != 2 {
        fail $"expected 2 raw objects in the report, got ($report | length)"
    }
    let stdout_row = ($report | where stream == stdout | first)
    if $stdout_row.byte_length != 11 {
        fail $"stdout byte_length ($stdout_row.byte_length) != 11"
    }
    if $stdout_row.raw_object_id != $stdout_id {
        fail "report stdout id diverged"
    }
    if $"sha256:($stdout_row.sha256)" != $stdout_id {
        fail "report sha256 does not match the canonical id"
    }

    # 8. Corrupt digest is rejected fail-closed.
    let corrupt = (
        $envelope_json | from json
        | update frames {|e| $e.frames | each {|f| $f | update sha256 ("0" | fill --width 64 --character "0") } }
        | to json --raw
    )
    let corrupt_path = ([$root corrupt.json] | path join)
    $corrupt | save --raw $corrupt_path
    let corrupt_result = (
        ^$codedb ingest-envelope --input $corrupt_path --store ([$root corrupt.redb] | path join) --format json
        | complete
    )
    if $corrupt_result.exit_code == 0 {
        fail "corrupted digests must be rejected"
    }

    # Preserve protocol traces as committed evidence.
    let traces_dir = ([$repo_root logs raw-envelope-protocol] | path join)
    mkdir $traces_dir
    {
        schema_version: "codedb.raw-envelope-protocol-trace.v0",
        child_argv: $child,
        stdout_raw_object_id: $stdout_id,
        stderr_raw_object_id: $stderr_id,
        record_receipt: $receipt_record,
        reingest_receipt: $receipt_again,
        list_receipt: $receipt_list,
        jsonl_receipt: $receipt_jsonl,
        raw_report: $report,
    } | to json --indent 2 | save --raw --force ([$traces_dir traces.json] | path join)

    {status: "passed", canonical_ids: $record_ids} | to json --raw
}

# Test lane: default
# Defends: generated Yazelix init/extern bridge artifacts can be verified from manifest rows and file checksums.

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

def require_one [rows: list<any>, artifact: string] {
    let matches = ($rows | where artifact == $artifact)
    if ($matches | length) != 1 {
        fail $"expected exactly one row for artifact ($artifact), found (($matches | length))"
    }
    $matches | first
}

def assert_provenance_row [row: record] {
    if $row.table != "codedb_yazelix_bridge_artifacts" {
        fail $"unexpected table for ($row.artifact): ($row.table)"
    }
    if $row.generated != "true" {
        fail $"artifact was not marked generated: ($row.artifact)"
    }
    if $row.manual_edits_allowed != "false" {
        fail $"artifact allowed manual edits: ($row.artifact)"
    }
    if $row.mutates_plugin_registry != "false" {
        fail $"artifact mutates plugin registry: ($row.artifact)"
    }
    if $row.source_truth != "templates" {
        fail $"artifact source truth was not templates: ($row.artifact)"
    }
    if not ($row.path | path exists) {
        fail $"artifact path does not exist: ($row.path)"
    }
    let actual_hash = (open --raw $row.path | hash sha256)
    if $actual_hash != $row.sha256 {
        fail $"artifact checksum mismatch for ($row.artifact): row=($row.sha256) actual=($actual_hash)"
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
        run_checked cargo [build --quiet -p codedb] | ignore

        let out_dir = (mktemp -d)
        let output = (run_checked cargo [run --quiet -p codedb -- generate-yazelix-bridge --out-dir $out_dir --format json])
        let rows = ($output | from json)

        if ($rows | length) != 3 {
            fail $"expected 3 bridge artifact rows, found (($rows | length))"
        }

        let init_row = (require_one $rows codedb_init)
        let extern_row = (require_one $rows codedb_extern)
        let manifest_row = (require_one $rows codedb_bridge_manifest)

        assert_provenance_row $init_row
        assert_provenance_row $extern_row
        assert_provenance_row $manifest_row

        let manifest = (open $manifest_row.path)
        if $manifest.schema_version != 1 {
            fail $"unexpected bridge manifest schema version: ($manifest.schema_version)"
        }
        if $manifest.generator != "codedb generate-yazelix-bridge" {
            fail $"unexpected bridge manifest generator: ($manifest.generator)"
        }
        if (($manifest.source_templates | to json --raw) != ([
            templates/nushell/codedb_init.nu
            templates/nushell/codedb_extern.nu
        ] | to json --raw)) {
            fail "bridge manifest source templates changed"
        }

        for artifact in [$init_row $extern_row] {
            let manifest_matches = ($manifest.artifacts | where artifact == $artifact.artifact)
            if ($manifest_matches | length) != 1 {
                fail $"expected one manifest artifact entry for ($artifact.artifact)"
            }
            let manifest_artifact = ($manifest_matches | first)
            if $manifest_artifact.sha256 != $artifact.sha256 {
                fail $"manifest checksum disagrees with emitted row for ($artifact.artifact)"
            }
            if $manifest_artifact.mutates_plugin_registry != false {
                fail $"manifest artifact mutates plugin registry: ($artifact.artifact)"
            }
        }

        {
            status: passed,
            row_count: ($rows | length),
            out_dir: $out_dir,
            init_sha256: $init_row.sha256,
            extern_sha256: $extern_row.sha256,
            manifest_sha256: $manifest_row.sha256,
        }
    }
}

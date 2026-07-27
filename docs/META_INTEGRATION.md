# meta Integration

Source: PRD section 16.4.

meta supplies repo graph and selected project inputs. CodeDB does not replace meta.

## Contract

| Input from meta | CodeDB use |
|---|---|
| project ID | stable scan target label |
| repo path | scan root |
| tags/capabilities | export metadata |
| dependency graph | scan ordering hints |

## CLI inputs

CodeDB accepts explicit meta-selected repo inputs:

```bash
codedb scan --repo-id <meta_project_id> --repo-path <path> --store <path> --format json
codedb export meta_repo_selection --repo-id <meta_project_id> --repo-path <path> --store <path> --format nuon
```

`--repo-path` is the scan root. `--repo-id` is a stable label supplied by meta or
another orchestration layer. `--store` is accepted as selection metadata for the
future store boundary, but CDB036 does not create, open, or mutate that store.

For compatibility with direct CLI use, `codedb scan <path>` and `codedb export <table>
--repo <path>` remain accepted. If a positional path and an explicit repo path disagree,
the command fails instead of guessing.

The CLI emits a `meta_repo_selection` row with `repo_id`, `repo_path`, `store_path`,
`selection_source`, and `mutation_policy = read_only_no_meta_mutation`.

## Observable selection boundary

`codedb scan` treats the supplied `--repo-path` as the only scan root. Its JSON
output begins with the selection row, so a caller can verify the chosen stable
`repo_id`, canonical scan root, supplied store label, and
`selection_source = explicit_repo_path` before consuming the scan facts.

The command does not discover projects from meta, update a meta graph, or
materialize the supplied `--store` during this read-only scan. A positional
repository path may be retained for direct CLI compatibility only when it
matches `--repo-path`; otherwise CodeDB refuses the invocation with a
conflicting-selection error.

For a selected-repository integration check, run:

```bash
codedb scan --repo-id <meta_project_id> --repo-path <path> --store <path> --format json
```

Confirm that the resulting `meta_repo_selection` row has
`mutation_policy = read_only_no_meta_mutation` and that no meta-owned artifact
changed.

## Guard

Multi-repo scanning requires explicit selected project rows and no-mutation proof. CodeDB must not perform broad meta mutations.

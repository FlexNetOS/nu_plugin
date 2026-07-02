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

## Guard

Multi-repo scanning requires explicit selected project rows and no-mutation proof. CodeDB must not perform broad meta mutations.

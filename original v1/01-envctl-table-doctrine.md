# envctl Table Doctrine

`envctl` owns environment truth.

Permanent flow:

```text
files -> Nushell tables -> validated envctl tables -> generated files
```

Canonical tables are source of truth. Generated files are replaceable outputs. Original files are imported evidence, backups, or references.

Codex must not hand-patch scattered config as the long-term fix. It must inspect files, convert to tables, update rows, validate, generate, diff, and log changes.

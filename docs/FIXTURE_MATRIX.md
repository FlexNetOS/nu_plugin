# Fixture Matrix

Source: PRD section 18.

| Fixture | Purpose | Expected evidence |
|---|---|---|
| empty crate | baseline scan | source root + package rows |
| simple lib/bin | item/module/function inventory | rust item rows |
| workspace | workspace/package/target resolution | cargo workspace rows |
| feature-gated crate | cfg/feature context | feature/cfg rows |
| macro_rules crate | static macro definition/invocation | macro rows + expansion gap |
| proc-macro user | unsafe gap/default refusal | proc macro gap rows |
| build.rs crate | build-script detection | build script rows + unsafe refusal |
| include_str/include_bytes | static include edge | include/path edge rows |
| native link hints | linker/native gap rows | native/link rows |
| secret-looking fixture | leak guard | redaction/validation rows |
| dirty repo | no-mutation proof | before/after dirty state unchanged |
| clean repo | no-mutation proof | clean before and after |

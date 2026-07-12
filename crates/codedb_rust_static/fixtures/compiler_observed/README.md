# Compiler-observed fixture

This fixture is intentionally compiled only by the explicit
`CompilerEvidenceOptions::enabled` lane. The positive path requires a nightly
`rustc` and matching `rustdoc` with `-Zunpretty` and rustdoc JSON support.

The integration test compiles `proc_macro/src/lib.rs` as a real proc-macro
dynamic library, supplies its exact path and SHA-256 as an `--extern` input to
the consumer, and then captures declarative and procedural macro expansion,
resolution, hygiene, HIR, MIR, and rustdoc JSON.

From the repository root:

```text
cargo test -p codedb-rust-static --test compiler_observed -- --nocapture
```

To select another capable pinned toolchain:

```text
RUSTC=/absolute/path/to/nightly-rustc \
RUSTDOC=/absolute/path/to/matching-nightly-rustdoc \
cargo test -p codedb-rust-static --test compiler_observed -- --nocapture
```

If either tool lacks a required capability, collection fails closed:
no artifact pin, semantic hash, public-API hash, or compiler-observed status is
emitted.

//! Ingestion round-trip fixture: Rust source with a nested module item.

pub fn answer() -> u8 {
    42
}

pub mod inner {
    pub fn nested() -> &'static str {
        "nested"
    }
}

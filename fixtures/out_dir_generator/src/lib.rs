include!(concat!(env!("OUT_DIR"), "/generated.rs"));

pub fn generated_value() -> &'static str {
    GENERATED_VALUE
}

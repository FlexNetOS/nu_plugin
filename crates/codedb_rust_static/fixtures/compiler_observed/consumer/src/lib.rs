use codedb_observed_proc_macro::emit_observed_item;

macro_rules! emit_declarative_item {
    () => {
        pub fn generated_by_macro_rules() -> u32 {
            42
        }
    };
}

emit_declarative_item!();
emit_observed_item!("__CODEDB_SOURCE__", "__CODEDB_EXTERN__");

pub fn call_generated_items() -> u32 {
    generated_by_macro_rules() + generated_by_observed_proc_macro()
}

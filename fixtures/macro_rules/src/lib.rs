macro_rules! make_answer {
    () => {
        pub fn generated_answer() -> u32 {
            42
        }
    };
}

make_answer!();

pub fn call_generated() -> u32 {
    generated_answer()
}

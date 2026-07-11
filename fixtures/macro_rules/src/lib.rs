macro_rules! make_answer {
    () => {
        pub fn generated_answer() -> u32 {
            42
        }
    };
}

macro_rules! add_one_with_hygienic_local {
    ($value:expr) => {{
        let macro_local = $value;
        macro_local + 1
    }};
}

make_answer!();

pub fn call_generated() -> u32 {
    generated_answer()
}

pub fn call_hygienic_local() -> u32 {
    let macro_local = 0;
    let result = add_one_with_hygienic_local!(41);
    macro_local + result
}

use proc_macro::TokenStream;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

#[proc_macro_attribute]
pub fn demo_attr(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let output = item.clone();
    if let Some(path) = std::env::var_os("CODEDB_PROC_MACRO_LOG_PATH") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "macro_name=demo_attr");
            let _ = writeln!(file, "input={item}");
            let _ = writeln!(file, "output={output}");
            let _ = writeln!(file, "---");
        }
    }
    output
}

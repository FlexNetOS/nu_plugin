use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    fs::write(out_dir.join("generated.rs"), "pub const GENERATED_VALUE: &str = \"generated\";\n")
        .expect("write generated fixture source");
    #[cfg(unix)]
    std::os::unix::fs::symlink("generated.rs", out_dir.join("generated-link.rs"))
        .expect("create generated fixture symlink");
}

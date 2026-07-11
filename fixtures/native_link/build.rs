fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CODEDB_FIXTURE_EMIT_NATIVE_LINK").is_some() {
        println!("cargo:rustc-link-search=native=vendor/native");
        println!("cargo:rustc-link-lib=static=codedb_fixture_native");
        println!("cargo:rustc-link-arg=-Wl,--as-needed");
    }
}

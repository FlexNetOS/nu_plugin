fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-env=CODEDB_FIXTURE_BUILD_SCRIPT=observed");
}

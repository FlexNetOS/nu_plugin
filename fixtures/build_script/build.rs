fn main() {
    let secret = std::env::var("CODEDB_FIXTURE_LOG_SECRET")
        .unwrap_or_else(|_| "fixture-secret-not-supplied".to_string());

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CODEDB_FIXTURE_LOG_SECRET");
    println!("cargo:rustc-env=CODEDB_FIXTURE_BUILD_SCRIPT=observed");
    println!("cargo:rustc-env=CODEDB_FIXTURE_API_TOKEN={secret}");
    println!("cargo:warning=build-script-provenance secret={secret}");
}

//! Deterministic asset generator: writes the snapshot pack, manifest,
//! checksums, and license manifest into ./assets (or a given directory).

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "crates/codedb_single_binary_export/assets".to_string());
    if let Err(error) = codedb_single_binary_export::generate_assets(std::path::Path::new(&out)) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
    println!("assets generated deterministically into {out}");
}

//! All-peer consolidation rehearsal (read-only): accounts for every peer
//! capability of the CONSOLIDATE-003 contract and proves each independent
//! repository and its lockfile are preserved. Emits a bounded rehearsal
//! receipt; nothing here performs a cutover or retires anything.

fn main() {
    let mut spine_root = None;
    let mut out_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--spine-root" => spine_root = args.next(),
            "--out" => out_path = args.next(),
            _ => {}
        }
    }
    let (Some(spine_root), Some(out_path)) = (spine_root, out_path) else {
        eprintln!("usage: consolidation-rehearsal --spine-root <planning-spine-v0> --out <receipt.json>");
        std::process::exit(2);
    };
    match codedb_single_binary_export::rehearsal::run(
        std::path::Path::new(&spine_root),
        std::path::Path::new(&out_path),
    ) {
        Ok(receipt) => println!(
            "rehearsal: units={} capabilities={} peers={} all_preserved={}",
            receipt.units_checked, receipt.capabilities_accounted, receipt.peers_checked,
            receipt.all_preserved
        ),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

//! Supervised entrypoint for the single-owner redb service.

use flexnetos_redb_owner::OwnerService;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let root = args.next();
    match (command.as_str(), root) {
        ("serve", Some(root)) => match OwnerService::start(&root) {
            Ok(_owner) => {
                eprintln!("flexnetos-redb-owner: serving {root}");
                loop {
                    std::thread::park();
                }
            }
            Err(error) => {
                eprintln!("flexnetos-redb-owner: refusing to start: {error}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: flexnetos-redb-owner serve <root-dir>");
            std::process::exit(2);
        }
    }
}

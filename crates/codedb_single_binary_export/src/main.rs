//! Command surface of the single-binary snapshot artifact. Every command is
//! bounded to the embedded snapshot; nothing here claims a wider release.

use codedb_single_binary_export as sbx;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("verify") => sbx::verify_embedded().map(|report| {
            println!(
                "verify: pack_ok={} files_ok={} file_count={}",
                report.pack_sha256_ok, report.per_file_checksums_ok, report.file_count
            );
        }),
        Some("list") => sbx::list_entries().map(|entries| {
            for entry in entries {
                println!("{}  {}  {}", entry.sha256, entry.byte_length, entry.path);
            }
        }),
        Some("schema") => sbx::schema_info().map(|schema| {
            println!("{}", schema.schema_version);
        }),
        Some("summary") => sbx::summary().map(|summary| {
            println!(
                "files={} bytes={} source={}",
                summary.file_count, summary.total_bytes, summary.snapshot_source
            );
        }),
        Some("license-report") => sbx::license_report().map(|report| {
            for component in report.components {
                println!("{}: {}", component.name, component.license);
            }
        }),
        Some("export") => {
            let (Some(path), Some(dest)) = (args.get(1), args.get(2)) else {
                eprintln!("usage: export <embedded-path> <destination-file>");
                std::process::exit(2);
            };
            sbx::export_entry(path, std::path::Path::new(dest)).map(|bytes| {
                println!("exported {bytes} bytes to {dest}");
            })
        }
        Some("materialize") => {
            let Some(target) = args.get(1) else {
                eprintln!("usage: materialize <target-dir> [--allow-overwrite]");
                std::process::exit(2);
            };
            let allow = args.iter().any(|a| a == "--allow-overwrite");
            sbx::materialize_embedded(std::path::Path::new(target), allow).map(|receipt| {
                println!("materialized {} files", receipt.files_written);
            })
        }
        _ => {
            eprintln!(
                "usage: codedb-single-binary-export <verify|list|schema|summary|license-report|export|materialize>"
            );
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

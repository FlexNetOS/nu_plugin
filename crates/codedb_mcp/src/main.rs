#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let config = match codedb_mcp::server_config_from_environment() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("codedb-mcp: {error}");
            return ExitCode::from(2);
        }
    };

    match codedb_mcp::run_stdio(&config) {
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("codedb-mcp: {error}");
            ExitCode::from(1)
        }
    }
}

extern crate proc_macro;

use proc_macro::TokenStream;
use std::fs::OpenOptions;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

#[proc_macro]
pub fn emit_observed_item(input: TokenStream) -> TokenStream {
    let input = input.to_string();
    let quoted = input
        .split('"')
        .enumerate()
        .filter_map(|(index, value)| (index % 2 == 1).then_some(value))
        .collect::<Vec<_>>();
    let source = Path::new(quoted.first().copied().unwrap_or("/missing-source"));
    let external = Path::new(quoted.get(1).copied().unwrap_or("/missing-extern"));
    let network_address = "1.1.1.1:53"
        .parse::<SocketAddr>()
        .expect("fixture network address");
    let network_denied =
        TcpStream::connect_timeout(&network_address, Duration::from_millis(100)).is_err();
    let home_hidden = !Path::new("/home/flexnetos/.codex/config.toml").exists();
    let source_read_only = source.is_file()
        && OpenOptions::new().write(true).open(source).is_err();
    let external_read_only = external.is_file()
        && OpenOptions::new().write(true).open(external).is_err();
    format!(
        r#"
pub const CODEDB_SANDBOX_NETWORK_DENIED: bool = {network_denied};
pub const CODEDB_SANDBOX_HOME_HIDDEN: bool = {home_hidden};
pub const CODEDB_SANDBOX_SOURCE_READ_ONLY: bool = {source_read_only};
pub const CODEDB_SANDBOX_EXTERN_READ_ONLY: bool = {external_read_only};
pub fn generated_by_observed_proc_macro() -> u32 {{ 77 }}
"#
    )
    .parse()
    .expect("fixture proc-macro output is valid Rust")
}

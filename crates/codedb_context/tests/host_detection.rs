use std::path::Path;

use codedb_context::{CommandOutput, CommandRunner, ContextError, detect_host_triple_with_runner};

struct RustcRunner;

impl CommandRunner for RustcRunner {
    fn output(
        &self,
        program: &str,
        args: &[String],
        _current_dir: &Path,
    ) -> Result<CommandOutput, ContextError> {
        assert_eq!(program, "rustc");
        assert_eq!(args, ["-vV"]);
        Ok(CommandOutput::success(
            "rustc 1.97.0 (fixture)\nbinary: rustc\ncommit-hash: fixture\nhost: x86_64-unknown-linux-gnu\nrelease: 1.97.0\nLLVM version: 22.1.6\n",
            "",
        ))
    }
}

#[test]
fn detects_host_triple_from_verbose_rustc_identity() {
    let host = detect_host_triple_with_runner(&RustcRunner, Path::new("."))
        .expect("host triple detection");
    assert_eq!(host, "x86_64-unknown-linux-gnu");
}

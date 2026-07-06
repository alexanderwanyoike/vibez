//! See `vibez_plugin_host::scan_helper` for what this binary is for.
//! It lives in the root package so a plain `cargo build` puts the
//! helper next to the `vibez` executable.

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(vibez_plugin_host::scan_helper::run())
}

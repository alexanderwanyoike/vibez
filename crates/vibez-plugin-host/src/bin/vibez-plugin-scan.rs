//! See `vibez_plugin_host::scan_helper` for what this binary is for.

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(vibez_plugin_host::scan_helper::run())
}

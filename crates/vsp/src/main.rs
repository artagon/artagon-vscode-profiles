//! Binary entrypoint for the `vsp` CLI.
//!
//! Captures `clap` parse errors so `--help`/`--version` exit `0` while any unknown flag,
//! invalid argument, or usage error exits `4` ([`ExitCode::UsageOrConfig`]).
//!
//! When the binary runs as `vspcli`, when `VSP_COMPAT_VSPCLI=1` is set, or when the first
//! argument is `compat-vspcli`, the legacy command line is handled by
//! [`vsp::compat_vspcli`] instead of the `vsp` parser.

use std::ffi::OsString;
use std::process;

use clap::error::ErrorKind;
use clap::Parser;

use vsp::cli::{execute, Cli};
use vsp::compat_vspcli;
use vsp::exit_codes::ExitCode;

fn main() {
    let argv: Vec<OsString> = std::env::args_os().collect();
    if let Some(legacy_args) = compat_vspcli::legacy_args_from_argv(&argv) {
        let code = compat_vspcli::run(&legacy_args);
        if code == ExitCode::UsageOrConfig {
            if let Some(hint) = compat_vspcli::env_forced_mode_hint(&argv) {
                eprintln!("vspcli: note: {hint}");
            }
        }
        process::exit(code.as_i32());
    }

    let cli = match Cli::try_parse_from(&argv) {
        Ok(parsed) => parsed,
        Err(err) => {
            let code = match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::Ok,
                _ => ExitCode::UsageOrConfig,
            };
            let _ = err.print();
            process::exit(code.as_i32());
        }
    };

    let exit_code = match execute(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("vsp: {err}");
            err.exit_code()
        }
    };

    process::exit(exit_code.as_i32());
}

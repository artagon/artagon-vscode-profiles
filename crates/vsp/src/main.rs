//! Binary entrypoint for the `vsp` CLI.
//!
//! Captures `clap` parse errors so `--help`/`--version` exit `0` while any unknown flag,
//! invalid argument, or usage error exits `4` ([`ExitCode::UsageOrConfig`]).

use std::process;

use clap::error::ErrorKind;
use clap::Parser;

use vsp::cli::{execute, Cli};
use vsp::exit_codes::ExitCode;

fn main() {
    let cli = match Cli::try_parse() {
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

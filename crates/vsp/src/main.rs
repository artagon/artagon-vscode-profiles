//! Binary entrypoint for the `vsp` CLI.
//!
//! Captures `clap` parse errors so `--help`/`--version` exit `0` while any unknown flag,
//! invalid argument, or usage error exits `4` ([`ExitCode::UsageOrConfig`]).
//!
//! When `--json` is supplied, runtime errors (and CLI parse errors) are emitted as structured
//! JSON on stderr (`{"error":{"code":N,"kind":"...","message":"..."}}`) while stdout remains
//! empty. Closed downstream pipes (`BrokenPipe`) exit cleanly with `0` and empty stderr.
//!
//! When the binary runs as `vspcli`, when `VSP_COMPAT_VSPCLI=1` is set, or when the first
//! argument is `compat-vspcli`, the legacy command line is handled by
//! [`vsp::compat_vspcli`] instead of the `vsp` parser.

use std::ffi::OsString;
use std::io::{self, Write as _};
use std::panic::{self, AssertUnwindSafe};
use std::process;

use clap::error::ErrorKind;
use clap::Parser;

use vsp::cli::{execute, Cli};
use vsp::compat_vspcli;
use vsp::exit_codes::ExitCode;

fn argv_has_json_flag(argv: &[OsString]) -> bool {
    for arg in argv.iter().skip(1) {
        if arg == "--" {
            break;
        }
        if arg == "--json" {
            return true;
        }
    }
    false
}

fn emit_json_error(code: ExitCode, kind: &str, message: &str) {
    let payload = serde_json::json!({
        "error": {
            "code": code.as_i32(),
            "kind": kind,
            "message": message,
        }
    });
    let _ = writeln!(io::stderr(), "{payload}");
}

fn is_broken_pipe_panic(msg: &str) -> bool {
    msg.contains("Broken pipe") || msg.contains("failed printing to stdout")
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("")
}

fn main() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if is_broken_pipe_panic(panic_payload_message(info.payload())) {
            return;
        }
        default_hook(info);
    }));

    let argv: Vec<OsString> = std::env::args_os().collect();
    if let Some(legacy_args) = compat_vspcli::legacy_args_from_argv(&argv) {
        let code = match panic::catch_unwind(AssertUnwindSafe(|| compat_vspcli::run(&legacy_args)))
        {
            Ok(c) => c,
            Err(payload) => {
                if is_broken_pipe_panic(panic_payload_message(payload.as_ref())) {
                    ExitCode::Ok
                } else {
                    process::exit(101);
                }
            }
        };
        if code == ExitCode::UsageOrConfig {
            if let Some(hint) = compat_vspcli::env_forced_mode_hint(&argv) {
                let _ = writeln!(io::stderr(), "vspcli: note: {hint}");
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
            if code != ExitCode::Ok && argv_has_json_flag(&argv) {
                emit_json_error(code, "usage_or_config", err.to_string().trim());
            } else {
                let _ = err.print();
            }
            process::exit(code.as_i32());
        }
    };

    let json_mode = cli.json;
    let exit_code = match panic::catch_unwind(AssertUnwindSafe(|| execute(cli))) {
        Ok(Ok(code)) => code,
        Ok(Err(err)) => {
            if json_mode {
                emit_json_error(err.exit_code(), err.kind(), &err.to_string());
            } else {
                let _ = writeln!(io::stderr(), "vsp: {err}");
            }
            err.exit_code()
        }
        Err(payload) => {
            if is_broken_pipe_panic(panic_payload_message(payload.as_ref())) {
                ExitCode::Ok
            } else {
                process::exit(101);
            }
        }
    };

    process::exit(exit_code.as_i32());
}

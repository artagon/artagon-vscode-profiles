//! Legacy `vspcli` compatibility mode.
//!
//! The compatibility mode accepts the command line of the retired Bash `vspcli` front door
//! and runs the equivalent `vsp` command. It is active when one of these is true:
//!
//! 1. The basename of `argv[0]` is `vspcli` (for example, a `vspcli` symbolic link).
//! 2. The environment variable `VSP_COMPAT_VSPCLI` is exactly `1`.
//! 3. The hidden subcommand `vsp compat-vspcli ...` is used.
//!
//! # Translation
//!
//! | Legacy command | `vsp` command |
//! | --- | --- |
//! | `list`, `--list` | `vsp list presets` |
//! | `detect [--json] [path]` | `vsp detect` |
//! | `--detect [options] [path]` | same as `apply`, with a deprecation warning |
//! | `apply [options] [path]` | `vsp sync <path>`, `vsp sync -p <name>`, or `vsp sync --user` |
//! | `reconcile (--profile <n> [--as <r>] \| --all) ...` | `vsp sync -p <n> [--preset <r>]` or `vsp sync --all` |
//! | `doctor [--strict]` | `vsp doctor` |
//! | `validate`, `--validate` | `vsp dev validate` |
//! | `open <profile> [path]`, `--open` | `vsp open -p <profile> <path>` |
//! | `export [profile...]`, `--export` | `vsp export` |
//! | `import <name> <bundle>`, `--profile-import` | `vsp sync --from <bundle> -p <name>` |
//! | `install-ext <profile> <id>`, `--install-ext` | one gated `code --install-extension` |
//! | `completions <shell>`, `--completion <shell>` | `vspcli` completion script |
//!
//! # Exit codes
//!
//! The legacy codes are kept: `0` success, `1` failure, `2` missing target, `3` negative
//! detection, and `4` usage error (including an unknown command or flag). The legacy
//! `doctor`, `reconcile`, and `install-ext` commands returned `1` for each failure, so the
//! newer codes `5`, `6`, and `7` are reported as `1` for these three commands.
//!
//! Generator and migration commands (`build`, `compose`, `install`, `--open-profiles`,
//! `migrate`) have no equivalent and exit `4` with guidance.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;

use clap::error::ErrorKind;
use clap::Parser;

use crate::cli::{execute, Cli};
use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{ModuleKind, WorkspaceCatalog};
use crate::schema_loader::load_catalog_auto;
use crate::sync::install_single_extension;

/// Environment variable that forces compatibility mode when its value is exactly `1`.
pub const COMPAT_ENV_VAR: &str = "VSP_COMPAT_VSPCLI";

/// Hidden `vsp` subcommand that runs one legacy command line.
pub const COMPAT_SUBCOMMAND: &str = "compat-vspcli";

/// Legacy verbs that the compatibility mode accepts (used for help and completions).
const LEGACY_VERBS: &[&str] = &[
    "apply",
    "detect",
    "reconcile",
    "doctor",
    "validate",
    "export",
    "import",
    "completions",
    "list",
    "open",
    "install-ext",
];

/// Legacy flags offered by the completion scripts.
const LEGACY_FLAGS: &[&str] = &[
    "--list",
    "--completion",
    "--open",
    "--export",
    "--install-ext",
    "--profile-import",
    "--detect",
    "--target",
    "--profile",
    "--ux",
    "--with",
    "--without",
    "--no-rtk",
    "--force",
    "--dry-run",
    "--help",
    "--version",
];

/// How a `vsp` exit code is reported for one legacy command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitMapping {
    /// Report the `vsp` exit code unchanged.
    PassThrough,
    /// Report `5`, `6`, and `7` as `1` (legacy `doctor`, `reconcile`, and `install-ext`).
    CollapseToFailure,
}

impl ExitMapping {
    /// Converts a `vsp` exit code to the legacy exit code for this mapping.
    #[must_use]
    pub const fn apply(self, code: ExitCode) -> ExitCode {
        match (self, code) {
            (
                Self::CollapseToFailure,
                ExitCode::SafetyRefusal | ExitCode::RetryableLock | ExitCode::IncompleteObservation,
            ) => ExitCode::Operational,
            (_, other) => other,
        }
    }
}

/// Result of translating one legacy `vspcli` command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translation {
    /// Run `vsp` with `argv` (the program name is not included).
    Vsp {
        /// Arguments for `vsp`.
        argv: Vec<String>,
        /// Exit code mapping for the legacy command.
        exit: ExitMapping,
        /// Deprecation or compatibility notes to print on standard error first.
        notes: Vec<String>,
    },
    /// Print the compatibility usage text and exit `0`.
    Help,
    /// Print one usage line for a subcommand and exit `0`.
    Usage(String),
    /// Print the version line and exit `0`.
    Version,
    /// Print the `vspcli` completion script for the named shell.
    Completions(String),
    /// Install one extension into a profile (`install-ext <profile> <id>`).
    InstallExt {
        /// Target VS Code profile name.
        profile: String,
        /// Extension ID (`publisher.name`).
        id: String,
    },
    /// Usage error: print the message and exit `4`.
    Reject(String),
}

/// Returns `true` when `argv0` names the legacy `vspcli` command.
#[must_use]
pub fn is_vspcli_invocation(argv0: &OsStr) -> bool {
    let Some(name) = Path::new(argv0).file_name().and_then(OsStr::to_str) else {
        return false;
    };
    name == "vspcli" || name.eq_ignore_ascii_case("vspcli.exe")
}

/// Returns the legacy arguments when the process arguments select compatibility mode.
///
/// `argv` is the full process argument list, including `argv[0]`. `None` means that the
/// normal `vsp` command-line parser must handle the arguments.
#[must_use]
pub fn legacy_args_from_argv(argv: &[OsString]) -> Option<Vec<OsString>> {
    let argv0_is_vspcli = argv.first().is_some_and(|a| is_vspcli_invocation(a));
    let env_forces = env::var(COMPAT_ENV_VAR).ok().as_deref() == Some("1");
    let explicit_subcommand = argv
        .get(1)
        .is_some_and(|a| a.as_os_str() == OsStr::new(COMPAT_SUBCOMMAND));

    if explicit_subcommand {
        return Some(argv[2..].to_vec());
    }
    if argv0_is_vspcli || env_forces {
        return Some(argv.get(1..).unwrap_or(&[]).to_vec());
    }
    None
}

/// Runs one legacy `vspcli` command line and returns the legacy exit code.
///
/// `args` does not include the program name. Messages go to standard error with the
/// `vspcli:` prefix.
#[must_use]
pub fn run(args: &[OsString]) -> ExitCode {
    let mut utf8: Vec<String> = Vec::with_capacity(args.len());
    for arg in args {
        match arg.to_str() {
            Some(s) => utf8.push(s.to_string()),
            None => {
                eprintln!("vspcli: arguments must be valid UTF-8");
                return ExitCode::UsageOrConfig;
            }
        }
    }

    match translate(&utf8) {
        Translation::Help => {
            print!("{}", usage_text());
            ExitCode::Ok
        }
        Translation::Usage(line) => {
            println!("{line}");
            ExitCode::Ok
        }
        Translation::Version => {
            println!(
                "vspcli {} (vsp compatibility mode)",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::Ok
        }
        Translation::Reject(message) => {
            eprintln!("vspcli: {message}");
            ExitCode::UsageOrConfig
        }
        Translation::Completions(shell) => match run_completions(&shell) {
            Ok(script) => {
                print!("{script}");
                ExitCode::Ok
            }
            Err(err) => {
                eprintln!("vspcli: {err}");
                err.exit_code()
            }
        },
        Translation::InstallExt { profile, id } => {
            let result = load_catalog_auto(None).and_then(|catalog| {
                let target = catalog
                    .resolve_alias(&profile)
                    .map_or_else(|| profile.clone(), |(canonical, _)| canonical);
                install_single_extension(&catalog, &target, &id, None, false)
            });
            match result {
                Ok(()) => ExitCode::Ok,
                Err(err) => {
                    eprintln!("vspcli: {err}");
                    ExitMapping::CollapseToFailure.apply(err.exit_code())
                }
            }
        }
        Translation::Vsp { argv, exit, notes } => {
            for note in notes {
                eprintln!("vspcli: {note}");
            }
            let mut full = Vec::with_capacity(argv.len() + 1);
            full.push("vsp".to_string());
            full.extend(argv);
            let cli = match Cli::try_parse_from(&full) {
                Ok(cli) => cli,
                Err(err) => {
                    let _ = err.print();
                    return match err.kind() {
                        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => ExitCode::Ok,
                        _ => ExitCode::UsageOrConfig,
                    };
                }
            };
            let code = match execute(cli) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("vspcli: {err}");
                    err.exit_code()
                }
            };
            exit.apply(code)
        }
    }
}

/// Translates legacy `vspcli` arguments (without the program name) to a [`Translation`].
///
/// This function has no side effects.
#[must_use]
pub fn translate(args: &[String]) -> Translation {
    let Some((first, rest)) = args.split_first() else {
        return Translation::Help;
    };

    match first.as_str() {
        "-h" | "--help" | "help" => Translation::Help,
        "-V" | "--version" | "version" => Translation::Version,
        "list" | "--list" => translate_list(rest),
        "completions" | "--completion" => match rest {
            [shell] => Translation::Completions(shell.clone()),
            [] => Translation::Reject(
                "completions requires a shell name (bash, zsh, fish)".to_string(),
            ),
            _ => Translation::Reject("completions accepts exactly one shell name".to_string()),
        },
        "detect" => translate_detect(rest),
        "--detect" => match translate_apply(rest) {
            Translation::Vsp {
                argv,
                exit,
                mut notes,
            } => {
                notes.insert(
                    0,
                    "Warning: legacy 'vspcli --detect' is deprecated and mutates the workspace; use read-only 'vsp detect' to inspect toolchains or 'vsp sync <path>' to write configuration."
                        .to_string(),
                );
                Translation::Vsp { argv, exit, notes }
            }
            other => other,
        },
        "apply" => translate_apply(rest),
        "reconcile" => translate_reconcile(rest),
        "doctor" => translate_doctor(rest),
        "validate" | "--validate" => match rest {
            [] => vsp(&["dev", "validate"], ExitMapping::PassThrough),
            [flag] if is_help(flag) => Translation::Usage("Usage: vspcli validate".to_string()),
            [other, ..] => unknown_option("validate", other),
        },
        "open" | "--open" => translate_open(rest),
        "export" | "--export" => translate_export(rest),
        "import" | "--profile-import" => translate_import(rest),
        "install-ext" | "--install-ext" => match rest {
            [profile, id] if !profile.starts_with('-') && !id.starts_with('-') => {
                Translation::InstallExt {
                    profile: profile.clone(),
                    id: id.clone(),
                }
            }
            _ => Translation::Reject("install-ext requires <profile> <extension-id>".to_string()),
        },
        "build" | "compose" | "--compose" => Translation::Reject(format!(
            "'{first}' is not available in vsp 3; generated bundles come from 'vsp dev dist' and 'vsp export'"
        )),
        "install" | "--install" | "--open-profiles" => Translation::Reject(format!(
            "'{first}' is not available in vsp 3; use 'vsp sync -p <name>' (or 'vsp sync --all')"
        )),
        "migrate" | "--migrate-catalog" => Translation::Reject(format!(
            "'{first}' is not available in vsp 3; the catalog migration is complete"
        )),
        other => {
            if let Some(shell) = other.strip_prefix("--completion=") {
                if rest.is_empty() && !shell.is_empty() {
                    return Translation::Completions(shell.to_string());
                }
            }
            Translation::Reject(format!(
                "unknown command or flag '{other}' (run vspcli --help)"
            ))
        }
    }
}

/// Returns the usage text printed by `vspcli --help` in compatibility mode.
#[must_use]
pub fn usage_text() -> String {
    format!(
        concat!(
            "vspcli {version} (vsp compatibility mode)\n",
            "\n",
            "vspcli is the legacy command name. Each command runs the equivalent 'vsp' command.\n",
            "Use 'vsp --help' for the current command set.\n",
            "\n",
            "Verbs:\n",
            "  vspcli apply [options] [path]         vsp sync <path> | -p <name> | --user\n",
            "  vspcli detect [--json] [path]         vsp detect (read-only)\n",
            "  vspcli reconcile [options]            vsp sync -p <name> | --all\n",
            "  vspcli doctor [--strict]              vsp doctor\n",
            "  vspcli validate                       vsp dev validate\n",
            "  vspcli export [profile...]            vsp export\n",
            "  vspcli import <name> <bundle> [opts]  vsp sync --from <bundle> -p <name>\n",
            "  vspcli list                           vsp list presets\n",
            "  vspcli open <profile> [path]          vsp open -p <profile> <path>\n",
            "  vspcli install-ext <profile> <id>     install one extension with allowlist and pin checks\n",
            "  vspcli completions <bash|zsh|fish>    print a vspcli completion script\n",
            "\n",
            "Backwards-compatible flags:\n",
            "  --list, --completion <sh>, --open <profile> [path], --export [profile...],\n",
            "  --install-ext <profile> <id>, --profile-import <name> <bundle> [opts],\n",
            "  --detect [options] [path] (deprecated: writes the workspace),\n",
            "  --validate, --help, -h, --version\n",
            "\n",
            "Not available in vsp 3: build, compose, install, --open-profiles, migrate.\n",
        ),
        version = env!("CARGO_PKG_VERSION")
    )
}

/// Renders the `vspcli` completion script for `shell` (`bash`, `zsh`, or `fish`) with the
/// preset and pack names of `catalog`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] (exit `4`) for an unsupported shell name.
pub fn render_completions(shell: &str, catalog: &WorkspaceCatalog) -> VspResult<String> {
    let verbs = LEGACY_VERBS.join(" ");
    let flags = LEGACY_FLAGS.join(" ");
    let profiles = catalog
        .profiles
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let packs = catalog
        .modules
        .values()
        .filter(|m| m.manifest.kind != ModuleKind::Ux)
        .map(|m| m.manifest.id.clone())
        .collect::<Vec<_>>()
        .join(" ");

    match shell {
        "bash" => Ok(format!(
            concat!(
                "_vspcli_completions() {{\n",
                "  local cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n",
                "  local words=\"{verbs} {flags} {profiles} {packs}\"\n",
                "  COMPREPLY=( $(compgen -W \"${{words}}\" -- \"${{cur}}\") )\n",
                "}}\n",
                "complete -F _vspcli_completions vspcli\n",
            ),
            verbs = verbs,
            flags = flags,
            profiles = profiles,
            packs = packs
        )),
        "zsh" => Ok(format!(
            concat!(
                "#compdef vspcli\n",
                "_vspcli() {{\n",
                "  _arguments '1:command:({verbs} {flags})' '*:target:({profiles} {packs})'\n",
                "}}\n",
                "compdef _vspcli vspcli\n",
            ),
            verbs = verbs,
            flags = flags,
            profiles = profiles,
            packs = packs
        )),
        "fish" => Ok(format!(
            concat!(
                "complete -c vspcli -f\n",
                "complete -c vspcli -a \"{verbs}\" -d \"vspcli command\"\n",
                "complete -c vspcli -a \"{profiles}\" -d \"profile\"\n",
                "complete -c vspcli -l with -xa \"{packs}\" -d \"add pack\"\n",
                "complete -c vspcli -l without -xa \"{packs}\" -d \"exclude pack\"\n",
                "complete -c vspcli -l ux -xa \"crisp retina default\" -d \"UX preset\"\n",
            ),
            verbs = verbs,
            profiles = profiles,
            packs = packs
        )),
        other => Err(VspError::UsageOrConfig(format!(
            "unsupported shell '{other}' (supported: bash, zsh, fish)"
        ))),
    }
}

fn run_completions(shell: &str) -> VspResult<String> {
    // Reject an unsupported shell before the catalog is loaded.
    if !matches!(shell, "bash" | "zsh" | "fish") {
        return Err(VspError::UsageOrConfig(format!(
            "unsupported shell '{shell}' (supported: bash, zsh, fish)"
        )));
    }
    let catalog = load_catalog_auto(None)?;
    render_completions(shell, &catalog)
}

fn vsp(argv: &[&str], exit: ExitMapping) -> Translation {
    Translation::Vsp {
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        exit,
        notes: Vec::new(),
    }
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

fn unknown_option(verb: &str, arg: &str) -> Translation {
    Translation::Reject(format!("{verb}: unknown option '{arg}'"))
}

/// Cursor over legacy arguments that understands `--flag value` and `--flag=value`.
struct ArgCursor<'a> {
    args: &'a [String],
    pos: usize,
}

impl<'a> ArgCursor<'a> {
    const fn new(args: &'a [String]) -> Self {
        Self { args, pos: 0 }
    }

    fn take(&mut self) -> Option<&'a str> {
        let item = self.args.get(self.pos)?;
        self.pos += 1;
        Some(item.as_str())
    }

    /// Returns the value of `flag` when `arg` is `flag` or `flag=value`.
    ///
    /// `Ok(None)` means that `arg` is a different argument. `Err` means that the value is
    /// missing or empty.
    fn value_of(&mut self, arg: &str, flag: &str) -> Result<Option<String>, String> {
        if arg == flag {
            return match self.take() {
                Some(value) if !value.is_empty() => Ok(Some(value.to_string())),
                _ => Err(format!("{flag} requires a value")),
            };
        }
        if let Some(value) = arg.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            if value.is_empty() {
                return Err(format!("{flag} requires a value"));
            }
            return Ok(Some(value.to_string()));
        }
        Ok(None)
    }
}

fn translate_list(rest: &[String]) -> Translation {
    match rest {
        [] => vsp(&["list", "presets"], ExitMapping::PassThrough),
        [flag] if is_help(flag) => Translation::Usage("Usage: vspcli list".to_string()),
        [other, ..] => unknown_option("list", other),
    }
}

fn translate_detect(rest: &[String]) -> Translation {
    let mut argv = vec!["detect".to_string()];
    let mut path: Option<String> = None;
    for arg in rest {
        match arg.as_str() {
            "-h" | "--help" => {
                return Translation::Usage(
                    "Usage: vspcli detect [--json] [--explain] [path]".to_string(),
                );
            }
            "--json" => argv.push("--json".to_string()),
            "--explain" => argv.push("--explain".to_string()),
            "--no-detect" | "--toolchain" => {
                return Translation::Reject(format!(
                    "detect: '{arg}' is not supported in compatibility mode (use 'vsp detect' or 'vsp sync --with <pack>')"
                ));
            }
            flag if flag.starts_with('-') => return unknown_option("detect", flag),
            value => {
                if path.is_some() {
                    return Translation::Reject("detect accepts one path only".to_string());
                }
                path = Some(value.to_string());
            }
        }
    }
    if let Some(p) = path {
        argv.push("--".to_string());
        argv.push(p);
    }
    Translation::Vsp {
        argv,
        exit: ExitMapping::PassThrough,
        notes: Vec::new(),
    }
}

fn translate_doctor(rest: &[String]) -> Translation {
    let mut argv = vec!["doctor".to_string()];
    for arg in rest {
        match arg.as_str() {
            "-h" | "--help" => {
                return Translation::Usage("Usage: vspcli doctor [--strict]".to_string());
            }
            "--strict" => argv.push("--strict".to_string()),
            other => return unknown_option("doctor", other),
        }
    }
    Translation::Vsp {
        argv,
        exit: ExitMapping::CollapseToFailure,
        notes: Vec::new(),
    }
}

fn translate_open(rest: &[String]) -> Translation {
    match rest {
        [flag] if is_help(flag) => {
            Translation::Usage("Usage: vspcli open <profile> [path]".to_string())
        }
        [profile] if !profile.starts_with('-') => vsp(
            &["open", "-p", profile.as_str(), "--", "."],
            ExitMapping::PassThrough,
        ),
        [profile, path] if !profile.starts_with('-') => vsp(
            &["open", "-p", profile.as_str(), "--", path.as_str()],
            ExitMapping::PassThrough,
        ),
        _ => Translation::Reject("open requires <profile> [path]".to_string()),
    }
}

fn translate_export(rest: &[String]) -> Translation {
    let mut argv = vec!["export".to_string()];
    for arg in rest {
        if is_help(arg) {
            return Translation::Usage("Usage: vspcli export [profile...]".to_string());
        }
        if arg.starts_with('-') {
            return unknown_option("export", arg);
        }
        argv.push(arg.clone());
    }
    Translation::Vsp {
        argv,
        exit: ExitMapping::PassThrough,
        notes: Vec::new(),
    }
}

fn translate_import(rest: &[String]) -> Translation {
    const USAGE: &str =
        "Usage: vspcli import <name> <bundle> [--ux <preset>] [--force] [--dry-run]";
    let mut positional: Vec<String> = Vec::new();
    let mut ux: Option<String> = None;
    let mut force = false;
    let mut dry_run = false;

    let mut cursor = ArgCursor::new(rest);
    while let Some(arg) = cursor.take() {
        match arg {
            "-h" | "--help" => return Translation::Usage(USAGE.to_string()),
            "--force" => force = true,
            "--dry-run" => dry_run = true,
            _ => match cursor.value_of(arg, "--ux") {
                Err(msg) => return Translation::Reject(format!("import: {msg}")),
                Ok(Some(value)) => ux = Some(value),
                Ok(None) => {
                    if arg.starts_with('-') {
                        return unknown_option("import", arg);
                    }
                    positional.push(arg.to_string());
                }
            },
        }
    }
    let [name, bundle] = positional.as_slice() else {
        return Translation::Reject("import requires <name> <bundle>".to_string());
    };

    let mut argv: Vec<String> = Vec::new();
    if dry_run {
        argv.push("--dry-run".to_string());
    }
    for item in ["sync", "--from", bundle.as_str(), "-p", name.as_str()] {
        argv.push(item.to_string());
    }
    if let Some(preset) = ux {
        argv.push("--ux".to_string());
        argv.push(preset);
    }
    if force {
        argv.push("--force".to_string());
    }
    Translation::Vsp {
        argv,
        exit: ExitMapping::PassThrough,
        notes: Vec::new(),
    }
}

fn translate_apply(rest: &[String]) -> Translation {
    const USAGE: &str = "Usage: vspcli apply [--target workspace|profile|global] [--profile <name>] [--toolchain <pack>] [--with <packs>] [--without <packs>] [--ux <preset>] [--no-rtk] [--force] [--dry-run] [path]";
    let mut target: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut with: Vec<String> = Vec::new();
    let mut without: Vec<String> = Vec::new();
    let mut ux: Option<String> = None;
    let mut no_rtk = false;
    let mut force = false;
    let mut dry_run = false;
    let mut path: Option<String> = None;
    let mut notes: Vec<String> = Vec::new();

    let mut cursor = ArgCursor::new(rest);
    while let Some(arg) = cursor.take() {
        match arg {
            "-h" | "--help" => return Translation::Usage(USAGE.to_string()),
            "--no-rtk" => {
                no_rtk = true;
                continue;
            }
            "--force" => {
                force = true;
                continue;
            }
            "--dry-run" => {
                dry_run = true;
                continue;
            }
            "--allow-comment-loss" => {
                notes.push(
                    "note: --allow-comment-loss is ignored; vsp keeps JSONC comments".to_string(),
                );
                continue;
            }
            "--no-detect" | "--ux-in-workspace" | "--rtk-in-workspace" => {
                return Translation::Reject(format!(
                    "apply: '{arg}' is not supported in compatibility mode (see 'vsp sync --help')"
                ));
            }
            _ => {}
        }

        // Flags with a value. The first match wins.
        let mut handled = false;
        for flag in [
            "--target",
            "--profile",
            "--toolchain",
            "--with",
            "--without",
            "--ux",
        ] {
            match cursor.value_of(arg, flag) {
                Err(msg) => return Translation::Reject(format!("apply: {msg}")),
                Ok(None) => {}
                Ok(Some(value)) => {
                    match flag {
                        "--target" => {
                            if target.is_some() {
                                return Translation::Reject(
                                    "apply: --target accepts only one value".to_string(),
                                );
                            }
                            target = Some(value);
                        }
                        "--profile" => profile = Some(value),
                        "--toolchain" | "--with" => with.push(value),
                        "--without" => without.push(value),
                        _ => ux = Some(value),
                    }
                    handled = true;
                    break;
                }
            }
        }
        if handled {
            continue;
        }

        match cursor.value_of(arg, "--check-compat") {
            Err(msg) => return Translation::Reject(format!("apply: {msg}")),
            Ok(None) => {}
            Ok(Some(_)) => {
                notes.push(
                    "note: --check-compat is ignored; run 'vsp dev compat' for the engine compatibility audit"
                        .to_string(),
                );
                continue;
            }
        }

        for flag in [
            "--ux-target",
            "--font",
            "--font-size",
            "--theme",
            "--icon-theme",
        ] {
            if arg == flag || arg.starts_with(&format!("{flag}=")) {
                return Translation::Reject(format!(
                    "apply: '{flag}' is not supported in compatibility mode (set these values in the user config 'overrides'; see 'vsp init')"
                ));
            }
        }

        if arg.starts_with('-') {
            return unknown_option("apply", arg);
        }
        if path.is_some() {
            return Translation::Reject("apply accepts one path only".to_string());
        }
        path = Some(arg.to_string());
    }

    let mut argv: Vec<String> = Vec::new();
    if dry_run {
        argv.push("--dry-run".to_string());
    }
    argv.push("sync".to_string());
    if !with.is_empty() {
        argv.push("--with".to_string());
        argv.push(with.join(","));
    }
    if !without.is_empty() {
        argv.push("--without".to_string());
        argv.push(without.join(","));
    }
    if let Some(preset) = ux {
        argv.push("--ux".to_string());
        argv.push(preset);
    }
    if no_rtk {
        argv.push("--no-rtk".to_string());
    }
    if force {
        argv.push("--force".to_string());
    }

    match target.as_deref().unwrap_or("workspace") {
        "workspace" => {
            if profile.is_some() {
                return Translation::Reject(
                    "apply: --profile requires --target=profile".to_string(),
                );
            }
            argv.push("--".to_string());
            argv.push(path.unwrap_or_else(|| ".".to_string()));
        }
        "profile" => {
            let Some(name) = profile else {
                return Translation::Reject(
                    "apply: --target=profile requires --profile <name>".to_string(),
                );
            };
            if path.is_some() {
                return Translation::Reject(
                    "apply: a path is not accepted with --target=profile".to_string(),
                );
            }
            argv.push("-p".to_string());
            argv.push(name);
        }
        "global" => {
            if profile.is_some() || path.is_some() {
                return Translation::Reject(
                    "apply: --target=global accepts no --profile and no path".to_string(),
                );
            }
            argv.push("--user".to_string());
        }
        other => {
            return Translation::Reject(format!(
                "apply: invalid --target '{other}' (expected workspace, profile, or global)"
            ));
        }
    }

    Translation::Vsp {
        argv,
        exit: ExitMapping::PassThrough,
        notes,
    }
}

fn translate_reconcile(rest: &[String]) -> Translation {
    const USAGE: &str = "Usage: vspcli reconcile (--profile <name> [--as <recipe>] | --all) [--dry-run] [--compromised-only] [--prune-retired] [--prune-unmanaged] [--yes]";
    let mut profile: Option<String> = None;
    let mut recipe: Option<String> = None;
    let mut all = false;
    let mut dry_run = false;
    let mut yes = false;
    let mut flags: Vec<&str> = Vec::new();

    let mut cursor = ArgCursor::new(rest);
    while let Some(arg) = cursor.take() {
        match arg {
            "-h" | "--help" => return Translation::Usage(USAGE.to_string()),
            "--all" => all = true,
            "--dry-run" => dry_run = true,
            "-y" | "--yes" => yes = true,
            "--compromised-only" => flags.push("--compromised-only"),
            "--prune-retired" => flags.push("--prune-retired"),
            "--prune-unmanaged" => flags.push("--prune-unmanaged"),
            _ => {
                match cursor.value_of(arg, "--profile") {
                    Err(msg) => return Translation::Reject(format!("reconcile: {msg}")),
                    Ok(Some(value)) => {
                        profile = Some(value);
                        continue;
                    }
                    Ok(None) => {}
                }
                match cursor.value_of(arg, "--as") {
                    Err(msg) => return Translation::Reject(format!("reconcile: {msg}")),
                    Ok(Some(value)) => {
                        recipe = Some(value);
                        continue;
                    }
                    Ok(None) => {}
                }
                return unknown_option("reconcile", arg);
            }
        }
    }

    if !all && profile.is_none() {
        return Translation::Reject("reconcile: specify --profile <name> or --all".to_string());
    }

    let mut argv: Vec<String> = Vec::new();
    if dry_run {
        argv.push("--dry-run".to_string());
    }
    if yes {
        argv.push("--yes".to_string());
    }
    argv.push("sync".to_string());
    if all {
        argv.push("--all".to_string());
    } else if let Some(name) = profile {
        argv.push("-p".to_string());
        argv.push(name);
        if let Some(preset) = recipe {
            argv.push("--preset".to_string());
            argv.push(preset);
        }
    }
    argv.extend(flags.into_iter().map(str::to_string));

    Translation::Vsp {
        argv,
        exit: ExitMapping::CollapseToFailure,
        notes: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_loader::load_embedded;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    fn vsp_argv(items: &[&str]) -> Vec<String> {
        match translate(&args(items)) {
            Translation::Vsp { argv, .. } => argv,
            other => panic!("expected Vsp translation for {items:?}, got {other:?}"),
        }
    }

    fn assert_reject(items: &[&str]) {
        assert!(
            matches!(translate(&args(items)), Translation::Reject(_)),
            "expected Reject for {items:?}"
        );
    }

    #[test]
    fn detects_compat_mode_from_argv0_and_subcommand() {
        assert!(is_vspcli_invocation(OsStr::new("vspcli")));
        assert!(is_vspcli_invocation(OsStr::new("/opt/homebrew/bin/vspcli")));
        assert!(!is_vspcli_invocation(OsStr::new("/usr/local/bin/vsp")));
        assert!(!is_vspcli_invocation(OsStr::new("vspcli-old")));

        let via_link: Vec<OsString> = ["/x/vspcli", "list"].iter().map(OsString::from).collect();
        assert_eq!(
            legacy_args_from_argv(&via_link),
            Some(vec![OsString::from("list")])
        );
        let via_sub: Vec<OsString> = ["vsp", COMPAT_SUBCOMMAND, "--list"]
            .iter()
            .map(OsString::from)
            .collect();
        assert_eq!(
            legacy_args_from_argv(&via_sub),
            Some(vec![OsString::from("--list")])
        );
    }

    #[test]
    fn translates_read_only_verbs() {
        assert_eq!(translate(&[]), Translation::Help);
        assert_eq!(translate(&args(&["--help"])), Translation::Help);
        assert_eq!(translate(&args(&["--version"])), Translation::Version);
        assert_eq!(vsp_argv(&["list"]), args(&["list", "presets"]));
        assert_eq!(vsp_argv(&["--list"]), args(&["list", "presets"]));
        assert_eq!(vsp_argv(&["validate"]), args(&["dev", "validate"]));
        assert_eq!(
            vsp_argv(&["detect", "--json", "ws"]),
            args(&["detect", "--json", "--", "ws"])
        );
        assert_eq!(
            vsp_argv(&["doctor", "--strict"]),
            args(&["doctor", "--strict"])
        );
        assert_eq!(
            translate(&args(&["completions", "zsh"])),
            Translation::Completions("zsh".to_string())
        );
        assert_eq!(
            translate(&args(&["--completion=fish"])),
            Translation::Completions("fish".to_string())
        );
    }

    #[test]
    fn translates_apply_targets_and_deprecated_detect_flag() {
        assert_eq!(vsp_argv(&["apply"]), args(&["sync", "--", "."]));
        assert_eq!(
            vsp_argv(&[
                "apply",
                "--dry-run",
                "--with",
                "git-pro",
                "--ux=retina",
                "ws"
            ]),
            args(&[
                "--dry-run",
                "sync",
                "--with",
                "git-pro",
                "--ux",
                "retina",
                "--",
                "ws"
            ])
        );
        assert_eq!(
            vsp_argv(&["apply", "--target=profile", "--profile", "rust"]),
            args(&["sync", "-p", "rust"])
        );
        assert_eq!(
            vsp_argv(&["apply", "--target", "global", "--no-rtk"]),
            args(&["sync", "--no-rtk", "--user"])
        );
        match translate(&args(&["--detect", "ws"])) {
            Translation::Vsp { argv, notes, .. } => {
                assert_eq!(argv, args(&["sync", "--", "ws"]));
                assert!(notes[0].contains("deprecated"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_reject(&["apply", "--target=profile"]);
        assert_reject(&["apply", "--target", "moon"]);
        assert_reject(&["apply", "--with"]);
        assert_reject(&["apply", "--bogus"]);
        assert_reject(&["apply", "--font", "X"]);
    }

    #[test]
    fn translates_reconcile_import_open_and_install_ext() {
        assert_eq!(
            vsp_argv(&[
                "reconcile",
                "--profile",
                "rust",
                "--as=rust",
                "--prune-retired"
            ]),
            args(&["sync", "-p", "rust", "--preset", "rust", "--prune-retired"])
        );
        assert_eq!(
            vsp_argv(&["reconcile", "--all", "--dry-run", "--compromised-only"]),
            args(&["--dry-run", "sync", "--all", "--compromised-only"])
        );
        assert_reject(&["reconcile"]);
        assert_reject(&["reconcile", "--profile"]);
        assert_eq!(
            vsp_argv(&["import", "rust", "b.code-profile", "--ux", "crisp"]),
            args(&[
                "sync",
                "--from",
                "b.code-profile",
                "-p",
                "rust",
                "--ux",
                "crisp"
            ])
        );
        assert_reject(&["import", "rust"]);
        assert_eq!(
            vsp_argv(&["open", "rust"]),
            args(&["open", "-p", "rust", "--", "."])
        );
        assert_reject(&["open"]);
        assert_eq!(
            translate(&args(&["install-ext", "rust", "acme.tool"])),
            Translation::InstallExt {
                profile: "rust".to_string(),
                id: "acme.tool".to_string()
            }
        );
        assert_reject(&["install-ext", "rust"]);
    }

    #[test]
    fn rejects_unknown_and_removed_commands() {
        assert_reject(&["--bogus"]);
        assert_reject(&["frobnicate"]);
        assert_reject(&["build"]);
        assert_reject(&["--migrate-catalog"]);
        assert_reject(&["list", "--bogus"]);
        assert_reject(&["doctor", "--bogus"]);
        assert_reject(&["completions"]);
    }

    #[test]
    fn exit_mapping_collapses_only_for_legacy_single_failure_commands() {
        assert_eq!(
            ExitMapping::CollapseToFailure.apply(ExitCode::IncompleteObservation),
            ExitCode::Operational
        );
        assert_eq!(
            ExitMapping::CollapseToFailure.apply(ExitCode::UsageOrConfig),
            ExitCode::UsageOrConfig
        );
        assert_eq!(
            ExitMapping::PassThrough.apply(ExitCode::SafetyRefusal),
            ExitCode::SafetyRefusal
        );
    }

    #[test]
    fn completions_include_live_presets_and_packs() {
        let catalog = load_embedded().expect("embedded catalog");
        for shell in ["bash", "zsh", "fish"] {
            let script = render_completions(shell, &catalog).expect("script");
            assert!(script.contains("vspcli"), "{shell}");
            assert!(script.contains("rust"), "{shell}");
            assert!(script.contains("git-pro"), "{shell}");
        }
        let err = render_completions("tcsh", &catalog).expect_err("unsupported shell");
        assert_eq!(err.exit_code(), ExitCode::UsageOrConfig);
    }
}

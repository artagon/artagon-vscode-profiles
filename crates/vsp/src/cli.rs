//! `clap` Derive CLI definitions and command dispatch for `vsp`.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use clap::builder::PossibleValuesParser;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;
use serde_json::Map;

use crate::detect::detect_workspace;
use crate::dev_ops::{
    run_dev_compat, run_dev_docs, run_dev_hooks, run_dev_pins, CompatOptions,
};
use crate::doctor::{audit_user_config, run_doctor, CheckSeverity};
use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{
    AiMode, ModuleKind, UxPreset, WorkspaceCatalog,
    LEGACY_BUNDLE_ALIASES,
};
use crate::open::{run_open, OpenOptions};
use crate::plan::{
    compute_plan, validate_selector_combination, Plan, PlanOptions, SelectorKinds,
};
use crate::policy::validate_repository;
use crate::resolver::{
    build_dist, compile_canonical_profile_settings, compile_profile_extensions,
    export_profiles_with_additions, resolve_profile_recipe, to_canonical_json_string,
};
use crate::schema_loader::{load_catalog_auto, resolve_repo_dir};
use crate::self_ops::{self, LinkAction, SelfLinkReport};
use crate::sync::{prepare_bundle_import, run_sync, PruneMode, SyncOptions, SyncReport};
use crate::user_config::{
    apply_user_config, check_user_config_target, load_user_config, parse_user_config,
    render_starter_config, resolve_user_config_path, write_user_config, StarterSeed, UserConfig,
    UserDefaults, UserOverlay, UserProfile,
};
use crate::vscode::{list_storage_profiles, resolve_vscode_store};

/// Unified CLI for Artagon VS Code profiles, capability packs, and workspace plans.
#[derive(Debug, Parser)]
#[command(
    name = "vsp",
    version,
    about = "Manage Artagon VS Code capability packs, workspaces, and profiles",
    propagate_version = false
)]
pub struct Cli {
    /// Preview actions without modifying files.
    #[arg(short = 'n', long = "dry-run", global = true)]
    pub dry_run: bool,

    /// Emit machine-readable JSON output.
    #[arg(long = "json", global = true)]
    pub json: bool,

    /// Assume yes for interactive confirmations.
    #[arg(short = 'y', long = "yes", global = true)]
    pub yes: bool,

    /// Suppress non-essential output.
    #[arg(short = 'q', long = "quiet", global = true)]
    pub quiet: bool,

    /// Enable verbose diagnostic output.
    #[arg(short = 'v', long = "verbose", global = true)]
    pub verbose: bool,

    /// Override catalog/repository root directory.
    #[arg(long = "catalog", value_name = "DIR", global = true)]
    pub catalog: Option<PathBuf>,

    /// Override user configuration file path (`~/.config/vsp/config.jsonc`).
    #[arg(long = "config", value_name = "PATH", global = true)]
    pub config: Option<PathBuf>,

    /// Override VS Code user data directory.
    #[arg(long = "vscode-data-dir", value_name = "DIR", global = true)]
    pub vscode_data_dir: Option<PathBuf>,

    /// Subcommand to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level `vsp` subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List canonical presets, packs, UX presets, or installed VS Code profiles.
    List {
        /// Category to list (defaults to `presets`).
        #[arg(value_enum)]
        kind: Option<ListKind>,
    },

    /// Inspect a canonical preset, pack/module, UX preset, or legacy alias.
    Show {
        /// Name of preset, pack, UX preset, or legacy alias.
        name: String,
    },

    /// Strictly read-only workspace toolchain and capability detector.
    Detect {
        /// Workspace directory to inspect (defaults to `.`).
        path: Option<PathBuf>,

        /// Show matched signals, rules, and superseded modules.
        #[arg(long)]
        explain: bool,
    },

    /// Compute a read-only `vsp-plan/v1` diff for a workspace, profile, preset, or user target.
    Diff(DiffArgs),

    /// Audit environment, catalog, Default-profile APPLICATION security keys, and live VS Code profiles.
    Doctor {
        /// Fail on warnings in addition to hard errors.
        #[arg(long)]
        strict: bool,
    },

    /// Export deterministic UX-agnostic `.code-profile` bundles and `SHA256SUMS`.
    Export {
        /// Optional profile names (defaults to all canonical profiles).
        names: Vec<String>,

        /// Output directory for exported bundles (defaults to `./exports`).
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
    },

    /// Generate shell completion scripts with live preset and pack names from the catalog.
    Completions {
        /// Target shell (`bash`, `zsh`, or `fish`).
        #[arg(value_enum)]
        shell: CompletionShell,
    },

    /// Create a commented starter user config (`~/.config/vsp/config.jsonc` or `--config`).
    Init(InitArgs),

    /// Apply or preview profile/workspace synchronization (`--dry-run` runs the unified planner).
    Sync(SyncArgs),

    /// Open a workspace in a VS Code profile (hidden alias: `use`).
    #[command(alias = "use")]
    Open(OpenArgs),

    /// Repository maintainer and release workflows.
    Dev {
        /// Developer subcommand.
        #[command(subcommand)]
        command: DevCommand,
    },

    /// Manage the `vsp` installation itself.
    #[command(name = "self")]
    SelfCmd {
        /// Self-management subcommand.
        #[command(subcommand)]
        command: SelfCommand,
    },

    /// Run a legacy `vspcli` command line in compatibility mode.
    #[command(name = "compat-vspcli", hide = true, disable_help_flag = true)]
    CompatVspcli {
        /// Legacy `vspcli` arguments, passed through unchanged.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<OsString>,
    },
}

/// Resource categories accepted by `vsp list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ListKind {
    /// Canonical profile recipes (`profiles/`).
    Presets,
    /// Composable capability and toolchain packs (`modules/`).
    Packs,
    /// Display UX presets (`crisp`, `retina`, `default`).
    Ux,
    /// Registered VS Code profiles in `globalStorage/storage.json`.
    Installed,
}

/// Shell targets accepted by `vsp completions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CompletionShell {
    /// GNU Bash completion script.
    Bash,
    /// Zsh completion script.
    Zsh,
    /// Fish completion script.
    Fish,
}

/// UX preset CLI value enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UxArg {
    /// Crisp 1x QHD preset.
    Crisp,
    /// Retina 2x HiDPI preset.
    Retina,
    /// UX-agnostic default preset.
    Default,
}

impl From<UxArg> for UxPreset {
    fn from(arg: UxArg) -> Self {
        match arg {
            UxArg::Crisp => Self::Crisp,
            UxArg::Retina => Self::Retina,
            UxArg::Default => Self::Default,
        }
    }
}

/// AI composition mode CLI value enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AiArg {
    /// Full AI agent suite (`ai-agents`).
    Full,
    /// Strict Copilot-only (`ai-copilot`).
    Copilot,
    /// Disable AI extensions and built-in chat features.
    Off,
}

impl From<AiArg> for AiMode {
    fn from(arg: AiArg) -> Self {
        match arg {
            AiArg::Full => Self::Full,
            AiArg::Copilot => Self::Copilot,
            AiArg::Off => Self::Off,
        }
    }
}

/// Shared selector and composition flags for `vsp diff` and `vsp sync`.
#[derive(Debug, Clone, Args)]
pub struct SelectorArgs {
    /// Workspace directory path.
    pub path: Option<PathBuf>,

    /// Explicit workspace path alias for positional `PATH`.
    #[arg(long = "workspace", value_name = "PATH")]
    pub workspace: Option<PathBuf>,

    /// Target VS Code profile name.
    #[arg(short = 'p', long = "profile", value_name = "NAME")]
    pub profile: Option<String>,

    /// Canonical or legacy preset/recipe name.
    #[arg(long = "preset", value_name = "NAME")]
    pub preset: Option<String>,

    /// Target the VS Code Default user profile.
    #[arg(long = "user")]
    pub user: bool,

    /// Additional pack/module IDs (comma-separated or repeatable).
    #[arg(long = "with", value_name = "ID,..", value_delimiter = ',')]
    pub with: Vec<String>,

    /// Excluded pack/module IDs (comma-separated or repeatable).
    #[arg(long = "without", value_name = "ID,..", value_delimiter = ',')]
    pub without: Vec<String>,

    /// Display UX preset (`crisp`, `retina`, or `default`).
    #[arg(long = "ux", value_enum)]
    pub ux: Option<UxArg>,

    /// AI mode (`full`, `copilot`, or `off`).
    #[arg(long = "ai", value_enum)]
    pub ai: Option<AiArg>,

    /// Shorthand for `--ai off`.
    #[arg(long = "no-ai")]
    pub no_ai: bool,
}

impl SelectorArgs {
    /// Resolves effective [`AiMode`], rejecting conflicting `--ai` + `--no-ai` combinations.
    ///
    /// # Errors
    ///
    /// Returns [`VspError::UsageOrConfig`] (exit `4`) when `--no-ai` is combined with
    /// `--ai full` or `--ai copilot`.
    pub fn effective_ai(&self) -> VspResult<Option<AiMode>> {
        match (self.ai, self.no_ai) {
            (Some(AiArg::Full | AiArg::Copilot), true) => Err(VspError::UsageOrConfig(
                "contradictory flags: --no-ai cannot be combined with --ai full|copilot"
                    .to_string(),
            )),
            (_, true) => Ok(Some(AiMode::Off)),
            (Some(mode), false) => Ok(Some(mode.into())),
            (None, false) => Ok(None),
        }
    }

    /// Resolves effective workspace path, rejecting conflicting positional + `--workspace`.
    ///
    /// # Errors
    ///
    /// Returns [`VspError::UsageOrConfig`] (exit `4`) if both positional `PATH` and
    /// `--workspace` are provided.
    pub fn effective_workspace(&self) -> VspResult<Option<PathBuf>> {
        match (&self.path, &self.workspace) {
            (Some(_), Some(_)) => Err(VspError::UsageOrConfig(
                "specify either positional <PATH> or --workspace <PATH>, not both".to_string(),
            )),
            (Some(p), None) | (None, Some(p)) => Ok(Some(p.clone())),
            (None, None) => Ok(None),
        }
    }

    /// Converts these CLI flags into [`PlanOptions`].
    ///
    /// # Errors
    ///
    /// Returns [`VspError::UsageOrConfig`] on contradictory flags.
    pub fn to_plan_options(&self, vscode_data_dir: Option<PathBuf>) -> VspResult<PlanOptions> {
        Ok(PlanOptions {
            workspace_path: self.effective_workspace()?,
            profile: self.profile.clone(),
            preset: self.preset.clone(),
            user: self.user,
            with_modules: self.with.clone(),
            without_modules: self.without.clone(),
            ux: self.ux.map(Into::into),
            ai: self.effective_ai()?,
            vscode_data_dir,
            user_overlay: UserOverlay::default(),
        })
    }
}

/// Arguments for `vsp diff`.
#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    /// Target selector and composition flags.
    #[command(flatten)]
    pub selector: SelectorArgs,

    /// Exit `3` if drift exists, or `0` if clean.
    #[arg(long = "check")]
    pub check: bool,
}

/// Extension pruning scope accepted by `vsp sync --prune[=MODE]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PruneArg {
    /// Prune retired (plus superseded/removed) catalog extensions.
    Retired,
    /// Prune retired AND catalog-known unmanaged extensions (requires `--yes` in non-TTY).
    Unmanaged,
    /// Alias for `unmanaged` (prunes retired and unmanaged catalog-known extensions).
    All,
}

impl From<PruneArg> for PruneMode {
    fn from(arg: PruneArg) -> Self {
        match arg {
            PruneArg::Retired => Self::Retired,
            PruneArg::Unmanaged | PruneArg::All => Self::Unmanaged,
        }
    }
}

/// Arguments for `vsp sync`.
#[derive(Debug, Clone, Args)]
pub struct SyncArgs {
    /// Target selector and composition flags.
    #[command(flatten)]
    pub selector: SelectorArgs,

    /// Import a `.code-profile` bundle file.
    #[arg(long = "from", value_name = "BUNDLE")]
    pub from: Option<PathBuf>,

    /// Reconcile all registered VS Code profiles plus Default.
    #[arg(long = "all")]
    pub all: bool,

    /// Only execute Step 0 compromised/removed extension remediation.
    #[arg(long = "compromised-only")]
    pub compromised_only: bool,

    /// Prune retired (`--prune` / `--prune=retired`) or unmanaged (`--prune=unmanaged`) extensions.
    #[arg(
        long = "prune",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "retired"
    )]
    pub prune: Option<PruneArg>,

    /// Legacy compatibility flag equivalent to `--prune=retired`.
    #[arg(long = "prune-retired", hide = true)]
    pub prune_retired: bool,

    /// Legacy compatibility flag equivalent to `--prune=unmanaged`.
    #[arg(long = "prune-unmanaged", hide = true)]
    pub prune_unmanaged: bool,

    /// Re-verify and repair drifted pinned extensions.
    #[arg(long = "repair-pins")]
    pub repair_pins: bool,

    /// Bypass fail-closed VSIX pin checks (prints warning & records unverified state).
    #[arg(long = "allow-unlocked")]
    pub allow_unlocked: bool,

    /// Allow bundle settings matching the autonomy/exec deny-list.
    #[arg(long = "allow-unsafe-settings")]
    pub allow_unsafe_settings: bool,

    /// Allow mutating VS Code `storage.json` while VS Code is running.
    #[arg(long = "force")]
    pub force: bool,

    /// Install extensions serially instead of batched.
    #[arg(long = "no-batch")]
    pub no_batch: bool,

    /// Skip XDG RTK shell wrapper installation.
    #[arg(long = "no-rtk")]
    pub no_rtk: bool,
}

impl SyncArgs {
    /// Resolves the effective [`PruneMode`] combining `--prune`, `--prune-retired`, and `--prune-unmanaged`.
    #[must_use]
    pub fn effective_prune(&self) -> Option<PruneMode> {
        if self.prune_unmanaged {
            return Some(PruneMode::Unmanaged);
        }
        if let Some(p) = self.prune {
            return Some(p.into());
        }
        if self.prune_retired {
            return Some(PruneMode::Retired);
        }
        None
    }
}

/// Arguments for `vsp init`.
#[derive(Debug, Clone, Args)]
pub struct InitArgs {
    /// After writing the config, synchronize the target given by `-p/--profile` or `--user`.
    #[arg(long = "apply")]
    pub apply: bool,

    /// Replace an existing user config file.
    #[arg(long = "force")]
    pub force: bool,

    /// Custom profile to add to the config (and the `--apply` target).
    #[arg(short = 'p', long = "profile", value_name = "NAME")]
    pub profile: Option<String>,

    /// Catalog preset or pack that the custom profile extends.
    #[arg(long = "preset", value_name = "NAME")]
    pub preset: Option<String>,

    /// With `--apply`, synchronize the VS Code Default user profile.
    #[arg(long = "user")]
    pub user: bool,

    /// Pack IDs to add to the custom profile (comma-separated or repeatable).
    #[arg(long = "with", value_name = "ID,..", value_delimiter = ',')]
    pub with: Vec<String>,

    /// Pack IDs to exclude from the custom profile (comma-separated or repeatable).
    #[arg(long = "without", value_name = "ID,..", value_delimiter = ',')]
    pub without: Vec<String>,

    /// Value for `defaults.ux` (`crisp`, `retina`, or `default`).
    #[arg(long = "ux", value_enum)]
    pub ux: Option<UxArg>,

    /// Value for `defaults.ai` (`full`, `copilot`, or `off`).
    #[arg(long = "ai", value_enum)]
    pub ai: Option<AiArg>,

    /// Shorthand for `--ai off`.
    #[arg(long = "no-ai")]
    pub no_ai: bool,

    /// Set `defaults.rtkTerminal` to `true`.
    #[arg(long = "rtk-terminal")]
    pub rtk_terminal: bool,

    /// Set `defaults.rtkTerminal` to `false`.
    #[arg(long = "no-rtk")]
    pub no_rtk: bool,
}

/// Arguments for `vsp open` (alias `use`).
#[derive(Debug, Clone, Args)]
pub struct OpenArgs {
    /// Optional workspace path to open.
    pub path: Option<PathBuf>,

    /// Named VS Code profile to open.
    #[arg(short = 'p', long = "profile", value_name = "NAME")]
    pub profile: Option<String>,

    /// Synchronize target before opening (aborts launch on sync failure).
    #[arg(long = "sync")]
    pub sync: bool,

    /// Allow mutating VS Code `storage.json` while VS Code is running during `--sync`.
    #[arg(long = "force")]
    pub force: bool,
}

/// Maintainer subcommands under `vsp dev`.
#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Validate catalog schema, publisher allowlist, pins, DAG, collisions, scopes, security, and UX symmetry.
    Validate,

    /// Emit 15 canonical `.code-profile` bundles, 22 materialized legacy aliases, and sorted `SHA256SUMS`.
    Dist {
        /// Output directory for release bundles (defaults to `dist`).
        #[arg(long = "out", value_name = "DIR", default_value = "dist")]
        out: PathBuf,
    },

    /// Validate (`--check`) or refresh (`--refresh`) `catalog/extension-pins.json`.
    Pins {
        /// Verify pin manifest digests, semver versions, and Marketplace URLs.
        #[arg(long)]
        check: bool,

        /// Refresh pins from the VS Code Marketplace and rewrite `catalog/extension-pins.json`.
        #[arg(long)]
        refresh: bool,
    },

    /// Audit VS Code Marketplace engine compatibility across profiles or an extensions file.
    Compat {
        /// Audit all canonical profiles in the catalog.
        #[arg(long)]
        all: bool,

        /// Path to newline-separated or JSON array extensions file.
        #[arg(long = "extensions-file", value_name = "PATH")]
        extensions_file: Option<PathBuf>,

        /// Override target VS Code semver version (e.g. `1.147.0`).
        #[arg(long = "vscode-version", value_name = "VER")]
        vscode_version: Option<String>,

        /// Query Marketplace engine constraints without requiring locally installed extensions.
        #[arg(long = "marketplace-only")]
        marketplace_only: bool,

        /// Specific profile names to audit.
        profiles: Vec<String>,
    },

    /// Check (`--check`) or synchronize (`--write`) Rust docs policy markers and `README.md` matrix.
    Docs {
        /// Verify agent doc policy markers and README matrix are in sync without modifying files.
        #[arg(long)]
        check: bool,

        /// Rewrite agent doc policy markers and README matrix atomically.
        #[arg(long)]
        write: bool,
    },

    /// Configure repository git hooks (`core.hooksPath = scripts/git-hooks`).
    Hooks,

    /// Show repository test suite entrypoints.
    Test,

    /// Release distribution workflow helper.
    Release,
}

/// Self-management subcommands under `vsp self`.
#[derive(Debug, Subcommand)]
pub enum SelfCommand {
    /// Print the canonical path of the running `vsp` binary.
    Path,

    /// Create a `vsp` symbolic link in the user bin directory (`~/.local/bin` by default).
    Install {
        /// Directory for the link (default `${XDG_BIN_HOME:-~/.local/bin}`).
        #[arg(long = "bin-dir", alias = "dir", value_name = "DIR")]
        bin_dir: Option<PathBuf>,

        /// Also create a `vspcli` link that runs the legacy compatibility mode.
        #[arg(long = "with-vspcli")]
        with_vspcli: bool,
    },

    /// Remove the `vsp` symbolic link from the user bin directory.
    Uninstall {
        /// Directory that contains the link (default `${XDG_BIN_HOME:-~/.local/bin}`).
        #[arg(long = "bin-dir", alias = "dir", value_name = "DIR")]
        bin_dir: Option<PathBuf>,
    },

    /// Print the current version and upgrade guidance (no network access).
    Update,
}

/// Executes a parsed [`Cli`] command and returns its canonical [`ExitCode`].
///
/// # Errors
///
/// Returns [`VspError`] carrying the exact exit code (`1..=7`) on operational,
/// missing-target, negative-check, usage/config, safety, lock, or incomplete-observation errors.
pub fn execute(cli: Cli) -> VspResult<ExitCode> {
    let Cli {
        dry_run,
        json,
        yes,
        quiet,
        catalog: catalog_override,
        config: config_override,
        vscode_data_dir,
        command,
        ..
    } = cli;

    match command {
        Command::Completions { shell } => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            let mut cmd = command_with_live_values(&catalog);
            let clap_shell = match shell {
                CompletionShell::Bash => Shell::Bash,
                CompletionShell::Zsh => Shell::Zsh,
                CompletionShell::Fish => Shell::Fish,
            };
            clap_complete::generate(clap_shell, &mut cmd, "vsp", &mut io::stdout());
            Ok(ExitCode::Ok)
        }
        Command::Init(args) => run_init(
            &args,
            catalog_override.as_deref(),
            config_override.as_deref(),
            vscode_data_dir,
            dry_run,
            json,
            yes,
            quiet,
        ),
        Command::CompatVspcli { args } => Ok(crate::compat_vspcli::run(&args)),
        Command::SelfCmd { command } => run_self(command, dry_run, json, quiet),
        Command::List { kind } => {
            let kind = kind.unwrap_or(ListKind::Presets);
            if kind == ListKind::Installed {
                return run_list_installed(vscode_data_dir.as_deref(), json);
            }
            let (catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            run_list(&catalog, &user.overlay, kind, json)
        }
        Command::Show { name } => {
            let (mut catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            // A toolchain pack ID shows the pack. `--preset <toolchain>` uses its implicit
            // preset, which `show` reports for a name that is not a pack or preset alias.
            if !catalog.modules.contains_key(&name) {
                catalog.materialize_implicit_preset(&name);
            }
            run_show(&catalog, &user.overlay, &name, json)
        }
        Command::Detect { path, explain } => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            let target = path.unwrap_or_else(|| PathBuf::from("."));
            run_detect(&catalog, &target, explain, json, quiet)
        }
        Command::Diff(args) => {
            let (mut catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            materialize_selector_presets(&mut catalog, &args.selector);
            let mut opts = args.selector.to_plan_options(vscode_data_dir)?;
            apply_user_context_to_plan(&catalog, &mut opts, &args.selector, &user, false);
            let plan = compute_plan(&catalog, &opts)?;
            print_plan(&plan, json, quiet)?;
            Ok(plan_exit_code(&plan, args.check))
        }
        Command::Doctor { strict } => {
            // Repository validation must see the unchanged catalog, so the user profiles
            // are resolved against a copy.
            let catalog = load_catalog(catalog_override.as_deref())?;
            let user_config = load_user_config(config_override.as_deref())?;
            let overlay = match &user_config {
                Some(cfg) => apply_user_config(&mut catalog.clone(), cfg)?,
                None => UserOverlay::default(),
            };
            let mut report = run_doctor(&catalog, vscode_data_dir.as_deref(), strict);
            audit_user_config(
                &mut report,
                &resolve_user_config_path(config_override.as_deref()),
                user_config.as_ref(),
                &overlay,
            );
            if json {
                print!("{}", to_canonical_json_string(&report)?);
            } else if !quiet {
                for check in &report.checks {
                    let tag = match check.severity {
                        CheckSeverity::Pass => "PASS",
                        CheckSeverity::Warn => "WARN",
                        CheckSeverity::Fail => "FAIL",
                    };
                    println!("{tag}: {}", check.message);
                }
                println!(
                    "Summary: {} fail(s), {} warning(s)",
                    report.fails, report.warns
                );
            }
            Ok(report.exit_code())
        }
        Command::Export { names, out } => {
            let (mut catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            for name in &names {
                catalog.materialize_implicit_preset(name);
            }
            let out_dir = out.unwrap_or_else(|| PathBuf::from("exports"));
            let report = export_profiles_with_additions(
                &catalog,
                &names,
                &out_dir,
                false,
                &user.overlay.bundle_additions(),
            )?;
            print_dist_report(&report, json, quiet)?;
            Ok(ExitCode::Ok)
        }
        Command::Sync(args) => {
            let has_phase2_selector = args.from.is_some() || args.all;
            let has_std_selector = args.selector.path.is_some()
                || args.selector.workspace.is_some()
                || args.selector.profile.is_some()
                || args.selector.preset.is_some()
                || args.selector.user;
            if !has_std_selector && !has_phase2_selector {
                return Err(VspError::UsageOrConfig(
                    "vsp sync requires an explicit selector (<PATH>, -p/--profile, --preset, --user, --from, or --all)"
                        .to_string(),
                ));
            }
            let effective_ai = args.selector.effective_ai()?;
            let effective_ws = args.selector.effective_workspace()?;
            // The same contradictory-selector rules as the mutating engine, applied before
            // any preview is printed (`sync --dry-run --all -p x` is exit 4, not a preview).
            validate_selector_combination(SelectorKinds {
                workspace: effective_ws.is_some(),
                profile: args.selector.profile.is_some(),
                user: args.selector.user,
                from_bundle: args.from.is_some(),
                all: args.all,
            })?;
            if let Some(ref bundle) = args.from {
                if !bundle.is_file() {
                    return Err(VspError::TargetMissing(format!(
                        "bundle file not found: {}",
                        bundle.display()
                    )));
                }
            }

            let (mut catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            materialize_selector_presets(&mut catalog, &args.selector);

            // Unified read-only planner path: `vsp sync --dry-run` emits the EXACT same
            // `vsp-plan/v1` JSON and text output as `vsp diff`!
            if dry_run && has_std_selector && args.from.is_none() && !args.all && !args.compromised_only {
                let mut opts = args.selector.to_plan_options(vscode_data_dir)?;
                apply_user_context_to_plan(&catalog, &mut opts, &args.selector, &user, args.no_rtk);
                let plan = compute_plan(&catalog, &opts)?;
                print_plan(&plan, json, quiet)?;
                return Ok(plan_exit_code(&plan, false));
            }

            // Personal defaults apply to a single personal target only: not to workspaces,
            // bundle imports, `--all` sweeps, or `--compromised-only` remediation.
            let defaults_apply = personal_defaults_apply(&args.selector)
                && args.from.is_none()
                && !args.all
                && !args.compromised_only;
            let personal = if defaults_apply {
                resolve_personal_defaults(
                    &catalog,
                    &args.selector,
                    args.selector.ux.map(Into::into),
                    effective_ai,
                    args.no_rtk,
                    user.defaults(),
                )
            } else {
                PersonalDefaults {
                    ux: args.selector.ux.map(Into::into),
                    ai: effective_ai,
                    with_modules: args.selector.with.clone(),
                    no_rtk: args.no_rtk,
                }
            };

            let sync_opts = SyncOptions {
                workspace_path: effective_ws,
                profile: args.selector.profile.clone(),
                preset: args.selector.preset.clone(),
                from_bundle: args.from.clone(),
                user: args.selector.user,
                all: args.all,
                compromised_only: args.compromised_only,
                with_modules: personal.with_modules,
                without_modules: args.selector.without.clone(),
                ux: personal.ux,
                ai: personal.ai,
                prune: args.effective_prune(),
                repair_pins: args.repair_pins,
                allow_unlocked: args.allow_unlocked,
                allow_unsafe_settings: args.allow_unsafe_settings,
                force: args.force,
                yes,
                no_batch: args.no_batch,
                no_rtk: personal.no_rtk,
                vscode_data_dir,
                user_overlay: user.overlay,
            };

            if dry_run {
                // `--from`, `--all`, and `--compromised-only` previews. A bundle goes
                // through the full pre-mutation validation of the real import (checksum,
                // schema, deny-list, profile name, extension IDs, publisher allowlist).
                let bundle_plan = match &sync_opts.from_bundle {
                    Some(bundle) => Some(prepare_bundle_import(&catalog, bundle, &sync_opts)?),
                    None => None,
                };
                if json {
                    let payload = serde_json::json!({
                        "dry_run": true,
                        "all": args.all,
                        "compromised_only": args.compromised_only,
                        "from": args.from.as_ref().map(|p| p.display().to_string()),
                        "profile": args.selector.profile,
                        "bundle": bundle_plan,
                    });
                    print!("{}", to_canonical_json_string(&payload)?);
                } else if !quiet {
                    println!(
                        "DRY-RUN sync: all={} compromised_only={} from={} profile={}",
                        args.all,
                        args.compromised_only,
                        args.from
                            .as_deref()
                            .map_or_else(|| "none".to_string(), |p| p.display().to_string()),
                        args.selector.profile.as_deref().unwrap_or("none")
                    );
                    if let Some(plan) = &bundle_plan {
                        println!(
                            "  bundle ok: profile={} settings={} extensions={}",
                            plan.profile_name,
                            plan.settings_count,
                            plan.install_ids.len()
                        );
                        for w in &plan.warnings {
                            eprintln!("warning: {w}");
                        }
                    }
                }
                return Ok(ExitCode::Ok);
            }

            let report = run_sync(&catalog, &sync_opts)?;
            print_sync_report(&report, json, quiet)?;
            Ok(ExitCode::Ok)
        }
        Command::Open(args) => {
            if args.profile.is_none() && args.path.is_none() {
                return Err(VspError::UsageOrConfig(
                    "vsp open requires -p/--profile <NAME> or <PATH>".to_string(),
                ));
            }
            let (mut catalog, user) =
                load_catalog_with_user(catalog_override.as_deref(), config_override.as_deref())?;
            if let Some(name) = &args.profile {
                catalog.materialize_implicit_preset(name);
            }
            // `open --sync` syncs the named profile with the same personal defaults and
            // user-profile data as `vsp sync -p <NAME>`.
            let profile_selector = SelectorArgs {
                path: None,
                workspace: None,
                profile: args.profile.clone(),
                preset: None,
                user: false,
                with: Vec::new(),
                without: Vec::new(),
                ux: None,
                ai: None,
                no_ai: false,
            };
            let personal = resolve_personal_defaults(
                &catalog,
                &profile_selector,
                None,
                None,
                false,
                user.defaults(),
            );
            run_open(
                &catalog,
                &OpenOptions {
                    path: args.path,
                    profile: args.profile,
                    sync_first: args.sync,
                    dry_run,
                    json,
                    quiet,
                    vscode_data_dir,
                    yes,
                    force: args.force,
                    user_overlay: user.overlay,
                    ux: personal.ux,
                    ai: personal.ai,
                    with_modules: personal.with_modules,
                    no_rtk: personal.no_rtk,
                },
            )
        }
        Command::Dev { command } => run_dev(catalog_override.as_deref(), json, quiet, command),
    }
}

fn load_catalog(catalog_override: Option<&std::path::Path>) -> VspResult<WorkspaceCatalog> {
    load_catalog_auto(catalog_override)
}

/// Personal user configuration state for one command invocation.
struct UserContext {
    /// Loaded configuration, or `None` when no configuration file exists.
    config: Option<UserConfig>,
    /// Resolved user-profile data (empty when `config` is `None`).
    overlay: UserOverlay,
}

impl UserContext {
    fn defaults(&self) -> Option<&UserDefaults> {
        self.config.as_ref().map(|cfg| &cfg.defaults)
    }
}

/// Loads the catalog, then loads the personal user configuration and adds its custom
/// profiles to the catalog. Only `--config <PATH>` or the XDG default file is read.
fn load_catalog_with_user(
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
) -> VspResult<(WorkspaceCatalog, UserContext)> {
    let mut catalog = load_catalog(catalog_override)?;
    let config = load_user_config(config_override)?;
    let overlay = match &config {
        Some(cfg) => apply_user_config(&mut catalog, cfg)?,
        None => UserOverlay::default(),
    };
    Ok((catalog, UserContext { config, overlay }))
}

/// Effective composition values after personal defaults are applied.
struct PersonalDefaults {
    ux: Option<UxPreset>,
    ai: Option<AiMode>,
    with_modules: Vec<String>,
    no_rtk: bool,
}

/// Returns `true` when the selector names a personal target (no workspace path).
fn personal_defaults_apply(selector: &SelectorArgs) -> bool {
    selector.path.is_none() && selector.workspace.is_none()
}

/// Combines command-line values with personal defaults. Command-line values have priority.
///
/// - `defaults.ux` fills a missing `--ux`, unless the profile or preset name is a legacy
///   alias that already selects a UX preset.
/// - `defaults.ai` fills a missing `--ai` / `--no-ai`.
/// - `defaults.rtkTerminal = true` adds the `rtk-terminal` pack unless `--no-rtk` or
///   `--without rtk-terminal` is given. `false` has the same effect as `--no-rtk`.
fn resolve_personal_defaults(
    catalog: &WorkspaceCatalog,
    selector: &SelectorArgs,
    cli_ux: Option<UxPreset>,
    cli_ai: Option<AiMode>,
    cli_no_rtk: bool,
    defaults: Option<&UserDefaults>,
) -> PersonalDefaults {
    let mut out = PersonalDefaults {
        ux: cli_ux,
        ai: cli_ai,
        with_modules: selector.with.clone(),
        no_rtk: cli_no_rtk,
    };
    let Some(defaults) = defaults else {
        return out;
    };

    let names_legacy_alias = [selector.profile.as_deref(), selector.preset.as_deref()]
        .into_iter()
        .flatten()
        .any(|name| catalog.resolve_alias(name).is_some());
    if out.ux.is_none() && !names_legacy_alias {
        out.ux = defaults.ux;
    }
    if out.ai.is_none() {
        out.ai = defaults.ai;
    }
    match defaults.rtk_terminal {
        Some(true) => {
            let excluded = selector
                .without
                .iter()
                .flat_map(|entry| entry.split(','))
                .any(|id| id.trim() == "rtk-terminal");
            if !cli_no_rtk && !excluded && !out.with_modules.iter().any(|m| m == "rtk-terminal") {
                out.with_modules.push("rtk-terminal".to_string());
            }
        }
        Some(false) => out.no_rtk = true,
        None => {}
    }
    out
}

/// Applies the user overlay and (for personal targets) the personal defaults to a plan.
fn apply_user_context_to_plan(
    catalog: &WorkspaceCatalog,
    opts: &mut PlanOptions,
    selector: &SelectorArgs,
    user: &UserContext,
    cli_no_rtk: bool,
) {
    opts.user_overlay = user.overlay.clone();
    if personal_defaults_apply(selector) {
        let personal = resolve_personal_defaults(
            catalog,
            selector,
            opts.ux,
            opts.ai,
            cli_no_rtk,
            user.defaults(),
        );
        opts.ux = personal.ux;
        opts.ai = personal.ai;
        opts.with_modules = personal.with_modules;
    }
}

/// Adds the implicit toolchain preset (`general + <toolchain>`) for a `--preset` or
/// `-p/--profile` name that is a toolchain pack ID and not an explicit preset. For example,
/// `--preset biome` then resolves in `diff`, `sync`, and `open` like any preset.
fn materialize_selector_presets(catalog: &mut WorkspaceCatalog, selector: &SelectorArgs) {
    for name in [selector.preset.as_deref(), selector.profile.as_deref()]
        .into_iter()
        .flatten()
    {
        catalog.materialize_implicit_preset(name);
    }
}

/// Exit code for a printed plan: `7` when the observation is incomplete (this has priority
/// over a drift result), `3` for `--check` with drift, otherwise `0`.
fn plan_exit_code(plan: &Plan, check: bool) -> ExitCode {
    if !plan.observation_complete {
        ExitCode::IncompleteObservation
    } else if check && plan.has_drift {
        ExitCode::NegativeCheck
    } else {
        ExitCode::Ok
    }
}

/// Builds the `clap` command tree with live catalog names as possible values, so the
/// generated completion scripts offer presets, legacy aliases, packs, and UX presets.
///
/// Use the result for completion generation only. Argument parsing stays unrestricted.
fn command_with_live_values(catalog: &WorkspaceCatalog) -> clap::Command {
    // `clap` possible values need `'static` strings. This runs once in a short-lived
    // `vsp completions` process, so the leaked names are released at process exit.
    fn leak_all(names: impl IntoIterator<Item = String>) -> Vec<&'static str> {
        names
            .into_iter()
            .map(|name| &*Box::leak(name.into_boxed_str()))
            .collect()
    }

    let mut preset_names: Vec<String> = catalog.profiles.keys().cloned().collect();
    for (alias_file, _) in LEGACY_BUNDLE_ALIASES {
        if let Some(alias) = alias_file.strip_suffix(".code-profile") {
            if !preset_names.iter().any(|n| n == alias) {
                preset_names.push(alias.to_string());
            }
        }
    }
    let pack_names: Vec<String> = catalog
        .modules
        .values()
        .filter(|m| m.manifest.kind != ModuleKind::Ux)
        .map(|m| m.manifest.id.clone())
        .collect();
    let mut show_names = preset_names.clone();
    show_names.extend(catalog.modules.keys().cloned());

    let presets = leak_all(preset_names);
    let packs = leak_all(pack_names);
    let show = leak_all(show_names);

    let mut cmd = Cli::command();
    for sub in ["diff", "sync", "init"] {
        let (presets, packs) = (presets.clone(), packs.clone());
        cmd = cmd.mut_subcommand(sub, move |c| {
            c.mut_arg("preset", |a| {
                a.value_parser(PossibleValuesParser::new(presets.clone()))
            })
            .mut_arg("profile", |a| {
                a.value_parser(PossibleValuesParser::new(presets.clone()))
            })
            .mut_arg("with", |a| {
                a.value_parser(PossibleValuesParser::new(packs.clone()))
            })
            .mut_arg("without", |a| {
                a.value_parser(PossibleValuesParser::new(packs.clone()))
            })
        });
    }
    let open_presets = presets.clone();
    cmd = cmd.mut_subcommand("open", move |c| {
        c.mut_arg("profile", |a| {
            a.value_parser(PossibleValuesParser::new(open_presets.clone()))
        })
    });
    let export_presets = presets;
    cmd = cmd.mut_subcommand("export", move |c| {
        c.mut_arg("names", |a| {
            a.value_parser(PossibleValuesParser::new(export_presets.clone()))
        })
    });
    cmd.mut_subcommand("show", move |c| {
        c.mut_arg("name", |a| {
            a.value_parser(PossibleValuesParser::new(show.clone()))
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn run_init(
    args: &InitArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    dry_run: bool,
    json: bool,
    yes: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    // 1. Validate the flag combination before any filesystem access.
    let ai: Option<AiMode> = match (args.ai, args.no_ai) {
        (Some(AiArg::Full | AiArg::Copilot), true) => {
            return Err(VspError::UsageOrConfig(
                "contradictory flags: --no-ai cannot be combined with --ai full|copilot"
                    .to_string(),
            ));
        }
        (_, true) => Some(AiMode::Off),
        (Some(mode), false) => Some(mode.into()),
        (None, false) => None,
    };
    if args.rtk_terminal && args.no_rtk {
        return Err(VspError::UsageOrConfig(
            "contradictory flags: --rtk-terminal cannot be combined with --no-rtk".to_string(),
        ));
    }
    if args.user && args.profile.is_some() {
        return Err(VspError::UsageOrConfig(
            "cannot combine --user with -p/--profile".to_string(),
        ));
    }
    if args.apply && !args.user && args.profile.is_none() {
        return Err(VspError::UsageOrConfig(
            "vsp init --apply requires a target: -p/--profile <NAME> or --user".to_string(),
        ));
    }
    let has_composition =
        args.preset.is_some() || !args.with.is_empty() || !args.without.is_empty();
    if has_composition && args.profile.is_none() && !(args.user && args.apply) {
        return Err(VspError::UsageOrConfig(
            "--preset, --with, and --without require -p/--profile <NAME> (or --user --apply)"
                .to_string(),
        ));
    }
    let rtk_terminal = if args.rtk_terminal {
        Some(true)
    } else if args.no_rtk {
        Some(false)
    } else {
        None
    };

    // 2. Build the starter text. A `-p <NAME>` that is not a catalog preset becomes a
    //    custom profile entry in the config.
    let catalog = load_catalog(catalog_override)?;
    let custom_profile = match &args.profile {
        Some(name)
            if !catalog.profiles.contains_key(name)
                && catalog.resolve_alias(name).is_none() =>
        {
            Some(UserProfile {
                name: name.clone(),
                description: String::new(),
                extends: args.preset.clone(),
                with: args.with.clone(),
                without: args.without.clone(),
                overrides: Map::new(),
                extra_extensions: Vec::new(),
            })
        }
        _ => None,
    };
    let content = render_starter_config(&StarterSeed {
        ux: args.ux.map(Into::into),
        ai,
        rtk_terminal,
        profile: custom_profile.clone(),
    });

    // 3. Validate the text with the real loader before it is written.
    let path = resolve_user_config_path(config_override);
    let parsed = parse_user_config(&content, &path)?;
    let mut user_catalog = catalog.clone();
    let overlay = apply_user_config(&mut user_catalog, &parsed)?;

    // 4. Refuse to replace an existing config unless --force is given (exit 5).
    check_user_config_target(&path, args.force)?;

    if dry_run {
        if json {
            let payload = serde_json::json!({
                "dry_run": true,
                "path": path.display().to_string(),
                "created": false,
                "applied": false,
                "content": content,
            });
            print!("{}", to_canonical_json_string(&payload)?);
        } else if !quiet {
            println!("DRY-RUN: would write user config {}", path.display());
            print!("{content}");
        }
        return Ok(ExitCode::Ok);
    }

    write_user_config(&path, &content, args.force)?;
    if !json && !quiet {
        println!("Wrote user config {}", path.display());
    }

    // 5. Optional convergence of the selected personal target.
    let sync_report = if args.apply {
        let mut with_modules = if custom_profile.is_some() {
            Vec::new()
        } else {
            args.with.clone()
        };
        if rtk_terminal == Some(true) && !with_modules.iter().any(|m| m == "rtk-terminal") {
            with_modules.push("rtk-terminal".to_string());
        }
        let sync_opts = SyncOptions {
            profile: args.profile.clone(),
            preset: if custom_profile.is_some() {
                None
            } else {
                args.preset.clone()
            },
            user: args.user,
            with_modules,
            without_modules: if custom_profile.is_some() {
                Vec::new()
            } else {
                args.without.clone()
            },
            ux: args.ux.map(Into::into),
            ai,
            yes,
            no_rtk: args.no_rtk,
            vscode_data_dir,
            user_overlay: overlay,
            ..Default::default()
        };
        Some(run_sync(&user_catalog, &sync_opts)?)
    } else {
        None
    };

    if json {
        let payload = serde_json::json!({
            "dry_run": false,
            "path": path.display().to_string(),
            "created": true,
            "applied": sync_report.is_some(),
            "sync": sync_report,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else if let Some(report) = &sync_report {
        print_sync_report(report, false, quiet)?;
    }
    Ok(ExitCode::Ok)
}

fn run_self(command: SelfCommand, dry_run: bool, json: bool, quiet: bool) -> VspResult<ExitCode> {
    match command {
        SelfCommand::Path => {
            let canon = self_ops::current_executable()?;
            if json {
                let payload = serde_json::json!({ "path": canon.display().to_string() });
                print!("{}", to_canonical_json_string(&payload)?);
            } else {
                println!("{}", canon.display());
            }
            Ok(ExitCode::Ok)
        }
        SelfCommand::Install {
            bin_dir,
            with_vspcli,
        } => {
            let report = self_ops::install(bin_dir.as_deref(), with_vspcli, dry_run)?;
            print_self_link_report(&report, json, quiet)?;
            Ok(ExitCode::Ok)
        }
        SelfCommand::Uninstall { bin_dir } => {
            let report = self_ops::uninstall(bin_dir.as_deref(), dry_run)?;
            print_self_link_report(&report, json, quiet)?;
            Ok(ExitCode::Ok)
        }
        SelfCommand::Update => {
            let guidance = self_ops::update_guidance();
            if json {
                print!("{}", to_canonical_json_string(&guidance)?);
            } else {
                print!("{}", self_ops::format_update_guidance(&guidance));
            }
            Ok(ExitCode::Ok)
        }
    }
}

fn print_self_link_report(report: &SelfLinkReport, json: bool, quiet: bool) -> VspResult<()> {
    if json {
        print!("{}", to_canonical_json_string(report)?);
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    let prefix = if report.dry_run { "DRY-RUN: " } else { "" };
    for item in &report.links {
        let verb = match item.action {
            LinkAction::Created => "created",
            LinkAction::Replaced => "replaced",
            LinkAction::Unchanged => "unchanged",
            LinkAction::Removed => "removed",
            LinkAction::Absent => "not installed",
            LinkAction::Skipped => "kept (not owned by this installation)",
        };
        match &report.target {
            Some(target) => println!("{prefix}{verb}: {} -> {target}", item.link),
            None => println!("{prefix}{verb}: {}", item.link),
        }
    }
    Ok(())
}

fn run_list_installed(vscode_data_dir: Option<&std::path::Path>, json: bool) -> VspResult<ExitCode> {
    let store = resolve_vscode_store(vscode_data_dir, false)?;
    let mut profiles = vec!["default".to_string()];
    profiles.extend(list_storage_profiles(&store.user_dir)?);
    if json {
        let payload = serde_json::json!({
            "kind": "installed",
            "user_dir": store.user_dir.display().to_string(),
            "profiles": profiles,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else {
        for p in profiles {
            println!("{p}");
        }
    }
    Ok(ExitCode::Ok)
}

fn run_list(
    catalog: &WorkspaceCatalog,
    overlay: &UserOverlay,
    kind: ListKind,
    json: bool,
) -> VspResult<ExitCode> {
    #[derive(Serialize)]
    struct ListItem<'a> {
        id: &'a str,
        kind: &'a str,
        description: &'a str,
    }

    match kind {
        ListKind::Presets => {
            if json {
                let items: Vec<ListItem<'_>> = catalog
                    .profiles
                    .values()
                    .map(|p| ListItem {
                        id: &p.name,
                        kind: if overlay.is_user_profile(&p.name) {
                            "user-profile"
                        } else {
                            "preset"
                        },
                        description: &p.description,
                    })
                    .collect();
                print!("{}", to_canonical_json_string(&items)?);
            } else {
                for name in catalog.profiles.keys() {
                    println!("{name}");
                }
            }
        }
        ListKind::Packs => {
            let items: Vec<ListItem<'_>> = catalog
                .modules
                .values()
                .filter(|m| m.manifest.kind != ModuleKind::Ux)
                .map(|m| ListItem {
                    id: &m.manifest.id,
                    kind: m.manifest.kind.as_str(),
                    description: &m.manifest.description,
                })
                .collect();
            if json {
                print!("{}", to_canonical_json_string(&items)?);
            } else {
                for item in items {
                    println!("{}\t{}\t{}", item.id, item.kind, item.description);
                }
            }
        }
        ListKind::Ux => {
            let items: Vec<ListItem<'_>> = catalog
                .modules
                .values()
                .filter(|m| m.manifest.kind == ModuleKind::Ux)
                .map(|m| ListItem {
                    id: &m.manifest.id,
                    kind: "ux",
                    description: &m.manifest.description,
                })
                .collect();
            if json {
                print!("{}", to_canonical_json_string(&items)?);
            } else {
                for item in items {
                    println!("{}\t{}", item.id, item.description);
                }
            }
        }
        ListKind::Installed => {}
    }
    Ok(ExitCode::Ok)
}

fn run_show(
    catalog: &WorkspaceCatalog,
    overlay: &UserOverlay,
    name: &str,
    json: bool,
) -> VspResult<ExitCode> {
    // Check legacy alias first.
    let (lookup_name, legacy_note) = match catalog.resolve_alias(name) {
        Some((canon, ux)) => (
            canon.clone(),
            Some(format!("legacy alias '{name}' -> '{canon}' (--ux={ux})")),
        ),
        None => (name.to_string(), None),
    };

    if let Some(recipe) = catalog.profiles.get(&lookup_name) {
        let resolved = resolve_profile_recipe(
            catalog,
            &lookup_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )?;
        let settings = compile_canonical_profile_settings(catalog, &lookup_name)?;
        let exts = compile_profile_extensions(catalog, &resolved)?;
        let mut ext_ids: Vec<&str> = exts.iter().map(|e| e.identifier.id.as_str()).collect();

        // A custom user profile also lists its exact-ID extra extensions and their tiers.
        let user_profile = overlay.profiles.get(&lookup_name);
        if let Some(up) = user_profile {
            for extra in &up.extra_extensions {
                if !ext_ids.iter().any(|id| id.eq_ignore_ascii_case(&extra.id)) {
                    ext_ids.push(extra.id.as_str());
                }
            }
        }

        if json {
            let mut payload = serde_json::json!({
                "kind": if user_profile.is_some() { "user-profile" } else { "preset" },
                "name": recipe.name,
                "description": recipe.description,
                "alias": legacy_note,
                "useDefaults": recipe.use_defaults,
                "modules": recipe.modules,
                "excludeModules": recipe.exclude_modules,
                "resolvedModules": resolved,
                "settingsCount": settings.len(),
                "extensions": ext_ids,
            });
            if let (Some(up), Some(obj)) = (user_profile, payload.as_object_mut()) {
                obj.insert("extends".to_string(), serde_json::json!(up.extends));
                obj.insert(
                    "extraExtensions".to_string(),
                    serde_json::json!(up.extra_extensions),
                );
            }
            print!("{}", to_canonical_json_string(&payload)?);
        } else {
            if let Some(note) = legacy_note {
                println!("Alias: {note}");
            }
            match user_profile {
                Some(up) => {
                    println!("User profile: {}", recipe.name);
                    println!("Extends: {}", up.extends.as_deref().unwrap_or("(defaults)"));
                }
                None => println!("Preset: {}", recipe.name),
            }
            println!("Description: {}", recipe.description);
            println!("Resolved modules: {}", resolved.join(", "));
            println!("Settings keys: {}", settings.len());
            println!("Extensions ({}):", ext_ids.len());
            for id in ext_ids {
                let tier = user_profile
                    .and_then(|up| up.extra_extensions.iter().find(|e| e.id == id))
                    .map(|e| e.tier.as_str());
                match tier {
                    Some(label) => println!("  - {id} [{label}]"),
                    None => println!("  - {id}"),
                }
            }
        }
        return Ok(ExitCode::Ok);
    }

    if let Some(module) = catalog.modules.get(name) {
        if json {
            let payload = serde_json::json!({
                "kind": module.manifest.kind.as_str(),
                "id": module.manifest.id,
                "description": module.manifest.description,
                "requires": module.manifest.requires,
                "defaultWith": module.manifest.default_with,
                "conflicts": module.manifest.conflicts,
                "extensions": module.manifest.extensions,
                "workspaceSettingsCount": module.workspace_settings.as_ref().map_or(0, serde_json::Map::len),
                "profileSettingsCount": module.profile_settings.as_ref().map_or(0, serde_json::Map::len),
                "hasTasks": module.tasks_doc.is_some(),
                "hasDetect": module.manifest.detect.is_some(),
            });
            print!("{}", to_canonical_json_string(&payload)?);
        } else {
            println!("Pack: {} ({})", module.manifest.id, module.manifest.kind);
            println!("Description: {}", module.manifest.description);
            if !module.manifest.requires.is_empty() {
                println!("Requires: {}", module.manifest.requires.join(", "));
            }
            if !module.manifest.default_with.is_empty() {
                println!("Default with: {}", module.manifest.default_with.join(", "));
            }
            if !module.manifest.conflicts.is_empty() {
                println!("Conflicts: {}", module.manifest.conflicts.join(", "));
            }
            println!(
                "Extensions: workspace={}, profile={}, unwanted={}",
                module.manifest.extensions.workspace.len(),
                module.manifest.extensions.profile.len(),
                module.manifest.extensions.unwanted.len()
            );
        }
        return Ok(ExitCode::Ok);
    }

    Err(VspError::TargetMissing(format!(
        "unknown preset or pack '{name}'"
    )))
}

fn run_detect(
    catalog: &WorkspaceCatalog,
    target: &std::path::Path,
    explain: bool,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    let result = detect_workspace(catalog, target)?;

    if result.toolchains.is_empty() {
        if json {
            print!("{}", to_canonical_json_string(&result)?);
        } else if !quiet {
            eprintln!("vsp detect: no toolchain detected in {}", target.display());
        }
        return Ok(ExitCode::NegativeCheck);
    }

    if json {
        print!("{}", to_canonical_json_string(&result)?);
    } else if !quiet {
        if explain {
            for tc in &result.toolchains {
                let sigs = result
                    .signals
                    .get(tc)
                    .map(|v| v.join(", "))
                    .unwrap_or_default();
                println!("{tc}\tmatched: {sigs}");
            }
            for sup in &result.superseded {
                println!(
                    "{}\tsuperseded by {} (signals: {})",
                    sup.module,
                    sup.superseded_by,
                    sup.signals.join(", ")
                );
            }
        } else {
            println!("{}", result.toolchains.join(" "));
        }
    }

    Ok(ExitCode::Ok)
}

fn print_plan(plan: &Plan, json: bool, quiet: bool) -> VspResult<()> {
    if json {
        print!("{}", to_canonical_json_string(plan)?);
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    println!(
        "Plan ({}) target={:?} modules=[{}]",
        plan.schema,
        plan.target.kind,
        plan.resolved_modules.join(", ")
    );
    if let Some(ref p) = plan.target.path {
        println!("  path: {p}");
    }
    if let Some(ref prof) = plan.target.profile {
        println!("  profile: {prof}");
    }
    if let Some(ref preset) = plan.target.preset {
        println!("  preset: {preset}");
    }
    println!(
        "  changes: settings={}, extensions={}, tasks={}, drift={}",
        plan.settings_changes.len(),
        plan.extension_changes.len(),
        plan.task_changes.len(),
        plan.has_drift
    );
    for ch in &plan.settings_changes {
        println!("  setting {:?}: {}", ch.action, ch.key);
    }
    for ex in &plan.extension_changes {
        println!("  extension {:?}: {}", ex.action, ex.id);
    }
    for tk in &plan.task_changes {
        println!("  task {:?}: {}", tk.action, tk.label);
    }
    for w in &plan.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn print_sync_report(report: &SyncReport, json: bool, quiet: bool) -> VspResult<()> {
    if json {
        print!("{}", to_canonical_json_string(report)?);
        return Ok(());
    }
    if quiet {
        return Ok(());
    }
    println!(
        "Synced {} ({}): files={}, step0_removed={}, installed={}, pruned={}, repaired_pins={}",
        report.kind,
        report.target,
        report.files_written.len(),
        report.step0_uninstalled.len(),
        report.installed.len(),
        report.pruned.len(),
        report.repaired_pins.len()
    );
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

fn print_dist_report(
    report: &crate::resolver::DistReport,
    json: bool,
    quiet: bool,
) -> VspResult<()> {
    if json {
        let payload = serde_json::json!({
            "out_dir": report.out_dir.display().to_string(),
            "canonical_bundles": report.canonical_bundles,
            "alias_bundles": report.alias_bundles,
            "sha256sums": report.sha256sums_path.display().to_string(),
            "checksum_count": report.checksums.len(),
        });
        print!("{}", to_canonical_json_string(&payload)?);
        return Ok(());
    }
    if !quiet {
        println!(
            "Wrote {} canonical bundle(s), {} legacy alias(es), and {} ({} digests)",
            report.canonical_bundles.len(),
            report.alias_bundles.len(),
            report.sha256sums_path.display(),
            report.checksums.len()
        );
    }
    Ok(())
}

fn run_dev(
    catalog_override: Option<&std::path::Path>,
    json: bool,
    quiet: bool,
    command: DevCommand,
) -> VspResult<ExitCode> {
    match command {
        DevCommand::Validate => {
            let catalog = load_catalog(catalog_override)?;
            let report = validate_repository(&catalog);
            if json {
                print!("{}", to_canonical_json_string(&report)?);
            } else if report.is_ok() {
                if !quiet {
                    println!(
                        "vsp dev validate: validated {} files and all catalog/module/security invariants successfully",
                        report.checked_files
                    );
                }
            } else {
                for v in &report.violations {
                    eprintln!("FAILED: {}: {}", v.source, v.message);
                }
                eprintln!(
                    "vsp dev validate: {} violation(s) found",
                    report.violations.len()
                );
            }
            if report.is_ok() {
                Ok(ExitCode::Ok)
            } else {
                Ok(ExitCode::Operational)
            }
        }
        DevCommand::Dist { out } => {
            let catalog = load_catalog(catalog_override)?;
            let report = build_dist(&catalog, &out)?;
            print_dist_report(&report, json, quiet)?;
            Ok(ExitCode::Ok)
        }
        DevCommand::Pins { check, refresh } => {
            let catalog = load_catalog(catalog_override)?;
            run_dev_pins(&catalog, check, refresh, json, quiet)
        }
        DevCommand::Compat {
            all,
            extensions_file,
            vscode_version,
            marketplace_only: _,
            profiles,
        } => {
            let catalog = load_catalog(catalog_override)?;
            run_dev_compat(
                &catalog,
                &CompatOptions {
                    all,
                    extensions_file,
                    vscode_version,
                    profiles,
                },
                json,
                quiet,
            )
        }
        DevCommand::Docs { check, write } => {
            let catalog = load_catalog(catalog_override)?;
            run_dev_docs(&catalog, check, write, json, quiet)
        }
        DevCommand::Hooks => {
            let root = resolve_repo_dir(catalog_override)?;
            run_dev_hooks(&root, json, quiet)
        }
        DevCommand::Test => {
            if !quiet {
                println!("Run Rust suite: cargo test --workspace");
                println!("Run full Bash/BATS suite: bash scripts/tests/run.sh");
            }
            Ok(ExitCode::Ok)
        }
        DevCommand::Release => Err(VspError::Operational(
            "'vsp dev release' is orchestrated by .github/workflows/release.yml and 'vsp dev dist --out dist'".to_string(),
        )),
    }
}

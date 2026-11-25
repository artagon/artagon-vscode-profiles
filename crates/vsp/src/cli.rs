//! `clap` Derive CLI definitions and command dispatch for `vsp`.

use std::io;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;

use crate::detect::detect_workspace;
use crate::dev_ops::{
    run_dev_compat, run_dev_docs, run_dev_hooks, run_dev_pins, CompatOptions,
};
use crate::doctor::{run_doctor, CheckSeverity};
use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::legacy_loader::{load_legacy_catalog, resolve_catalog_root};
use crate::model::{
    resolve_legacy_profile_name, AiMode, ModuleKind, UxPreset, WorkspaceCatalog,
};
use crate::open::{run_open, OpenOptions};
use crate::plan::{compute_plan, Plan, PlanOptions};
use crate::policy::validate_repository;
use crate::resolver::{
    build_dist, compile_canonical_profile_settings, compile_profile_extensions, export_profiles,
    resolve_profile_recipe, to_canonical_json_string,
};
use crate::sync::{run_sync, PruneMode, SyncOptions, SyncReport};
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

    /// Generate shell completion scripts via `clap_complete`.
    Completions {
        /// Target shell (`bash`, `zsh`, or `fish`).
        #[arg(value_enum)]
        shell: CompletionShell,
    },

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
    /// Print the resolved path of the running `vsp` binary.
    Path,

    /// Install `vsp` into user bin directory (full automation in Phase 3).
    Install {
        /// Target installation directory.
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },

    /// Uninstall `vsp` from user bin directory (Phase 3).
    Uninstall,

    /// Update `vsp` binary (Phase 3).
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
        vscode_data_dir,
        command,
        ..
    } = cli;

    match command {
        Command::Completions { shell } => {
            let mut cmd = Cli::command();
            let clap_shell = match shell {
                CompletionShell::Bash => Shell::Bash,
                CompletionShell::Zsh => Shell::Zsh,
                CompletionShell::Fish => Shell::Fish,
            };
            clap_complete::generate(clap_shell, &mut cmd, "vsp", &mut io::stdout());
            Ok(ExitCode::Ok)
        }
        Command::SelfCmd { command } => run_self(command, json),
        Command::List { kind } => {
            let kind = kind.unwrap_or(ListKind::Presets);
            if kind == ListKind::Installed {
                return run_list_installed(vscode_data_dir.as_deref(), json);
            }
            let catalog = load_catalog(catalog_override.as_deref())?;
            run_list(&catalog, kind, json)
        }
        Command::Show { name } => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            run_show(&catalog, &name, json)
        }
        Command::Detect { path, explain } => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            let target = path.unwrap_or_else(|| PathBuf::from("."));
            run_detect(&catalog, &target, explain, json, quiet)
        }
        Command::Diff(args) => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            let opts = args.selector.to_plan_options(vscode_data_dir)?;
            let plan = compute_plan(&catalog, &opts)?;
            print_plan(&plan, json, quiet)?;
            if args.check && plan.has_drift {
                Ok(ExitCode::NegativeCheck)
            } else {
                Ok(ExitCode::Ok)
            }
        }
        Command::Doctor { strict } => {
            let catalog = load_catalog(catalog_override.as_deref())?;
            let report = run_doctor(&catalog, vscode_data_dir.as_deref(), strict);
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
            let catalog = load_catalog(catalog_override.as_deref())?;
            let out_dir = out.unwrap_or_else(|| PathBuf::from("exports"));
            let report = export_profiles(&catalog, &names, &out_dir, false)?;
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
            if let Some(ref bundle) = args.from {
                if !bundle.is_file() {
                    return Err(VspError::TargetMissing(format!(
                        "bundle file not found: {}",
                        bundle.display()
                    )));
                }
            }

            let catalog = load_catalog(catalog_override.as_deref())?;

            // Unified read-only planner path: `vsp sync --dry-run` emits the EXACT same
            // `vsp-plan/v1` JSON and text output as `vsp diff`!
            if dry_run && has_std_selector && args.from.is_none() && !args.all && !args.compromised_only {
                let opts = args.selector.to_plan_options(vscode_data_dir)?;
                let plan = compute_plan(&catalog, &opts)?;
                print_plan(&plan, json, quiet)?;
                return Ok(ExitCode::Ok);
            }

            if dry_run {
                if json {
                    let payload = serde_json::json!({
                        "dry_run": true,
                        "all": args.all,
                        "compromised_only": args.compromised_only,
                        "from": args.from.as_ref().map(|p| p.display().to_string()),
                        "profile": args.selector.profile,
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
                }
                return Ok(ExitCode::Ok);
            }

            let sync_opts = SyncOptions {
                workspace_path: effective_ws,
                profile: args.selector.profile.clone(),
                preset: args.selector.preset.clone(),
                from_bundle: args.from.clone(),
                user: args.selector.user,
                all: args.all,
                compromised_only: args.compromised_only,
                with_modules: args.selector.with.clone(),
                without_modules: args.selector.without.clone(),
                ux: args.selector.ux.map(Into::into),
                ai: effective_ai,
                prune: args.effective_prune(),
                repair_pins: args.repair_pins,
                allow_unlocked: args.allow_unlocked,
                allow_unsafe_settings: args.allow_unsafe_settings,
                force: args.force,
                yes,
                no_batch: args.no_batch,
                no_rtk: args.no_rtk,
                vscode_data_dir,
            };

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
            let catalog = load_catalog(catalog_override.as_deref())?;
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
                },
            )
        }
        Command::Dev { command } => run_dev(catalog_override.as_deref(), json, quiet, command),
    }
}

fn load_catalog(catalog_override: Option<&std::path::Path>) -> VspResult<WorkspaceCatalog> {
    let root = resolve_catalog_root(catalog_override)?;
    load_legacy_catalog(&root)
}

fn run_self(command: SelfCommand, json: bool) -> VspResult<ExitCode> {
    match command {
        SelfCommand::Path => {
            let exe = std::env::current_exe().map_err(|err| {
                VspError::Operational(format!("cannot determine current executable path: {err}"))
            })?;
            let canon = std::fs::canonicalize(&exe).unwrap_or(exe);
            if json {
                let payload = serde_json::json!({ "path": canon.display().to_string() });
                print!("{}", to_canonical_json_string(&payload)?);
            } else {
                println!("{}", canon.display());
            }
            Ok(ExitCode::Ok)
        }
        SelfCommand::Install { dir } => Err(VspError::Operational(format!(
            "'vsp self install' (target={}) is scheduled for Phase 3 (v3.0.0); use bin/vsp self install in Phase 1",
            dir.as_deref()
                .map_or_else(|| "~/.local/bin".to_string(), |d| d.display().to_string())
        ))),
        SelfCommand::Uninstall => Err(VspError::Operational(
            "'vsp self uninstall' is scheduled for Phase 3 (v3.0.0); use bin/vsp self uninstall in Phase 1".to_string(),
        )),
        SelfCommand::Update => Err(VspError::Operational(
            "'vsp self update' is scheduled for Phase 3 (v3.0.0)".to_string(),
        )),
    }
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

fn run_list(catalog: &WorkspaceCatalog, kind: ListKind, json: bool) -> VspResult<ExitCode> {
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
                        kind: "preset",
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

fn run_show(catalog: &WorkspaceCatalog, name: &str, json: bool) -> VspResult<ExitCode> {
    // Check legacy alias first.
    let (lookup_name, legacy_note) = match resolve_legacy_profile_name(name) {
        Some((canon, ux)) => (
            canon.to_string(),
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
        let ext_ids: Vec<&str> = exts.iter().map(|e| e.identifier.id.as_str()).collect();

        if json {
            let payload = serde_json::json!({
                "kind": "preset",
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
            print!("{}", to_canonical_json_string(&payload)?);
        } else {
            if let Some(note) = legacy_note {
                println!("Alias: {note}");
            }
            println!("Preset: {}", recipe.name);
            println!("Description: {}", recipe.description);
            println!("Resolved modules: {}", resolved.join(", "));
            println!("Settings keys: {}", settings.len());
            println!("Extensions ({}):", ext_ids.len());
            for id in ext_ids {
                println!("  - {id}");
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
            let root = resolve_catalog_root(catalog_override)?;
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

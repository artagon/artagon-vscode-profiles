//! `clap` Derive CLI definitions and command dispatch for `vsp`.

use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::builder::PossibleValuesParser;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;
use serde_json::Map;

use crate::detect::detect_workspace;
use crate::dev_ops::{
    run_dev_compat, run_dev_docs_with_options, run_dev_hooks_with_options,
    run_dev_pins_with_options, CompatOptions, DevDocsOptions, DevHooksOptions, DevPinsOptions,
};
use crate::doctor::{
    audit_user_config, audit_user_config_error, run_doctor_with_options, CheckSeverity,
};
use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{AiMode, ModuleKind, UxPreset, WorkspaceCatalog, LEGACY_BUNDLE_ALIASES};
use crate::open::{run_open, run_use, OpenOptions};
use crate::plan::{
    compute_plan_with_extras, validate_selector_combination, ExtensionChangeKind, Plan, PlanExtras,
    PlanOptions, PlanScopeKind, SelectorKinds, SettingChangeKind, TaskChangeKind,
};
use crate::policy::validate_repository;
use crate::resolver::{
    build_dist_with_options, compile_canonical_profile_settings, compile_profile_extensions,
    export_profiles_with_options, resolve_profile_recipe, to_canonical_json_string,
};
use crate::schema_loader::{load_catalog_auto, resolve_repo_dir};
use crate::self_ops::{self, LinkAction, SelfLinkReport};
use crate::sync::{prepare_bundle_import, run_sync, PruneMode, SyncOptions, SyncReport};
use crate::user_config::{
    apply_user_config, check_user_config_target, load_user_config_scoped, parse_user_config,
    render_starter_config, resolve_user_config_path, trusted_default_user_config_path,
    write_user_config_with_backup, StarterSeed, UserConfig, UserConfigScope, UserDefaults,
    UserOverlay, UserProfile,
};
use crate::vscode::{list_storage_profiles, resolve_vscode_store};

const ROOT_AFTER_LONG_HELP: &str = "\
Examples:
  vsp list -l
  vsp show --pack rust
  vsp diff --check .
  vsp sync .
  vsp sync -p rust --ux retina --allow-unlocked
  vsp sync -p my-rust --preset rust --with git-pro
  vsp use rust
  vsp doctor --strict

Exit codes:
  0  Ok, clean check, or ordinary preview
  1  Operational, audit, or validation failure
  2  Target missing (path, profile, preset, pack, or bundle)
  3  Complete negative check (detect found no match, or diff --check verified drift)
  4  Usage, config, selector, or module composition/conflict error
  5  Safety guard refusal before mutation (or sync --dry-run predicted refusal)
  6  Retryable lock contention, preimage mismatch, or partial apply error
  7  Incomplete observation or external verification unavailable (e.g. missing/failing `code`)";

const DIFF_AFTER_LONG_HELP: &str = "\
Examples:
  vsp diff .
  vsp diff --check .
  vsp diff --workspace ./repo --preset rust
  vsp diff -p rust --ux retina --prune --allow-unlocked
  vsp diff -p my-rust --preset rust --with git-pro --json
  vsp diff --user --ux crisp --ai off

Exit codes:
  0  Plan computed (or clean under --check)
  3  Verified drift under --check (or workspace detect found no toolchain)
  7  Incomplete observation (for example `code --list-extensions` failed on a registered profile)";

const SYNC_AFTER_LONG_HELP: &str = "\
Examples:
  vsp sync .
  vsp sync -p rust --ux retina --allow-unlocked
  vsp sync -p my-rust --preset rust --with git-pro
  vsp sync -p rust --extensions-only
  vsp sync --dry-run -p rust --prune=retired
  vsp sync --user
  vsp sync --all --compromised-only

Exit codes:
  0  Converged cleanly (or dry-run preview with no predicted refusals)
  3  Workspace auto-detect matched no toolchain (pass --preset or --with)
  5  Safety refusal before write (including sync --dry-run when `would_refuse` is non-empty)
  6  Lock contention or mid-apply partial failure
  7  Incomplete observation (for example `code --list-extensions` failed)";

const OPEN_AFTER_LONG_HELP: &str = "\
Examples:
  vsp open .
  vsp open . -p rust
  vsp open ./my-repo -p rust --sync
  vsp open -p rust --dry-run";

const USE_AFTER_LONG_HELP: &str = "\
Examples:
  vsp use rust
  vsp use shell ./my-repo
  vsp use rust --sync
  vsp use rust --dry-run --json";

const DOCTOR_AFTER_LONG_HELP: &str = "\
Examples:
  vsp doctor
  vsp doctor --strict
  vsp doctor --json

Exit codes:
  0  All checks passed (and 0 warnings under --strict)
  1  One or more checks failed (or warnings present under --strict)
  7  Audit incomplete (VS Code `code` CLI missing or failed to list extensions)";

/// Runtime execution flags shared across all `vsp` command handlers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunContext {
    /// Preview actions without modifying files (`-n/--dry-run`).
    pub dry_run: bool,
    /// Emit machine-readable JSON output (`--json`).
    pub json: bool,
    /// Suppress non-essential output (`-q/--quiet`).
    pub quiet: bool,
    /// Emit verbose diagnostic trace to stderr (`-v/--verbose`).
    pub verbose: bool,
    /// Assume yes for interactive confirmations (`-y/--yes`).
    pub yes: bool,
}

/// Unified CLI for Artagon VS Code profiles, capability packs, and workspace plans.
#[derive(Debug, Parser)]
#[command(
    name = "vsp",
    version,
    about = "Manage Artagon VS Code capability packs, workspaces, and profiles",
    after_long_help = ROOT_AFTER_LONG_HELP,
    propagate_version = false
)]
pub struct Cli {
    /// Preview actions without modifying files.
    #[arg(
        short = 'n',
        long = "dry-run",
        global = true,
        help_heading = "Output & Runtime"
    )]
    pub dry_run: bool,

    /// Emit machine-readable JSON output.
    #[arg(long = "json", global = true, help_heading = "Output & Runtime")]
    pub json: bool,

    /// Assume yes for interactive confirmations.
    #[arg(
        short = 'y',
        long = "yes",
        global = true,
        help_heading = "Output & Runtime"
    )]
    pub yes: bool,

    /// Suppress non-essential output.
    #[arg(
        short = 'q',
        long = "quiet",
        global = true,
        help_heading = "Output & Runtime"
    )]
    pub quiet: bool,

    /// Enable verbose diagnostic output on stderr.
    #[arg(
        short = 'v',
        long = "verbose",
        global = true,
        help_heading = "Output & Runtime"
    )]
    pub verbose: bool,

    /// Override catalog/repository root directory.
    #[arg(
        long = "catalog",
        value_name = "DIR",
        global = true,
        help_heading = "Advanced / Overrides"
    )]
    pub catalog: Option<PathBuf>,

    /// Override user configuration file path (`~/.config/vsp/config.jsonc`).
    #[arg(
        long = "config",
        value_name = "PATH",
        global = true,
        help_heading = "Advanced / Overrides"
    )]
    pub config: Option<PathBuf>,

    /// Override VS Code user data directory.
    #[arg(
        long = "vscode-data-dir",
        value_name = "DIR",
        global = true,
        help_heading = "Advanced / Overrides"
    )]
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

        /// Show aligned columns with badges/kinds and descriptions (default on interactive TTY).
        #[arg(short = 'l', long = "long", help_heading = "Output & Runtime")]
        long: bool,
    },

    /// Inspect a canonical preset, pack/module, UX preset, or legacy alias.
    Show {
        /// Name of preset, pack, UX preset, or legacy alias (supports `pack:<id>` and `preset:<name>` prefixes).
        name: String,

        /// Force lookup as a capability, toolchain, or UX pack (disambiguates names shared with presets).
        #[arg(long = "pack", conflicts_with = "preset")]
        pack: bool,

        /// Force lookup as a preset, user profile, or legacy alias.
        #[arg(long = "preset", conflicts_with = "pack")]
        preset: bool,
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
    #[command(after_long_help = DOCTOR_AFTER_LONG_HELP)]
    Doctor {
        /// Fail on warnings in addition to hard errors.
        #[arg(long, help_heading = "Safety & Supply Chain")]
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

    /// Open a workspace and/or VS Code profile in a single VS Code window.
    Open(OpenArgs),

    /// Open VS Code with a named profile and optional workspace path.
    Use(UseArgs),

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
    /// Canonical profile recipes (`profiles/` / `presets.jsonc`).
    Presets,
    /// Composable capability and toolchain packs (`packs/` / `modules/`).
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
    /// Retina 2x `HiDPI` preset.
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
    #[arg(help_heading = "Target Selectors")]
    pub path: Option<PathBuf>,

    /// Explicit workspace path alias for positional `PATH`.
    #[arg(
        long = "workspace",
        value_name = "PATH",
        help_heading = "Target Selectors"
    )]
    pub workspace: Option<PathBuf>,

    /// Target VS Code profile name.
    #[arg(
        short = 'p',
        long = "profile",
        value_name = "NAME",
        help_heading = "Target Selectors"
    )]
    pub profile: Option<String>,

    /// Canonical or legacy preset/recipe name.
    #[arg(
        long = "preset",
        value_name = "NAME",
        help_heading = "Target Selectors"
    )]
    pub preset: Option<String>,

    /// Target the VS Code Default user profile.
    #[arg(long = "user", help_heading = "Target Selectors")]
    pub user: bool,

    /// Additional pack/module IDs (comma-separated or repeatable).
    #[arg(
        long = "with",
        value_name = "ID,..",
        value_delimiter = ',',
        help_heading = "Composition"
    )]
    pub with: Vec<String>,

    /// Excluded pack/module IDs (comma-separated or repeatable).
    #[arg(
        long = "without",
        value_name = "ID,..",
        value_delimiter = ',',
        help_heading = "Composition"
    )]
    pub without: Vec<String>,

    /// Display UX preset (`crisp`, `retina`, or `default`).
    #[arg(long = "ux", value_enum, help_heading = "Composition")]
    pub ux: Option<UxArg>,

    /// AI mode (`full`, `copilot`, or `off`).
    #[arg(long = "ai", value_enum, help_heading = "Composition")]
    pub ai: Option<AiArg>,

    /// Shorthand for `--ai off`.
    #[arg(long = "no-ai", help_heading = "Composition")]
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
#[command(after_long_help = DIFF_AFTER_LONG_HELP)]
pub struct DiffArgs {
    /// Target selector and composition flags.
    #[command(flatten)]
    pub selector: SelectorArgs,

    /// Exit `3` if drift exists, or `0` if clean.
    #[arg(long = "check", help_heading = "Output & Runtime")]
    pub check: bool,

    /// Preview pruning retired (`--prune` / `--prune=retired`) or unmanaged (`--prune=unmanaged`) extensions.
    #[arg(
        long = "prune",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "retired",
        help_heading = "Safety & Supply Chain"
    )]
    pub prune: Option<PruneArg>,

    /// Explicit pruning mode (`retired`, `unmanaged`, or `all`) for extension removal preview.
    #[arg(
        long = "prune-mode",
        value_enum,
        value_name = "MODE",
        help_heading = "Safety & Supply Chain"
    )]
    pub prune_mode: Option<PruneArg>,

    /// Preview with fail-closed VSIX pin checks bypassed (matches `vsp sync --allow-unlocked`).
    #[arg(long = "allow-unlocked", help_heading = "Safety & Supply Chain")]
    pub allow_unlocked: bool,
}

impl DiffArgs {
    /// Resolves the effective [`PruneMode`] combining `--prune-mode` and `--prune`.
    #[must_use]
    pub fn effective_prune(&self) -> Option<PruneMode> {
        self.prune_mode.or(self.prune).map(Into::into)
    }

    /// Builds [`PlanExtras`] from this command's prune and lock-preview flags.
    #[must_use]
    pub fn to_plan_extras(&self) -> PlanExtras {
        let prune_mode = self.effective_prune();
        PlanExtras {
            prune: prune_mode.is_some(),
            prune_mode,
            allow_unlocked: self.allow_unlocked,
        }
    }
}

/// Extension pruning scope accepted by `vsp sync --prune[=MODE]` and `vsp diff --prune[=MODE]`.
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
#[command(after_long_help = SYNC_AFTER_LONG_HELP)]
pub struct SyncArgs {
    /// Target selector and composition flags.
    #[command(flatten)]
    pub selector: SelectorArgs,

    /// Import a `.code-profile` bundle file.
    #[arg(
        long = "from",
        value_name = "BUNDLE",
        help_heading = "Target Selectors"
    )]
    pub from: Option<PathBuf>,

    /// Reconcile all registered VS Code profiles plus Default.
    #[arg(long = "all", help_heading = "Target Selectors")]
    pub all: bool,

    /// Only execute Step 0 compromised/removed extension remediation.
    #[arg(long = "compromised-only", help_heading = "Safety & Supply Chain")]
    pub compromised_only: bool,

    /// Prune retired (`--prune` / `--prune=retired`) or unmanaged (`--prune=unmanaged`) extensions.
    #[arg(
        long = "prune",
        value_enum,
        num_args = 0..=1,
        default_missing_value = "retired",
        help_heading = "Safety & Supply Chain"
    )]
    pub prune: Option<PruneArg>,

    /// Explicit pruning mode (`retired`, `unmanaged`, or `all`).
    #[arg(
        long = "prune-mode",
        value_enum,
        value_name = "MODE",
        help_heading = "Safety & Supply Chain"
    )]
    pub prune_mode: Option<PruneArg>,

    /// Legacy compatibility flag equivalent to `--prune=retired`.
    #[arg(long = "prune-retired", hide = true)]
    pub prune_retired: bool,

    /// Legacy compatibility flag equivalent to `--prune=unmanaged`.
    #[arg(long = "prune-unmanaged", hide = true)]
    pub prune_unmanaged: bool,

    /// Re-verify and repair drifted pinned extensions.
    #[arg(long = "repair-pins", help_heading = "Safety & Supply Chain")]
    pub repair_pins: bool,

    /// Bypass fail-closed VSIX pin checks selectively (prints warning & records unverified state).
    #[arg(long = "allow-unlocked", help_heading = "Safety & Supply Chain")]
    pub allow_unlocked: bool,

    /// Allow bundle settings matching the autonomy/exec deny-list.
    #[arg(long = "allow-unsafe-settings", help_heading = "Safety & Supply Chain")]
    pub allow_unsafe_settings: bool,

    /// Allow mutating VS Code `storage.json` while VS Code is running.
    #[arg(long = "force", help_heading = "Safety & Supply Chain")]
    pub force: bool,

    /// Install extensions serially instead of batched.
    #[arg(long = "no-batch", help_heading = "Advanced / Overrides")]
    pub no_batch: bool,

    /// Skip XDG RTK shell wrapper installation.
    #[arg(long = "no-rtk", help_heading = "Composition")]
    pub no_rtk: bool,

    /// Reconcile extensions only without modifying `settings.json`, `storage.json`, or RTK wrappers (native equivalent of `vspcli reconcile`).
    #[arg(long = "extensions-only", help_heading = "Composition")]
    pub extensions_only: bool,
}

impl SyncArgs {
    /// Resolves the effective [`PruneMode`] combining `--prune-mode`, `--prune`, `--prune-retired`, and `--prune-unmanaged`.
    #[must_use]
    pub fn effective_prune(&self) -> Option<PruneMode> {
        if let Some(pm) = self.prune_mode {
            return Some(pm.into());
        }
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

/// Arguments for `vsp open`.
#[derive(Debug, Clone, Args)]
#[command(after_long_help = OPEN_AFTER_LONG_HELP)]
pub struct OpenArgs {
    /// Optional workspace path to open.
    #[arg(help_heading = "Target Selectors")]
    pub path: Option<PathBuf>,

    /// Named VS Code profile to open.
    #[arg(
        short = 'p',
        long = "profile",
        value_name = "NAME",
        help_heading = "Target Selectors"
    )]
    pub profile: Option<String>,

    /// Synchronize target before opening (aborts launch on sync failure).
    #[arg(long = "sync", help_heading = "Composition")]
    pub sync: bool,

    /// Allow mutating VS Code `storage.json` while VS Code is running during `--sync`.
    #[arg(long = "force", help_heading = "Safety & Supply Chain")]
    pub force: bool,
}

/// Arguments for `vsp use <PROFILE> [PATH]`.
#[derive(Debug, Clone, Args)]
#[command(after_long_help = USE_AFTER_LONG_HELP)]
pub struct UseArgs {
    /// Named VS Code profile to open (always resolved as a profile name, even if a matching directory exists).
    #[arg(index = 1, value_name = "PROFILE", help_heading = "Target Selectors")]
    pub profile: String,

    /// Optional workspace directory path to open inside the profile.
    #[arg(index = 2, value_name = "PATH", help_heading = "Target Selectors")]
    pub path: Option<PathBuf>,

    /// Synchronize target before opening (aborts launch on sync failure).
    #[arg(long = "sync", help_heading = "Composition")]
    pub sync: bool,

    /// Allow mutating VS Code `storage.json` while VS Code is running during `--sync`.
    #[arg(long = "force", help_heading = "Safety & Supply Chain")]
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

    /// Validate (`--check`) or refresh (`--refresh`) `vsp.lock.json` / `catalog/extension-pins.json`.
    Pins {
        /// Verify pin manifest digests, semver versions, and Marketplace URLs.
        #[arg(long)]
        check: bool,

        /// Refresh pins from the VS Code Marketplace and rewrite lockfile/manifest.
        #[arg(long)]
        refresh: bool,

        /// Fail `--check` on any `pending_refresh` or unpinned Tier 2 entry (release gate).
        #[arg(long = "strict", alias = "release-gate")]
        strict: bool,
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

        /// Legacy no-op flag (`vsp dev compat` always queries Marketplace directly).
        #[arg(long = "marketplace-only", hide = true)]
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
    #[command(hide = true)]
    Test,

    /// Release distribution workflow helper.
    #[command(hide = true)]
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

        /// Also replace a symbolic link that this installation does not own.
        #[arg(long)]
        force: bool,
    },

    /// Remove the `vsp` symbolic link from the user bin directory.
    Uninstall {
        /// Directory that contains the link (default `${XDG_BIN_HOME:-~/.local/bin}`).
        #[arg(long = "bin-dir", alias = "dir", value_name = "DIR")]
        bin_dir: Option<PathBuf>,

        /// Also remove a symbolic link that this installation does not own.
        #[arg(long)]
        force: bool,
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
    let started = Instant::now();
    let ctx = RunContext {
        dry_run: cli.dry_run,
        json: cli.json,
        quiet: cli.quiet,
        verbose: cli.verbose,
        yes: cli.yes,
    };
    let Cli {
        catalog: catalog_override,
        config: config_override,
        vscode_data_dir,
        command,
        ..
    } = cli;

    let result = dispatch_command(
        command,
        catalog_override.as_deref(),
        config_override.as_deref(),
        vscode_data_dir,
        ctx,
    );

    match &result {
        Ok(code) => verbose_log(
            ctx,
            format_args!(
                "done: exit={} elapsed={}ms",
                code.as_i32(),
                started.elapsed().as_millis()
            ),
        ),
        Err(err) => verbose_log(
            ctx,
            format_args!(
                "failed: exit={} kind={} elapsed={}ms",
                err.exit_code().as_i32(),
                err.kind(),
                started.elapsed().as_millis()
            ),
        ),
    }

    result
}

fn dispatch_command(
    command: Command,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    match command {
        Command::Completions { shell } => run_completions(shell, catalog_override, ctx),
        Command::Init(args) => run_init(
            &args,
            catalog_override,
            config_override,
            vscode_data_dir,
            ctx,
        ),
        Command::CompatVspcli { args } => Ok(crate::compat_vspcli::run(&args)),
        Command::SelfCmd { command } => run_self(command, ctx),
        Command::List { kind, long } => {
            let kind = kind.unwrap_or(ListKind::Presets);
            if kind == ListKind::Installed {
                verbose_log(ctx, "target: list installed");
                return run_list_installed(vscode_data_dir.as_deref(), long, ctx);
            }
            let scope = match kind {
                ListKind::Presets => UserConfigScope::Required,
                ListKind::Packs | ListKind::Ux | ListKind::Installed => {
                    UserConfigScope::SkipDefault
                }
            };
            let (catalog, user) =
                load_catalog_with_user_scoped(catalog_override, config_override, scope, ctx)?;
            verbose_log(ctx, format_args!("target: list {kind:?} long={long}"));
            run_list(&catalog, &user.overlay, kind, long, ctx)
        }
        Command::Show { name, pack, preset } => {
            run_show_cmd(&name, pack, preset, catalog_override, config_override, ctx)
        }
        Command::Detect { path, explain } => {
            let catalog = load_catalog(catalog_override, ctx)?;
            let target = path.unwrap_or_else(|| PathBuf::from("."));
            verbose_log(
                ctx,
                format_args!("target: detect path={} explain={explain}", target.display()),
            );
            run_detect(&catalog, &target, explain, ctx)
        }
        Command::Diff(args) => run_diff_cmd(
            &args,
            catalog_override,
            config_override,
            vscode_data_dir,
            ctx,
        ),
        Command::Doctor { strict } => run_doctor_cmd(
            strict,
            catalog_override,
            config_override,
            vscode_data_dir.as_deref(),
            ctx,
        ),
        Command::Export { names, out } => {
            run_export_cmd(&names, out, catalog_override, config_override, ctx)
        }
        Command::Sync(args) => run_sync_cmd(
            &args,
            catalog_override,
            config_override,
            vscode_data_dir,
            ctx,
        ),
        Command::Open(args) => run_open_cmd(
            &args,
            catalog_override,
            config_override,
            vscode_data_dir,
            ctx,
        ),
        Command::Use(args) => run_use_cmd(
            &args,
            catalog_override,
            config_override,
            vscode_data_dir,
            ctx,
        ),
        Command::Dev { command } => run_dev(catalog_override, config_override, ctx, command),
    }
}

/// Writes UTF-8 text to stdout, treating `BrokenPipe` as clean exit `0` with empty stderr.
fn write_stdout(text: &str) -> VspResult<()> {
    write_stdout_bytes(text.as_bytes())
}

/// Writes raw bytes to stdout, treating `BrokenPipe` as clean exit `0` with empty stderr.
fn write_stdout_bytes(bytes: &[u8]) -> VspResult<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    match handle.write_all(bytes).and_then(|()| handle.flush()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(VspError::Operational(format!(
            "failed to write to stdout: {err}"
        ))),
    }
}

fn verbose_log(ctx: RunContext, msg: impl fmt::Display) {
    if ctx.verbose {
        let _ = writeln!(io::stderr(), "[vsp] {msg}");
    }
}

fn run_completions(
    shell: CompletionShell,
    catalog_override: Option<&Path>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    if ctx.json {
        return Err(VspError::UsageOrConfig(
            "--json is not supported with completions".to_string(),
        ));
    }
    let catalog = load_catalog(catalog_override, ctx)?;
    verbose_log(ctx, format_args!("target: completions shell={shell:?}"));
    let mut cmd = command_with_live_values(&catalog);
    let clap_shell = match shell {
        CompletionShell::Bash => Shell::Bash,
        CompletionShell::Zsh => Shell::Zsh,
        CompletionShell::Fish => Shell::Fish,
    };
    let mut buf = Vec::new();
    clap_complete::generate(clap_shell, &mut cmd, "vsp", &mut buf);
    write_stdout_bytes(&buf)?;
    Ok(ExitCode::Ok)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShowTargetKind {
    Auto,
    Pack,
    Preset,
}

fn parse_show_target(
    raw: &str,
    pack_flag: bool,
    preset_flag: bool,
) -> VspResult<(ShowTargetKind, &str)> {
    if let Some(stripped) = raw.strip_prefix("pack:") {
        if preset_flag {
            return Err(VspError::UsageOrConfig(
                "cannot combine --preset with 'pack:' prefix".to_string(),
            ));
        }
        if stripped.is_empty() {
            return Err(VspError::UsageOrConfig(
                "missing pack ID after 'pack:' prefix".to_string(),
            ));
        }
        return Ok((ShowTargetKind::Pack, stripped));
    }
    if let Some(stripped) = raw.strip_prefix("preset:") {
        if pack_flag {
            return Err(VspError::UsageOrConfig(
                "cannot combine --pack with 'preset:' prefix".to_string(),
            ));
        }
        if stripped.is_empty() {
            return Err(VspError::UsageOrConfig(
                "missing preset name after 'preset:' prefix".to_string(),
            ));
        }
        return Ok((ShowTargetKind::Preset, stripped));
    }
    let kind = if pack_flag {
        ShowTargetKind::Pack
    } else if preset_flag {
        ShowTargetKind::Preset
    } else {
        ShowTargetKind::Auto
    };
    Ok((kind, raw))
}

fn run_show_cmd(
    raw_name: &str,
    pack_flag: bool,
    preset_flag: bool,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    let (target_kind, name) = parse_show_target(raw_name, pack_flag, preset_flag)?;
    let scope = if target_kind == ShowTargetKind::Pack {
        UserConfigScope::SkipDefault
    } else {
        UserConfigScope::FallbackOnDefaultError
    };
    let (mut catalog, mut user) =
        load_catalog_with_user_scoped(catalog_override, config_override, scope, ctx)?;

    let is_builtin = match target_kind {
        ShowTargetKind::Pack => catalog.modules.contains_key(name),
        ShowTargetKind::Preset => is_builtin_preset_or_alias(&catalog, name),
        ShowTargetKind::Auto => {
            catalog.modules.contains_key(name) || is_builtin_preset_or_alias(&catalog, name)
        }
    };

    if !is_builtin {
        // Custom profile lookup must never succeed silently or mask a broken default config.
        user.fail_if_default_config_broken()?;
    } else {
        user.emit_fallback_warnings(ctx);
    }

    match target_kind {
        ShowTargetKind::Preset => {
            catalog.materialize_implicit_preset(name);
        }
        ShowTargetKind::Auto => {
            // A toolchain pack ID shows the pack. `--preset <toolchain>` uses its implicit
            // preset, which `show` reports for a name that is not a pack or preset alias.
            if !catalog.modules.contains_key(name) {
                catalog.materialize_implicit_preset(name);
            }
        }
        ShowTargetKind::Pack => {}
    }

    verbose_log(
        ctx,
        format_args!("target: show name={name} mode={target_kind:?}"),
    );
    run_show(&catalog, &user.overlay, name, target_kind, ctx)
}

fn run_diff_cmd(
    args: &DiffArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    // 1. Validate empty and conflicting selectors BEFORE loading config or catalog (ASTRA-04).
    let has_selector = args.selector.path.is_some()
        || args.selector.workspace.is_some()
        || args.selector.profile.is_some()
        || args.selector.preset.is_some()
        || args.selector.user;
    if !has_selector {
        return Err(VspError::UsageOrConfig(
            "vsp diff requires an explicit selector (<PATH>, -p/--profile <NAME>, --preset <NAME>, or --user)"
                .to_string(),
        ));
    }
    let _ = args.selector.effective_ai()?;
    let effective_ws = args.selector.effective_workspace()?;
    validate_selector_combination(SelectorKinds {
        workspace: effective_ws.is_some(),
        profile: args.selector.profile.is_some(),
        user: args.selector.user,
        from_bundle: false,
        all: false,
    })?;

    // 2. Scope user config loading: workspace diff tolerates malformed default config when
    //    using auto-detect or a built-in preset, while explicit `--config` and custom profile
    //    lookups always fail closed.
    let scope = if effective_ws.is_some() {
        UserConfigScope::FallbackOnDefaultError
    } else {
        UserConfigScope::Required
    };
    let (mut catalog, mut user) =
        load_catalog_with_user_scoped(catalog_override, config_override, scope, ctx)?;

    if effective_ws.is_some() {
        let needs_custom_preset = args
            .selector
            .preset
            .as_deref()
            .is_some_and(|p| !is_builtin_preset_or_alias(&catalog, p));
        if needs_custom_preset {
            user.fail_if_default_config_broken()?;
        } else {
            user.emit_fallback_warnings(ctx);
        }
    }

    if let Some(ref ws_path) = effective_ws {
        check_workspace_path_profile_hint("diff", ws_path, &catalog, &user.overlay)?;
    }

    materialize_selector_presets(&mut catalog, &args.selector);
    let mut opts = args.selector.to_plan_options(vscode_data_dir)?;
    apply_user_context_to_plan(&catalog, &mut opts, &args.selector, &user, false);
    let extras = args.to_plan_extras();
    verbose_log(
        ctx,
        format_args!(
            "target: diff workspace={:?} profile={:?} preset={:?} user={} prune={:?} allow_unlocked={}",
            opts.workspace_path,
            opts.profile,
            opts.preset,
            opts.user,
            extras.effective_prune(),
            extras.allow_unlocked
        ),
    );
    let plan = compute_plan_with_extras(&catalog, &opts, &extras)
        .map_err(|err| enrich_unknown_profile_error("diff", &args.selector, err))?;
    print_plan(&plan, ctx)?;
    Ok(plan_exit_code(&plan, args.check))
}

fn run_doctor_cmd(
    strict: bool,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<&Path>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    // Repository validation must see the unchanged catalog, so the user profiles
    // are resolved against a copy.
    let catalog = load_catalog(catalog_override, ctx)?;
    let config_path = resolve_user_config_path(config_override);
    // A broken or stale user config must not stop the security audit. The error is
    // recorded as a FAIL check and every other check still runs.
    let user_state =
        load_user_config_scoped(config_override, UserConfigScope::Required).and_then(|loaded| {
            let overlay = match &loaded.config {
                Some(cfg) => apply_user_config(&mut catalog.clone(), cfg)?,
                None => UserOverlay::default(),
            };
            Ok((loaded.config, overlay))
        });

    match &user_state {
        Ok((Some(cfg), _)) => verbose_log(
            ctx,
            format_args!(
                "config: {} (loaded, {} custom profile(s))",
                cfg.path.display(),
                cfg.profiles.len()
            ),
        ),
        Ok((None, _)) => verbose_log(
            ctx,
            format_args!("config: {} (absent)", config_path.display()),
        ),
        Err(err) => verbose_log(
            ctx,
            format_args!(
                "config: {} (error recorded for audit: {err})",
                config_path.display()
            ),
        ),
    }

    let loaded_user_cfg = user_state.as_ref().ok().and_then(|(cfg, _)| cfg.as_ref());
    verbose_log(ctx, format_args!("target: doctor strict={strict}"));
    let mut report = run_doctor_with_options(&catalog, vscode_data_dir, strict, loaded_user_cfg);
    match &user_state {
        Ok((config, overlay)) => {
            audit_user_config(&mut report, &config_path, config.as_ref(), overlay);
        }
        Err(err) => audit_user_config_error(&mut report, &config_path, err),
    }
    if ctx.json {
        write_stdout(&to_canonical_json_string(&report)?)?;
    } else if !ctx.quiet {
        let mut out = String::new();
        for check in &report.checks {
            let tag = match check.severity {
                CheckSeverity::Pass => "PASS",
                CheckSeverity::Warn => "WARN",
                CheckSeverity::Fail => "FAIL",
            };
            let _ = writeln!(out, "{tag}: {}", check.message);
        }
        let _ = writeln!(
            out,
            "Summary: {} fail(s), {} warning(s)",
            report.fails, report.warns
        );
        write_stdout(&out)?;
    }
    Ok(report.exit_code())
}

fn run_export_cmd(
    names: &[String],
    out: Option<PathBuf>,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    let (mut catalog, user) = load_catalog_with_user_scoped(
        catalog_override,
        config_override,
        UserConfigScope::Required,
        ctx,
    )?;
    for name in names {
        catalog.materialize_implicit_preset(name);
    }
    let out_dir = out.unwrap_or_else(|| PathBuf::from("exports"));
    verbose_log(
        ctx,
        format_args!(
            "target: export names={names:?} out={} dry_run={}",
            out_dir.display(),
            ctx.dry_run
        ),
    );
    let report = export_profiles_with_options(
        &catalog,
        names,
        &out_dir,
        false,
        &user.overlay.bundle_additions(),
        ctx.dry_run,
    )?;
    print_dist_report(&report, ctx)?;
    Ok(ExitCode::Ok)
}

fn run_sync_cmd(
    args: &SyncArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
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
    if args.extensions_only && (effective_ws.is_some() || args.selector.user || args.from.is_some())
    {
        return Err(VspError::UsageOrConfig(
            "--extensions-only applies to a profile target (-p/--profile, --preset) or --all only"
                .to_string(),
        ));
    }
    if let Some(ref bundle) = args.from {
        if !bundle.is_file() {
            return Err(VspError::TargetMissing(format!(
                "bundle file not found: {}",
                bundle.display()
            )));
        }
    }

    // Emergency remediation (`--compromised-only`, with or without `--all`) and bundle
    // import (`--from`) must work when the personal user config is broken or stale
    // after a catalog update. Workspace sync using built-in presets/auto-detect also
    // falls back on default config errors while explicit `--config` always fails closed.
    let is_emergency_or_bundle = args.compromised_only || args.from.is_some();
    let scope = if is_emergency_or_bundle || effective_ws.is_some() {
        UserConfigScope::FallbackOnDefaultError
    } else {
        UserConfigScope::Required
    };
    let (mut catalog, mut user) =
        load_catalog_with_user_scoped(catalog_override, config_override, scope, ctx)?;

    if is_emergency_or_bundle {
        if user.has_fallback_error() {
            let reason = user.fallback_summary();
            let _ = writeln!(
                io::stderr(),
                "vsp: WARNING: the personal user config is ignored for this command because it cannot be used: {reason}"
            );
            let _ = writeln!(
                io::stderr(),
                "vsp: WARNING: custom profiles, defaults, and extraExtensions are not applied; fix the config (see 'vsp doctor')"
            );
        }
    } else if effective_ws.is_some() {
        let needs_custom_preset = args
            .selector
            .preset
            .as_deref()
            .is_some_and(|p| !is_builtin_preset_or_alias(&catalog, p));
        if needs_custom_preset {
            user.fail_if_default_config_broken()?;
        } else {
            user.emit_fallback_warnings(ctx);
        }
    }

    if let Some(ref ws_path) = effective_ws {
        check_workspace_path_profile_hint("sync", ws_path, &catalog, &user.overlay)?;
    }

    materialize_selector_presets(&mut catalog, &args.selector);
    let effective_prune = args.effective_prune();

    // Unified read-only planner path: `vsp sync --dry-run` emits the EXACT same
    // `vsp-plan/v1` JSON and text output as `vsp diff`, and exits `5` when `would_refuse`
    // is non-empty so dry-run accurately predicts apply refusals.
    if ctx.dry_run && has_std_selector && args.from.is_none() && !args.all && !args.compromised_only
    {
        let mut opts = args.selector.to_plan_options(vscode_data_dir)?;
        apply_user_context_to_plan(&catalog, &mut opts, &args.selector, &user, args.no_rtk);
        let extras = PlanExtras {
            prune: effective_prune.is_some(),
            prune_mode: effective_prune,
            allow_unlocked: args.allow_unlocked,
        };
        verbose_log(
            ctx,
            format_args!(
                "target: sync --dry-run workspace={:?} profile={:?} preset={:?} user={} prune={:?} allow_unlocked={}",
                opts.workspace_path,
                opts.profile,
                opts.preset,
                opts.user,
                extras.effective_prune(),
                extras.allow_unlocked
            ),
        );
        let mut plan = compute_plan_with_extras(&catalog, &opts, &extras)
            .map_err(|err| enrich_unknown_profile_error("sync", &args.selector, err))?;
        if args.extensions_only {
            // An extensions-only sync writes no settings, so the preview shows none.
            plan.settings_changes.clear();
            plan.task_changes.clear();
            plan.has_drift = plan.observation_complete && !plan.extension_changes.is_empty();
        }
        print_plan(&plan, ctx)?;
        if !plan.observation_complete {
            return Ok(ExitCode::IncompleteObservation);
        }
        if !plan.would_refuse.is_empty() {
            return Ok(ExitCode::SafetyRefusal);
        }
        return Ok(ExitCode::Ok);
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
        prune: effective_prune,
        repair_pins: args.repair_pins,
        allow_unlocked: args.allow_unlocked,
        allow_unsafe_settings: args.allow_unsafe_settings,
        force: args.force,
        yes: ctx.yes,
        no_batch: args.no_batch,
        no_rtk: personal.no_rtk,
        vscode_data_dir,
        user_overlay: user.overlay,
        extensions_only: args.extensions_only,
    };

    verbose_log(
        ctx,
        format_args!(
            "target: sync workspace={:?} profile={:?} preset={:?} user={} all={} from={:?} extensions_only={} dry_run={}",
            sync_opts.workspace_path,
            sync_opts.profile,
            sync_opts.preset,
            sync_opts.user,
            sync_opts.all,
            sync_opts.from_bundle,
            sync_opts.extensions_only,
            ctx.dry_run
        ),
    );

    if ctx.dry_run {
        // `--from`, `--all`, and `--compromised-only` previews. A bundle goes
        // through the full pre-mutation validation of the real import (checksum,
        // schema, deny-list, profile name, extension IDs, publisher allowlist).
        let bundle_plan = match &sync_opts.from_bundle {
            Some(bundle) => Some(prepare_bundle_import(&catalog, bundle, &sync_opts)?),
            None => None,
        };
        if ctx.json {
            let payload = serde_json::json!({
                "dry_run": true,
                "all": args.all,
                "compromised_only": args.compromised_only,
                "from": args.from.as_ref().map(|p| p.display().to_string()),
                "profile": args.selector.profile,
                "bundle": bundle_plan,
            });
            write_stdout(&to_canonical_json_string(&payload)?)?;
        } else if !ctx.quiet {
            let mut out = String::new();
            let _ = writeln!(
                out,
                "DRY-RUN sync: all={} compromised_only={} from={} profile={}",
                args.all,
                args.compromised_only,
                args.from
                    .as_deref()
                    .map_or_else(|| "none".to_string(), |p| p.display().to_string()),
                args.selector.profile.as_deref().unwrap_or("none")
            );
            if let Some(plan) = &bundle_plan {
                let _ = writeln!(
                    out,
                    "  bundle ok: profile={} settings={} extensions={}",
                    plan.profile_name,
                    plan.settings_count,
                    plan.install_ids.len()
                );
                for w in &plan.warnings {
                    let _ = writeln!(io::stderr(), "warning: {w}");
                }
            }
            write_stdout(&out)?;
        }
        return Ok(ExitCode::Ok);
    }

    let report = run_sync(&catalog, &sync_opts)
        .map_err(|err| enrich_unknown_profile_error("sync", &args.selector, err))?;
    print_sync_report(&report, ctx)?;
    Ok(ExitCode::Ok)
}

fn run_open_cmd(
    args: &OpenArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    if args.profile.is_none() && args.path.is_none() {
        return Err(VspError::UsageOrConfig(
            "vsp open requires -p/--profile <NAME> or <PATH>".to_string(),
        ));
    }
    let (mut catalog, user) = load_catalog_with_user_scoped(
        catalog_override,
        config_override,
        UserConfigScope::Required,
        ctx,
    )?;
    if let Some(name) = &args.profile {
        catalog.materialize_implicit_preset(name);
    }
    let open_opts = build_open_options(
        &catalog,
        args.profile.clone(),
        args.path.clone(),
        args.sync,
        args.force,
        vscode_data_dir,
        &user,
        ctx,
    );
    verbose_log(
        ctx,
        format_args!(
            "target: open profile={:?} path={:?} sync={} dry_run={}",
            open_opts.profile, open_opts.path, open_opts.sync_first, open_opts.dry_run
        ),
    );
    run_open(&catalog, &open_opts)
}

fn run_use_cmd(
    args: &UseArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    let (mut catalog, user) = load_catalog_with_user_scoped(
        catalog_override,
        config_override,
        UserConfigScope::Required,
        ctx,
    )?;
    catalog.materialize_implicit_preset(&args.profile);
    let open_opts = build_open_options(
        &catalog,
        Some(args.profile.clone()),
        args.path.clone(),
        args.sync,
        args.force,
        vscode_data_dir,
        &user,
        ctx,
    );
    verbose_log(
        ctx,
        format_args!(
            "target: use profile={} path={:?} sync={} dry_run={}",
            args.profile, args.path, args.sync, ctx.dry_run
        ),
    );
    run_use(&catalog, &args.profile, args.path.as_deref(), &open_opts).map(|_| ExitCode::Ok)
}

#[allow(clippy::too_many_arguments)]
fn build_open_options(
    catalog: &WorkspaceCatalog,
    profile: Option<String>,
    path: Option<PathBuf>,
    sync_first: bool,
    force: bool,
    vscode_data_dir: Option<PathBuf>,
    user: &UserContext,
    ctx: RunContext,
) -> OpenOptions {
    let profile_selector = SelectorArgs {
        path: None,
        workspace: None,
        profile: profile.clone(),
        preset: None,
        user: false,
        with: Vec::new(),
        without: Vec::new(),
        ux: None,
        ai: None,
        no_ai: false,
    };
    let personal = resolve_personal_defaults(
        catalog,
        &profile_selector,
        None,
        None,
        false,
        user.defaults(),
    );
    OpenOptions {
        path,
        profile,
        sync_first,
        dry_run: ctx.dry_run,
        json: ctx.json,
        quiet: ctx.quiet,
        vscode_data_dir,
        yes: ctx.yes,
        force,
        user_overlay: user.overlay.clone(),
        ux: personal.ux,
        ai: personal.ai,
        with_modules: personal.with_modules,
        no_rtk: personal.no_rtk,
    }
}

fn load_catalog(catalog_override: Option<&Path>, ctx: RunContext) -> VspResult<WorkspaceCatalog> {
    let source_desc = if let Some(p) = catalog_override {
        format!("dir={}", p.display())
    } else if let Some(env_dir) = std::env::var("VSPCLI_ROOT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        format!("env(VSPCLI_ROOT)={env_dir}")
    } else {
        "embedded".to_string()
    };
    let catalog = load_catalog_auto(catalog_override)?;
    verbose_log(
        ctx,
        format_args!(
            "catalog: {source_desc} ({} presets, {} packs)",
            catalog.profiles.len(),
            catalog.modules.len()
        ),
    );
    Ok(catalog)
}

/// Personal user configuration state for one command invocation.
struct UserContext {
    /// Loaded configuration, or `None` when no configuration file exists or was skipped.
    config: Option<UserConfig>,
    /// Resolved user-profile data (empty when `config` is `None`).
    overlay: UserOverlay,
    /// Non-fatal fallback warnings captured under [`UserConfigScope::FallbackOnDefaultError`].
    fallback_warnings: Vec<String>,
    /// Deferred `apply_user_config` failure captured under [`UserConfigScope::FallbackOnDefaultError`].
    fallback_apply_error: Option<VspError>,
    /// `true` when `load_user_config_scoped` fell back from a default config load/parse error.
    had_default_load_error: bool,
}

impl UserContext {
    fn empty() -> Self {
        Self {
            config: None,
            overlay: UserOverlay::default(),
            fallback_warnings: Vec::new(),
            fallback_apply_error: None,
            had_default_load_error: false,
        }
    }

    fn defaults(&self) -> Option<&UserDefaults> {
        self.config.as_ref().map(|cfg| &cfg.defaults)
    }

    fn has_fallback_error(&self) -> bool {
        self.had_default_load_error || self.fallback_apply_error.is_some()
    }

    fn fallback_summary(&self) -> String {
        if let Some(err) = &self.fallback_apply_error {
            return err.to_string();
        }
        self.fallback_warnings.join("; ")
    }

    fn emit_fallback_warnings(&self, ctx: RunContext) {
        if ctx.quiet {
            return;
        }
        for w in &self.fallback_warnings {
            let _ = writeln!(io::stderr(), "vsp: WARNING: {w}");
        }
    }

    fn fail_if_default_config_broken(&mut self) -> VspResult<()> {
        if let Some(err) = self.fallback_apply_error.take() {
            return Err(err);
        }
        if self.had_default_load_error {
            // Re-run with `Required` to surface the exact fail-closed error and exit code.
            let _ = load_user_config_scoped(None, UserConfigScope::Required)?;
        }
        Ok(())
    }
}

/// Loads the catalog and personal user configuration according to `scope`.
///
/// Explicit `--config <PATH>` always loads and fails closed regardless of `scope`.
fn load_catalog_with_user_scoped(
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    scope: UserConfigScope,
    ctx: RunContext,
) -> VspResult<(WorkspaceCatalog, UserContext)> {
    let catalog = load_catalog(catalog_override, ctx)?;
    let config_path = resolve_user_config_path(config_override);
    let loaded = load_user_config_scoped(config_override, scope)?;

    if !loaded.warnings.is_empty() {
        verbose_log(
            ctx,
            format_args!(
                "config: {} (fallback on error: {})",
                config_path.display(),
                loaded.warnings.join("; ")
            ),
        );
        return Ok((
            catalog,
            UserContext {
                config: None,
                overlay: UserOverlay::default(),
                fallback_warnings: loaded.warnings,
                fallback_apply_error: None,
                had_default_load_error: true,
            },
        ));
    }

    let Some(cfg) = loaded.config else {
        if config_override.is_none() && scope == UserConfigScope::SkipDefault {
            verbose_log(ctx, "config: skipped (scope=skip-default)");
        } else {
            verbose_log(
                ctx,
                format_args!("config: {} (absent)", config_path.display()),
            );
        }
        return Ok((catalog, UserContext::empty()));
    };

    // The profiles are added to a copy: a failure part-way leaves `catalog` unchanged.
    let mut with_user = catalog.clone();
    match apply_user_config(&mut with_user, &cfg) {
        Ok(overlay) => {
            verbose_log(
                ctx,
                format_args!(
                    "config: {} (loaded, {} custom profile(s))",
                    cfg.path.display(),
                    cfg.profiles.len()
                ),
            );
            Ok((
                with_user,
                UserContext {
                    config: Some(cfg),
                    overlay,
                    fallback_warnings: Vec::new(),
                    fallback_apply_error: None,
                    had_default_load_error: false,
                },
            ))
        }
        Err(err)
            if config_override.is_none() && scope == UserConfigScope::FallbackOnDefaultError =>
        {
            let warn = format!("ignoring unusable default user config: {err}");
            verbose_log(
                ctx,
                format_args!(
                    "config: {} (fallback on apply error: {err})",
                    cfg.path.display()
                ),
            );
            Ok((
                catalog,
                UserContext {
                    config: None,
                    overlay: UserOverlay::default(),
                    fallback_warnings: vec![warn],
                    fallback_apply_error: Some(err),
                    had_default_load_error: false,
                },
            ))
        }
        Err(err) => Err(err),
    }
}

fn is_builtin_preset_or_alias(catalog: &WorkspaceCatalog, name: &str) -> bool {
    catalog.profiles.contains_key(name)
        || catalog.resolve_alias(name).is_some()
        || catalog.implicit_toolchain_recipe(name).is_some()
}

fn check_workspace_path_profile_hint(
    cmd_name: &str,
    ws_path: &Path,
    catalog: &WorkspaceCatalog,
    overlay: &UserOverlay,
) -> VspResult<()> {
    if !ws_path.is_dir() {
        if let Some(candidate) = ws_path.to_str() {
            let matches_profile = is_builtin_preset_or_alias(catalog, candidate)
                || overlay.is_user_profile(candidate);
            if matches_profile {
                return Err(VspError::TargetMissing(format!(
                    "workspace path does not exist or is not a directory: {} (did you mean profile '{candidate}'? Run 'vsp {cmd_name} -p {candidate}')",
                    ws_path.display()
                )));
            }
        }
    }
    Ok(())
}

fn enrich_unknown_profile_error(
    cmd_name: &str,
    selector: &SelectorArgs,
    err: VspError,
) -> VspError {
    if let VspError::TargetMissing(ref msg) = err {
        if let Some(profile_name) = selector.profile.as_deref() {
            if selector.preset.is_none()
                && selector.with.is_empty()
                && msg.contains("unknown profile")
            {
                return VspError::TargetMissing(format!(
                    "{msg} (hint: to target a custom profile name, pass --preset, e.g. 'vsp {cmd_name} -p {profile_name} --preset rust', or run 'vsp list' to see available presets)"
                ));
            }
        }
    }
    err
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
    // The `string` feature of `clap` lets a possible value own its `String`, so the live
    // names need no `'static` lifetime.
    fn values(names: &[String]) -> PossibleValuesParser {
        PossibleValuesParser::new(names.to_vec())
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

    let mut cmd = Cli::command();
    for sub in ["diff", "sync", "init"] {
        cmd = cmd.mut_subcommand(sub, |c| {
            c.mut_arg("preset", |a| a.value_parser(values(&preset_names)))
                .mut_arg("profile", |a| a.value_parser(values(&preset_names)))
                .mut_arg("with", |a| a.value_parser(values(&pack_names)))
                .mut_arg("without", |a| a.value_parser(values(&pack_names)))
        });
    }
    cmd.mut_subcommand("open", |c| {
        c.mut_arg("profile", |a| a.value_parser(values(&preset_names)))
    })
    .mut_subcommand("use", |c| {
        c.mut_arg("profile", |a| a.value_parser(values(&preset_names)))
    })
    .mut_subcommand("export", |c| {
        c.mut_arg("names", |a| a.value_parser(values(&preset_names)))
    })
    .mut_subcommand("show", |c| {
        c.mut_arg("name", |a| a.value_parser(values(&show_names)))
    })
}

fn run_init(
    args: &InitArgs,
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    vscode_data_dir: Option<PathBuf>,
    ctx: RunContext,
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

    // 2. Build the starter text. Reject composition flags targeting a reserved built-in preset
    //    name when `--apply` is not set so user composition intent is never silently discarded.
    let catalog = load_catalog(catalog_override, ctx)?;
    if let Some(name) = &args.profile {
        let is_reserved_preset =
            catalog.profiles.contains_key(name) || catalog.resolve_alias(name).is_some();
        if is_reserved_preset && has_composition && !args.apply {
            return Err(VspError::UsageOrConfig(format!(
                "cannot attach --preset/--with/--without to reserved built-in preset name '{name}' without --apply; pick a custom -p <name> (e.g. 'vsp init -p my-{name} --preset {name}')"
            )));
        }
    }

    let custom_profile = match &args.profile {
        Some(name)
            if !catalog.profiles.contains_key(name) && catalog.resolve_alias(name).is_none() =>
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

    // 3. Validate the text with the real loader before it is written. The default path is
    //    used only when it does not depend on the working directory.
    let path = match config_override {
        Some(explicit) => explicit.to_path_buf(),
        None => trusted_default_user_config_path().ok_or_else(|| {
            VspError::UsageOrConfig(
                "cannot resolve the default user config path: HOME is not an absolute path and XDG_CONFIG_HOME is not set to one (pass --config <PATH>)"
                    .to_string(),
            )
        })?,
    };
    verbose_log(
        ctx,
        format_args!(
            "target: init path={} apply={} dry_run={}",
            path.display(),
            args.apply,
            ctx.dry_run
        ),
    );
    let parsed = parse_user_config(&content, &path)?;
    let mut user_catalog = catalog.clone();
    let overlay = apply_user_config(&mut user_catalog, &parsed)?;

    // 4. Refuse to replace an existing config unless --force is given (exit 5).
    check_user_config_target(&path, args.force)?;

    if ctx.dry_run {
        if ctx.json {
            let payload = serde_json::json!({
                "dry_run": true,
                "path": path.display().to_string(),
                "created": false,
                "applied": false,
                "content": content,
            });
            write_stdout(&to_canonical_json_string(&payload)?)?;
        } else if !ctx.quiet {
            let mut out = String::new();
            let _ = writeln!(out, "DRY-RUN: would write user config {}", path.display());
            out.push_str(&content);
            write_stdout(&out)?;
        }
        return Ok(ExitCode::Ok);
    }

    let backup = write_user_config_with_backup(&path, &content, args.force)?;
    if !ctx.json && !ctx.quiet {
        let mut out = String::new();
        let _ = writeln!(out, "Wrote user config {}", path.display());
        if let Some(backup_path) = &backup {
            let _ = writeln!(out, "Previous config saved as {}", backup_path.display());
        }
        write_stdout(&out)?;
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
            yes: ctx.yes,
            no_rtk: args.no_rtk,
            vscode_data_dir,
            user_overlay: overlay,
            ..Default::default()
        };
        Some(run_sync(&user_catalog, &sync_opts)?)
    } else {
        None
    };

    if ctx.json {
        let payload = serde_json::json!({
            "dry_run": false,
            "path": path.display().to_string(),
            "created": true,
            "backup": backup.as_ref().map(|p| p.display().to_string()),
            "applied": sync_report.is_some(),
            "sync": sync_report,
        });
        write_stdout(&to_canonical_json_string(&payload)?)?;
    } else if let Some(report) = &sync_report {
        print_sync_report(report, RunContext { json: false, ..ctx })?;
    }
    Ok(ExitCode::Ok)
}

fn run_self(command: SelfCommand, ctx: RunContext) -> VspResult<ExitCode> {
    match command {
        SelfCommand::Path => {
            verbose_log(ctx, "target: self path");
            let canon = self_ops::current_executable()?;
            if ctx.json {
                let payload = serde_json::json!({ "path": canon.display().to_string() });
                write_stdout(&to_canonical_json_string(&payload)?)?;
            } else {
                write_stdout(&format!("{}\n", canon.display()))?;
            }
            Ok(ExitCode::Ok)
        }
        SelfCommand::Install {
            bin_dir,
            with_vspcli,
            force,
        } => {
            verbose_log(
                ctx,
                format_args!(
                    "target: self install bin_dir={bin_dir:?} with_vspcli={with_vspcli} force={force} dry_run={}",
                    ctx.dry_run
                ),
            );
            let report = self_ops::install(bin_dir.as_deref(), with_vspcli, force, ctx.dry_run)?;
            print_self_link_report(&report, ctx)?;
            Ok(ExitCode::Ok)
        }
        SelfCommand::Uninstall { bin_dir, force } => {
            verbose_log(
                ctx,
                format_args!(
                    "target: self uninstall bin_dir={bin_dir:?} force={force} dry_run={}",
                    ctx.dry_run
                ),
            );
            let report = self_ops::uninstall(bin_dir.as_deref(), force, ctx.dry_run)?;
            print_self_link_report(&report, ctx)?;
            Ok(ExitCode::Ok)
        }
        SelfCommand::Update => {
            verbose_log(ctx, "target: self update");
            let guidance = self_ops::update_guidance();
            if ctx.json {
                write_stdout(&to_canonical_json_string(&guidance)?)?;
            } else {
                write_stdout(&self_ops::format_update_guidance(&guidance))?;
            }
            Ok(ExitCode::Ok)
        }
    }
}

fn print_self_link_report(report: &SelfLinkReport, ctx: RunContext) -> VspResult<()> {
    if ctx.json {
        return write_stdout(&to_canonical_json_string(report)?);
    }
    if ctx.quiet {
        return Ok(());
    }
    let prefix = if report.dry_run { "DRY-RUN: " } else { "" };
    let mut out = String::new();
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
            Some(target) => {
                let _ = writeln!(out, "{prefix}{verb}: {} -> {target}", item.link);
            }
            None => {
                let _ = writeln!(out, "{prefix}{verb}: {}", item.link);
            }
        }
    }
    write_stdout(&out)
}

fn run_list_installed(
    vscode_data_dir: Option<&Path>,
    long: bool,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    let store = resolve_vscode_store(vscode_data_dir, false)?;
    let mut profiles = vec!["default".to_string()];
    profiles.extend(list_storage_profiles(&store.user_dir)?);
    if ctx.json {
        let payload = serde_json::json!({
            "kind": "installed",
            "user_dir": store.user_dir.display().to_string(),
            "profiles": profiles,
        });
        write_stdout(&to_canonical_json_string(&payload)?)?;
    } else {
        let detailed = long || io::stdout().is_terminal();
        let mut out = String::new();
        if detailed {
            let width = profiles.iter().map(String::len).max().unwrap_or(0);
            for p in profiles {
                let badge = if p == "default" {
                    "[default]"
                } else {
                    "[profile]"
                };
                let _ = writeln!(out, "{p:<width$}  {badge}");
            }
        } else {
            for p in profiles {
                let _ = writeln!(out, "{p}");
            }
        }
        write_stdout(&out)?;
    }
    Ok(ExitCode::Ok)
}

fn run_list(
    catalog: &WorkspaceCatalog,
    overlay: &UserOverlay,
    kind: ListKind,
    long: bool,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    #[derive(Serialize)]
    struct ListItem<'a> {
        id: &'a str,
        kind: &'a str,
        description: &'a str,
    }

    let detailed = long || io::stdout().is_terminal();

    match kind {
        ListKind::Presets => {
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
            if ctx.json {
                write_stdout(&to_canonical_json_string(&items)?)?;
            } else if detailed {
                let id_w = items.iter().map(|i| i.id.len()).max().unwrap_or(0);
                let mut out = String::new();
                for item in items {
                    let badge = if item.kind == "user-profile" {
                        "[user]"
                    } else {
                        "[preset]"
                    };
                    if item.description.is_empty() {
                        let _ = writeln!(out, "{:<id_w$}  {badge}", item.id);
                    } else {
                        let _ =
                            writeln!(out, "{:<id_w$}  {badge:<8}  {}", item.id, item.description);
                    }
                }
                write_stdout(&out)?;
            } else {
                let mut out = String::new();
                for item in items {
                    let _ = writeln!(out, "{}", item.id);
                }
                write_stdout(&out)?;
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
            if ctx.json {
                write_stdout(&to_canonical_json_string(&items)?)?;
            } else if detailed {
                let id_w = items.iter().map(|i| i.id.len()).max().unwrap_or(0);
                let badge_w = items.iter().map(|i| i.kind.len() + 2).max().unwrap_or(0);
                let mut out = String::new();
                for item in items {
                    let badge = format!("[{}]", item.kind);
                    if item.description.is_empty() {
                        let _ = writeln!(out, "{:<id_w$}  {badge}", item.id);
                    } else {
                        let _ = writeln!(
                            out,
                            "{:<id_w$}  {badge:<badge_w$}  {}",
                            item.id, item.description
                        );
                    }
                }
                write_stdout(&out)?;
            } else {
                let mut out = String::new();
                for item in items {
                    let _ = writeln!(out, "{}", item.id);
                }
                write_stdout(&out)?;
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
            if ctx.json {
                write_stdout(&to_canonical_json_string(&items)?)?;
            } else if detailed {
                let id_w = items.iter().map(|i| i.id.len()).max().unwrap_or(0);
                let mut out = String::new();
                for item in items {
                    if item.description.is_empty() {
                        let _ = writeln!(out, "{:<id_w$}  [ux]", item.id);
                    } else {
                        let _ = writeln!(out, "{:<id_w$}  [ux]  {}", item.id, item.description);
                    }
                }
                write_stdout(&out)?;
            } else {
                let mut out = String::new();
                for item in items {
                    let _ = writeln!(out, "{}", item.id);
                }
                write_stdout(&out)?;
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
    target_kind: ShowTargetKind,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    if target_kind != ShowTargetKind::Pack {
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

            if ctx.json {
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
                write_stdout(&to_canonical_json_string(&payload)?)?;
            } else {
                let mut out = String::new();
                if let Some(note) = legacy_note {
                    let _ = writeln!(out, "Alias: {note}");
                }
                match user_profile {
                    Some(up) => {
                        let _ = writeln!(out, "User profile: {}", recipe.name);
                        let _ = writeln!(
                            out,
                            "Extends: {}",
                            up.extends.as_deref().unwrap_or("(defaults)")
                        );
                    }
                    None => {
                        let _ = writeln!(out, "Preset: {}", recipe.name);
                    }
                }
                let _ = writeln!(out, "Description: {}", recipe.description);
                let _ = writeln!(out, "Resolved modules: {}", resolved.join(", "));
                let _ = writeln!(out, "Settings keys: {}", settings.len());
                let _ = writeln!(out, "Extensions ({}):", ext_ids.len());
                for id in ext_ids {
                    let tier = user_profile
                        .and_then(|up| up.extra_extensions.iter().find(|e| e.id == id))
                        .map(|e| e.tier.as_str());
                    match tier {
                        Some(label) => {
                            let _ = writeln!(out, "  - {id} [{label}]");
                        }
                        None => {
                            let _ = writeln!(out, "  - {id}");
                        }
                    }
                }
                write_stdout(&out)?;
            }
            return Ok(ExitCode::Ok);
        }

        if target_kind == ShowTargetKind::Preset {
            return Err(VspError::TargetMissing(format!("unknown preset '{name}'")));
        }
    }

    if let Some(module) = catalog.modules.get(name) {
        if ctx.json {
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
            write_stdout(&to_canonical_json_string(&payload)?)?;
        } else {
            let mut out = String::new();
            let _ = writeln!(
                out,
                "Pack: {} ({})",
                module.manifest.id, module.manifest.kind
            );
            let _ = writeln!(out, "Description: {}", module.manifest.description);
            if !module.manifest.requires.is_empty() {
                let _ = writeln!(out, "Requires: {}", module.manifest.requires.join(", "));
            }
            if !module.manifest.default_with.is_empty() {
                let _ = writeln!(
                    out,
                    "Default with: {}",
                    module.manifest.default_with.join(", ")
                );
            }
            if !module.manifest.conflicts.is_empty() {
                let _ = writeln!(out, "Conflicts: {}", module.manifest.conflicts.join(", "));
            }
            let _ = writeln!(
                out,
                "Extensions: workspace={}, profile={}, unwanted={}",
                module.manifest.extensions.workspace.len(),
                module.manifest.extensions.profile.len(),
                module.manifest.extensions.unwanted.len()
            );
            if !module.manifest.extensions.workspace.is_empty() {
                let _ = writeln!(
                    out,
                    "Workspace extensions ({}):",
                    module.manifest.extensions.workspace.len()
                );
                for id in &module.manifest.extensions.workspace {
                    let _ = writeln!(out, "  - {id}");
                }
            }
            if !module.manifest.extensions.profile.is_empty() {
                let _ = writeln!(
                    out,
                    "Profile extensions ({}):",
                    module.manifest.extensions.profile.len()
                );
                for id in &module.manifest.extensions.profile {
                    let _ = writeln!(out, "  - {id}");
                }
            }
            if !module.manifest.extensions.unwanted.is_empty() {
                let _ = writeln!(
                    out,
                    "Unwanted extensions ({}):",
                    module.manifest.extensions.unwanted.len()
                );
                for id in &module.manifest.extensions.unwanted {
                    let _ = writeln!(out, "  - {id}");
                }
            }
            write_stdout(&out)?;
        }
        return Ok(ExitCode::Ok);
    }

    if target_kind == ShowTargetKind::Pack {
        Err(VspError::TargetMissing(format!("unknown pack '{name}'")))
    } else {
        Err(VspError::TargetMissing(format!(
            "unknown preset or pack '{name}'"
        )))
    }
}

fn run_detect(
    catalog: &WorkspaceCatalog,
    target: &Path,
    explain: bool,
    ctx: RunContext,
) -> VspResult<ExitCode> {
    let result = detect_workspace(catalog, target)?;

    if result.toolchains.is_empty() {
        if ctx.json {
            write_stdout(&to_canonical_json_string(&result)?)?;
        } else if !ctx.quiet {
            let _ = writeln!(
                io::stderr(),
                "vsp detect: no toolchain detected in {}",
                target.display()
            );
        }
        return Ok(ExitCode::NegativeCheck);
    }

    if ctx.json {
        write_stdout(&to_canonical_json_string(&result)?)?;
    } else if !ctx.quiet {
        let mut out = String::new();
        if explain {
            for tc in &result.toolchains {
                let sigs = result
                    .signals
                    .get(tc)
                    .map(|v| v.join(", "))
                    .unwrap_or_default();
                let _ = writeln!(out, "{tc}\tmatched: {sigs}");
            }
            for sup in &result.superseded {
                let _ = writeln!(
                    out,
                    "{}\tsuperseded by {} (signals: {})",
                    sup.module,
                    sup.superseded_by,
                    sup.signals.join(", ")
                );
            }
        } else {
            let _ = writeln!(out, "{}", result.toolchains.join(" "));
        }
        write_stdout(&out)?;
    }

    Ok(ExitCode::Ok)
}

const fn scope_kind_label(kind: &PlanScopeKind) -> &'static str {
    match kind {
        PlanScopeKind::Workspace => "workspace",
        PlanScopeKind::Profile => "profile",
        PlanScopeKind::User => "user",
    }
}

const fn setting_action_label(kind: SettingChangeKind) -> &'static str {
    match kind {
        SettingChangeKind::Add => "add",
        SettingChangeKind::Update => "update",
        SettingChangeKind::Remove => "remove",
    }
}

const fn extension_action_label(kind: ExtensionChangeKind) -> &'static str {
    match kind {
        ExtensionChangeKind::Recommend => "recommend",
        ExtensionChangeKind::RemoveRecommendation => "remove_recommendation",
        ExtensionChangeKind::Unwant => "unwant",
        ExtensionChangeKind::EnsureInstalled => "ensure_installed",
        ExtensionChangeKind::UninstallCompromised => "uninstall_compromised",
        ExtensionChangeKind::Uninstall => "uninstall",
    }
}

const fn task_action_label(kind: TaskChangeKind) -> &'static str {
    match kind {
        TaskChangeKind::Add => "add",
        TaskChangeKind::Update => "update",
    }
}

fn stdout_supports_color() -> bool {
    io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

fn print_plan(plan: &Plan, ctx: RunContext) -> VspResult<()> {
    if ctx.json {
        return write_stdout(&to_canonical_json_string(plan)?);
    }
    if ctx.quiet {
        return Ok(());
    }
    let use_color = stdout_supports_color();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Plan ({}) target={} modules=[{}]",
        plan.schema,
        scope_kind_label(&plan.target.kind),
        plan.resolved_modules.join(", ")
    );
    if let Some(ref p) = plan.target.path {
        let _ = writeln!(out, "  path: {p}");
    }
    if let Some(ref prof) = plan.target.profile {
        let _ = writeln!(out, "  profile: {prof}");
    }
    if let Some(ref preset) = plan.target.preset {
        let _ = writeln!(out, "  preset: {preset}");
    }
    for ch in &plan.settings_changes {
        let _ = writeln!(
            out,
            "  setting {}: {}",
            setting_action_label(ch.action),
            ch.key
        );
        for line in ch.format_diff_lines() {
            if use_color && line.starts_with("- ") {
                let _ = writeln!(out, "    \x1b[31m{line}\x1b[0m");
            } else if use_color && line.starts_with("+ ") {
                let _ = writeln!(out, "    \x1b[32m{line}\x1b[0m");
            } else {
                let _ = writeln!(out, "    {line}");
            }
        }
    }
    for ex in &plan.extension_changes {
        match &ex.lock_status {
            Some(status) => {
                let _ = writeln!(
                    out,
                    "  extension {}: {} [{status}]",
                    extension_action_label(ex.action),
                    ex.id
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "  extension {}: {}",
                    extension_action_label(ex.action),
                    ex.id
                );
            }
        }
    }
    for tk in &plan.task_changes {
        let _ = writeln!(out, "  task {}: {}", task_action_label(tk.action), tk.label);
    }
    if !plan.would_refuse.is_empty() {
        if use_color {
            let _ = writeln!(
                out,
                "\x1b[31mWould refuse ({}):\x1b[0m",
                plan.would_refuse.len()
            );
        } else {
            let _ = writeln!(out, "Would refuse ({}):", plan.would_refuse.len());
        }
        for reason in &plan.would_refuse {
            let _ = writeln!(out, "  - {reason}");
        }
    }
    let _ = write!(
        out,
        "Summary: settings={}, extensions={}, tasks={}, drift={}",
        plan.settings_changes.len(),
        plan.extension_changes.len(),
        plan.task_changes.len(),
        plan.has_drift
    );
    if !plan.would_refuse.is_empty() {
        let _ = write!(out, ", would_refuse={}", plan.would_refuse.len());
    }
    if !plan.observation_complete {
        out.push_str(", observation_complete=false");
    }
    out.push('\n');
    write_stdout(&out)?;
    for w in &plan.warnings {
        let _ = writeln!(io::stderr(), "warning: {w}");
    }
    Ok(())
}

fn print_sync_report(report: &SyncReport, ctx: RunContext) -> VspResult<()> {
    if ctx.json {
        return write_stdout(&to_canonical_json_string(report)?);
    }
    if ctx.quiet {
        return Ok(());
    }
    write_stdout(&format!(
        "Synced {} ({}): files={}, step0_removed={}, installed={}, pruned={}, repaired_pins={}\n",
        report.kind,
        report.target,
        report.files_written.len(),
        report.step0_uninstalled.len(),
        report.installed.len(),
        report.pruned.len(),
        report.repaired_pins.len()
    ))?;
    for w in &report.warnings {
        let _ = writeln!(io::stderr(), "warning: {w}");
    }
    Ok(())
}

fn print_dist_report(report: &crate::resolver::DistReport, ctx: RunContext) -> VspResult<()> {
    let planned_paths: Vec<String> = report
        .planned_paths()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    if ctx.json {
        let payload = serde_json::json!({
            "dry_run": ctx.dry_run,
            "out_dir": report.out_dir.display().to_string(),
            "canonical_bundles": report.canonical_bundles,
            "alias_bundles": report.alias_bundles,
            "sha256sums": report.sha256sums_path.display().to_string(),
            "checksum_count": report.checksums.len(),
            "planned_paths": planned_paths,
        });
        return write_stdout(&to_canonical_json_string(&payload)?);
    }
    if !ctx.quiet {
        let mut out = String::new();
        if ctx.dry_run {
            let _ = writeln!(
                out,
                "DRY-RUN: would write {} canonical bundle(s), {} legacy alias(es), and {} ({} digests)",
                report.canonical_bundles.len(),
                report.alias_bundles.len(),
                report.sha256sums_path.display(),
                report.checksums.len()
            );
            for path in &planned_paths {
                let _ = writeln!(out, "  {path}");
            }
        } else {
            let _ = writeln!(
                out,
                "Wrote {} canonical bundle(s), {} legacy alias(es), and {} ({} digests)",
                report.canonical_bundles.len(),
                report.alias_bundles.len(),
                report.sha256sums_path.display(),
                report.checksums.len()
            );
        }
        write_stdout(&out)?;
    }
    Ok(())
}

fn run_dev(
    catalog_override: Option<&Path>,
    config_override: Option<&Path>,
    ctx: RunContext,
    command: DevCommand,
) -> VspResult<ExitCode> {
    // Skip default personal user config on maintainer/catalog commands, while still failing
    // closed when `--config <PATH>` is passed explicitly (SOL-06).
    let _ = load_user_config_scoped(config_override, UserConfigScope::SkipDefault)?;
    if config_override.is_none() {
        verbose_log(ctx, "config: skipped (scope=skip-default)");
    }

    match command {
        DevCommand::Validate => {
            let catalog = load_catalog(catalog_override, ctx)?;
            verbose_log(ctx, "target: dev validate");
            let report = validate_repository(&catalog);
            if ctx.json {
                write_stdout(&to_canonical_json_string(&report)?)?;
            } else if report.is_ok() {
                if !ctx.quiet {
                    write_stdout(&format!(
                        "vsp dev validate: validated {} files and all catalog/module/security invariants successfully\n",
                        report.checked_files
                    ))?;
                }
            } else {
                for v in &report.violations {
                    let _ = writeln!(io::stderr(), "FAILED: {}: {}", v.source, v.message);
                }
                let _ = writeln!(
                    io::stderr(),
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
            let catalog = load_catalog(catalog_override, ctx)?;
            verbose_log(
                ctx,
                format_args!("target: dev dist out={} dry_run={}", out.display(), ctx.dry_run),
            );
            let report = build_dist_with_options(&catalog, &out, ctx.dry_run)?;
            print_dist_report(&report, ctx)?;
            Ok(ExitCode::Ok)
        }
        DevCommand::Pins {
            check,
            refresh,
            strict,
        } => {
            let catalog = load_catalog(catalog_override, ctx)?;
            verbose_log(
                ctx,
                format_args!(
                    "target: dev pins check={check} refresh={refresh} strict={strict} dry_run={}",
                    ctx.dry_run
                ),
            );
            run_dev_pins_with_options(
                &catalog,
                DevPinsOptions {
                    check,
                    refresh,
                    strict_release_gate: strict,
                    dry_run: ctx.dry_run,
                    json: ctx.json,
                    quiet: ctx.quiet,
                },
            )
        }
        DevCommand::Compat {
            all,
            extensions_file,
            vscode_version,
            marketplace_only: _,
            profiles,
        } => {
            let catalog = load_catalog(catalog_override, ctx)?;
            verbose_log(
                ctx,
                format_args!("target: dev compat all={all} profiles={profiles:?}"),
            );
            run_dev_compat(
                &catalog,
                &CompatOptions {
                    all,
                    extensions_file,
                    vscode_version,
                    profiles,
                },
                ctx.json,
                ctx.quiet,
            )
        }
        DevCommand::Docs { check, write } => {
            let catalog = load_catalog(catalog_override, ctx)?;
            verbose_log(
                ctx,
                format_args!(
                    "target: dev docs check={check} write={write} dry_run={}",
                    ctx.dry_run
                ),
            );
            run_dev_docs_with_options(
                &catalog,
                DevDocsOptions {
                    check,
                    write,
                    dry_run: ctx.dry_run,
                    json: ctx.json,
                    quiet: ctx.quiet,
                },
            )
        }
        DevCommand::Hooks => {
            let root = resolve_repo_dir(catalog_override)?;
            verbose_log(
                ctx,
                format_args!(
                    "target: dev hooks root={} dry_run={}",
                    root.display(),
                    ctx.dry_run
                ),
            );
            run_dev_hooks_with_options(
                &root,
                DevHooksOptions {
                    dry_run: ctx.dry_run,
                    json: ctx.json,
                    quiet: ctx.quiet,
                },
            )
        }
        DevCommand::Test => {
            verbose_log(ctx, "target: dev test");
            if ctx.json {
                let payload = serde_json::json!({
                    "rust_suite": "cargo test --workspace",
                    "bats_suite": "bash scripts/tests/run.sh",
                });
                write_stdout(&to_canonical_json_string(&payload)?)?;
            } else if !ctx.quiet {
                write_stdout(
                    "Run Rust suite: cargo test --workspace\nRun full Bash/BATS suite: bash scripts/tests/run.sh\n",
                )?;
            }
            Ok(ExitCode::Ok)
        }
        DevCommand::Release => Err(VspError::Operational(
            "'vsp dev release' is orchestrated by .github/workflows/release.yml and 'vsp dev dist --out dist'".to_string(),
        )),
    }
}

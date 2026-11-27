//! Unified read-only `Plan` (`vsp-plan/v1`) engine powering `vsp diff` and `vsp sync --dry-run`.
//!
//! Computes desired state from the catalog, resolves target observation read-only from
//! disk, and produces a deterministic structured diff without mutating any files.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::detect::detect_workspace;
use crate::exit_codes::{VspError, VspResult};
use crate::jsonc::parse_jsonc_value;
use crate::model::{
    AiMode, ExtensionStatus, PinsLoadState, UxPreset, WorkspaceCatalog, COMPILER_UNION_KEYS,
};
use crate::provenance::{load_profile_provenance, load_workspace_provenance};
use crate::resolver::{
    apply_ai_mode_to_lists, compile_profile_extensions, compile_settings, compile_tasks,
    compile_workspace_extensions, recursive_object_multiply, resolve_modules,
    resolve_profile_recipe, ResolveRequest, RtkTaskMode, SettingsScope,
};
use crate::schema_loader::{LockEntry, LockEntryStatus};
use crate::sync::PruneMode;
use crate::user_config::{ExtensionTrustTier, UserOverlay};
use crate::vscode::{
    find_code_binary, find_storage_profile_location, list_installed_extensions,
    resolve_vscode_store, validate_profile_name,
};
use crate::vsix::{current_lock_platform, unlocked_bypass_active};

/// Canonical schema identifier emitted by `vsp diff --json` and `vsp sync --dry-run --json`.
pub const PLAN_SCHEMA_V1: &str = "vsp-plan/v1";

/// Target scope of a computed [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanScopeKind {
    /// Team-shareable workspace directory (`.vscode/{settings,extensions,tasks}.json`).
    Workspace,
    /// Named VS Code profile (`User/profiles/<id>/settings.json` + profile extensions).
    Profile,
    /// VS Code Default user profile (`User/settings.json`).
    User,
}

/// Input options for building a [`Plan`].
#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    /// Workspace directory path (positional `PATH` or `--workspace`).
    pub workspace_path: Option<PathBuf>,
    /// Target VS Code profile name (`-p/--profile`).
    pub profile: Option<String>,
    /// Profile preset/recipe name (`--preset`).
    pub preset: Option<String>,
    /// Target VS Code Default user profile (`--user`).
    pub user: bool,
    /// Additional modules (`--with`).
    pub with_modules: Vec<String>,
    /// Excluded modules (`--without`).
    pub without_modules: Vec<String>,
    /// Optional UX preset overlay (`--ux crisp|retina|default`).
    pub ux: Option<UxPreset>,
    /// Optional AI mode (`--ai full|copilot|off` or `--no-ai`).
    pub ai: Option<AiMode>,
    /// Optional `--vscode-data-dir` override.
    pub vscode_data_dir: Option<PathBuf>,
    /// Resolved user-profile data from the personal user configuration (empty by default).
    pub user_overlay: UserOverlay,
}

/// Additional read-only planning controls for [`compute_plan_with_extras`].
///
/// Kept separate from [`PlanOptions`] so existing struct-literal callers remain
/// source-compatible while callers such as `vsp sync --dry-run --prune` and
/// `--allow-unlocked` can surface prune removals and lock-refusal previews.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlanExtras {
    /// When `true` (and [`Self::prune_mode`] is `None`), include planned
    /// [`ExtensionChangeKind::Uninstall`] entries for catalog-retired and
    /// `retireOnSwitch` rival extensions (matching `vsp sync --prune` / `--prune=retired`).
    pub prune: bool,
    /// Explicit pruning mode (`Retired` or `Unmanaged`). When `Some`, takes precedence
    /// over [`Self::prune`].
    pub prune_mode: Option<PruneMode>,
    /// Reflect `--allow-unlocked` in lock-refusal previews: clears pending lock entries
    /// from [`Plan::would_refuse`] while preserving `"verified"` lock statuses and
    /// Tier 0 refusals.
    pub allow_unlocked: bool,
}

impl PlanExtras {
    /// Resolves the effective [`PruneMode`] from [`Self::prune_mode`] and [`Self::prune`].
    #[must_use]
    pub const fn effective_prune(&self) -> Option<PruneMode> {
        if self.prune_mode.is_some() {
            self.prune_mode
        } else if self.prune {
            Some(PruneMode::Retired)
        } else {
            None
        }
    }
}

/// Target metadata included in [`Plan`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTargetSummary {
    /// Scope kind (`workspace`, `profile`, or `user`).
    pub kind: PlanScopeKind,
    /// Resolved workspace or settings file path, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Resolved profile name, when targeting a named profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Resolved preset/recipe name, when a preset was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Active UX override, if specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ux: Option<UxPreset>,
    /// Active AI mode override, if specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiMode>,
}

/// Kind of key-level settings mutation planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingChangeKind {
    /// Key is absent in observed file and would be added.
    Add,
    /// Key exists with a different value and would be updated.
    Update,
    /// Managed/UX key exists in observed file and would be removed.
    Remove,
}

/// Individual setting key diff entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingChange {
    /// Target settings file path.
    pub file: String,
    /// Setting key name.
    pub key: String,
    /// Action (`add`, `update`, or `remove`).
    pub action: SettingChangeKind,
    /// Currently observed value, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<Value>,
    /// Desired value, if adding or updating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired: Option<Value>,
}

impl SettingChange {
    /// Renders concise human-readable `- current` / `+ desired` diff lines for this entry.
    #[must_use]
    pub fn format_diff_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(cur) = &self.current {
            lines.push(format!("- {}: {cur}", self.key));
        }
        if let Some(des) = &self.desired {
            lines.push(format!("+ {}: {des}", self.key));
        }
        lines
    }
}

/// Kind of extension recommendation or profile extension action planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionChangeKind {
    /// Add an extension ID to workspace `.vscode/extensions.json` `recommendations`.
    Recommend,
    /// Remove an unwanted extension ID from workspace `recommendations`.
    RemoveRecommendation,
    /// Add an extension ID to workspace `.vscode/extensions.json` `unwantedRecommendations`.
    Unwant,
    /// Install an extension that is missing from the target VS Code profile.
    EnsureInstalled,
    /// Uninstall an installed extension that the catalog marks compromised or removed
    /// (sync Step 0).
    UninstallCompromised,
    /// Uninstall an installed extension: a superseded extension whose replacement is
    /// present or planned, an `AI`-group extension when `--ai off` is selected, or a
    /// pruned retired/unmanaged/switch-retired extension under `--prune`.
    Uninstall,
}

/// Selector kinds given on the command line, for [`validate_selector_combination`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectorKinds {
    /// A workspace path (positional `PATH` or `--workspace`).
    pub workspace: bool,
    /// `-p/--profile <NAME>`.
    pub profile: bool,
    /// `--user`.
    pub user: bool,
    /// `--from <BUNDLE>`.
    pub from_bundle: bool,
    /// `--all`.
    pub all: bool,
}

/// Rejects contradictory target selectors.
///
/// `vsp diff`, `vsp sync --dry-run`, and mutating `vsp sync` all call this one function, so
/// a preview can never accept a selector combination that the real run refuses.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] (exit `4`) for `--all` with a workspace, `--profile`,
/// `--from`, or `--user`; `--from` with a workspace or `--user`; a workspace with `--user`;
/// and `--user` with `--profile`.
pub fn validate_selector_combination(kinds: SelectorKinds) -> VspResult<()> {
    if kinds.all && (kinds.workspace || kinds.profile || kinds.from_bundle || kinds.user) {
        return Err(VspError::UsageOrConfig(
            "--all cannot be combined with workspace, --profile, --from, or --user".to_string(),
        ));
    }
    if kinds.from_bundle && (kinds.workspace || kinds.user) {
        return Err(VspError::UsageOrConfig(
            "--from cannot be combined with workspace path or --user".to_string(),
        ));
    }
    if kinds.workspace && kinds.user {
        return Err(VspError::UsageOrConfig(
            "cannot combine workspace path with --user".to_string(),
        ));
    }
    if kinds.user && kinds.profile {
        return Err(VspError::UsageOrConfig(
            "cannot combine --user with -p/--profile".to_string(),
        ));
    }
    Ok(())
}

/// Individual extension diff entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionChange {
    /// Extension identifier (`publisher.name`).
    pub id: String,
    /// Planned action.
    pub action: ExtensionChangeKind,
    /// Supply-chain lock status for planned profile installs (`"verified"`, `"version"`,
    /// `"pending"`, `"unlocked"`, `"user-unlocked"`, or `"tier0-blocked"`). Omitted for
    /// uninstall and workspace-recommendation entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_status: Option<String>,
}

/// Kind of task label change in `.vscode/tasks.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskChangeKind {
    /// Task label absent and would be added.
    Add,
    /// Task label present with different definition and would be updated.
    Update,
}

/// Individual task diff entry by `.label`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskChange {
    /// Task `.label` value.
    pub label: String,
    /// Planned action (`add` or `update`).
    pub action: TaskChangeKind,
}

/// Unified, serializable `vsp-plan/v1` preview shared by `vsp diff` and `vsp sync --dry-run`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    /// Schema tag (`"vsp-plan/v1"`).
    pub schema: String,
    /// Target metadata.
    pub target: PlanTargetSummary,
    /// Ordered resolved module IDs backing this plan.
    pub resolved_modules: Vec<String>,
    /// Key-level settings differences between observed and desired state.
    pub settings_changes: Vec<SettingChange>,
    /// Extension differences between observed and desired state.
    pub extension_changes: Vec<ExtensionChange>,
    /// Task label differences for workspace targets.
    pub task_changes: Vec<TaskChange>,
    /// `true` if any setting, extension, or task differs from desired state.
    pub has_drift: bool,
    /// `true` when target observation completed without missing external probes.
    pub observation_complete: bool,
    /// Non-fatal informational or advisory warnings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Fail-closed refusal reasons that would prevent `vsp sync` from applying this plan
    /// (for example pending Tier 1 hash locks without `--allow-unlocked`, or Tier 0 blocked
    /// extensions).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub would_refuse: Vec<String>,
}

/// Computes a read-only [`Plan`] from `catalog` and `opts` with default [`PlanExtras`].
///
/// Requires at least one explicit selector (`workspace_path`, `profile`, `preset`, or `user`),
/// exiting with [`VspError::UsageOrConfig`] (exit `4`) when omitted.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`): missing selector or DAG conflict/error
/// - [`VspError::TargetMissing`] (exit `2`): missing workspace path or unknown preset
/// - [`VspError::NegativeCheck`] (exit `3`): workspace auto-detection matched no toolchain
pub fn compute_plan(catalog: &WorkspaceCatalog, opts: &PlanOptions) -> VspResult<Plan> {
    compute_plan_with_extras(catalog, opts, &PlanExtras::default())
}

/// Computes a read-only [`Plan`] from `catalog`, `opts`, and additional [`PlanExtras`]
/// controlling prune preview and `--allow-unlocked` refusal preview.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`): missing selector or DAG conflict/error
/// - [`VspError::TargetMissing`] (exit `2`): missing workspace path or unknown preset
/// - [`VspError::NegativeCheck`] (exit `3`): workspace auto-detection matched no toolchain
pub fn compute_plan_with_extras(
    catalog: &WorkspaceCatalog,
    opts: &PlanOptions,
    extras: &PlanExtras,
) -> VspResult<Plan> {
    let has_selector = opts.workspace_path.is_some()
        || opts.profile.is_some()
        || opts.preset.is_some()
        || opts.user;
    if !has_selector {
        return Err(VspError::UsageOrConfig(
            "explicit target selector required: pass <PATH>, -p/--profile <NAME>, --preset <NAME>, or --user".to_string(),
        ));
    }
    validate_selector_combination(SelectorKinds {
        workspace: opts.workspace_path.is_some(),
        profile: opts.profile.is_some(),
        user: opts.user,
        from_bundle: false,
        all: false,
    })?;

    if let Some(ws_path) = &opts.workspace_path {
        return compute_workspace_plan(catalog, ws_path, opts);
    }

    if opts.user {
        return compute_user_plan(catalog, opts);
    }

    compute_profile_plan(catalog, opts, extras)
}

fn compute_workspace_plan(
    catalog: &WorkspaceCatalog,
    ws_path: &Path,
    opts: &PlanOptions,
) -> VspResult<Plan> {
    if !ws_path.is_dir() {
        return Err(VspError::TargetMissing(format!(
            "workspace path does not exist or is not a directory: {}",
            ws_path.display()
        )));
    }
    let ws_canon = fs::canonicalize(ws_path).map_err(|err| {
        VspError::TargetMissing(format!(
            "cannot resolve workspace path {}: {err}",
            ws_path.display()
        ))
    })?;

    let mut warnings = Vec::new();
    let (resolved_modules, ws_overrides, effective_preset, effective_ux) =
        if let Some(raw_preset) = &opts.preset {
            let (canonical_preset, legacy_ux) = resolve_preset_name(catalog, raw_preset)?;
            let ux = opts.ux.or(legacy_ux);
            let recipe = catalog
                .profiles
                .get(&canonical_preset)
                .ok_or_else(|| VspError::TargetMissing(format!("unknown preset '{raw_preset}'")))?;
            let (ov_ws, _) = recipe.split_overrides();
            let mods = resolve_profile_recipe(
                catalog,
                &canonical_preset,
                &opts.with_modules,
                &opts.without_modules,
                Some(UxPreset::Default),
                opts.ai,
            )?;
            (mods, ov_ws, Some(canonical_preset), ux)
        } else {
            let det = detect_workspace(catalog, &ws_canon)?;
            if det.toolchains.is_empty() && opts.with_modules.is_empty() {
                return Err(VspError::NegativeCheck(format!(
                    "no toolchain detected in {} (pass --preset <NAME> or --with <MODULE>)",
                    ws_path.display()
                )));
            }
            let mut roots = catalog.defaults.capabilities.clone();
            roots.extend(det.toolchains);
            let mut eff_with = opts.with_modules.clone();
            let mut eff_without = opts.without_modules.clone();
            apply_ai_mode_to_lists(opts.ai, &mut eff_with, &mut eff_without);
            let mods = resolve_modules(
                catalog,
                &ResolveRequest {
                    roots,
                    recipe_excludes: Vec::new(),
                    cli_with: eff_with,
                    cli_without: eff_without,
                },
            )?;
            (mods, Map::new(), None, opts.ux)
        };

    let mut desired_settings = compile_settings(
        catalog,
        SettingsScope::Workspace,
        &resolved_modules,
        Some(&ws_overrides),
    )?;
    let mut delete_keys = Vec::new();
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }

    let mut incomplete = false;
    let settings_file = ws_canon.join(".vscode/settings.json");
    let current_settings =
        read_optional_jsonc_object(&settings_file, &mut warnings, &mut incomplete);
    if let Some(prev) = load_workspace_provenance(&ws_canon) {
        for old_k in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                delete_keys.push(old_k.clone());
            }
        }
    }
    for retired_k in retired_switch_setting_keys(catalog, &resolved_modules, &desired_settings) {
        if !delete_keys.contains(&retired_k) {
            delete_keys.push(retired_k);
        }
    }
    let delete_paths =
        compute_retire_on_switch_delete_paths(catalog, &resolved_modules, &desired_settings);
    let settings_changes = diff_settings_maps(
        &settings_file.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    let desired_exts = compile_workspace_extensions(catalog, &resolved_modules)?;
    let ext_file = ws_canon.join(".vscode/extensions.json");
    let current_ext_obj = read_optional_jsonc_object(&ext_file, &mut warnings, &mut incomplete);
    let extension_changes = diff_workspace_extensions(&current_ext_obj, &desired_exts);

    let desired_tasks = compile_tasks(catalog, RtkTaskMode::Plain, &resolved_modules)?;
    let tasks_file = ws_canon.join(".vscode/tasks.json");
    let task_changes = match desired_tasks {
        Some(dt) => {
            let cur_tasks = read_optional_jsonc_object(&tasks_file, &mut warnings, &mut incomplete);
            diff_tasks(&cur_tasks, &dt)
        }
        None => Vec::new(),
    };

    Ok(finish_plan(
        PlanTargetSummary {
            kind: PlanScopeKind::Workspace,
            path: Some(ws_canon.display().to_string()),
            profile: None,
            preset: effective_preset,
            ux: effective_ux,
            ai: opts.ai,
        },
        resolved_modules,
        settings_changes,
        extension_changes,
        task_changes,
        incomplete,
        warnings,
        Vec::new(),
    ))
}

fn compute_profile_plan(
    catalog: &WorkspaceCatalog,
    opts: &PlanOptions,
    extras: &PlanExtras,
) -> VspResult<Plan> {
    let raw_profile = opts
        .profile
        .as_deref()
        .or(opts.preset.as_deref())
        .ok_or_else(|| VspError::UsageOrConfig("profile or preset name required".to_string()))?;

    let (profile_name, legacy_ux) = match catalog.resolve_alias(raw_profile) {
        Some((canon, ux)) => (canon, Some(ux)),
        None => (raw_profile.to_string(), None),
    };

    let (preset_name, preset_legacy_ux) = match &opts.preset {
        Some(p) => {
            let (c, u) = resolve_preset_name(catalog, p)?;
            (Some(c), u)
        }
        None if catalog.profiles.contains_key(&profile_name) => (Some(profile_name.clone()), None),
        None => (None, None),
    };
    let effective_ux = opts.ux.or(preset_legacy_ux).or(legacy_ux);

    let (resolved_modules, combined_ov) = if let Some(ref p_name) = preset_name {
        let recipe = catalog
            .profiles
            .get(p_name)
            .ok_or_else(|| VspError::TargetMissing(format!("unknown preset '{p_name}'")))?;
        let (ov_ws, ov_prof) = recipe.split_overrides();
        let mods = resolve_profile_recipe(
            catalog,
            p_name,
            &opts.with_modules,
            &opts.without_modules,
            Some(UxPreset::Default),
            opts.ai,
        )?;
        (mods, recursive_object_multiply(&ov_ws, &ov_prof))
    } else if !opts.with_modules.is_empty() {
        let mut eff_with = opts.with_modules.clone();
        let mut eff_without = opts.without_modules.clone();
        apply_ai_mode_to_lists(opts.ai, &mut eff_with, &mut eff_without);
        let mods = resolve_modules(
            catalog,
            &ResolveRequest {
                roots: catalog.defaults.capabilities.clone(),
                recipe_excludes: Vec::new(),
                cli_with: eff_with,
                cli_without: eff_without,
            },
        )?;
        (mods, Map::new())
    } else {
        return Err(VspError::TargetMissing(format!(
            "unknown profile/preset '{raw_profile}'"
        )));
    };

    let mut desired_settings = compile_settings(
        catalog,
        SettingsScope::Both,
        &resolved_modules,
        Some(&combined_ov),
    )?;
    for ux_key in catalog.ux_managed_keys() {
        desired_settings.remove(&ux_key);
    }

    let mut delete_keys = Vec::new();
    apply_ai_overrides(
        opts.ai,
        &resolved_modules,
        &mut desired_settings,
        &mut delete_keys,
    );
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }
    if let Some(p_name) = &preset_name {
        opts.user_overlay
            .apply_final_overrides(p_name, &mut desired_settings, &mut delete_keys);
    }

    // The profile name goes to the `code` command line for the extension observation.
    validate_profile_name(&profile_name)?;

    let mut warnings = Vec::new();
    let mut incomplete = false;
    // Named-profile diff invokes `code --list-extensions`, so require a representable
    // `--user-data-dir` (same preflight as `run_profile_sync`) instead of silently
    // querying the default VS Code store when `VSCODE_USER_DIR` does not end in `User`.
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
    let profiles_dir = store.user_dir.join("profiles");
    let profile_location = find_storage_profile_location(&store.user_dir, &profile_name)?;
    let profile_registered = profile_location.is_some() || profile_name == "default";
    let (settings_path, current_settings) = if let Some(loc) = profile_location {
        let path = profiles_dir.join(loc).join("settings.json");
        let current = read_optional_jsonc_object(&path, &mut warnings, &mut incomplete);
        (path, current)
    } else {
        let path = profiles_dir
            .join(format!("<new:{profile_name}>"))
            .join("settings.json");
        (path, Map::new())
    };

    if opts.ai == Some(AiMode::Off) {
        for k in retired_ai_off_keys(catalog, &current_settings) {
            if !delete_keys.contains(&k) {
                delete_keys.push(k);
            }
        }
    }
    if let Some(prev) = load_profile_provenance(&store.user_dir, &profile_name) {
        for old_k in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                delete_keys.push(old_k.clone());
            }
        }
    }
    for retired_k in retired_switch_setting_keys(catalog, &resolved_modules, &desired_settings) {
        if !delete_keys.contains(&retired_k) {
            delete_keys.push(retired_k);
        }
    }
    let delete_paths =
        compute_retire_on_switch_delete_paths(catalog, &resolved_modules, &desired_settings);

    let settings_changes = diff_settings_maps(
        &settings_path.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    let mut desired_ext_ids: Vec<String> = compile_profile_extensions(catalog, &resolved_modules)?
        .into_iter()
        .map(|entry| entry.identifier.id)
        .collect();
    let mut user_unlocked_ids: BTreeSet<String> = BTreeSet::new();

    // Exact-ID extra extensions from a custom user profile (personal user configuration).
    // `effective_extras` checks each entry again (blocked publisher, trust tier) and applies
    // the AI mode, so an extra can neither use a blocked publisher nor override `--ai`.
    if let Some(p_name) = &preset_name {
        for extra in opts
            .user_overlay
            .effective_extras(catalog, p_name, opts.ai)?
        {
            if !desired_ext_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&extra.id))
            {
                desired_ext_ids.push(extra.id.clone());
            }
            if extra.tier == ExtensionTrustTier::UserUnlocked {
                user_unlocked_ids.insert(extra.id.to_ascii_lowercase());
                warnings.push(format!(
                    "user-unlocked (Tier 3): extension '{}' is catalog-external and installs by exact ID without VSIX pin verification",
                    extra.id
                ));
            }
        }
    }

    let retired_switch_exts =
        retired_switch_extensions(catalog, &resolved_modules, &desired_ext_ids);
    let effective_prune = extras.effective_prune();
    let allow_unlocked = unlocked_bypass_active(extras.allow_unlocked);

    // Observe the installed extensions with the same observer as `vsp sync`.
    // - For a profile already registered in `storage.json` (or `"default"`), a missing
    //   `code` CLI or a failed `code --list-extensions` is an incomplete observation (exit 7):
    //   the planner never reports "no drift" for extensions that it could not see.
    // - For a profile not yet registered in `storage.json` (`FABLE-01`), real VS Code CLI
    //   exits 1 with `Profile '<name>' not found.` — treat installed extensions as empty with
    //   `observation_complete = true` when `code` is missing or reports an error.
    let observed_installed: Option<Vec<crate::vscode::InstalledExtension>> = if profile_registered {
        match find_code_binary() {
            None => {
                incomplete = true;
                warnings.push(
                    "observation incomplete: VS Code 'code' CLI not found on PATH; installed extensions were not observed"
                        .to_string(),
                );
                None
            }
            Some(code_bin) => match list_installed_extensions(&code_bin, &store, &profile_name) {
                Ok(installed) => Some(installed),
                Err(err @ VspError::SafetyRefusal(_)) => return Err(err),
                Err(err) => {
                    incomplete = true;
                    warnings.push(format!("observation incomplete: {err}"));
                    None
                }
            },
        }
    } else {
        match find_code_binary() {
            None => Some(Vec::new()),
            Some(code_bin) => match list_installed_extensions(&code_bin, &store, &profile_name) {
                Ok(installed) => Some(installed),
                Err(err @ VspError::SafetyRefusal(_)) => return Err(err),
                Err(_) => Some(Vec::new()),
            },
        }
    };

    let (extension_changes, would_refuse) = match observed_installed {
        Some(installed) => diff_profile_extensions(
            catalog,
            &desired_ext_ids,
            &installed,
            opts.ai == Some(AiMode::Off),
            effective_prune,
            &retired_switch_exts,
            &user_unlocked_ids,
            allow_unlocked,
        ),
        None => (Vec::new(), Vec::new()),
    };

    Ok(finish_plan(
        PlanTargetSummary {
            kind: PlanScopeKind::Profile,
            path: Some(settings_path.display().to_string()),
            profile: Some(profile_name),
            preset: preset_name,
            ux: effective_ux,
            ai: opts.ai,
        },
        resolved_modules,
        settings_changes,
        extension_changes,
        Vec::new(),
        incomplete,
        warnings,
        would_refuse,
    ))
}

fn compute_user_plan(catalog: &WorkspaceCatalog, opts: &PlanOptions) -> VspResult<Plan> {
    let (resolved_modules, combined_ov, preset_name, effective_ux) =
        if let Some(raw_preset) = &opts.preset {
            let (canon, legacy_ux) = resolve_preset_name(catalog, raw_preset)?;
            let recipe = catalog
                .profiles
                .get(&canon)
                .ok_or_else(|| VspError::TargetMissing(format!("unknown preset '{raw_preset}'")))?;
            let (ov_ws, ov_prof) = recipe.split_overrides();
            let mods = resolve_profile_recipe(
                catalog,
                &canon,
                &opts.with_modules,
                &opts.without_modules,
                Some(UxPreset::Default),
                opts.ai,
            )?;
            (
                mods,
                recursive_object_multiply(&ov_ws, &ov_prof),
                Some(canon),
                opts.ux.or(legacy_ux),
            )
        } else {
            let mut eff_with = opts.with_modules.clone();
            let mut eff_without = opts.without_modules.clone();
            apply_ai_mode_to_lists(opts.ai, &mut eff_with, &mut eff_without);
            let mods = resolve_modules(
                catalog,
                &ResolveRequest {
                    roots: catalog.defaults.capabilities.clone(),
                    recipe_excludes: Vec::new(),
                    cli_with: eff_with,
                    cli_without: eff_without,
                },
            )?;
            (mods, Map::new(), None, opts.ux)
        };

    let mut desired_settings = compile_settings(
        catalog,
        SettingsScope::Both,
        &resolved_modules,
        Some(&combined_ov),
    )?;
    for ux_key in catalog.ux_managed_keys() {
        desired_settings.remove(&ux_key);
    }

    let mut delete_keys = Vec::new();
    apply_ai_overrides(
        opts.ai,
        &resolved_modules,
        &mut desired_settings,
        &mut delete_keys,
    );
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }
    if let Some(p_name) = &preset_name {
        opts.user_overlay
            .apply_final_overrides(p_name, &mut desired_settings, &mut delete_keys);
    }

    let mut warnings = Vec::new();
    let mut incomplete = false;
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), false)?;
    let settings_path = store.user_dir.join("settings.json");
    let current_settings =
        read_optional_jsonc_object(&settings_path, &mut warnings, &mut incomplete);
    if opts.ai == Some(AiMode::Off) {
        for k in retired_ai_off_keys(catalog, &current_settings) {
            if !delete_keys.contains(&k) {
                delete_keys.push(k);
            }
        }
    }
    if let Some(prev) = load_profile_provenance(&store.user_dir, "default") {
        for old_k in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                delete_keys.push(old_k.clone());
            }
        }
    }
    for retired_k in retired_switch_setting_keys(catalog, &resolved_modules, &desired_settings) {
        if !delete_keys.contains(&retired_k) {
            delete_keys.push(retired_k);
        }
    }
    let delete_paths =
        compute_retire_on_switch_delete_paths(catalog, &resolved_modules, &desired_settings);

    let settings_changes = diff_settings_maps(
        &settings_path.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    Ok(finish_plan(
        PlanTargetSummary {
            kind: PlanScopeKind::User,
            path: Some(settings_path.display().to_string()),
            profile: Some("default".to_string()),
            preset: preset_name,
            ux: effective_ux,
            ai: opts.ai,
        },
        resolved_modules,
        settings_changes,
        Vec::new(),
        Vec::new(),
        incomplete,
        warnings,
        Vec::new(),
    ))
}

/// Resolves a canonical or legacy preset name (such as `ai-profile-retina`) into
/// `(canonical_preset_name, optional_legacy_ux)`.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] (exit `2`) when `raw` does not match any canonical or
/// legacy preset in `catalog`.
pub fn resolve_preset_name(
    catalog: &WorkspaceCatalog,
    raw: &str,
) -> VspResult<(String, Option<UxPreset>)> {
    if let Some((canon, ux)) = catalog.resolve_alias(raw) {
        if catalog.profiles.contains_key(&canon) {
            return Ok((canon, Some(ux)));
        }
    }
    if catalog.profiles.contains_key(raw) {
        return Ok((raw.to_string(), None));
    }
    Err(VspError::TargetMissing(format!("unknown preset '{raw}'")))
}

/// Applies `--ai full|copilot|off` settings mutations and key deletions to `desired_settings`.
pub fn apply_ai_overrides(
    ai_mode: Option<AiMode>,
    resolved_modules: &[String],
    desired_settings: &mut Map<String, Value>,
    delete_keys: &mut Vec<String>,
) {
    let has_ai_mod = resolved_modules
        .iter()
        .any(|m| m == "ai-agents" || m == "ai-copilot");
    if ai_mode == Some(AiMode::Off) {
        desired_settings.insert("chat.disableAIFeatures".to_string(), Value::Bool(true));
    } else if has_ai_mod {
        desired_settings.remove("chat.disableAIFeatures");
        if !delete_keys.iter().any(|k| k == "chat.disableAIFeatures") {
            delete_keys.push("chat.disableAIFeatures".to_string());
        }
    }
}

/// Applies a UX preset (`crisp`, `retina`, or `default`) to `desired_settings` and records
/// retired UX keys in `delete_keys` when switching to `default`.
pub fn apply_ux_to_map(
    catalog: &WorkspaceCatalog,
    ux: UxPreset,
    desired_settings: &mut Map<String, Value>,
    delete_keys: &mut Vec<String>,
) {
    match ux {
        UxPreset::Default => {
            for k in catalog.ux_managed_keys() {
                desired_settings.remove(&k);
                if !delete_keys.contains(&k) {
                    delete_keys.push(k);
                }
            }
        }
        UxPreset::Crisp | UxPreset::Retina => {
            if let Some(ux_map) = catalog
                .modules
                .get(ux.as_str())
                .and_then(|m| m.profile_settings.as_ref())
            {
                for (k, v) in ux_map {
                    desired_settings.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

/// Returns top-level setting keys declared under `retireOnSwitch.settings` in `catalog`
/// for any pack in `resolved_modules`, excluding keys present in `desired_settings`.
#[must_use]
pub fn retired_switch_setting_keys(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
    desired_settings: &Map<String, Value>,
) -> Vec<String> {
    let mut retired = BTreeSet::new();
    for mod_id in resolved_modules {
        if let Some(rule) = catalog.retire_on_switch.get(mod_id) {
            for key in &rule.settings {
                if !desired_settings.contains_key(key) {
                    retired.insert(key.clone());
                }
            }
        }
    }
    retired.into_iter().collect()
}

/// Returns lowercase extension IDs declared under `retireOnSwitch.extensions` in `catalog`
/// for any pack in `resolved_modules`, excluding IDs present in `desired_ids`.
#[must_use]
pub fn retired_switch_extensions(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
    desired_ids: &[String],
) -> BTreeSet<String> {
    let desired_lc: BTreeSet<String> = desired_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let mut retired = BTreeSet::new();
    for mod_id in resolved_modules {
        if let Some(rule) = catalog.retire_on_switch.get(mod_id) {
            for ext_id in &rule.extensions {
                let lc = ext_id.to_ascii_lowercase();
                if !desired_lc.contains(&lc) {
                    retired.insert(lc);
                }
            }
        }
    }
    retired
}

/// Derives nested `[language]` `editor.codeActionsOnSave` delete paths dynamically from
/// mutually exclusive (`conflicts`) packs in `catalog` instead of hardcoding formatter names.
///
/// Whenever a resolved pack declares a conflict with a rival pack that is not active, any
/// `[language].editor.codeActionsOnSave.<action>` entry contributed by the rival pack and
/// absent from the corresponding `desired_settings` language block is scheduled for deletion.
#[must_use]
pub fn compute_retire_on_switch_delete_paths(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
    desired_settings: &Map<String, Value>,
) -> Vec<Vec<String>> {
    let active: BTreeSet<&str> = resolved_modules.iter().map(String::as_str).collect();
    let mut delete_paths: BTreeSet<Vec<String>> = BTreeSet::new();

    for mod_id in resolved_modules {
        let Some(active_mod) = catalog.modules.get(mod_id) else {
            continue;
        };
        for rival_id in &active_mod.manifest.conflicts {
            if active.contains(rival_id.as_str()) {
                continue;
            }
            let Some(rival_mod) = catalog.modules.get(rival_id) else {
                continue;
            };
            for settings_map in [&rival_mod.workspace_settings, &rival_mod.profile_settings]
                .into_iter()
                .flatten()
            {
                for (lang_key, lang_val) in settings_map {
                    if !(lang_key.starts_with('[') && lang_key.ends_with(']')) {
                        continue;
                    }
                    let Some(rival_actions) = lang_val
                        .as_object()
                        .and_then(|obj| obj.get("editor.codeActionsOnSave"))
                        .and_then(Value::as_object)
                    else {
                        continue;
                    };
                    let desired_actions = desired_settings
                        .get(lang_key)
                        .and_then(Value::as_object)
                        .and_then(|obj| obj.get("editor.codeActionsOnSave"))
                        .and_then(Value::as_object);
                    for action_key in rival_actions.keys() {
                        let kept = desired_actions.is_some_and(|m| m.contains_key(action_key));
                        if !kept {
                            delete_paths.insert(vec![
                                lang_key.clone(),
                                "editor.codeActionsOnSave".to_string(),
                                action_key.clone(),
                            ]);
                        }
                    }
                }
            }
        }
    }

    delete_paths.into_iter().collect()
}

/// Returns nested key paths to delete when switching between mutually exclusive JS/TS
/// formatter packs (`biome` <-> `ts-eslint-prettier`) so stale `editor.codeActionsOnSave`
/// entries inside `[language]` blocks are retired cleanly.
#[must_use]
pub fn compute_formatter_delete_paths(resolved_modules: &[String]) -> Vec<Vec<String>> {
    let has_biome = resolved_modules.iter().any(|m| m == "biome");
    let has_eslint = resolved_modules.iter().any(|m| m == "ts-eslint-prettier");
    let langs = [
        "[javascript]",
        "[typescript]",
        "[javascriptreact]",
        "[typescriptreact]",
    ];

    let mut out = Vec::new();
    if has_biome && !has_eslint {
        for lang in langs {
            out.push(vec![
                lang.to_string(),
                "editor.codeActionsOnSave".to_string(),
                "source.fixAll.eslint".to_string(),
            ]);
        }
    } else if has_eslint && !has_biome {
        for lang in langs {
            for action in ["source.fixAll.biome", "source.organizeImports.biome"] {
                out.push(vec![
                    lang.to_string(),
                    "editor.codeActionsOnSave".to_string(),
                    action.to_string(),
                ]);
            }
        }
    }
    out
}

/// Returns existing AI vendor, chat, inlineChat, and AI-only inlineSuggest keys that should
/// be retired when switching to `--ai off` while preserving shared `core-editor` settings.
#[must_use]
pub fn retired_ai_off_keys(
    catalog: &WorkspaceCatalog,
    existing: &Map<String, Value>,
) -> Vec<String> {
    let mut ai_module_keys: BTreeSet<String> = BTreeSet::new();
    for mod_id in ["ai-agents", "ai-copilot"] {
        if let Some(prof_map) = catalog
            .modules
            .get(mod_id)
            .and_then(|m| m.profile_settings.as_ref())
        {
            for k in prof_map.keys() {
                if k != "editor.quickSuggestions" && k != "editor.inlineSuggest.enabled" {
                    ai_module_keys.insert(k.clone());
                }
            }
        }
    }

    let mut out = Vec::new();
    for key in existing.keys() {
        if key == "chat.disableAIFeatures" {
            continue;
        }
        let matches_prefix = key.starts_with("antigravity.")
            || key.starts_with("claudeCode.")
            || key.starts_with("chatgpt.")
            || key.starts_with("github.copilot.")
            || key.starts_with("geminicodeassist.")
            || key.starts_with("chat.")
            || key.starts_with("inlineChat.")
            || key == "editor.inlineSuggest.fontFamily"
            || key.starts_with("editor.inlineSuggest.edits.");
        if matches_prefix || ai_module_keys.contains(key) {
            out.push(key.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Reads an existing target JSONC file for the diff.
///
/// A file that does not exist is a complete observation (an empty object). Every other
/// failure (unreadable file, malformed JSONC, or a root that is not an object) is an
/// INCOMPLETE observation: `incomplete` is set and a warning is recorded. The planner then
/// reports exit `7` and classifies no drift, because a diff against an empty map would be
/// wrong and `vsp sync` cannot merge that file safely either.
fn read_optional_jsonc_object(
    path: &Path,
    warnings: &mut Vec<String>,
    incomplete: &mut bool,
) -> Map<String, Value> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Map::new(),
        Err(err) => {
            *incomplete = true;
            warnings.push(format!(
                "observation incomplete: failed to read existing {}: {err}",
                path.display()
            ));
            return Map::new();
        }
    };
    match parse_jsonc_value(&raw, &path.display().to_string()) {
        Ok(Value::Object(map)) => map,
        Ok(_) => {
            *incomplete = true;
            warnings.push(format!(
                "observation incomplete: existing {} is not a JSON object",
                path.display()
            ));
            Map::new()
        }
        Err(err) => {
            *incomplete = true;
            warnings.push(format!(
                "observation incomplete: failed to parse existing {}: {err}",
                path.display()
            ));
            Map::new()
        }
    }
}

/// Builds the final [`Plan`]. When the observation is incomplete, no change is classified
/// and `has_drift` is `false`: the caller must report exit `7`, not a drift result.
#[allow(clippy::too_many_arguments)]
fn finish_plan(
    target: PlanTargetSummary,
    resolved_modules: Vec<String>,
    settings_changes: Vec<SettingChange>,
    extension_changes: Vec<ExtensionChange>,
    task_changes: Vec<TaskChange>,
    incomplete: bool,
    warnings: Vec<String>,
    would_refuse: Vec<String>,
) -> Plan {
    if incomplete {
        return Plan {
            schema: PLAN_SCHEMA_V1.to_string(),
            target,
            resolved_modules,
            settings_changes: Vec::new(),
            extension_changes: Vec::new(),
            task_changes: Vec::new(),
            has_drift: false,
            observation_complete: false,
            warnings,
            would_refuse: Vec::new(),
        };
    }
    let has_drift =
        !settings_changes.is_empty() || !extension_changes.is_empty() || !task_changes.is_empty();
    Plan {
        schema: PLAN_SCHEMA_V1.to_string(),
        target,
        resolved_modules,
        settings_changes,
        extension_changes,
        task_changes,
        has_drift,
        observation_complete: true,
        warnings,
        would_refuse,
    }
}

/// Computes the extension changes and lock-refusal preview of a profile target from the
/// observed installed set.
///
/// - A desired extension that is not installed: [`ExtensionChangeKind::EnsureInstalled`]
///   annotated with its host-platform `lock_status` (`OPUS-06`).
/// - An installed compromised or removed extension:
///   [`ExtensionChangeKind::UninstallCompromised`].
/// - An installed superseded extension whose replacement is installed or desired, an
///   installed `AI`-group extension under `--ai off`, or (when `prune_mode` is set) a
///   retired, switch-retired, or unmanaged catalog extension: [`ExtensionChangeKind::Uninstall`].
#[allow(clippy::too_many_arguments)]
fn diff_profile_extensions(
    catalog: &WorkspaceCatalog,
    desired_ids: &[String],
    installed: &[crate::vscode::InstalledExtension],
    ai_off: bool,
    prune_mode: Option<PruneMode>,
    retired_switch_exts: &BTreeSet<String>,
    user_unlocked_ids: &BTreeSet<String>,
    allow_unlocked: bool,
) -> (Vec<ExtensionChange>, Vec<String>) {
    let installed_lc: BTreeSet<String> = installed
        .iter()
        .map(|e| e.id.to_ascii_lowercase())
        .collect();
    let desired_lc: BTreeSet<String> = desired_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();

    let prune_retired = prune_mode.is_some();
    let prune_unmanaged = prune_mode == Some(PruneMode::Unmanaged);

    let mut retained_pack_members = BTreeSet::new();
    for id in &desired_lc {
        if let Some(rec) = catalog.extension_record(id) {
            for pm in &rec.pack_members {
                retained_pack_members.insert(pm.to_ascii_lowercase());
            }
        }
    }
    if !prune_retired {
        for ext in installed {
            if let Some(rec) = catalog.extension_record(&ext.id) {
                if rec.status == ExtensionStatus::Retired {
                    for pm in &rec.pack_members {
                        retained_pack_members.insert(pm.to_ascii_lowercase());
                    }
                }
            }
        }
    }

    let mut known_pack_members = BTreeSet::new();
    if prune_unmanaged {
        for rec in catalog.extensions.extensions.values() {
            for pm in &rec.pack_members {
                known_pack_members.insert(pm.to_ascii_lowercase());
            }
        }
    }

    let mut changes = Vec::new();
    let mut emitted_uninstalls = BTreeSet::new();
    for ext in installed {
        let lc = ext.id.to_ascii_lowercase();
        let action = match catalog.extension_record(&lc) {
            Some(rec) => match rec.status {
                ExtensionStatus::Compromised | ExtensionStatus::Removed => {
                    Some(ExtensionChangeKind::UninstallCompromised)
                }
                ExtensionStatus::Unknown(_) => None,
                ExtensionStatus::Superseded => rec
                    .replaced_by
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .filter(|rep| installed_lc.contains(rep) || desired_lc.contains(rep))
                    .map(|_| ExtensionChangeKind::Uninstall),
                _ if ai_off && rec.group.eq_ignore_ascii_case("AI") => {
                    Some(ExtensionChangeKind::Uninstall)
                }
                ExtensionStatus::Retired => prune_retired.then_some(ExtensionChangeKind::Uninstall),
                ExtensionStatus::Active => {
                    let is_switch_retired = prune_retired
                        && retired_switch_exts.contains(&lc)
                        && !desired_lc.contains(&lc)
                        && !retained_pack_members.contains(&lc);
                    let is_unmanaged = prune_unmanaged
                        && !desired_lc.contains(&lc)
                        && !retained_pack_members.contains(&lc);
                    (is_switch_retired || is_unmanaged).then_some(ExtensionChangeKind::Uninstall)
                }
            },
            None => {
                let is_unmanaged_member = prune_unmanaged
                    && known_pack_members.contains(&lc)
                    && !desired_lc.contains(&lc)
                    && !retained_pack_members.contains(&lc);
                is_unmanaged_member.then_some(ExtensionChangeKind::Uninstall)
            }
        };
        if let Some(action) = action {
            if emitted_uninstalls.insert(lc.clone()) {
                changes.push(ExtensionChange {
                    id: lc,
                    action,
                    lock_status: None,
                });
            }
        }
    }

    let host_platform = current_lock_platform();
    let pins_env_override = env::var("ARTAGON_VSCODE_PINS_FILE")
        .ok()
        .is_some_and(|s| !s.trim().is_empty());
    let mut would_refuse = Vec::new();

    for id in desired_ids {
        let lc = id.to_ascii_lowercase();
        if !installed_lc.contains(&lc) {
            let (lock_status, refusal) = classify_install_lock_status(
                catalog,
                &lc,
                user_unlocked_ids,
                host_platform,
                pins_env_override,
                allow_unlocked,
            );
            if let Some(reason) = refusal {
                would_refuse.push(reason);
            }
            changes.push(ExtensionChange {
                id: id.clone(),
                action: ExtensionChangeKind::EnsureInstalled,
                lock_status: Some(lock_status.to_string()),
            });
        }
    }

    (changes, would_refuse)
}

/// Returns `(lock_status, optional_would_refuse_reason)` for a missing profile extension
/// planned for [`ExtensionChangeKind::EnsureInstalled`].
fn classify_install_lock_status(
    catalog: &WorkspaceCatalog,
    ext_id_lc: &str,
    user_unlocked_ids: &BTreeSet<String>,
    host_platform: &str,
    pins_env_override: bool,
    allow_unlocked: bool,
) -> (&'static str, Option<String>) {
    let record = catalog.extension_record(ext_id_lc);
    let is_tier0_blocked = catalog.is_blocked_publisher(ext_id_lc)
        || record.is_some_and(|rec| {
            matches!(
                rec.status,
                ExtensionStatus::Compromised
                    | ExtensionStatus::Removed
                    | ExtensionStatus::Unknown(_)
            )
        });
    if is_tier0_blocked {
        return (
            "tier0-blocked",
            Some(format!(
                "{ext_id_lc}: blocked by catalog (compromised, removed, or blocked publisher)"
            )),
        );
    }

    if user_unlocked_ids.contains(ext_id_lc) {
        return ("user-unlocked", None);
    }

    if !pins_env_override {
        if let Some(lockfile) = &catalog.lockfile {
            return classify_from_lockfile_entry(
                lockfile.extensions.get(ext_id_lc),
                lockfile.pin_required,
                ext_id_lc,
                host_platform,
                allow_unlocked,
            );
        }
    }

    // Legacy layout or explicit `ARTAGON_VSCODE_PINS_FILE` override: evaluate `catalog.pins`.
    match &catalog.pins {
        PinsLoadState::Loaded(manifest) => {
            if let Some(pin) = catalog.lookup_pin(ext_id_lc) {
                if !pin.sha256.is_empty() && !pin.vsix_url.is_empty() {
                    return ("verified", None);
                }
            }
            if manifest.pin_required {
                let refusal = (!allow_unlocked).then(|| {
                    format!("{ext_id_lc}: pinRequired=true but no VSIX pin exists (pass --allow-unlocked)")
                });
                ("pending", refusal)
            } else {
                ("unlocked", None)
            }
        }
        PinsLoadState::Missing(path) => {
            let refusal = (!allow_unlocked).then(|| {
                format!(
                    "{ext_id_lc}: pins manifest missing: {} (pass --allow-unlocked)",
                    path.display()
                )
            });
            ("pending", refusal)
        }
        PinsLoadState::Invalid { path, error } => {
            let refusal = (!allow_unlocked).then(|| {
                format!(
                    "{ext_id_lc}: invalid pins manifest {}: {error}",
                    path.display()
                )
            });
            ("pending", refusal)
        }
    }
}

fn classify_from_lockfile_entry(
    entry: Option<&LockEntry>,
    pin_required: bool,
    ext_id_lc: &str,
    host_platform: &str,
    allow_unlocked: bool,
) -> (&'static str, Option<String>) {
    let Some(entry) = entry else {
        return if pin_required {
            let refusal = (!allow_unlocked).then(|| {
                format!(
                    "{ext_id_lc}: pinRequired=true and missing from vsp.lock.json (pass --allow-unlocked)"
                )
            });
            ("pending", refusal)
        } else {
            ("unlocked", None)
        };
    };

    match entry.tier {
        1 => {
            if lock_entry_has_verified_host_artifact(entry, host_platform) {
                ("verified", None)
            } else {
                let refusal = (!allow_unlocked).then(|| {
                    format!(
                        "{ext_id_lc}: Tier 1 hash lock pending for platform '{host_platform}' (pass --allow-unlocked or refresh vsp.lock.json)"
                    )
                });
                ("pending", refusal)
            }
        }
        2 => {
            let pending = entry.status == Some(LockEntryStatus::PendingRefresh);
            let version = entry.version.as_deref().unwrap_or("");
            if !pending && !version.is_empty() {
                if semver::Version::parse(version).is_ok() {
                    ("version", None)
                } else {
                    let refusal = (!allow_unlocked).then(|| {
                        format!("{ext_id_lc}: Tier 2 version lock is not valid semver: '{version}'")
                    });
                    ("pending", refusal)
                }
            } else {
                let refusal = (pin_required && !allow_unlocked).then(|| {
                    format!(
                        "{ext_id_lc}: pinRequired=true and Tier 2 version lock is pending_refresh"
                    )
                });
                ("pending", refusal)
            }
        }
        3 => {
            let refusal = (pin_required && !allow_unlocked).then(|| {
                format!("{ext_id_lc}: pinRequired=true and Tier 3 extension has no VSIX pin")
            });
            ("unlocked", refusal)
        }
        _ => {
            let refusal = (!allow_unlocked).then(|| {
                format!(
                    "{ext_id_lc}: unrecognized lock tier {} in vsp.lock.json",
                    entry.tier
                )
            });
            ("pending", refusal)
        }
    }
}

fn lock_entry_has_verified_host_artifact(entry: &LockEntry, host_platform: &str) -> bool {
    if entry.status == Some(LockEntryStatus::PendingRefresh) {
        return false;
    }
    let has_target = |key: &str| {
        entry
            .targets
            .get(key)
            .is_some_and(|t| !t.sha256.is_empty() && !t.vsix_url.is_empty())
    };
    if entry.universal == Some(false) {
        return host_platform != "universal" && has_target(host_platform);
    }
    if has_target(host_platform) || has_target("universal") {
        return true;
    }
    entry.sha256.as_deref().is_some_and(|s| !s.is_empty())
        && entry.vsix_url.as_deref().is_some_and(|u| !u.is_empty())
}

fn diff_settings_maps(
    file_label: &str,
    current: &Map<String, Value>,
    desired: &Map<String, Value>,
    delete_keys: &[String],
    delete_paths: &[Vec<String>],
) -> Vec<SettingChange> {
    let mut changes = Vec::new();
    for (k, desired_val) in desired {
        match current.get(k) {
            None => changes.push(SettingChange {
                file: file_label.to_string(),
                key: k.clone(),
                action: SettingChangeKind::Add,
                current: None,
                desired: Some(desired_val.clone()),
            }),
            Some(cur_val) => {
                let has_retired_subpath = delete_paths.iter().any(|path| {
                    path.first().map(String::as_str) == Some(k.as_str())
                        && path_exists_in_map(current, path)
                });
                let converged =
                    !has_retired_subpath && setting_value_converged(k, cur_val, desired_val, true);
                if !converged {
                    changes.push(SettingChange {
                        file: file_label.to_string(),
                        key: k.clone(),
                        action: SettingChangeKind::Update,
                        current: Some(cur_val.clone()),
                        desired: Some(desired_val.clone()),
                    });
                }
            }
        }
    }

    let mut sorted_del: Vec<&String> = delete_keys.iter().collect();
    sorted_del.sort();
    sorted_del.dedup();
    for k in sorted_del {
        if !desired.contains_key(k) {
            if let Some(cur_val) = current.get(k) {
                changes.push(SettingChange {
                    file: file_label.to_string(),
                    key: k.clone(),
                    action: SettingChangeKind::Remove,
                    current: Some(cur_val.clone()),
                    desired: None,
                });
            }
        }
    }

    changes
}

fn setting_value_converged(
    key: &str,
    current: &Value,
    desired: &Value,
    is_top_level: bool,
) -> bool {
    match (current, desired) {
        (Value::Object(cur_obj), Value::Object(des_obj)) => des_obj.iter().all(|(sk, sv)| {
            cur_obj
                .get(sk)
                .is_some_and(|cv| setting_value_converged(sk, cv, sv, false))
        }),
        (Value::Array(cur_arr), Value::Array(des_arr))
            if is_top_level && COMPILER_UNION_KEYS.contains(&key) =>
        {
            des_arr.iter().all(|item| cur_arr.contains(item))
        }
        _ => current == desired,
    }
}

fn path_exists_in_map(map: &Map<String, Value>, path: &[String]) -> bool {
    let Some((first, rest)) = path.split_first() else {
        return false;
    };
    let Some(val) = map.get(first) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    match val {
        Value::Object(sub) => path_exists_in_map(sub, rest),
        _ => false,
    }
}

fn diff_workspace_extensions(
    current_obj: &Map<String, Value>,
    desired: &crate::model::WorkspaceExtensionsFile,
) -> Vec<ExtensionChange> {
    let cur_recs: Vec<String> = current_obj
        .get("recommendations")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let cur_recs_lc: BTreeSet<String> = cur_recs.iter().map(|s| s.to_ascii_lowercase()).collect();

    let cur_unw_lc: BTreeSet<String> = current_obj
        .get("unwantedRecommendations")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default();

    let desired_unw_lc: BTreeSet<String> = desired
        .unwanted_recommendations
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let mut changes = Vec::new();
    for rec in &desired.recommendations {
        if !cur_recs_lc.contains(&rec.to_ascii_lowercase()) {
            changes.push(ExtensionChange {
                id: rec.clone(),
                action: ExtensionChangeKind::Recommend,
                lock_status: None,
            });
        }
    }
    for cur in &cur_recs {
        if desired_unw_lc.contains(&cur.to_ascii_lowercase()) {
            changes.push(ExtensionChange {
                id: cur.clone(),
                action: ExtensionChangeKind::RemoveRecommendation,
                lock_status: None,
            });
        }
    }
    for unw in &desired.unwanted_recommendations {
        if !cur_unw_lc.contains(&unw.to_ascii_lowercase()) {
            changes.push(ExtensionChange {
                id: unw.clone(),
                action: ExtensionChangeKind::Unwant,
                lock_status: None,
            });
        }
    }
    changes
}

fn diff_tasks(current_obj: &Map<String, Value>, desired_doc: &Value) -> Vec<TaskChange> {
    let cur_tasks = current_obj
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(des_tasks) = desired_doc.get("tasks").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut changes = Vec::new();
    for dt in des_tasks {
        let Some(label) = dt.get("label").and_then(Value::as_str) else {
            continue;
        };
        match cur_tasks
            .iter()
            .find(|ct| ct.get("label").and_then(Value::as_str) == Some(label))
        {
            None => changes.push(TaskChange {
                label: label.to_string(),
                action: TaskChangeKind::Add,
            }),
            Some(ct) if ct != dt => changes.push(TaskChange {
                label: label.to_string(),
                action: TaskChangeKind::Update,
            }),
            Some(_) => {}
        }
    }
    changes
}

//! Unified read-only `Plan` (`vsp-plan/v1`) engine powering `vsp diff` and `vsp sync --dry-run`.
//!
//! Computes desired state from the catalog, resolves target observation read-only from
//! disk, and produces a deterministic structured diff without mutating any files.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::detect::detect_workspace;
use crate::exit_codes::{VspError, VspResult};
use crate::legacy_loader::parse_jsonc_value;
use crate::model::{
    resolve_legacy_profile_name, AiMode, UxPreset, WorkspaceCatalog, COMPILER_UNION_KEYS,
};
use crate::provenance::{load_profile_provenance, load_workspace_provenance};
use crate::resolver::{
    apply_ai_mode_to_lists, compile_profile_extensions, compile_settings, compile_tasks,
    compile_workspace_extensions, recursive_object_multiply, resolve_modules,
    resolve_profile_recipe, ResolveRequest, RtkTaskMode, SettingsScope,
};
use crate::user_config::{ExtensionTrustTier, UserOverlay};
use crate::vscode::{find_storage_profile_location, resolve_vscode_store};

/// Canonical schema identifier emitted by `vsp diff --json` and `vsp sync --dry-run --json`.
pub const PLAN_SCHEMA_V1: &str = "vsp-plan/v1";

/// Target scope of a computed [`Plan`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// Target metadata included in [`Plan`].
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize)]
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

/// Kind of extension recommendation or profile extension action planned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionChangeKind {
    /// Add an extension ID to workspace `.vscode/extensions.json` `recommendations`.
    Recommend,
    /// Remove an unwanted extension ID from workspace `recommendations`.
    RemoveRecommendation,
    /// Add an extension ID to workspace `.vscode/extensions.json` `unwantedRecommendations`.
    Unwant,
    /// Ensure an extension ID is installed in the target VS Code profile.
    EnsureInstalled,
}

/// Individual extension diff entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtensionChange {
    /// Extension identifier (`publisher.name`).
    pub id: String,
    /// Planned action.
    pub action: ExtensionChangeKind,
}

/// Kind of task label change in `.vscode/tasks.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskChangeKind {
    /// Task label absent and would be added.
    Add,
    /// Task label present with different definition and would be updated.
    Update,
}

/// Individual task diff entry by `.label`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskChange {
    /// Task `.label` value.
    pub label: String,
    /// Planned action (`add` or `update`).
    pub action: TaskChangeKind,
}

/// Unified, serializable `vsp-plan/v1` preview shared by `vsp diff` and `vsp sync --dry-run`.
#[derive(Debug, Clone, Serialize)]
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Computes a read-only [`Plan`] from `catalog` and `opts`.
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
    let has_selector = opts.workspace_path.is_some()
        || opts.profile.is_some()
        || opts.preset.is_some()
        || opts.user;
    if !has_selector {
        return Err(VspError::UsageOrConfig(
            "explicit target selector required: pass <PATH>, -p/--profile <NAME>, --preset <NAME>, or --user".to_string(),
        ));
    }

    if let Some(ws_path) = &opts.workspace_path {
        return compute_workspace_plan(catalog, ws_path, opts);
    }

    if opts.user {
        return compute_user_plan(catalog, opts);
    }

    compute_profile_plan(catalog, opts)
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

    let settings_file = ws_canon.join(".vscode/settings.json");
    let current_settings = read_optional_jsonc_object(&settings_file, &mut warnings);
    if let Some(prev) = load_workspace_provenance(&ws_canon) {
        for old_k in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                delete_keys.push(old_k.clone());
            }
        }
    }
    let delete_paths = compute_formatter_delete_paths(&resolved_modules);
    let settings_changes = diff_settings_maps(
        &settings_file.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    let desired_exts = compile_workspace_extensions(catalog, &resolved_modules)?;
    let ext_file = ws_canon.join(".vscode/extensions.json");
    let current_ext_obj = read_optional_jsonc_object(&ext_file, &mut warnings);
    let extension_changes = diff_workspace_extensions(&current_ext_obj, &desired_exts);

    let desired_tasks = compile_tasks(catalog, RtkTaskMode::Plain, &resolved_modules)?;
    let tasks_file = ws_canon.join(".vscode/tasks.json");
    let task_changes = match desired_tasks {
        Some(dt) => {
            let cur_tasks = read_optional_jsonc_object(&tasks_file, &mut warnings);
            diff_tasks(&cur_tasks, &dt)
        }
        None => Vec::new(),
    };

    let has_drift = !settings_changes.is_empty()
        || !extension_changes.is_empty()
        || !task_changes.is_empty();

    Ok(Plan {
        schema: PLAN_SCHEMA_V1.to_string(),
        target: PlanTargetSummary {
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
        has_drift,
        observation_complete: true,
        warnings,
    })
}

fn compute_profile_plan(catalog: &WorkspaceCatalog, opts: &PlanOptions) -> VspResult<Plan> {
    let raw_profile = opts
        .profile
        .as_deref()
        .or(opts.preset.as_deref())
        .ok_or_else(|| VspError::UsageOrConfig("profile or preset name required".to_string()))?;

    let (profile_name, legacy_ux) = match resolve_legacy_profile_name(raw_profile) {
        Some((canon, ux)) => (canon.to_string(), Some(ux)),
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
    apply_ai_overrides(opts.ai, &resolved_modules, &mut desired_settings, &mut delete_keys);
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }
    if let Some(p_name) = &preset_name {
        opts.user_overlay
            .apply_final_overrides(p_name, &mut desired_settings, &mut delete_keys);
    }

    let mut warnings = Vec::new();
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), false)?;
    let (settings_path, current_settings) =
        match find_storage_profile_location(&store.user_dir, &profile_name)? {
            Some(loc) => {
                let p = store.user_dir.join("profiles").join(loc).join("settings.json");
                let cur = read_optional_jsonc_object(&p, &mut warnings);
                (p, cur)
            }
            None => {
                let p = store
                    .user_dir
                    .join("profiles")
                    .join(format!("<new:{profile_name}>"))
                    .join("settings.json");
                (p, Map::new())
            }
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
    let delete_paths = compute_formatter_delete_paths(&resolved_modules);

    let settings_changes = diff_settings_maps(
        &settings_path.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    let desired_exts = compile_profile_extensions(catalog, &resolved_modules)?;
    let mut extension_changes: Vec<ExtensionChange> = desired_exts
        .into_iter()
        .map(|entry| ExtensionChange {
            id: entry.identifier.id,
            action: ExtensionChangeKind::EnsureInstalled,
        })
        .collect();

    // Exact-ID extra extensions from a custom user profile (personal user configuration).
    if let Some(p_name) = &preset_name {
        for extra in opts.user_overlay.extras_for(p_name) {
            if !extension_changes
                .iter()
                .any(|c| c.id.eq_ignore_ascii_case(&extra.id))
            {
                extension_changes.push(ExtensionChange {
                    id: extra.id.clone(),
                    action: ExtensionChangeKind::EnsureInstalled,
                });
            }
            if extra.tier == ExtensionTrustTier::UserUnlocked {
                warnings.push(format!(
                    "user-unlocked (Tier 3): extension '{}' is catalog-external and installs by exact ID without VSIX pin verification",
                    extra.id
                ));
            }
        }
    }

    let has_drift = !settings_changes.is_empty();

    Ok(Plan {
        schema: PLAN_SCHEMA_V1.to_string(),
        target: PlanTargetSummary {
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
        task_changes: Vec::new(),
        has_drift,
        observation_complete: true,
        warnings,
    })
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
    apply_ai_overrides(opts.ai, &resolved_modules, &mut desired_settings, &mut delete_keys);
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }
    if let Some(p_name) = &preset_name {
        opts.user_overlay
            .apply_final_overrides(p_name, &mut desired_settings, &mut delete_keys);
    }

    let mut warnings = Vec::new();
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), false)?;
    let settings_path = store.user_dir.join("settings.json");
    let current_settings = read_optional_jsonc_object(&settings_path, &mut warnings);
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
    let delete_paths = compute_formatter_delete_paths(&resolved_modules);

    let settings_changes = diff_settings_maps(
        &settings_path.display().to_string(),
        &current_settings,
        &desired_settings,
        &delete_keys,
        &delete_paths,
    );

    let has_drift = !settings_changes.is_empty();

    Ok(Plan {
        schema: PLAN_SCHEMA_V1.to_string(),
        target: PlanTargetSummary {
            kind: PlanScopeKind::User,
            path: Some(settings_path.display().to_string()),
            profile: Some("default".to_string()),
            preset: preset_name,
            ux: effective_ux,
            ai: opts.ai,
        },
        resolved_modules,
        settings_changes,
        extension_changes: Vec::new(),
        task_changes: Vec::new(),
        has_drift,
        observation_complete: true,
        warnings,
    })
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
    if let Some((canon, ux)) = resolve_legacy_profile_name(raw) {
        if catalog.profiles.contains_key(canon) {
            return Ok((canon.to_string(), Some(ux)));
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

fn read_optional_jsonc_object(path: &Path, warnings: &mut Vec<String>) -> Map<String, Value> {
    if !path.is_file() {
        return Map::new();
    }
    match fs::read_to_string(path) {
        Ok(raw) => match parse_jsonc_value(&raw, &path.display().to_string()) {
            Ok(Value::Object(map)) => map,
            Ok(_) => {
                warnings.push(format!(
                    "existing {} is not a JSON object; treating as empty for diff",
                    path.display()
                ));
                Map::new()
            }
            Err(err) => {
                warnings.push(format!(
                    "failed to parse existing {}: {err}",
                    path.display()
                ));
                Map::new()
            }
        },
        Err(err) => {
            warnings.push(format!(
                "failed to read existing {}: {err}",
                path.display()
            ));
            Map::new()
        }
    }
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
                let converged = !has_retired_subpath && setting_value_converged(k, cur_val, desired_val, true);
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

fn setting_value_converged(key: &str, current: &Value, desired: &Value, is_top_level: bool) -> bool {
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
                .map(|s| s.to_ascii_lowercase())
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
            });
        }
    }
    for cur in &cur_recs {
        if desired_unw_lc.contains(&cur.to_ascii_lowercase()) {
            changes.push(ExtensionChange {
                id: cur.clone(),
                action: ExtensionChangeKind::RemoveRecommendation,
            });
        }
    }
    for unw in &desired.unwanted_recommendations {
        if !cur_unw_lc.contains(&unw.to_ascii_lowercase()) {
            changes.push(ExtensionChange {
                id: unw.clone(),
                action: ExtensionChangeKind::Unwant,
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

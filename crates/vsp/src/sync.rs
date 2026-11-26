//! Unified mutating `vsp sync` engine for profiles, workspaces, Default user settings,
//! offline `.code-profile` bundle imports, and global `--all --compromised-only` sweeps.
//!
//! Uses the exact same desired-state compilation and `vsp-plan/v1` model as `vsp diff`,
//! acquires OS-level `fs4` advisory locks before re-observing and mutating, runs Step 0
//! compromised/removed extension uninstall BEFORE any network call or install, runs VSIX
//! lock preflight verification BEFORE mutating `storage.json` / RTK / extensions / settings /
//! provenance, and writes JSONC settings losslessly via [`crate::cst_merge`].

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::cst_merge::{
    merge_jsonc_object_str, merge_workspace_extensions_jsonc, merge_workspace_tasks_jsonc,
    CstMergeOptions,
};
use crate::detect::detect_workspace;
use crate::exit_codes::{VspError, VspResult};
use crate::jsonc::parse_jsonc_value;
use crate::model::{AiMode, ExtensionStatus, UxPreset, WorkspaceCatalog};
use crate::plan::{
    apply_ai_overrides, apply_ux_to_map, compute_formatter_delete_paths, resolve_preset_name,
    retired_ai_off_keys, validate_selector_combination, SelectorKinds,
};
use crate::policy::{check_autonomy_denylist, check_tasks_denylist};
use crate::provenance::{
    backup_file_bounded, backup_workspace_file, check_path_contained,
    compute_workspace_fingerprint, ensure_no_symlink_components, load_profile_provenance,
    load_workspace_provenance, now_rfc3339_utc, resolve_canonical_path, resolve_xdg_state_home,
    save_profile_provenance, save_workspace_provenance, write_file_atomic, ProfileProvenance,
    WorkspaceProvenance, DEFAULT_BACKUP_RETENTION,
};
use crate::resolver::{
    apply_ai_mode_to_lists, compile_profile_extensions, compile_settings, compile_tasks,
    compile_workspace_extensions, order_extensions_by_group, recursive_object_multiply,
    resolve_modules, resolve_profile_recipe, sha256_hex, ResolveRequest, RtkTaskMode,
    SettingsScope,
};
use crate::storage::{
    acquire_advisory_lock, effective_lock_timeout, ensure_storage_profile, is_vscode_running,
};
use crate::user_config::{ExtensionTrustTier, UserOverlay};
use crate::vscode::{
    code_command, find_binary, find_code_binary, find_storage_profile_location, home_dir,
    list_installed_extensions, list_storage_profiles, resolve_vscode_store,
    resolve_xdg_config_home, run_code_command, validate_profile_name, CodeExecOutput,
    InstalledExtension, VsCodeStore,
};
use crate::vsix::{
    compute_pin_repairs, confirm_pin_downgrades, format_vsix_refusal_summary,
    is_valid_extension_id_syntax, managed_extensions_fully_pinned, pins_file_override_path,
    resolve_and_verify_batch, resolve_vsix_cache_dir, stage_pin_repairs, VsixVerifyOptions,
};

/// Pruning mode for `vsp sync -p <NAME> --prune[=MODE]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneMode {
    /// Prune catalog-retired (in addition to superseded/removed) extensions.
    Retired,
    /// Prune retired AND catalog-known unmanaged extensions (requires `--yes` or TTY).
    Unmanaged,
}

/// Complete execution options for the mutating [`run_sync`] engine.
#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    /// Workspace directory path (positional `PATH` or `--workspace`).
    pub workspace_path: Option<PathBuf>,
    /// Target VS Code profile name (`-p/--profile`).
    pub profile: Option<String>,
    /// Canonical or legacy preset/recipe name (`--preset`).
    pub preset: Option<String>,
    /// Optional `.code-profile` bundle file to import (`--from`).
    pub from_bundle: Option<PathBuf>,
    /// Target the VS Code Default user profile (`--user`).
    pub user: bool,
    /// Sweep all registered VS Code profiles + Default (`--all`).
    pub all: bool,
    /// Execute only Step 0 compromised/removed extension remediation (`--compromised-only`).
    pub compromised_only: bool,
    /// Additional pack/module IDs (`--with`).
    pub with_modules: Vec<String>,
    /// Excluded pack/module IDs (`--without`).
    pub without_modules: Vec<String>,
    /// Optional UX preset overlay (`--ux crisp|retina|default`).
    pub ux: Option<UxPreset>,
    /// Optional AI mode (`--ai full|copilot|off` or `--no-ai`).
    pub ai: Option<AiMode>,
    /// Optional extension pruning mode (`--prune`, `--prune=retired`, `--prune=unmanaged`).
    pub prune: Option<PruneMode>,
    /// Stage, verify, and repair drifted pinned extensions (`--repair-pins`).
    pub repair_pins: bool,
    /// Bypass fail-closed VSIX pin checks selectively for pending/missing entries (`--allow-unlocked`).
    pub allow_unlocked: bool,
    /// Allow bundle settings matching autonomy deny-list (`--allow-unsafe-settings`).
    pub allow_unsafe_settings: bool,
    /// Allow creating new `storage.json` profiles while VS Code is running (`--force`).
    pub force: bool,
    /// Assume yes for interactive confirmations (`-y/--yes`).
    pub yes: bool,
    /// Install extensions serially instead of batched (`--no-batch`).
    pub no_batch: bool,
    /// Skip XDG RTK wrapper emission (`--no-rtk`).
    pub no_rtk: bool,
    /// Override VS Code user data directory (`--vscode-data-dir`).
    pub vscode_data_dir: Option<PathBuf>,
    /// Resolved user-profile data from the personal user configuration (empty by default).
    pub user_overlay: UserOverlay,
    /// Extensions-only profile sync (legacy `vspcli reconcile`). When `true`, a profile sync
    /// runs Step 0 (compromised uninstall), Step 2 (install missing extensions), and Step 3
    /// (prune and `--repair-pins`). It writes no `storage.json` entry, installs no RTK
    /// wrappers, and writes no `settings.json`. The profile must exist already.
    pub extensions_only: bool,
}

/// Structured execution summary returned by [`run_sync`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// Target scope synced (`workspace`, `profile`, `user`, `bundle`, or `sweep`).
    pub kind: String,
    /// Target path or profile name.
    pub target: String,
    /// Files written or updated during sync.
    pub files_written: Vec<String>,
    /// Extensions uninstalled in Step 0 (compromised/removed).
    pub step0_uninstalled: Vec<String>,
    /// Extensions installed in Step 2.
    pub installed: Vec<String>,
    /// Extensions pruned in Step 3.
    pub pruned: Vec<String>,
    /// Pinned extensions repaired via `--repair-pins`.
    pub repaired_pins: Vec<String>,
    /// Whether `storage.json` was created/updated (`false` when existing profile fast-path ran).
    pub storage_mutated: bool,
    /// Whether `--allow-unlocked` bypassed VSIX verification.
    pub unlocked_bypass: bool,
    /// Non-fatal warnings recorded during sync (including Tier 2 unpinned warnings and
    /// trimmed `code` stderr fallback notices).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Captured extension operation error details (including child exit code and stderr),
    /// omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ext_errors: Vec<String>,
}

/// Returns the canonical OS advisory lock path for a profile target.
#[must_use]
pub fn profile_sync_lock_path(user_dir: &Path, profile_name: &str) -> PathBuf {
    let key = format!("{}|{}", user_dir.display(), profile_name);
    let hash = sha256_hex(key.as_bytes());
    resolve_xdg_state_home()
        .join("vsp/locks")
        .join(format!("profile-{hash}.lock"))
}

/// Returns the canonical OS advisory lock path for a workspace target.
#[must_use]
pub fn workspace_sync_lock_path(ws_canon: &Path) -> PathBuf {
    let hash = sha256_hex(ws_canon.to_string_lossy().as_bytes());
    resolve_xdg_state_home()
        .join("vsp/locks")
        .join(format!("ws-{hash}.lock"))
}

/// Executes a mutating `vsp sync` operation according to `opts`.
///
/// # Errors
///
/// Returns [`VspError`] carrying canonical exit codes `1..=7`.
pub fn run_sync(catalog: &WorkspaceCatalog, opts: &SyncOptions) -> VspResult<SyncReport> {
    // One shared rule set for `diff`, `sync --dry-run`, and mutating `sync`.
    validate_selector_combination(SelectorKinds {
        workspace: opts.workspace_path.is_some(),
        profile: opts.profile.is_some(),
        user: opts.user,
        from_bundle: opts.from_bundle.is_some(),
        all: opts.all,
    })?;

    // Extensions-only mode is defined for VS Code profiles only. Refuse it for a target
    // whose sync is a settings write, so the flag is never ignored without notice.
    if opts.extensions_only
        && (opts.workspace_path.is_some() || opts.user || opts.from_bundle.is_some())
    {
        return Err(VspError::UsageOrConfig(
            "--extensions-only applies to a profile target (-p/--profile, --preset) or --all only"
                .to_string(),
        ));
    }

    if opts.all {
        return run_all_sweep(catalog, opts);
    }

    if let Some(bundle_path) = &opts.from_bundle {
        return run_bundle_import_sync(catalog, bundle_path, opts);
    }

    if let Some(ws_path) = &opts.workspace_path {
        return run_workspace_sync(catalog, ws_path, opts);
    }

    if opts.user {
        return run_user_default_sync(catalog, opts);
    }

    if opts.profile.is_some() || opts.preset.is_some() {
        return run_profile_sync_with_code(catalog, opts, None);
    }

    Err(VspError::UsageOrConfig(
        "vsp sync requires an explicit selector (<PATH>, -p/--profile, --preset, --user, --from, or --all)"
            .to_string(),
    ))
}

fn collect_retire_on_switch_settings(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
    desired_settings: &Map<String, Value>,
    delete_keys: &mut Vec<String>,
) {
    for mod_id in resolved_modules {
        if let Some(retire) = catalog.retire_on_switch.get(mod_id) {
            for key in &retire.settings {
                if !desired_settings.contains_key(key) && !delete_keys.contains(key) {
                    delete_keys.push(key.clone());
                }
            }
        }
    }
}

fn collect_retire_on_switch_extensions(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for mod_id in resolved_modules {
        if let Some(retire) = catalog.retire_on_switch.get(mod_id) {
            for ext in &retire.extensions {
                out.insert(ext.to_ascii_lowercase());
            }
        }
    }
    out
}

fn run_workspace_sync(
    catalog: &WorkspaceCatalog,
    ws_path: &Path,
    opts: &SyncOptions,
) -> VspResult<SyncReport> {
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

    // Pre-flight symlink-escape guards BEFORE acquiring lock or creating `.vscode/`.
    let vscode_dir = ws_canon.join(".vscode");
    let settings_file = vscode_dir.join("settings.json");
    let extensions_file = vscode_dir.join("extensions.json");
    let tasks_file = vscode_dir.join("tasks.json");
    check_path_contained(&ws_canon, &vscode_dir)?;
    check_path_contained(&ws_canon, &settings_file)?;
    check_path_contained(&ws_canon, &extensions_file)?;
    check_path_contained(&ws_canon, &tasks_file)?;

    // Acquire OS advisory target lock and re-observe under lock.
    let lock_path = workspace_sync_lock_path(&ws_canon);
    let _lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;

    let (resolved_modules, ws_overrides, effective_ux) = if let Some(raw_preset) = &opts.preset {
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
        (mods, ov_ws, ux)
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
        (mods, Map::new(), opts.ux)
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

    // Pre-flight autonomy deny-list check (zero mutation on failure).
    enforce_autonomy_gate(
        &desired_settings,
        ".vscode/settings.json",
        opts.allow_unsafe_settings,
    )?;

    let desired_exts = compile_workspace_extensions(catalog, &resolved_modules)?;
    let desired_tasks = compile_tasks(catalog, RtkTaskMode::Plain, &resolved_modules)?;

    // Post-emit / pre-write host-path leak lint ($HOME and hardcoded host paths).
    lint_workspace_no_host_path_leak(
        &Value::Object(desired_settings.clone()),
        ".vscode/settings.json",
    )?;
    if let Some(ref tasks_doc) = desired_tasks {
        lint_workspace_no_host_path_leak(tasks_doc, ".vscode/tasks.json")?;
        // A managed tasks file must never contain a task that runs when the folder opens.
        let auto_run = check_tasks_denylist(tasks_doc, ".vscode/tasks.json");
        if !auto_run.is_empty() {
            return Err(VspError::SafetyRefusal(format!(
                "tasks deny-list violation in .vscode/tasks.json: {}",
                auto_run
                    .into_iter()
                    .map(|v| v.message)
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
    }

    // Re-observe existing files and XDG workspace provenance under lock.
    let prev_prov = load_workspace_provenance(&ws_canon);
    if let Some(ref prev) = prev_prov {
        for old_key in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_key) && !delete_keys.contains(old_key) {
                delete_keys.push(old_key.clone());
            }
        }
    }
    collect_retire_on_switch_settings(
        catalog,
        &resolved_modules,
        &desired_settings,
        &mut delete_keys,
    );
    let delete_paths = compute_formatter_delete_paths(&resolved_modules);

    let existing_settings_text = read_optional_file_text(&settings_file)?;
    let merged_settings = merge_jsonc_object_str(
        &existing_settings_text,
        &desired_settings,
        &CstMergeOptions {
            delete_keys,
            delete_paths,
            ..Default::default()
        },
    )?;

    // Post-emit host-path leak lint on the final MERGED workspace settings, scoped to the
    // managed keys.
    let merged_map = parse_optional_object(&merged_settings);
    let merged_managed: Map<String, Value> = desired_settings
        .keys()
        .filter_map(|k| merged_map.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    lint_workspace_no_host_path_leak(
        &Value::Object(merged_managed),
        ".vscode/settings.json (merged)",
    )?;

    let existing_ext_text = read_optional_file_text(&extensions_file)?;
    let prev_managed_recs = prev_prov
        .as_ref()
        .map_or(&[][..], |p| p.managed_recommendations.as_slice());
    let (merged_ext_text, new_managed_recs) =
        merge_workspace_extensions_jsonc(&existing_ext_text, &desired_exts, prev_managed_recs)?;

    let merged_tasks_text = match &desired_tasks {
        Some(dt) => {
            let existing_tasks = read_optional_file_text(&tasks_file)?;
            Some(merge_workspace_tasks_jsonc(&existing_tasks, dt)?)
        }
        None => None,
    };

    fs::create_dir_all(&vscode_dir).map_err(|err| {
        VspError::Operational(format!("failed to create {}: {err}", vscode_dir.display()))
    })?;

    let mut files_written = Vec::new();
    if settings_file.is_file() && existing_settings_text != merged_settings {
        backup_workspace_file(&ws_canon, &settings_file, DEFAULT_BACKUP_RETENTION)?;
    }
    let settings_mode = workspace_file_mode(&settings_file);
    write_file_atomic(
        &settings_file,
        merged_settings.as_bytes(),
        settings_mode,
        false,
    )?;
    files_written.push(settings_file.display().to_string());

    let extensions_mode = workspace_file_mode(&extensions_file);
    write_file_atomic(
        &extensions_file,
        merged_ext_text.as_bytes(),
        extensions_mode,
        false,
    )?;
    files_written.push(extensions_file.display().to_string());

    if let Some(tasks_text) = merged_tasks_text {
        let tasks_mode = workspace_file_mode(&tasks_file);
        write_file_atomic(&tasks_file, tasks_text.as_bytes(), tasks_mode, false)?;
        files_written.push(tasks_file.display().to_string());
    }

    let managed_settings_keys: Vec<String> = desired_settings.keys().cloned().collect();
    let new_prov = WorkspaceProvenance {
        version: 1,
        workspace: ws_canon.display().to_string(),
        workspace_fingerprint: compute_workspace_fingerprint(&ws_canon),
        resolved_modules,
        managed_settings_keys,
        managed_recommendations: new_managed_recs,
        ux: effective_ux,
        ai_mode: opts.ai,
        last_sync_at: now_rfc3339_utc(),
    };
    save_workspace_provenance(&new_prov)?;

    Ok(SyncReport {
        kind: "workspace".to_string(),
        target: ws_canon.display().to_string(),
        files_written,
        step0_uninstalled: Vec::new(),
        installed: Vec::new(),
        pruned: Vec::new(),
        repaired_pins: Vec::new(),
        storage_mutated: false,
        unlocked_bypass: false,
        warnings: Vec::new(),
        ext_errors: Vec::new(),
    })
}

fn run_user_default_sync(catalog: &WorkspaceCatalog, opts: &SyncOptions) -> VspResult<SyncReport> {
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), false)?;
    let settings_path = store.user_dir.join("settings.json");
    check_path_contained(&store.user_dir, &settings_path)?;

    let lock_path = profile_sync_lock_path(&store.user_dir, "default");
    let _lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;

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

    // Apply declarative Default-profile expectations from `catalog/policy.jsonc` on top of
    // the built-in APPLICATION-scope security floor.
    if let Some(policy) = &catalog.policy {
        for (key, exp) in &policy.default_profile_expectations {
            if key == "extensions.autoUpdate" {
                desired_settings.insert(key.clone(), exp.expected.clone());
            } else {
                desired_settings
                    .entry(key.clone())
                    .or_insert_with(|| exp.expected.clone());
            }
        }
    }
    desired_settings
        .entry("telemetry.telemetryLevel".to_string())
        .or_insert_with(|| Value::String("off".to_string()));
    desired_settings
        .entry("security.workspace.trust.untrustedFiles".to_string())
        .or_insert_with(|| Value::String("prompt".to_string()));
    desired_settings.insert("extensions.autoUpdate".to_string(), Value::Bool(false));

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
    // A personal override cannot re-enable extension auto-update in the Default profile.
    desired_settings.insert("extensions.autoUpdate".to_string(), Value::Bool(false));

    enforce_autonomy_gate(
        &desired_settings,
        &settings_path.display().to_string(),
        opts.allow_unsafe_settings,
    )?;

    if !opts.no_rtk && resolved_modules.iter().any(|m| m == "rtk-terminal") {
        let rtk_dir = install_rtk_xdg_wrappers(&catalog.root)?;
        let rtk_settings = build_rtk_terminal_settings(&rtk_dir);
        for (k, v) in rtk_settings {
            desired_settings.insert(k, v);
        }
    }

    let existing_text = read_optional_file_text(&settings_path)?;
    let existing_map = parse_optional_object(&existing_text);
    if opts.ai == Some(AiMode::Off) {
        delete_keys.extend(retired_ai_off_keys(catalog, &existing_map));
        restore_quick_suggestions_if_needed(&existing_map, &mut desired_settings);
    }
    let prev_prov = load_profile_provenance(&store.user_dir, "default");
    if let Some(prev) = &prev_prov {
        for old_k in &prev.managed_settings_keys {
            if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                delete_keys.push(old_k.clone());
            }
        }
    }
    collect_retire_on_switch_settings(
        catalog,
        &resolved_modules,
        &desired_settings,
        &mut delete_keys,
    );
    let delete_paths = compute_formatter_delete_paths(&resolved_modules);

    if settings_path.is_file() {
        backup_file_bounded(&settings_path, DEFAULT_BACKUP_RETENTION)?;
    }
    let merged = merge_jsonc_object_str(
        &existing_text,
        &desired_settings,
        &CstMergeOptions {
            delete_keys,
            delete_paths,
            ..Default::default()
        },
    )?;
    write_file_atomic(&settings_path, merged.as_bytes(), 0o600, true)?;

    // A settings-only sync verifies no extension, so it preserves earlier bypass records.
    let prov = ProfileProvenance {
        version: 1,
        profile: "default".to_string(),
        user_dir: store.user_dir.display().to_string(),
        preset: preset_name,
        resolved_modules,
        managed_settings_keys: desired_settings.keys().cloned().collect(),
        managed_extensions: prev_prov
            .as_ref()
            .map_or_else(Vec::new, |p| p.managed_extensions.clone()),
        ux: effective_ux,
        ai_mode: opts.ai,
        unlocked_bypass: prev_prov.as_ref().is_some_and(|p| p.unlocked_bypass),
        publisher_trust_bypass: prev_prov.as_ref().is_some_and(|p| p.publisher_trust_bypass),
        pins_file_override: prev_prov.as_ref().is_some_and(|p| p.pins_file_override),
        tier2_unpinned: prev_prov
            .as_ref()
            .map_or_else(Vec::new, |p| p.tier2_unpinned.clone()),
        last_sync_at: now_rfc3339_utc(),
    };
    save_profile_provenance(&prov)?;

    Ok(SyncReport {
        kind: "user".to_string(),
        target: settings_path.display().to_string(),
        files_written: vec![settings_path.display().to_string()],
        step0_uninstalled: Vec::new(),
        installed: Vec::new(),
        pruned: Vec::new(),
        repaired_pins: Vec::new(),
        storage_mutated: false,
        unlocked_bypass: false,
        warnings: Vec::new(),
        ext_errors: Vec::new(),
    })
}

fn run_profile_sync_with_code(
    catalog: &WorkspaceCatalog,
    opts: &SyncOptions,
    resolved_code_bin: Option<&Path>,
) -> VspResult<SyncReport> {
    let raw_profile = opts
        .profile
        .as_deref()
        .or(opts.preset.as_deref())
        .ok_or_else(|| VspError::UsageOrConfig("profile or preset name required".to_string()))?;

    // The name goes to the `code` command line and to storage.json: validate it first.
    validate_profile_name(raw_profile)?;

    // If `--compromised-only` is set on a single profile, run Step 0 directly without
    // rewriting legacy profile display names or touching settings.json!
    if opts.compromised_only {
        let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
        let code_bin = match resolved_code_bin {
            Some(p) => p.to_path_buf(),
            None => require_code_binary()?,
        };
        let lock_path = profile_sync_lock_path(&store.user_dir, raw_profile);
        let _lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;
        let registered = raw_profile == "default"
            || find_storage_profile_location(&store.user_dir, raw_profile)?.is_some();
        let installed = if registered {
            list_installed_extensions(&code_bin, &store, raw_profile)?
        } else {
            Vec::new()
        };
        let (uninstalled, failures) = execute_step0_remediation(
            catalog,
            &code_bin,
            &store,
            raw_profile,
            &installed,
            !opts.no_batch,
        )?;
        if !failures.is_empty() {
            return Err(step0_failure(raw_profile, &failures));
        }
        return Ok(SyncReport {
            kind: "profile".to_string(),
            target: raw_profile.to_string(),
            files_written: Vec::new(),
            step0_uninstalled: uninstalled,
            installed: Vec::new(),
            pruned: Vec::new(),
            repaired_pins: Vec::new(),
            storage_mutated: false,
            unlocked_bypass: false,
            warnings: Vec::new(),
            ext_errors: Vec::new(),
        });
    }

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

    let desired_ext_entries = compile_profile_extensions(catalog, &resolved_modules)?;
    let mut desired_ext_ids: Vec<String> = order_extensions_by_group(
        catalog,
        &desired_ext_entries
            .into_iter()
            .map(|e| e.identifier.id)
            .collect::<Vec<_>>(),
    );

    let mut user_unlocked_ids: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if let Some(p_name) = &preset_name {
        for extra in opts
            .user_overlay
            .effective_extras(catalog, p_name, opts.ai)?
        {
            match extra.tier {
                ExtensionTrustTier::Catalog => {
                    if !desired_ext_ids
                        .iter()
                        .any(|d| d.eq_ignore_ascii_case(&extra.id))
                    {
                        desired_ext_ids.push(extra.id.clone());
                    }
                }
                ExtensionTrustTier::UserUnlocked => {
                    if !user_unlocked_ids
                        .iter()
                        .any(|u| u.eq_ignore_ascii_case(&extra.id))
                    {
                        user_unlocked_ids.push(extra.id.clone());
                        warnings.push(format!(
                            "user-unlocked (Tier 3): extension '{}' is catalog-external and installs by exact ID without VSIX pin verification",
                            extra.id
                        ));
                    }
                }
            }
        }
    }

    // Pre-flight gates (ZERO mutation before all pass!).
    validate_profile_name(&profile_name)?;
    enforce_autonomy_gate(&desired_settings, &profile_name, opts.allow_unsafe_settings)?;
    let publisher_trust_bypass = enforce_publisher_allowlist(catalog, &desired_ext_ids)?;
    if opts.prune == Some(PruneMode::Unmanaged) && !opts.yes && !io::stdin().is_terminal() {
        return Err(VspError::SafetyRefusal(
            "--prune=unmanaged requires --yes (-y) in non-interactive mode".to_string(),
        ));
    }

    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
    check_path_contained(
        &store.user_dir,
        &store.user_dir.join("globalStorage/storage.json"),
    )?;
    check_path_contained(&store.user_dir, &store.user_dir.join("profiles"))?;

    let pre_existing_loc = find_storage_profile_location(&store.user_dir, &profile_name)?;
    if opts.extensions_only {
        if pre_existing_loc.is_none() && profile_name != "default" {
            return Err(VspError::TargetMissing(format!(
                "VS Code profile '{profile_name}' is not registered; extensions-only sync does not create profiles (run 'vsp sync -p {profile_name}' first)"
            )));
        }
    } else if pre_existing_loc.is_none() && !opts.force && is_vscode_running() {
        return Err(VspError::SafetyRefusal(
            "VS Code appears to be running; creating a new profile in storage.json while VS Code is open risks lost updates (close VS Code or pass --force)".to_string(),
        ));
    }

    let code_bin = match resolved_code_bin {
        Some(p) => p.to_path_buf(),
        None => require_code_binary()?,
    };

    // Acquire OS advisory target lock and re-observe live state under lock!
    let lock_path = profile_sync_lock_path(&store.user_dir, &profile_name);
    let _target_lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;

    // FABLE-01: When the profile is not yet registered in `storage.json`, treat the installed
    // extension set as empty instead of calling `code --list-extensions --profile=<name>`
    // (real VS Code exits 1 "Profile '<name>' not found." before registration).
    let profile_registered = profile_name == "default"
        || find_storage_profile_location(&store.user_dir, &profile_name)?.is_some();
    let observed_installed = if profile_registered {
        list_installed_extensions(&code_bin, &store, &profile_name)?
    } else {
        Vec::new()
    };

    // STEP 0: Unconditionally uninstall all compromised/removed extensions BEFORE any network
    // call, storage write, or install!
    let (step0_uninstalled, step0_failures) = execute_step0_remediation(
        catalog,
        &code_bin,
        &store,
        &profile_name,
        &observed_installed,
        !opts.no_batch,
    )?;
    if !step0_failures.is_empty() {
        return Err(step0_failure(&profile_name, &step0_failures));
    }

    let mut installed_lc: BTreeSet<String> = observed_installed
        .iter()
        .map(|e| e.id.to_ascii_lowercase())
        .collect();
    for removed in &step0_uninstalled {
        installed_lc.remove(&removed.to_ascii_lowercase());
    }

    let missing_ids: Vec<String> = desired_ext_ids
        .iter()
        .filter(|id| !installed_lc.contains(&id.to_ascii_lowercase()))
        .cloned()
        .collect();

    let missing_unlocked_ids: Vec<String> = user_unlocked_ids
        .iter()
        .filter(|id| !installed_lc.contains(&id.to_ascii_lowercase()))
        .cloned()
        .collect();

    // PRE-WRITE VSIX LOCK PREFLIGHT (OPUS-02 / CARBON-02 / ASTRA-08):
    // Resolve and verify every missing catalog extension BEFORE Step 1 `ensure_storage_profile`,
    // RTK wrappers, `code --install-extension`, `settings.json`, or provenance writes!
    // Any Tier 1 safety refusal returns exit 5 immediately with ZERO mutations.
    let mut install_targets: Vec<(String, String)> = Vec::new();
    let mut unlocked_bypass = false;
    let mut pins_file_override = false;
    let mut run_tier2_unpinned: Vec<String> = Vec::new();

    if !missing_ids.is_empty() {
        let resolved_targets = resolve_and_verify_batch(
            catalog,
            &missing_ids,
            &VsixVerifyOptions {
                allow_unlocked: opts.allow_unlocked,
                cache_dir: None,
            },
        )?;
        if !resolved_targets.failures.is_empty() {
            let has_safety = resolved_targets
                .failures
                .iter()
                .any(|f| f.is_safety_refusal);
            let summary =
                format_vsix_refusal_summary(catalog, &profile_name, &resolved_targets.failures);
            return Err(if has_safety {
                VspError::SafetyRefusal(summary)
            } else {
                VspError::Operational(summary)
            });
        }
        unlocked_bypass = resolved_targets.unlocked_bypass;
        pins_file_override = resolved_targets.pins_file_override;
        run_tier2_unpinned = resolved_targets.tier2_unpinned;
        if !run_tier2_unpinned.is_empty() {
            warnings.push(format!(
                "Tier 2 unpinned (pending_refresh): {} installed without a verified version lock",
                run_tier2_unpinned.join(", ")
            ));
        }
        install_targets = resolved_targets.targets;
    }
    // Tier 3 user-unlocked extensions have no catalog pin: install by exact ID.
    for id in &missing_unlocked_ids {
        install_targets.push((id.clone(), id.clone()));
    }

    let mut ext_errors: Vec<String> = Vec::new();

    // STEP 1: Ensure storage.json entry (ONLY if new profile!) + write XDG RTK wrappers if enabled.
    let ensure_outcome = if opts.extensions_only {
        None
    } else {
        Some(ensure_storage_profile(
            &store.user_dir,
            &profile_name,
            opts.force,
            effective_lock_timeout(),
        )?)
    };
    let storage_mutated = ensure_outcome.as_ref().is_some_and(|o| o.created);
    let mut files_written = Vec::new();
    if storage_mutated {
        files_written.push(
            store
                .user_dir
                .join("globalStorage/storage.json")
                .display()
                .to_string(),
        );
    }

    if !opts.extensions_only && !opts.no_rtk && resolved_modules.iter().any(|m| m == "rtk-terminal")
    {
        let rtk_dir = install_rtk_xdg_wrappers(&catalog.root)?;
        let rtk_settings = build_rtk_terminal_settings(&rtk_dir);
        for (k, v) in rtk_settings {
            desired_settings.insert(k, v);
        }
    }

    // STEP 2: Batched `code --install-extension` with smart re-list + missing-only serial fallback.
    let mut installed_now = Vec::new();
    let mut failed_install_ids = Vec::new();

    if !install_targets.is_empty() {
        let (ok_ids, fail_details, fallback_warnings) = install_extensions_batched_with_fallback(
            &code_bin,
            &store,
            &profile_name,
            &install_targets,
            !opts.no_batch,
        )?;
        warnings.extend(fallback_warnings);
        for ok in ok_ids {
            installed_lc.insert(ok.to_ascii_lowercase());
            installed_now.push(ok);
        }
        for (fail_id, detail) in fail_details {
            failed_install_ids.push(fail_id.clone());
            ext_errors.push(format!("failed to install {fail_id} ({detail})"));
        }
    }

    // From here on, user-unlocked extensions are managed like the other desired extensions.
    let catalog_managed_ids = desired_ext_ids.clone();
    desired_ext_ids.extend(user_unlocked_ids);

    // STEP 3: `--ai off`, declarative `retireOnSwitch`, Step 3 prune, and `--repair-pins`.
    let mut pruned = Vec::new();

    if opts.ai == Some(AiMode::Off) {
        let (ai_uninstalled, ai_errs) = uninstall_ai_extensions(
            catalog,
            &code_bin,
            &store,
            &profile_name,
            &observed_installed,
            &mut installed_lc,
            !opts.no_batch,
        )?;
        pruned.extend(ai_uninstalled);
        ext_errors.extend(ai_errs);
    }

    // Declarative `retireOnSwitch.extensions` from `WorkspaceCatalog` (CARBON-05).
    let retire_exts = collect_retire_on_switch_extensions(catalog, &resolved_modules);
    if !retire_exts.is_empty() {
        let desired_lc: BTreeSet<String> = desired_ext_ids
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .collect();
        let to_retire: Vec<String> = retire_exts
            .into_iter()
            .filter(|id| installed_lc.contains(id) && !desired_lc.contains(id))
            .collect();
        if !to_retire.is_empty() {
            let (ok_ret, fail_ret) = uninstall_extensions_batched_with_fallback(
                &code_bin,
                &store,
                &profile_name,
                &to_retire,
                !opts.no_batch,
            )?;
            for id in ok_ret {
                installed_lc.remove(&id.to_ascii_lowercase());
                pruned.push(id);
            }
            for (id, detail) in fail_ret {
                ext_errors.push(format!("failed to retire rival extension {id} ({detail})"));
            }
        }
    }

    let (pruned_ids, prune_errs) = execute_step3_prune(
        catalog,
        &code_bin,
        &store,
        &profile_name,
        &observed_installed,
        &desired_ext_ids,
        &installed_lc,
        opts.prune,
        !opts.no_batch,
    )?;
    pruned.extend(pruned_ids);
    ext_errors.extend(prune_errs);

    let mut repaired_pins = Vec::new();
    // `true` only when `--repair-pins` ran without error and every catalog-managed
    // extension is then installed at its hash-locked version.
    let mut repair_reverified_all = false;
    let env_pins_override = pins_file_override_path().is_some();
    if opts.repair_pins {
        let post_installed = list_installed_extensions(&code_bin, &store, &profile_name)?;
        let repairs = compute_pin_repairs(catalog, &post_installed, opts.allow_unlocked)?;
        let mut repair_failed = false;
        if !repairs.is_empty() {
            confirm_pin_downgrades(&repairs, opts.yes)?;
            let cache_dir = resolve_vsix_cache_dir(None);
            let staged = stage_pin_repairs(&repairs, &cache_dir)?;
            for (item, vsix_path) in staged {
                let exec = run_code_install_single(
                    &code_bin,
                    &store,
                    &profile_name,
                    &vsix_path.display().to_string(),
                    true,
                )?;
                if exec.success {
                    repaired_pins.push(format!("{}@{}", item.id, item.pinned_version));
                } else {
                    repair_failed = true;
                    ext_errors.push(format!(
                        "failed to repair pin for {} ({})",
                        item.id,
                        exec.format_failure()
                    ));
                }
            }
        }
        if !repair_failed && ext_errors.is_empty() {
            let verified_view = list_installed_extensions(&code_bin, &store, &profile_name)?;
            repair_reverified_all =
                managed_extensions_fully_pinned(catalog, &verified_view, &catalog_managed_ids)
                    .unwrap_or(false);
        }
    }

    let prev_prov = load_profile_provenance(&store.user_dir, &profile_name);

    // Sticky bypass records: only `--repair-pins` re-verifying all managed extensions clears
    // `unlocked_bypass` and (when run without `ARTAGON_VSCODE_PINS_FILE`) `pins_file_override`.
    let prev_unlocked = prev_prov.as_ref().is_some_and(|p| p.unlocked_bypass);
    let recorded_unlocked_bypass = unlocked_bypass || (prev_unlocked && !repair_reverified_all);
    let prev_publisher_trust_bypass = prev_prov.as_ref().is_some_and(|p| p.publisher_trust_bypass);
    let recorded_publisher_trust_bypass = publisher_trust_bypass || prev_publisher_trust_bypass;
    let prev_pins_override = prev_prov.as_ref().is_some_and(|p| p.pins_file_override);
    let clear_pins_override = repair_reverified_all && !env_pins_override;
    let recorded_pins_file_override =
        pins_file_override || (prev_pins_override && !clear_pins_override);

    let desired_lc_set: BTreeSet<String> = desired_ext_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let mut recorded_tier2_unpinned: BTreeSet<String> = run_tier2_unpinned.into_iter().collect();
    if !repair_reverified_all {
        if let Some(prev) = &prev_prov {
            for id in &prev.tier2_unpinned {
                let lc = id.to_ascii_lowercase();
                if desired_lc_set.contains(&lc) {
                    recorded_tier2_unpinned.insert(lc);
                }
            }
        }
    }
    let recorded_tier2_unpinned: Vec<String> = recorded_tier2_unpinned.into_iter().collect();

    if let Some(outcome) = &ensure_outcome {
        // STEP 4: Theme failure fallback + lossless CST merge of settings.json + save XDG
        // provenance!
        apply_theme_failure_fallback(
            catalog,
            &failed_install_ids,
            &mut desired_settings,
            &mut delete_keys,
        );

        let settings_path = outcome.profile_dir.join("settings.json");
        check_path_contained(&store.user_dir, &settings_path)?;
        let existing_text = read_optional_file_text(&settings_path)?;
        let existing_map = parse_optional_object(&existing_text);

        if opts.ai == Some(AiMode::Off) {
            delete_keys.extend(retired_ai_off_keys(catalog, &existing_map));
            restore_quick_suggestions_if_needed(&existing_map, &mut desired_settings);
        }

        if let Some(prev) = &prev_prov {
            for old_k in &prev.managed_settings_keys {
                if !desired_settings.contains_key(old_k) && !delete_keys.contains(old_k) {
                    delete_keys.push(old_k.clone());
                }
            }
        }
        collect_retire_on_switch_settings(
            catalog,
            &resolved_modules,
            &desired_settings,
            &mut delete_keys,
        );
        let delete_paths = compute_formatter_delete_paths(&resolved_modules);

        if settings_path.is_file() {
            backup_file_bounded(&settings_path, DEFAULT_BACKUP_RETENTION)?;
        }

        let merged_settings = merge_jsonc_object_str(
            &existing_text,
            &desired_settings,
            &CstMergeOptions {
                delete_keys,
                delete_paths,
                ..Default::default()
            },
        )?;
        write_file_atomic(&settings_path, merged_settings.as_bytes(), 0o600, true)?;
        files_written.push(settings_path.display().to_string());

        let prov = ProfileProvenance {
            version: 1,
            profile: profile_name.clone(),
            user_dir: store.user_dir.display().to_string(),
            preset: preset_name,
            resolved_modules,
            managed_settings_keys: desired_settings.keys().cloned().collect(),
            managed_extensions: desired_ext_ids,
            ux: effective_ux,
            ai_mode: opts.ai,
            unlocked_bypass: recorded_unlocked_bypass,
            publisher_trust_bypass: recorded_publisher_trust_bypass,
            pins_file_override: recorded_pins_file_override,
            tier2_unpinned: recorded_tier2_unpinned,
            last_sync_at: now_rfc3339_utc(),
        };
        save_profile_provenance(&prov)?;
    } else {
        // Extensions-only mode wrote no settings, so the managed-settings record of an
        // earlier full sync stays as it is. Only the extension facts are updated, and any
        // bypass or unpinned Tier 2 state is always recorded.
        let audit_state_now = recorded_unlocked_bypass
            || recorded_publisher_trust_bypass
            || recorded_pins_file_override
            || !recorded_tier2_unpinned.is_empty();
        if prev_prov.is_some() || audit_state_now {
            let mut prov = prev_prov.unwrap_or_else(|| ProfileProvenance {
                version: 1,
                profile: profile_name.clone(),
                user_dir: store.user_dir.display().to_string(),
                preset: None,
                resolved_modules: Vec::new(),
                managed_settings_keys: Vec::new(),
                managed_extensions: Vec::new(),
                ux: None,
                ai_mode: None,
                unlocked_bypass: false,
                publisher_trust_bypass: false,
                pins_file_override: false,
                tier2_unpinned: Vec::new(),
                last_sync_at: String::new(),
            });
            prov.managed_extensions = desired_ext_ids;
            prov.unlocked_bypass = recorded_unlocked_bypass;
            prov.publisher_trust_bypass = recorded_publisher_trust_bypass;
            prov.pins_file_override = recorded_pins_file_override;
            prov.tier2_unpinned = recorded_tier2_unpinned;
            prov.last_sync_at = now_rfc3339_utc();
            save_profile_provenance(&prov)?;
        }
    }

    // Mid-apply external `code` failures after mutations began return exit `6` (`RetryableLock`
    // / PartialApply) with an explicit applied-vs-failed summary!
    if !ext_errors.is_empty() {
        let msg = format_partial_apply_summary(
            &profile_name,
            storage_mutated,
            &files_written,
            &step0_uninstalled,
            &installed_now,
            &pruned,
            &repaired_pins,
            &ext_errors,
        );
        return Err(VspError::RetryableLock(msg));
    }

    Ok(SyncReport {
        kind: "profile".to_string(),
        target: profile_name,
        files_written,
        step0_uninstalled,
        installed: installed_now,
        pruned,
        repaired_pins,
        storage_mutated,
        unlocked_bypass,
        warnings,
        ext_errors,
    })
}

/// Installs one extension into a VS Code profile with the same gates as `vsp sync`:
/// extension ID syntax, catalog lifecycle status, publisher allowlist, and fail-closed VSIX
/// pin verification. This backs the legacy `vspcli install-ext <profile> <id>` command.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): an invalid profile name or ID syntax, a
///   compromised, removed, or unrecognized catalog status, a blocked publisher, a publisher
///   outside the allowlist, a VSIX pin refusal, a failed Step 0 removal, or a missing
///   profile while VS Code runs.
/// - [`VspError::TargetMissing`] (exit `2`): the profile is not registered in
///   `storage.json`. This command never creates a profile.
/// - [`VspError::IncompleteObservation`] (exit `7`): the `code` CLI is not on `PATH` or timed out.
/// - [`VspError::Operational`] (exit `1`): VSIX download failure or `code` install failure.
pub fn install_single_extension(
    catalog: &WorkspaceCatalog,
    profile_name: &str,
    ext_id: &str,
    vscode_data_dir: Option<&Path>,
    allow_unlocked: bool,
) -> VspResult<()> {
    validate_profile_name(profile_name)?;
    if !is_valid_extension_id_syntax(ext_id) {
        return Err(VspError::SafetyRefusal(format!(
            "invalid extension id syntax: '{ext_id}'"
        )));
    }
    if let Some(rec) = catalog.extension_record(ext_id) {
        if matches!(
            rec.status,
            ExtensionStatus::Compromised | ExtensionStatus::Removed | ExtensionStatus::Unknown(_)
        ) {
            return Err(VspError::SafetyRefusal(format!(
                "extension '{ext_id}' has catalog status '{}' and cannot be installed",
                rec.status.as_str()
            )));
        }
    }
    let ids = vec![ext_id.to_string()];
    enforce_publisher_allowlist(catalog, &ids)?;

    let store = resolve_vscode_store(vscode_data_dir, true)?;

    if profile_name != "default"
        && find_storage_profile_location(&store.user_dir, profile_name)?.is_none()
    {
        if is_vscode_running() {
            return Err(VspError::SafetyRefusal(format!(
                "VS Code appears to be running and profile '{profile_name}' does not exist; refusing to create a profile while VS Code is open (close VS Code, then run 'vsp sync -p {profile_name}')"
            )));
        }
        return Err(VspError::TargetMissing(format!(
            "VS Code profile '{profile_name}' is not registered; create it first with 'vsp sync -p {profile_name}'"
        )));
    }

    let code_bin = require_code_binary()?;
    let lock_path = profile_sync_lock_path(&store.user_dir, profile_name);
    let _lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;

    let observed_installed = list_installed_extensions(&code_bin, &store, profile_name)?;
    let (_step0_uninstalled, step0_failures) = execute_step0_remediation(
        catalog,
        &code_bin,
        &store,
        profile_name,
        &observed_installed,
        true,
    )?;
    if !step0_failures.is_empty() {
        return Err(step0_failure(profile_name, &step0_failures));
    }

    let resolved = resolve_and_verify_batch(
        catalog,
        &ids,
        &VsixVerifyOptions {
            allow_unlocked,
            cache_dir: None,
        },
    )?;
    if !resolved.failures.is_empty() {
        let has_safety = resolved.failures.iter().any(|f| f.is_safety_refusal);
        let msg = format_vsix_refusal_summary(catalog, profile_name, &resolved.failures);
        return Err(if has_safety {
            VspError::SafetyRefusal(msg)
        } else {
            VspError::Operational(msg)
        });
    }
    let Some((_, install_arg)) = resolved.targets.first() else {
        return Err(VspError::Operational(format!(
            "no install target resolved for '{ext_id}'"
        )));
    };
    let exec = run_code_install_single(&code_bin, &store, profile_name, install_arg, false)?;
    if !exec.success {
        return Err(VspError::Operational(format!(
            "failed to install {ext_id} into profile '{profile_name}' ({})",
            exec.format_failure()
        )));
    }
    Ok(())
}

/// Result of validating a `.code-profile` bundle before any mutation.
///
/// [`prepare_bundle_import`] produces it. Both the mutating `vsp sync --from` path and the
/// `--dry-run` preview use the same function, so a preview applies every bundle check.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleImportPlan {
    /// Validated target profile name (legacy alias names are resolved).
    pub profile_name: String,
    /// Effective UX preset, if any.
    pub ux: Option<UxPreset>,
    /// Settings to merge into the profile after AI and UX transformations.
    #[serde(skip)]
    pub desired_settings: Map<String, Value>,
    /// Settings keys to delete from the profile.
    #[serde(skip)]
    pub delete_keys: Vec<String>,
    /// Number of settings keys in the bundle after transformations.
    pub settings_count: usize,
    /// Extension IDs to install, after catalog status and AI-mode filtering.
    pub install_ids: Vec<String>,
    /// `true` when `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` bypassed the allowlist.
    pub publisher_trust_bypass: bool,
    /// Non-fatal warnings.
    pub warnings: Vec<String>,
}

fn run_bundle_import_sync(
    catalog: &WorkspaceCatalog,
    bundle_path: &Path,
    opts: &SyncOptions,
) -> VspResult<SyncReport> {
    let plan = prepare_bundle_import(catalog, bundle_path, opts)?;
    run_bundle_import_apply(catalog, opts, plan)
}

/// Reads and validates a `.code-profile` bundle without changing any state.
///
/// Checks, in order: the bundle file, the adjacent `SHA256SUMS`, the `.code-profile`
/// schema, the autonomy deny-list, the target profile name (with
/// [`validate_profile_name`], because the bundle `.name` is untrusted), each extension ID
/// and its catalog status, and the publisher allowlist.
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`): the bundle file does not exist.
/// - [`VspError::UsageOrConfig`] (exit `4`): no target profile name can be determined.
/// - [`VspError::SafetyRefusal`] (exit `5`): checksum mismatch, deny-list hit, unsafe
///   profile name, invalid extension ID, or a publisher outside the allowlist.
/// - [`VspError::Operational`] (exit `1`): unreadable file or invalid bundle schema.
pub fn prepare_bundle_import(
    catalog: &WorkspaceCatalog,
    bundle_path: &Path,
    opts: &SyncOptions,
) -> VspResult<BundleImportPlan> {
    if !bundle_path.is_file() {
        return Err(VspError::TargetMissing(format!(
            "bundle file not found: {}",
            bundle_path.display()
        )));
    }

    let canon_bundle = resolve_canonical_path(bundle_path)?;
    let raw_bytes = fs::read(&canon_bundle).map_err(|err| {
        VspError::Operational(format!(
            "failed to read bundle {}: {err}",
            canon_bundle.display()
        ))
    })?;

    let mut warnings = Vec::new();
    verify_bundle_sha256sums(catalog, &canon_bundle, &raw_bytes, &mut warnings)?;

    let bundle_val: Value = serde_json::from_slice(&raw_bytes).map_err(|err| {
        VspError::Operational(format!(
            "invalid JSON in bundle {}: {err}",
            bundle_path.display()
        ))
    })?;
    let bundle_obj = bundle_val.as_object().ok_or_else(|| {
        VspError::Operational(format!(
            "invalid .code-profile schema in {} (expected JSON object)",
            bundle_path.display()
        ))
    })?;

    let mut desired_settings = match bundle_obj.get("settings") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Err(VspError::Operational(format!(
                "invalid .code-profile schema in {}: .settings must be an object",
                bundle_path.display()
            )));
        }
    };

    let raw_ext_ids: Vec<String> = match bundle_obj.get("extensions") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Object(ext_obj)) => match ext_obj.get("enabled") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(arr)) => {
                let mut ids = Vec::with_capacity(arr.len());
                for elem in arr {
                    let s = elem.as_str().filter(|s| !s.is_empty()).ok_or_else(|| {
                        VspError::Operational(format!(
                            "invalid .code-profile schema in {}: .extensions.enabled must contain non-empty strings",
                            bundle_path.display()
                        ))
                    })?;
                    ids.push(s.to_string());
                }
                ids
            }
            Some(_) => {
                return Err(VspError::Operational(format!(
                    "invalid .code-profile schema in {}: .extensions.enabled must be an array",
                    bundle_path.display()
                )));
            }
        },
        Some(_) => {
            return Err(VspError::Operational(format!(
                "invalid .code-profile schema in {}: .extensions must be an object",
                bundle_path.display()
            )));
        }
    };

    enforce_autonomy_gate(
        &desired_settings,
        &bundle_path.display().to_string(),
        opts.allow_unsafe_settings,
    )?;

    let bundle_stem = bundle_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let raw_target_name = opts
        .profile
        .as_deref()
        .or_else(|| bundle_obj.get("name").and_then(Value::as_str))
        .unwrap_or(bundle_stem);
    if raw_target_name.is_empty() {
        return Err(VspError::UsageOrConfig(
            "bundle import requires -p/--profile <NAME> or a bundle with a non-empty .name"
                .to_string(),
        ));
    }

    validate_profile_name(raw_target_name)?;

    let (profile_name, legacy_ux) = match catalog.resolve_alias(raw_target_name) {
        Some((canon, ux)) => (canon, Some(ux)),
        None => (raw_target_name.to_string(), None),
    };
    let stem_legacy = catalog.resolve_alias(bundle_stem);
    let effective_ux = opts
        .ux
        .or(legacy_ux)
        .or_else(|| stem_legacy.map(|(_, u)| u));
    let enforce_copilot_only = profile_name == "ai-copilot"
        || bundle_stem.starts_with("ai-profile-")
        || raw_target_name.starts_with("ai-profile-");

    let mut install_ids = Vec::new();
    for eid in raw_ext_ids {
        if !is_valid_extension_id_syntax(&eid) {
            return Err(VspError::SafetyRefusal(format!(
                "bundle contains invalid extension id syntax: '{eid}'"
            )));
        }
        if let Some(rec) = catalog.extension_record(&eid) {
            match rec.status {
                ExtensionStatus::Compromised => {
                    eprintln!(
                        "SECURITY: skipping compromised extension {eid} from bundle (marked compromised in catalog)"
                    );
                    continue;
                }
                ExtensionStatus::Removed
                | ExtensionStatus::Superseded
                | ExtensionStatus::Retired => {
                    warnings.push(format!(
                        "skipping catalog-{} extension {eid} from bundle",
                        rec.status.as_str()
                    ));
                    continue;
                }
                ExtensionStatus::Active | ExtensionStatus::Unknown(_) => {}
            }
        }
        let is_ai_group = catalog.extension_group(&eid).eq_ignore_ascii_case("AI");
        if opts.ai == Some(AiMode::Off) && is_ai_group {
            continue;
        }
        if (opts.ai == Some(AiMode::Copilot) || enforce_copilot_only)
            && is_ai_group
            && !eid.eq_ignore_ascii_case("github.copilot-chat")
        {
            continue;
        }
        install_ids.push(eid);
    }

    let publisher_trust_bypass = enforce_publisher_allowlist(catalog, &install_ids)?;

    let mut delete_keys = Vec::new();
    if opts.ai == Some(AiMode::Off) {
        let strip_keys = retired_ai_off_keys(catalog, &desired_settings);
        for k in &strip_keys {
            desired_settings.remove(k);
        }
        delete_keys.extend(strip_keys);
        restore_quick_suggestions_if_needed(&desired_settings.clone(), &mut desired_settings);
        desired_settings.insert("chat.disableAIFeatures".to_string(), Value::Bool(true));
    }
    if let Some(ux) = effective_ux {
        apply_ux_to_map(catalog, ux, &mut desired_settings, &mut delete_keys);
    }

    Ok(BundleImportPlan {
        profile_name,
        ux: effective_ux,
        settings_count: desired_settings.len(),
        desired_settings,
        delete_keys,
        install_ids,
        publisher_trust_bypass,
        warnings,
    })
}

fn run_bundle_import_apply(
    catalog: &WorkspaceCatalog,
    opts: &SyncOptions,
    plan: BundleImportPlan,
) -> VspResult<SyncReport> {
    let BundleImportPlan {
        profile_name,
        ux: effective_ux,
        mut desired_settings,
        mut delete_keys,
        install_ids,
        publisher_trust_bypass,
        mut warnings,
        ..
    } = plan;
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
    check_path_contained(
        &store.user_dir,
        &store.user_dir.join("globalStorage/storage.json"),
    )?;
    check_path_contained(&store.user_dir, &store.user_dir.join("profiles"))?;

    let pre_existing_loc = find_storage_profile_location(&store.user_dir, &profile_name)?;
    if pre_existing_loc.is_none() && !opts.force && is_vscode_running() {
        return Err(VspError::SafetyRefusal(
            "VS Code appears to be running; importing a new profile into storage.json risks lost updates (close VS Code or pass --force)".to_string(),
        ));
    }

    let code_bin = require_code_binary()?;
    let lock_path = profile_sync_lock_path(&store.user_dir, &profile_name);
    let _target_lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;

    let profile_registered = profile_name == "default"
        || find_storage_profile_location(&store.user_dir, &profile_name)?.is_some();
    let observed_installed = if profile_registered {
        list_installed_extensions(&code_bin, &store, &profile_name)?
    } else {
        Vec::new()
    };

    // Step 0: unconditional compromised/removed remediation first!
    let (step0_uninstalled, step0_failures) = execute_step0_remediation(
        catalog,
        &code_bin,
        &store,
        &profile_name,
        &observed_installed,
        !opts.no_batch,
    )?;
    if !step0_failures.is_empty() {
        return Err(step0_failure(&profile_name, &step0_failures));
    }

    let mut installed_lc: BTreeSet<String> = observed_installed
        .iter()
        .map(|e| e.id.to_ascii_lowercase())
        .collect();
    for rem in &step0_uninstalled {
        installed_lc.remove(&rem.to_ascii_lowercase());
    }

    let ordered_ids = order_extensions_by_group(catalog, &install_ids);
    let missing_ids: Vec<String> = ordered_ids
        .iter()
        .filter(|id| !installed_lc.contains(&id.to_ascii_lowercase()))
        .cloned()
        .collect();

    // PRE-WRITE VSIX LOCK PREFLIGHT BEFORE Step 1 `ensure_storage_profile`!
    let mut install_targets: Vec<(String, String)> = Vec::new();
    let mut unlocked_bypass = false;
    let mut pins_file_override = false;
    let mut run_tier2_unpinned: Vec<String> = Vec::new();

    if !missing_ids.is_empty() {
        let resolved_targets = resolve_and_verify_batch(
            catalog,
            &missing_ids,
            &VsixVerifyOptions {
                allow_unlocked: opts.allow_unlocked,
                cache_dir: None,
            },
        )?;
        if !resolved_targets.failures.is_empty() {
            let has_safety = resolved_targets
                .failures
                .iter()
                .any(|f| f.is_safety_refusal);
            let summary =
                format_vsix_refusal_summary(catalog, &profile_name, &resolved_targets.failures);
            return Err(if has_safety {
                VspError::SafetyRefusal(summary)
            } else {
                VspError::Operational(summary)
            });
        }
        unlocked_bypass = resolved_targets.unlocked_bypass;
        pins_file_override = resolved_targets.pins_file_override;
        run_tier2_unpinned = resolved_targets.tier2_unpinned;
        if !run_tier2_unpinned.is_empty() {
            warnings.push(format!(
                "Tier 2 unpinned (pending_refresh): {} installed without a verified version lock",
                run_tier2_unpinned.join(", ")
            ));
        }
        install_targets = resolved_targets.targets;
    }

    let mut ext_errors: Vec<String> = Vec::new();

    // Step 1: ensure storage.json entry (skips write if profile already exists!).
    let ensure_outcome = ensure_storage_profile(
        &store.user_dir,
        &profile_name,
        opts.force,
        effective_lock_timeout(),
    )?;
    let mut files_written = Vec::new();
    if ensure_outcome.created {
        files_written.push(
            store
                .user_dir
                .join("globalStorage/storage.json")
                .display()
                .to_string(),
        );
    }

    // Step 2: install extensions (batched with smart re-list + missing-only serial fallback).
    let mut installed_now = Vec::new();
    let mut failed_install_ids = Vec::new();

    if !install_targets.is_empty() {
        let (ok_ids, fail_details, fallback_warnings) = install_extensions_batched_with_fallback(
            &code_bin,
            &store,
            &profile_name,
            &install_targets,
            !opts.no_batch,
        )?;
        warnings.extend(fallback_warnings);
        for ok in ok_ids {
            installed_lc.insert(ok.to_ascii_lowercase());
            installed_now.push(ok);
        }
        for (fail_id, detail) in fail_details {
            failed_install_ids.push(fail_id.clone());
            ext_errors.push(format!("failed to install {fail_id} ({detail})"));
        }
    }

    // Step 3: `--ai off` also removes catalog AI-group extensions that are already installed.
    let mut pruned = Vec::new();
    if opts.ai == Some(AiMode::Off) {
        let (ai_uninstalled, ai_errs) = uninstall_ai_extensions(
            catalog,
            &code_bin,
            &store,
            &profile_name,
            &observed_installed,
            &mut installed_lc,
            !opts.no_batch,
        )?;
        pruned.extend(ai_uninstalled);
        ext_errors.extend(ai_errs);
    }

    // Step 4: theme fallback + lossless CST merge + XDG provenance!
    apply_theme_failure_fallback(
        catalog,
        &failed_install_ids,
        &mut desired_settings,
        &mut delete_keys,
    );

    let settings_path = ensure_outcome.profile_dir.join("settings.json");
    check_path_contained(&store.user_dir, &settings_path)?;
    let existing_text = read_optional_file_text(&settings_path)?;
    let existing_map = parse_optional_object(&existing_text);
    if opts.ai == Some(AiMode::Off) {
        delete_keys.extend(retired_ai_off_keys(catalog, &existing_map));
    }
    if settings_path.is_file() {
        backup_file_bounded(&settings_path, DEFAULT_BACKUP_RETENTION)?;
    }

    let merged = merge_jsonc_object_str(
        &existing_text,
        &desired_settings,
        &CstMergeOptions {
            delete_keys,
            delete_paths: Vec::new(),
            ..Default::default()
        },
    )?;
    write_file_atomic(&settings_path, merged.as_bytes(), 0o600, true)?;
    files_written.push(settings_path.display().to_string());

    // Sticky: an import cannot clear earlier bypass audit records.
    let prev_prov = load_profile_provenance(&store.user_dir, &profile_name);
    let prev_unlocked = prev_prov.as_ref().is_some_and(|p| p.unlocked_bypass);
    let prev_publisher_trust_bypass = prev_prov.as_ref().is_some_and(|p| p.publisher_trust_bypass);
    let prev_pins_override = prev_prov.as_ref().is_some_and(|p| p.pins_file_override);
    let desired_lc_set: BTreeSet<String> = ordered_ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect();
    let mut recorded_tier2_unpinned: BTreeSet<String> = run_tier2_unpinned.into_iter().collect();
    if let Some(prev) = &prev_prov {
        for id in &prev.tier2_unpinned {
            let lc = id.to_ascii_lowercase();
            if desired_lc_set.contains(&lc) {
                recorded_tier2_unpinned.insert(lc);
            }
        }
    }
    let prov = ProfileProvenance {
        version: 1,
        profile: profile_name.clone(),
        user_dir: store.user_dir.display().to_string(),
        preset: Some(profile_name.clone()),
        resolved_modules: Vec::new(),
        managed_settings_keys: desired_settings.keys().cloned().collect(),
        managed_extensions: ordered_ids,
        ux: effective_ux,
        ai_mode: opts.ai,
        unlocked_bypass: unlocked_bypass || prev_unlocked,
        publisher_trust_bypass: publisher_trust_bypass || prev_publisher_trust_bypass,
        pins_file_override: pins_file_override || prev_pins_override,
        tier2_unpinned: recorded_tier2_unpinned.into_iter().collect(),
        last_sync_at: now_rfc3339_utc(),
    };
    save_profile_provenance(&prov)?;

    if !ext_errors.is_empty() {
        let msg = format_partial_apply_summary(
            &profile_name,
            ensure_outcome.created,
            &files_written,
            &step0_uninstalled,
            &installed_now,
            &pruned,
            &[],
            &ext_errors,
        );
        return Err(VspError::RetryableLock(msg));
    }

    Ok(SyncReport {
        kind: "bundle".to_string(),
        target: profile_name,
        files_written,
        step0_uninstalled,
        installed: installed_now,
        pruned,
        repaired_pins: Vec::new(),
        storage_mutated: ensure_outcome.created,
        unlocked_bypass,
        warnings,
        ext_errors,
    })
}

fn run_all_sweep(catalog: &WorkspaceCatalog, opts: &SyncOptions) -> VspResult<SyncReport> {
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
    // Resolve `code` binary ONCE for the entire sweep (OPUS-09).
    let code_bin = require_code_binary()?;

    let mut targets = vec!["default".to_string()];
    let registered = list_storage_profiles(&store.user_dir)?;
    targets.extend(registered);

    let mut all_step0 = Vec::new();
    let mut all_installed = Vec::new();
    let mut all_pruned = Vec::new();
    let mut all_written = Vec::new();
    let mut all_warnings = Vec::new();
    let mut incomplete_errors = Vec::new();
    let mut partial_errors = Vec::new();
    let mut op_errors = Vec::new();
    // Safety refusals keep exit 5 for the whole sweep (never downgraded to exit 1).
    let mut safety_errors: Vec<String> = Vec::new();

    for prof in &targets {
        // A profile name from storage.json is untrusted input for the `code` command line.
        if let Err(err) = validate_profile_name(prof) {
            safety_errors.push(err.to_string());
            continue;
        }
        if opts.compromised_only || prof == "default" || !catalog.profiles.contains_key(prof) {
            let lock_path = profile_sync_lock_path(&store.user_dir, prof);
            let _lock = acquire_advisory_lock(&lock_path, effective_lock_timeout())?;
            match list_installed_extensions(&code_bin, &store, prof) {
                Err(err @ VspError::SafetyRefusal(_)) => safety_errors.push(err.to_string()),
                Err(err) => {
                    incomplete_errors.push(format!("{prof}: {err}"));
                }
                Ok(installed) => match execute_step0_remediation(
                    catalog,
                    &code_bin,
                    &store,
                    prof,
                    &installed,
                    !opts.no_batch,
                ) {
                    Ok((uninstalled, failures)) => {
                        for u in uninstalled {
                            all_step0.push(format!("{prof}:{u}"));
                        }
                        if !failures.is_empty() {
                            safety_errors.push(step0_failure(prof, &failures).to_string());
                        }
                    }
                    Err(VspError::IncompleteObservation(msg)) => {
                        incomplete_errors.push(format!("{prof}: {msg}"));
                    }
                    Err(VspError::SafetyRefusal(msg)) => {
                        safety_errors.push(format!("{prof}: {msg}"));
                    }
                    Err(err) => op_errors.push(format!("{prof}: {err}")),
                },
            }
        } else {
            let mut single_opts = opts.clone();
            single_opts.all = false;
            single_opts.profile = Some(prof.clone());
            match run_profile_sync_with_code(catalog, &single_opts, Some(&code_bin)) {
                Ok(rep) => {
                    for u in rep.step0_uninstalled {
                        all_step0.push(format!("{prof}:{u}"));
                    }
                    all_installed.extend(rep.installed);
                    all_pruned.extend(rep.pruned);
                    all_written.extend(rep.files_written);
                    all_warnings.extend(rep.warnings);
                }
                Err(VspError::IncompleteObservation(msg)) => incomplete_errors.push(msg),
                Err(VspError::SafetyRefusal(msg)) => safety_errors.push(format!("{prof}: {msg}")),
                Err(VspError::RetryableLock(msg)) => partial_errors.push(format!("{prof}: {msg}")),
                Err(err) => op_errors.push(format!("{prof}: {err}")),
            }
        }
    }

    if !safety_errors.is_empty() {
        return Err(VspError::SafetyRefusal(format!(
            "sweep refused for {} profile(s):\n{}",
            safety_errors.len(),
            safety_errors.join("\n")
        )));
    }
    if !incomplete_errors.is_empty() {
        return Err(VspError::IncompleteObservation(format!(
            "sweep incomplete across {} profile(s):\n{}",
            incomplete_errors.len(),
            incomplete_errors.join("\n")
        )));
    }
    if !partial_errors.is_empty() {
        return Err(VspError::RetryableLock(format!(
            "sweep partially applied across {} profile(s):\n{}",
            partial_errors.len(),
            partial_errors.join("\n")
        )));
    }
    if !op_errors.is_empty() {
        return Err(VspError::Operational(format!(
            "sweep encountered {} error(s):\n{}",
            op_errors.len(),
            op_errors.join("\n")
        )));
    }

    Ok(SyncReport {
        kind: "sweep".to_string(),
        target: targets.join(","),
        files_written: all_written,
        step0_uninstalled: all_step0,
        installed: all_installed,
        pruned: all_pruned,
        repaired_pins: Vec::new(),
        storage_mutated: false,
        unlocked_bypass: false,
        warnings: all_warnings,
        ext_errors: Vec::new(),
    })
}

#[allow(clippy::too_many_arguments)]
fn format_partial_apply_summary(
    profile_name: &str,
    storage_mutated: bool,
    files_written: &[String],
    step0_uninstalled: &[String],
    installed: &[String],
    pruned: &[String],
    repaired_pins: &[String],
    ext_errors: &[String],
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "partial apply for profile '{profile_name}': {} extension operation(s) failed after mutations began:",
        ext_errors.len()
    );
    let _ = writeln!(
        out,
        "  Applied: storage_mutated={storage_mutated}, files_written={}, step0_uninstalled={}, installed={}, pruned={}, repaired_pins={}",
        files_written.len(),
        step0_uninstalled.len(),
        installed.len(),
        pruned.len(),
        repaired_pins.len()
    );
    let _ = writeln!(out, "  Failed:");
    for err in ext_errors {
        let _ = writeln!(out, "    - {err}");
    }
    let _ = write!(out, "  Remedy: inspect the failed extension error(s) above and re-run `vsp sync -p {profile_name}` to converge remaining state.");
    out
}

fn execute_step0_remediation(
    catalog: &WorkspaceCatalog,
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    installed: &[InstalledExtension],
    batch: bool,
) -> VspResult<(Vec<String>, Vec<String>)> {
    let to_remove: Vec<String> = installed
        .iter()
        .filter_map(|ext| {
            let rec = catalog.extension_record(&ext.id)?;
            matches!(
                rec.status,
                ExtensionStatus::Compromised | ExtensionStatus::Removed
            )
            .then(|| ext.id.clone())
        })
        .collect();

    if to_remove.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let (uninstalled, failed) = uninstall_extensions_batched_with_fallback(
        code_bin,
        store,
        profile_name,
        &to_remove,
        batch,
    )?;
    let failures = failed
        .into_iter()
        .map(|(id, detail)| format!("{id} ({detail})"))
        .collect();
    Ok((uninstalled, failures))
}

#[allow(clippy::too_many_arguments)]
fn execute_step3_prune(
    catalog: &WorkspaceCatalog,
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    observed_installed: &[InstalledExtension],
    desired_ids: &[String],
    installed_now_lc: &BTreeSet<String>,
    prune_mode: Option<PruneMode>,
    batch: bool,
) -> VspResult<(Vec<String>, Vec<String>)> {
    let desired_lc: BTreeSet<String> = desired_ids.iter().map(|s| s.to_ascii_lowercase()).collect();
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
        for ext in observed_installed {
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
    for rec in catalog.extensions.extensions.values() {
        for pm in &rec.pack_members {
            known_pack_members.insert(pm.to_ascii_lowercase());
        }
    }

    let mut candidates: Vec<(String, &'static str)> = Vec::new();
    for ext in observed_installed {
        let lc = ext.id.to_ascii_lowercase();
        if !installed_now_lc.contains(&lc) {
            continue;
        }
        match catalog.extension_record(&lc) {
            Some(rec) => match rec.status {
                ExtensionStatus::Compromised
                | ExtensionStatus::Removed
                | ExtensionStatus::Unknown(_) => {}
                ExtensionStatus::Superseded => {
                    let rep_installed = rec
                        .replaced_by
                        .as_deref()
                        .map(str::to_ascii_lowercase)
                        .is_some_and(|rep| installed_now_lc.contains(&rep));
                    if rep_installed {
                        candidates.push((lc, "superseded"));
                    }
                }
                ExtensionStatus::Retired => {
                    if prune_retired {
                        candidates.push((lc, "retired"));
                    }
                }
                ExtensionStatus::Active => {
                    if prune_unmanaged
                        && !desired_lc.contains(&lc)
                        && !retained_pack_members.contains(&lc)
                    {
                        candidates.push((lc, "unmanaged"));
                    }
                }
            },
            None => {
                if prune_unmanaged
                    && known_pack_members.contains(&lc)
                    && !desired_lc.contains(&lc)
                    && !retained_pack_members.contains(&lc)
                {
                    candidates.push((lc, "unmanaged pack member"));
                }
            }
        }
    }

    if candidates.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let reason_map: BTreeMap<String, &'static str> = candidates.iter().cloned().collect();
    let ids: Vec<String> = candidates.into_iter().map(|(id, _)| id).collect();
    let (pruned, failed) =
        uninstall_extensions_batched_with_fallback(code_bin, store, profile_name, &ids, batch)?;
    let errors = failed
        .into_iter()
        .map(|(id, detail)| {
            let kind = reason_map.get(&id).copied().unwrap_or("extension");
            format!("failed to uninstall {kind} {id} ({detail})")
        })
        .collect();
    Ok((pruned, errors))
}

fn verify_bundle_sha256sums(
    catalog: &WorkspaceCatalog,
    canon_bundle: &Path,
    staged_bytes: &[u8],
    warnings: &mut Vec<String>,
) -> VspResult<()> {
    let Some(canon_dir) = canon_bundle.parent() else {
        return Ok(());
    };
    let Some(canon_base) = canon_bundle.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    let is_managed = catalog.root != Path::new(crate::schema_loader::EMBEDDED_ROOT_LABEL) && {
        let managed_exports = resolve_canonical_path(&catalog.root.join("exports"))
            .unwrap_or_else(|_| catalog.root.join("exports"));
        canon_dir == managed_exports
    };
    let sums_path = canon_dir.join("SHA256SUMS");

    if !sums_path.is_file() {
        if is_managed {
            return Err(VspError::SafetyRefusal(format!(
                "SHA256SUMS missing in managed exports directory {}",
                canon_dir.display()
            )));
        }
        warnings.push(format!(
            "SHA256SUMS not found next to {canon_base}; proceeding without checksum verification"
        ));
        return Ok(());
    }

    let sums_text = fs::read_to_string(&sums_path).map_err(|err| {
        VspError::SafetyRefusal(format!("failed to read {}: {err}", sums_path.display()))
    })?;
    let mut matches = Vec::new();
    for line in sums_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((hash_part, rest)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let fname = rest.trim_start().trim_start_matches('*');
        if fname == canon_base {
            matches.push(hash_part.to_ascii_lowercase());
        }
    }

    if matches.len() > 1 {
        return Err(VspError::SafetyRefusal(format!(
            "duplicate SHA256SUMS entries for '{canon_base}' in {}",
            sums_path.display()
        )));
    }
    if let Some(expected) = matches.first() {
        let actual = sha256_hex(staged_bytes);
        if &actual != expected {
            return Err(VspError::SafetyRefusal(format!(
                "SHA256SUMS mismatch for '{canon_base}' (expected {expected}, actual {actual})"
            )));
        }
        return Ok(());
    }

    if is_managed {
        return Err(VspError::SafetyRefusal(format!(
            "managed export '{canon_base}' is not listed in {}",
            sums_path.display()
        )));
    }
    warnings.push(format!(
        "{canon_base} not listed in {}; proceeding without checksum verification",
        sums_path.display()
    ));
    Ok(())
}

fn enforce_autonomy_gate(
    settings: &Map<String, Value>,
    label: &str,
    allow_unsafe: bool,
) -> VspResult<()> {
    if allow_unsafe {
        return Ok(());
    }
    let hits = check_autonomy_denylist(&Value::Object(settings.clone()), label);
    if !hits.is_empty() {
        let summary: Vec<String> = hits.into_iter().map(|v| v.message).collect();
        return Err(VspError::SafetyRefusal(format!(
            "autonomy/exec-override deny-list violation in {label}: {}",
            summary.join("; ")
        )));
    }
    Ok(())
}

/// Builds the error for a failed Step 0 uninstall. Step 0 failures are safety refusals
/// (exit `5`): a compromised extension is still installed, so nothing else may proceed.
fn step0_failure(profile_name: &str, failures: &[String]) -> VspError {
    VspError::SafetyRefusal(format!(
        "Step 0 failed to uninstall {} compromised/removed extension(s) in '{}': {}; no install or settings write was done",
        failures.len(),
        profile_name,
        failures.join(", ")
    ))
}

fn workspace_file_mode(path: &Path) -> u32 {
    const DEFAULT_MODE: u32 = 0o644;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            return meta.permissions().mode() & DEFAULT_MODE;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    DEFAULT_MODE
}

fn installed_ai_extensions(
    catalog: &WorkspaceCatalog,
    installed: &[InstalledExtension],
) -> Vec<String> {
    installed
        .iter()
        .filter(|ext| {
            catalog
                .extension_record(&ext.id)
                .is_some_and(|rec| rec.group.eq_ignore_ascii_case("AI"))
        })
        .map(|ext| ext.id.to_ascii_lowercase())
        .collect()
}

fn uninstall_ai_extensions(
    catalog: &WorkspaceCatalog,
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    observed_installed: &[InstalledExtension],
    installed_lc: &mut BTreeSet<String>,
    batch: bool,
) -> VspResult<(Vec<String>, Vec<String>)> {
    let to_remove: Vec<String> = installed_ai_extensions(catalog, observed_installed)
        .into_iter()
        .filter(|id| installed_lc.contains(id))
        .collect();
    if to_remove.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let (uninstalled, failed) = uninstall_extensions_batched_with_fallback(
        code_bin,
        store,
        profile_name,
        &to_remove,
        batch,
    )?;
    for id in &uninstalled {
        installed_lc.remove(&id.to_ascii_lowercase());
    }
    let errors = failed
        .into_iter()
        .map(|(id, detail)| format!("failed to uninstall AI extension {id} (--ai off) ({detail})"))
        .collect();
    Ok((uninstalled, errors))
}

fn enforce_publisher_allowlist(catalog: &WorkspaceCatalog, ext_ids: &[String]) -> VspResult<bool> {
    if ext_ids.is_empty() {
        return Ok(false);
    }
    for id in ext_ids {
        if !is_valid_extension_id_syntax(id) {
            return Err(VspError::SafetyRefusal(format!(
                "invalid extension id syntax: '{id}'"
            )));
        }
    }
    if let Some(id) = ext_ids.iter().find(|id| catalog.is_blocked_publisher(id)) {
        return Err(VspError::SafetyRefusal(format!(
            "extension '{id}' is refused: publisher '{}' is blocked by the catalog",
            crate::model::extension_publisher(id)
        )));
    }
    if env::var("ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER")
        .ok()
        .as_deref()
        == Some("1")
    {
        eprintln!(
            "vsp: WARNING: ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 bypasses the publisher allowlist for {} extension(s); the bypass is recorded in provenance and reported by 'vsp doctor'",
            ext_ids.len()
        );
        return Ok(true);
    }
    let allowed: BTreeSet<String> = catalog
        .publishers
        .publishers
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();
    if allowed.is_empty() {
        return Err(VspError::SafetyRefusal(
            "publisher allowlist is empty or missing (catalog publishers); refusing to install extensions (fail closed)"
                .to_string(),
        ));
    }
    for id in ext_ids {
        let pub_lc = id.split('.').next().unwrap_or("").to_ascii_lowercase();
        if !allowed.contains(&pub_lc) {
            return Err(VspError::SafetyRefusal(format!(
                "extension '{id}' publisher '{pub_lc}' is not in catalog/publishers.json allowlist"
            )));
        }
    }
    Ok(false)
}

fn lint_workspace_no_host_path_leak(value: &Value, label: &str) -> VspResult<()> {
    let serialized = value.to_string();
    let home = home_dir();
    let home_str = home.to_string_lossy();
    let home_trimmed = home_str.trim_end_matches('/');

    if !home_trimmed.is_empty() && home_trimmed != "/" && serialized.contains(home_trimmed) {
        return Err(VspError::SafetyRefusal(format!(
            "host-path leak blocked in {label}: literal $HOME ({home_trimmed}) must not appear in workspace files"
        )));
    }
    if let Ok(home_canon) = fs::canonicalize(&home) {
        let hc_str = home_canon.to_string_lossy();
        let hc_trimmed = hc_str.trim_end_matches('/');
        if !hc_trimmed.is_empty() && hc_trimmed != "/" && serialized.contains(hc_trimmed) {
            return Err(VspError::SafetyRefusal(format!(
                "host-path leak blocked in {label}: canonical $HOME ({hc_trimmed}) must not appear in workspace files"
            )));
        }
    }
    for bad in [
        "/opt/homebrew/",
        "/usr/local/opt/",
        "/Users/",
        "${command:jenv.javaHome}",
    ] {
        if serialized.contains(bad) {
            return Err(VspError::SafetyRefusal(format!(
                "host-path leak blocked in {label}: forbidden token '{bad}'"
            )));
        }
    }
    Ok(())
}

fn apply_theme_failure_fallback(
    catalog: &WorkspaceCatalog,
    failed_exts: &[String],
    desired_settings: &mut Map<String, Value>,
    delete_keys: &mut Vec<String>,
) {
    if failed_exts.is_empty() {
        return;
    }
    let cur_color = desired_settings
        .get("workbench.colorTheme")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let cur_dark = desired_settings
        .get("workbench.preferredDarkColorTheme")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let cur_icon = desired_settings
        .get("workbench.iconTheme")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let cur_prod = desired_settings
        .get("workbench.productIconTheme")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let mut strip_color = false;
    let mut strip_dark = false;
    let mut strip_icon = false;
    let mut strip_prod = false;

    for fid in failed_exts {
        let fid_lc = fid.to_ascii_lowercase();
        if !cur_color.is_empty() && cur_color == fid_lc {
            strip_color = true;
            strip_dark = true;
        }
        if !cur_dark.is_empty() && cur_dark == fid_lc {
            strip_dark = true;
        }
        if !cur_icon.is_empty() && cur_icon == fid_lc {
            strip_icon = true;
        }
        if !cur_prod.is_empty() && cur_prod == fid_lc {
            strip_prod = true;
        }
        if let Some(rec) = catalog.extension_record(&fid_lc) {
            for lbl in &rec.theme_labels {
                let lbl_lc = lbl.to_ascii_lowercase();
                if !cur_color.is_empty() && cur_color == lbl_lc {
                    strip_color = true;
                    strip_dark = true;
                }
                if !cur_dark.is_empty() && cur_dark == lbl_lc {
                    strip_dark = true;
                }
                if !cur_icon.is_empty() && cur_icon == lbl_lc {
                    strip_icon = true;
                }
                if !cur_prod.is_empty() && cur_prod == lbl_lc {
                    strip_prod = true;
                }
            }
        }
    }

    for (flag, key) in [
        (strip_color, "workbench.colorTheme"),
        (strip_dark, "workbench.preferredDarkColorTheme"),
        (strip_icon, "workbench.iconTheme"),
        (strip_prod, "workbench.productIconTheme"),
    ] {
        if flag {
            desired_settings.remove(key);
            if !delete_keys.iter().any(|k| k == key) {
                delete_keys.push(key.to_string());
            }
        }
    }
}

fn restore_quick_suggestions_if_needed(
    existing: &Map<String, Value>,
    desired: &mut Map<String, Value>,
) {
    let needs_restore = existing
        .get("editor.quickSuggestions")
        .or_else(|| desired.get("editor.quickSuggestions"))
        .and_then(Value::as_object)
        .and_then(|o| o.get("other"))
        .and_then(Value::as_str)
        == Some("offWhenInlineCompletions");
    if needs_restore {
        let mut qs = desired
            .get("editor.quickSuggestions")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        qs.insert("other".to_string(), Value::String("on".to_string()));
        desired.insert("editor.quickSuggestions".to_string(), Value::Object(qs));
    }
}

/// Installs portable XDG RTK shell wrappers under `${XDG_CONFIG_HOME:-~/.config}/vsp/rtk/`
/// atomically (never following symlinks).
///
/// # Errors
///
/// Returns [`VspError::Operational`] or [`VspError::SafetyRefusal`] if source scripts are
/// missing or writing fails.
pub fn install_rtk_xdg_wrappers(catalog_root: &Path) -> VspResult<PathBuf> {
    let xdg_root = resolve_xdg_config_home();
    let dest_dir = xdg_root.join("vsp/rtk");
    let zsh_dir = dest_dir.join("zsh");
    ensure_no_symlink_components(&xdg_root, &zsh_dir)?;

    let created: Vec<PathBuf> = [xdg_root.join("vsp"), dest_dir.clone(), zsh_dir.clone()]
        .into_iter()
        .filter(|dir| !dir.exists())
        .collect();
    fs::create_dir_all(&zsh_dir).map_err(|err| {
        VspError::Operational(format!(
            "failed to create XDG RTK directory {}: {err}",
            zsh_dir.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for dir in &created {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
        }
    }
    #[cfg(not(unix))]
    let _ = &created;
    ensure_no_symlink_components(&xdg_root, &zsh_dir)?;
    let write_script = |dest: &Path, bytes: &[u8]| -> VspResult<()> {
        ensure_no_symlink_components(&xdg_root, dest)?;
        write_file_atomic(dest, bytes, 0o644, true)
    };

    if catalog_root == Path::new(crate::schema_loader::EMBEDDED_ROOT_LABEL) {
        for (rel_path, bytes) in crate::schema_loader::embedded_rtk_scripts() {
            let Some(suffix) = rel_path.strip_prefix("packs/rtk-terminal/scripts/") else {
                continue;
            };
            write_script(&dest_dir.join(suffix), bytes)?;
        }
        return Ok(dest_dir);
    }

    let v2_src_dir = catalog_root.join("packs/rtk-terminal/scripts");
    let src_dir = if v2_src_dir.is_dir() {
        v2_src_dir
    } else {
        catalog_root.join("modules/capabilities/rtk-terminal/scripts")
    };

    for fname in ["rtk-init.bash", "rtk-init.fish", "rtk-init.zsh"] {
        let src = src_dir.join(fname);
        let bytes = fs::read(&src).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", src.display()))
        })?;
        write_script(&dest_dir.join(fname), &bytes)?;
    }
    for fname in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
        let src = src_dir.join("zsh").join(fname);
        let bytes = fs::read(&src).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", src.display()))
        })?;
        write_script(&zsh_dir.join(fname), &bytes)?;
    }

    Ok(dest_dir)
}

fn build_rtk_terminal_settings(xdg_rtk_dir: &Path) -> BTreeMap<String, Value> {
    let dir_str = xdg_rtk_dir.display().to_string();
    let orig_zdotdir = env::var("ZDOTDIR").ok();
    let zset = if orig_zdotdir.is_some() { "1" } else { "0" };
    let zorig = orig_zdotdir.unwrap_or_default();

    let osx_def =
        if env::var("SHELL").ok().is_some_and(|s| s.ends_with("fish")) && command_exists("fish") {
            "rtk-fish"
        } else {
            "rtk-zsh"
        };
    let linux_def = if command_exists("zsh") {
        "rtk-zsh"
    } else if command_exists("fish") {
        "rtk-fish"
    } else {
        "rtk-bash"
    };

    let mut out = BTreeMap::new();
    out.insert(
        "terminal.integrated.profiles.osx".to_string(),
        json!({
            "rtk-zsh": {
                "path": "/bin/zsh",
                "args": ["-l"],
                "env": {
                    "ARTAGON_ORIG_ZDOTDIR_SET": zset,
                    "ARTAGON_ORIG_ZDOTDIR": zorig,
                    "ZDOTDIR": format!("{dir_str}/zsh")
                }
            },
            "rtk-fish": {
                "path": "fish",
                "args": ["--init-command", "source \"$ARTAGON_RTK_DIR/rtk-init.fish\""],
                "env": {
                    "ARTAGON_RTK_DIR": dir_str
                }
            },
            "rtk-bash": {
                "path": "/bin/bash",
                "args": ["--rcfile", format!("{dir_str}/rtk-init.bash")]
            }
        }),
    );
    out.insert(
        "terminal.integrated.defaultProfile.osx".to_string(),
        Value::String(osx_def.to_string()),
    );
    out.insert(
        "terminal.integrated.profiles.linux".to_string(),
        json!({
            "rtk-zsh": {
                "path": "/usr/bin/zsh",
                "args": ["-l"],
                "env": {
                    "ARTAGON_ORIG_ZDOTDIR_SET": zset,
                    "ARTAGON_ORIG_ZDOTDIR": zorig,
                    "ZDOTDIR": format!("{dir_str}/zsh")
                }
            },
            "rtk-fish": {
                "path": "/usr/bin/fish",
                "args": ["--init-command", "source \"$ARTAGON_RTK_DIR/rtk-init.fish\""],
                "env": {
                    "ARTAGON_RTK_DIR": dir_str
                }
            },
            "rtk-bash": {
                "path": "/bin/bash",
                "args": ["--rcfile", format!("{dir_str}/rtk-init.bash")]
            }
        }),
    );
    out.insert(
        "terminal.integrated.defaultProfile.linux".to_string(),
        Value::String(linux_def.to_string()),
    );
    out
}

fn command_exists(bin: &str) -> bool {
    find_binary(bin).is_some()
}

fn require_code_binary() -> VspResult<PathBuf> {
    find_code_binary().ok_or_else(|| {
        VspError::IncompleteObservation(
            "VS Code 'code' CLI not found on PATH (required for profile extension management)"
                .to_string(),
        )
    })
}

fn run_code_uninstall_single(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    ext_id: &str,
) -> VspResult<CodeExecOutput> {
    let mut cmd = code_command(code_bin, store, profile_name)?;
    cmd.arg("--uninstall-extension").arg(ext_id);
    run_code_command(
        &mut cmd,
        &format!("code --uninstall-extension {ext_id} (profile '{profile_name}')"),
    )
}

type BatchUninstallResult = (Vec<String>, Vec<(String, String)>);

fn uninstall_extensions_batched_with_fallback(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    ext_ids: &[String],
    batch: bool,
) -> VspResult<BatchUninstallResult> {
    if ext_ids.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    if batch && ext_ids.len() > 1 {
        let mut cmd = code_command(code_bin, store, profile_name)?;
        for id in ext_ids {
            cmd.arg("--uninstall-extension").arg(id);
        }
        let exec = run_code_command(
            &mut cmd,
            &format!("code batched --uninstall-extension (profile '{profile_name}')"),
        )?;
        if exec.success {
            return Ok((ext_ids.to_vec(), Vec::new()));
        }
        eprintln!(
            "vsp: batched extension uninstall for profile '{profile_name}' failed ({}); falling back to per-extension uninstall",
            exec.format_failure()
        );
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    for id in ext_ids {
        let exec = run_code_uninstall_single(code_bin, store, profile_name, id)?;
        if exec.success {
            succeeded.push(id.clone());
        } else {
            failed.push((id.clone(), exec.format_failure()));
        }
    }
    Ok((succeeded, failed))
}

fn run_code_install_single(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    target: &str,
    force: bool,
) -> VspResult<CodeExecOutput> {
    let mut cmd = code_command(code_bin, store, profile_name)?;
    cmd.arg("--install-extension").arg(target);
    if force {
        cmd.arg("--force");
    }
    run_code_command(
        &mut cmd,
        &format!("code --install-extension {target} (profile '{profile_name}')"),
    )
}

type BatchInstallResult = (Vec<String>, Vec<(String, String)>, Vec<String>);

fn install_extensions_batched_with_fallback(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
    targets: &[(String, String)],
    batch: bool,
) -> VspResult<BatchInstallResult> {
    if targets.is_empty() {
        return Ok((Vec::new(), Vec::new(), Vec::new()));
    }

    let mut succeeded = Vec::new();
    let mut failed = Vec::new();
    let mut fallback_warnings = Vec::new();
    let mut remaining: Vec<(String, String)> = targets.to_vec();

    if batch && targets.len() > 1 {
        let mut cmd = code_command(code_bin, store, profile_name)?;
        for (_, arg) in targets {
            cmd.arg("--install-extension").arg(arg);
        }
        let exec = run_code_command(
            &mut cmd,
            &format!("code batched --install-extension (profile '{profile_name}')"),
        )?;
        if exec.success {
            return Ok((
                targets.iter().map(|(id, _)| id.clone()).collect(),
                Vec::new(),
                Vec::new(),
            ));
        }

        let notice = format!(
            "batched extension install for profile '{profile_name}' failed ({}); re-listing installed extensions and retrying missing ones serially",
            exec.format_failure()
        );
        eprintln!("vsp: {notice}");
        fallback_warnings.push(notice);

        // Re-list installed extensions after partial batched failure and retry ONLY the
        // extensions that are still missing!
        if let Ok(after_batch) = list_installed_extensions(code_bin, store, profile_name) {
            let now_installed: BTreeSet<String> = after_batch
                .iter()
                .map(|e| e.id.to_ascii_lowercase())
                .collect();
            let mut still_missing = Vec::new();
            for (id, arg) in remaining {
                if now_installed.contains(&id.to_ascii_lowercase()) {
                    succeeded.push(id);
                } else {
                    still_missing.push((id, arg));
                }
            }
            remaining = still_missing;
        }
    }

    for (id, arg) in remaining {
        let exec = run_code_install_single(code_bin, store, profile_name, &arg, false)?;
        if exec.success {
            succeeded.push(id);
        } else {
            failed.push((id, exec.format_failure()));
        }
    }
    Ok((succeeded, failed, fallback_warnings))
}

fn read_optional_file_text(path: &Path) -> VspResult<String> {
    if !path.is_file() {
        return Ok(String::new());
    }
    fs::read_to_string(path)
        .map_err(|err| VspError::Operational(format!("failed to read {}: {err}", path.display())))
}

fn parse_optional_object(text: &str) -> Map<String, Value> {
    if text.trim().is_empty() {
        return Map::new();
    }
    match parse_jsonc_value(text, "existing") {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit_codes::ExitCode;
    use crate::schema_loader::load_embedded;

    /// Opus M2: an empty or missing publisher allowlist is a refusal, never a pass.
    #[test]
    fn empty_publisher_allowlist_fails_closed() {
        let mut catalog = load_embedded().expect("embedded catalog");
        let ids = vec!["editorconfig.editorconfig".to_string()];
        assert!(!enforce_publisher_allowlist(&catalog, &ids).expect("allowlisted"));
        assert_eq!(
            enforce_publisher_allowlist(&catalog, &["unlisted-publisher.tool".to_string()])
                .expect_err("unlisted publisher")
                .exit_code(),
            ExitCode::SafetyRefusal
        );

        catalog.publishers.publishers.clear();
        let err = enforce_publisher_allowlist(&catalog, &ids).expect_err("empty allowlist");
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
        assert!(
            err.to_string().contains("allowlist is empty or missing"),
            "{err}"
        );
        assert!(!enforce_publisher_allowlist(&catalog, &[]).expect("no extensions"));
        assert_eq!(
            enforce_publisher_allowlist(&catalog, &["not an id".to_string()])
                .expect_err("invalid id")
                .exit_code(),
            ExitCode::SafetyRefusal
        );
    }

    /// Opus M5 / L8: a Step 0 failure is a safety refusal (exit 5).
    #[test]
    fn step0_failure_is_a_safety_refusal() {
        let err = step0_failure("rust", &["bad.ext".to_string()]);
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
        assert!(err.to_string().contains("bad.ext"));
    }

    /// Opus M7: only catalog `AI`-group extensions are selected for `--ai off` removal.
    #[test]
    fn installed_ai_extensions_selects_catalog_ai_group_only() {
        let catalog = load_embedded().expect("embedded catalog");
        let installed: Vec<InstalledExtension> = [
            "Anthropic.Claude-Code",
            "github.copilot-chat",
            "editorconfig.editorconfig",
            "unknown-publisher.ai-thing",
        ]
        .iter()
        .map(|id| InstalledExtension {
            id: (*id).to_string(),
            version: None,
        })
        .collect();
        assert_eq!(
            installed_ai_extensions(&catalog, &installed),
            vec![
                "anthropic.claude-code".to_string(),
                "github.copilot-chat".to_string()
            ]
        );
    }

    /// CARBON-05: declarative `retireOnSwitch` settings and extensions come from catalog.
    #[test]
    fn retire_on_switch_collects_settings_and_extensions_from_catalog() {
        let catalog = load_embedded().expect("embedded catalog");
        let desired = Map::new();
        let mut delete_keys = Vec::new();
        collect_retire_on_switch_settings(
            &catalog,
            &["biome".to_string()],
            &desired,
            &mut delete_keys,
        );
        assert!(delete_keys.contains(&"eslint.workingDirectories".to_string()));
        assert!(delete_keys.contains(&"eslint.validate".to_string()));

        let exts = collect_retire_on_switch_extensions(&catalog, &["biome".to_string()]);
        assert!(exts.contains("dbaeumer.vscode-eslint"));
        assert!(exts.contains("esbenp.prettier-vscode"));
    }
}

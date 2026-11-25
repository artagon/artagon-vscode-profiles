//! DAG module resolver, deterministic settings/extensions/tasks compiler,
//! `.code-profile` exporter, and deterministic `dev dist` generator.
//!
//! Mirrors `scripts/lib/module-resolver.sh`, `scripts/lib/compiler.sh`, and
//! `scripts/export-profiles.sh` so resolved profiles and bundles achieve 100%
//! byte-for-byte parity with tracked artifacts.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::exit_codes::{VspError, VspResult};
use crate::model::{
    is_valid_module_id, resolve_legacy_profile_name, AiMode, BundleExtensions, CodeProfileBundle,
    ModuleKind, UxPreset, VsCodeExtensionEntry, VsCodeExtensionIdentifier, WorkspaceCatalog,
    WorkspaceExtensionsFile, COMPILER_UNION_KEYS, GROUP_ORDER, LEGACY_BUNDLE_ALIASES,
};

/// Scope selector when compiling module settings fragments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    /// Only `settings.workspace.jsonc` fragments.
    Workspace,
    /// Only `settings.profile.jsonc` fragments.
    Profile,
    /// Both workspace and profile fragments (workspace first, then profile per module).
    Both,
}

/// Task command prefix mode matching `compiler_compile_tasks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtkTaskMode {
    /// Portable plain commands (default for team-shareable workspaces).
    Plain,
    /// Prefix shell commands with `rtk ` when not already prefixed.
    Rtk,
}

/// Inputs to [`resolve_modules`].
#[derive(Debug, Clone, Default)]
pub struct ResolveRequest {
    /// Base root modules (defaults + recipe/detected modules).
    pub roots: Vec<String>,
    /// Recipe-level exclusions (can be overridden by `cli_with`).
    pub recipe_excludes: Vec<String>,
    /// Explicit CLI `--with` additions.
    pub cli_with: Vec<String>,
    /// Explicit CLI `--without` exclusions.
    pub cli_without: Vec<String>,
}

/// Result of writing `.code-profile` bundles and `SHA256SUMS`.
#[derive(Debug, Clone)]
pub struct DistReport {
    /// Destination directory where bundles and `SHA256SUMS` were written.
    pub out_dir: PathBuf,
    /// Canonical bundle filenames written.
    pub canonical_bundles: Vec<String>,
    /// Materialized legacy alias bundle filenames written.
    pub alias_bundles: Vec<String>,
    /// Path to the generated `SHA256SUMS` manifest.
    pub sha256sums_path: PathBuf,
    /// Ordered `(sha256_hex, filename)` entries recorded in `SHA256SUMS`.
    pub checksums: Vec<(String, String)>,
}

/// Resolves a module selection into a deterministic, parent-first ordered list of module IDs.
///
/// Enforces:
/// 1. Valid module ID syntax and non-contradictory `--with`/`--without`.
/// 2. Transitive `requires[]` closure with cycle detection and excluded-dependency rejection.
/// 3. Fixed-point conditional `defaultWith[]` expansion (suppressed on exclusion or conflict).
/// 4. Pairwise symmetric `conflicts[]` rejection.
/// 5. Deterministic total sort key ordering followed by a parent-first DFS pass.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] (exit `4`) on unknown module IDs, invalid ID syntax,
/// cycles, excluded required dependencies, or conflicting module selections.
pub fn resolve_modules(
    catalog: &WorkspaceCatalog,
    req: &ResolveRequest,
) -> VspResult<Vec<String>> {
    let roots = split_csv_items(&req.roots);
    let recipe_excludes = split_csv_items(&req.recipe_excludes);
    let cli_with = split_csv_items(&req.cli_with);
    let cli_without = split_csv_items(&req.cli_without);

    for item in &cli_with {
        if cli_without.iter().any(|w| w == item) {
            return Err(VspError::UsageOrConfig(format!(
                "contradictory selection: '{item}' passed in both --with and --without"
            )));
        }
    }

    // Unified effective exclusion set: (recipe_excludes + cli_without) - cli_with.
    let mut eff_excluded: Vec<String> = Vec::new();
    for item in &recipe_excludes {
        if cli_with.iter().any(|w| w == item) {
            continue;
        }
        push_unique(&mut eff_excluded, item);
    }
    for item in &cli_without {
        push_unique(&mut eff_excluded, item);
    }

    for item in &eff_excluded {
        if !is_valid_module_id(item) {
            return Err(VspError::UsageOrConfig(format!(
                "invalid excluded module id '{item}'"
            )));
        }
    }

    // Combine roots (minus eff_excluded) + cli_with.
    let mut initial: Vec<String> = Vec::new();
    for item in &roots {
        if eff_excluded.iter().any(|e| e == item) {
            continue;
        }
        push_unique(&mut initial, item);
    }
    for item in &cli_with {
        push_unique(&mut initial, item);
    }

    // Phase 1: Transitive requires[] DFS.
    let mut visited: Vec<String> = Vec::new();
    for item in &initial {
        dfs_visit(catalog, item, &mut Vec::new(), None, &eff_excluded, &mut visited)?;
    }

    // Phase 2: Fixed-point conditional defaultWith[] expansion.
    loop {
        let mut added = false;
        let snapshot = visited.clone();
        for mod_id in &snapshot {
            let def = catalog.modules.get(mod_id).ok_or_else(|| {
                VspError::UsageOrConfig(format!("unknown module '{mod_id}'"))
            })?;
            for comp in &def.manifest.default_with {
                if can_add_default_companion(catalog, comp, &visited, &eff_excluded) {
                    dfs_visit(
                        catalog,
                        comp,
                        &mut Vec::new(),
                        Some(mod_id),
                        &eff_excluded,
                        &mut visited,
                    )?;
                    added = true;
                }
            }
        }
        if !added {
            break;
        }
    }

    // Phase 3: Pairwise symmetric conflict check.
    for m1 in &visited {
        let def1 = catalog
            .modules
            .get(m1)
            .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{m1}'")))?;
        for m2 in &def1.manifest.conflicts {
            if visited.iter().any(|v| v == m2) {
                return Err(VspError::UsageOrConfig(format!(
                    "conflicting modules selected: '{m1}' conflicts with '{m2}' (resolve with --without <id>)"
                )));
            }
        }
    }

    // Phase 4: Deterministic total order preserving parent-first invariant.
    let mut keyed: Vec<(String, String)> = Vec::with_capacity(visited.len());
    for mod_id in &visited {
        let sk = module_sort_key(catalog, mod_id)?;
        keyed.push((sk, mod_id.clone()));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));

    let mut ordered: Vec<String> = Vec::with_capacity(visited.len());
    for (_, seed) in keyed {
        dfs_visit(
            catalog,
            &seed,
            &mut Vec::new(),
            None,
            &eff_excluded,
            &mut ordered,
        )?;
    }

    Ok(ordered)
}

/// Resolves a canonical profile recipe (`profiles/<name>/profile.json`) with optional
/// `--with`, `--without`, `--ux`, and `--ai` adjustments.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] if `profile_name` does not exist in the catalog,
/// or [`VspError::UsageOrConfig`] on DAG resolution failures.
pub fn resolve_profile_recipe(
    catalog: &WorkspaceCatalog,
    profile_name: &str,
    cli_with: &[String],
    cli_without: &[String],
    ux_override: Option<UxPreset>,
    ai_mode: Option<AiMode>,
) -> VspResult<Vec<String>> {
    let recipe = catalog.profiles.get(profile_name).ok_or_else(|| {
        VspError::TargetMissing(format!("unknown profile recipe '{profile_name}'"))
    })?;

    let mut roots: Vec<String> = Vec::new();
    if recipe.use_defaults {
        roots.extend(catalog.defaults.capabilities.iter().cloned());
    }
    roots.extend(recipe.modules.iter().cloned());

    let effective_ux = ux_override
        .map(|u| u.as_str().to_string())
        .unwrap_or_else(|| recipe.ux.clone());
    if !effective_ux.is_empty() && effective_ux != "default" {
        roots.push(effective_ux);
    }

    let mut effective_with = cli_with.to_vec();
    let mut effective_without = cli_without.to_vec();
    apply_ai_mode_to_lists(ai_mode, &mut effective_with, &mut effective_without);

    resolve_modules(
        catalog,
        &ResolveRequest {
            roots,
            recipe_excludes: recipe.exclude_modules.clone(),
            cli_with: effective_with,
            cli_without: effective_without,
        },
    )
}

/// Applies `--ai full|copilot|off` adjustments to `--with` and `--without` module lists.
pub fn apply_ai_mode_to_lists(
    ai_mode: Option<AiMode>,
    cli_with: &mut Vec<String>,
    cli_without: &mut Vec<String>,
) {
    match ai_mode {
        Some(AiMode::Full) => {
            push_unique(cli_with, "ai-agents");
            push_unique(cli_without, "ai-copilot");
        }
        Some(AiMode::Copilot) => {
            push_unique(cli_with, "ai-copilot");
            push_unique(cli_without, "ai-agents");
        }
        Some(AiMode::Off) => {
            push_unique(cli_without, "ai-agents");
            push_unique(cli_without, "ai-copilot");
        }
        None => {}
    }
}

/// Deep-merges two JSON values at key `key` using the exact semantics of `compiler.sh`:
/// - Object + Object: recursive merge
/// - Array + Array for keys in [`COMPILER_UNION_KEYS`]: order-preserving set union
/// - Everything else: right-hand replacement (`rhs`).
#[must_use]
pub fn merge_json_value(key: &str, lhs: Value, rhs: Value) -> Value {
    match (lhs, rhs) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (sub_k, sub_v) in b {
                match a.remove(&sub_k) {
                    Some(prev) => {
                        let merged = merge_json_value(&sub_k, prev, sub_v);
                        a.insert(sub_k, merged);
                    }
                    None => {
                        a.insert(sub_k, sub_v);
                    }
                }
            }
            Value::Object(a)
        }
        (Value::Array(mut a), Value::Array(b)) if COMPILER_UNION_KEYS.contains(&key) => {
            for elem in b {
                if !a.contains(&elem) {
                    a.push(elem);
                }
            }
            Value::Array(a)
        }
        (_, rhs_val) => rhs_val,
    }
}

/// Merges a JSON object `overlay` into `target` using [`merge_json_value`].
pub fn merge_json_object_into(target: &mut Map<String, Value>, overlay: Map<String, Value>) {
    for (k, v) in overlay {
        match target.remove(&k) {
            Some(prev) => {
                let merged = merge_json_value(&k, prev, v);
                target.insert(k, merged);
            }
            None => {
                target.insert(k, v);
            }
        }
    }
}

/// Performs a pure recursive object merge (`jq`'s `$a * $b`) where arrays overwrite.
#[must_use]
pub fn recursive_object_multiply(lhs: &Map<String, Value>, rhs: &Map<String, Value>) -> Map<String, Value> {
    let mut out = lhs.clone();
    for (k, v) in rhs {
        match (out.get_mut(k), v) {
            (Some(Value::Object(a_sub)), Value::Object(b_sub)) => {
                let merged = recursive_object_multiply(a_sub, b_sub);
                *a_sub = merged;
            }
            _ => {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    out
}

/// Compiles settings across resolved modules for the specified [`SettingsScope`],
/// applying an optional override map last.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if any module ID in `resolved_modules` is unknown.
pub fn compile_settings(
    catalog: &WorkspaceCatalog,
    scope: SettingsScope,
    resolved_modules: &[String],
    override_map: Option<&Map<String, Value>>,
) -> VspResult<Map<String, Value>> {
    let mut acc = Map::new();

    for mod_id in resolved_modules {
        let def = catalog
            .modules
            .get(mod_id)
            .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{mod_id}'")))?;

        if matches!(scope, SettingsScope::Workspace | SettingsScope::Both) {
            if let Some(ws) = &def.workspace_settings {
                merge_json_object_into(&mut acc, ws.clone());
            }
        }
        if matches!(scope, SettingsScope::Profile | SettingsScope::Both) {
            if let Some(prof) = &def.profile_settings {
                merge_json_object_into(&mut acc, prof.clone());
            }
        }
    }

    if let Some(ov) = override_map {
        let mut clean_ov = ov.clone();
        clean_ov.remove("$schema");
        clean_ov.remove("@extends");
        if !clean_ov.is_empty() {
            merge_json_object_into(&mut acc, clean_ov);
        }
    }

    Ok(acc)
}

/// Compiles canonical UX-agnostic merged profile settings matching `_merged/<name>.json`.
///
/// Resolves the recipe with `ux = default`, merges `workspace` + `profile` fragments,
/// applies combined recipe overrides, and strips all 15 symmetric managed UX keys.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] if the profile does not exist, or
/// [`VspError::UsageOrConfig`] if resolution fails.
pub fn compile_canonical_profile_settings(
    catalog: &WorkspaceCatalog,
    profile_name: &str,
) -> VspResult<Map<String, Value>> {
    let recipe = catalog.profiles.get(profile_name).ok_or_else(|| {
        VspError::TargetMissing(format!("unknown profile recipe '{profile_name}'"))
    })?;
    let resolved = resolve_profile_recipe(
        catalog,
        profile_name,
        &[],
        &[],
        Some(UxPreset::Default),
        None,
    )?;
    let (ov_ws, ov_prof) = recipe.split_overrides();
    let combined_ov = recursive_object_multiply(&ov_ws, &ov_prof);
    let mut merged = compile_settings(
        catalog,
        SettingsScope::Both,
        &resolved,
        Some(&combined_ov),
    )?;
    for ux_key in catalog.ux_managed_keys() {
        merged.remove(&ux_key);
    }
    Ok(merged)
}

/// Compiles profile extensions in exact resolved-module order (`workspace[]` then `profile[]`),
/// case-insensitively deduplicated, matching `profiles/<name>/extensions.json`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if any resolved module ID is unknown.
pub fn compile_profile_extensions(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
) -> VspResult<Vec<VsCodeExtensionEntry>> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<VsCodeExtensionEntry> = Vec::new();

    for mod_id in resolved_modules {
        let def = catalog
            .modules
            .get(mod_id)
            .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{mod_id}'")))?;
        for ext_id in def
            .manifest
            .extensions
            .workspace
            .iter()
            .chain(def.manifest.extensions.profile.iter())
        {
            let lc = ext_id.to_ascii_lowercase();
            if seen.insert(lc) {
                out.push(VsCodeExtensionEntry {
                    identifier: VsCodeExtensionIdentifier { id: ext_id.clone() },
                });
            }
        }
    }

    Ok(out)
}

/// Compiles workspace recommendations and unwanted recommendations matching
/// `compiler_compile_extensions workspace` in `scripts/lib/compiler.sh`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if any resolved module ID is unknown.
pub fn compile_workspace_extensions(
    catalog: &WorkspaceCatalog,
    resolved_modules: &[String],
) -> VspResult<WorkspaceExtensionsFile> {
    let mut rec_seen: BTreeSet<String> = BTreeSet::new();
    let mut recs: Vec<String> = Vec::new();

    let mut unw_seen: BTreeSet<String> = BTreeSet::new();
    let mut unwanted: Vec<String> = Vec::new();

    for mod_id in resolved_modules {
        let def = catalog
            .modules
            .get(mod_id)
            .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{mod_id}'")))?;

        for ext_id in &def.manifest.extensions.workspace {
            let lc = ext_id.to_ascii_lowercase();
            if rec_seen.insert(lc) {
                recs.push(ext_id.clone());
            }
        }

        let is_ws_active = !def.manifest.extensions.workspace.is_empty()
            || def.manifest.settings.workspace.is_some();
        if is_ws_active {
            for ext_id in &def.manifest.extensions.unwanted {
                let lc = ext_id.to_ascii_lowercase();
                if unw_seen.insert(lc) {
                    unwanted.push(ext_id.clone());
                }
            }
        }
    }

    let recommendations: Vec<String> = recs
        .into_iter()
        .filter(|r| !unw_seen.contains(&r.to_ascii_lowercase()))
        .collect();

    Ok(WorkspaceExtensionsFile {
        recommendations,
        unwanted_recommendations: unwanted,
    })
}

/// Orders extension IDs deterministically by [`GROUP_ORDER`] while preserving
/// relative order within each group.
#[must_use]
pub fn order_extensions_by_group(catalog: &WorkspaceCatalog, ids: &[String]) -> Vec<String> {
    let mut ordered = Vec::with_capacity(ids.len());
    let mut placed = vec![false; ids.len()];

    for grp in GROUP_ORDER {
        for (idx, id) in ids.iter().enumerate() {
            if !placed[idx] && catalog.extension_group(id).eq_ignore_ascii_case(grp) {
                ordered.push(id.clone());
                placed[idx] = true;
            }
        }
    }
    for (idx, id) in ids.iter().enumerate() {
        if !placed[idx] {
            ordered.push(id.clone());
        }
    }
    ordered
}

/// Compiles `tasks.json` across resolved modules, merging tasks by `.label`.
///
/// Returns `Ok(None)` when no resolved module contributes a tasks file.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if any resolved module ID is unknown.
pub fn compile_tasks(
    catalog: &WorkspaceCatalog,
    mode: RtkTaskMode,
    resolved_modules: &[String],
) -> VspResult<Option<Value>> {
    let mut tasks: Vec<Value> = Vec::new();
    let mut any_file = false;

    for mod_id in resolved_modules {
        let def = catalog
            .modules
            .get(mod_id)
            .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{mod_id}'")))?;
        if let Some(doc) = &def.tasks_doc {
            any_file = true;
            if let Some(arr) = doc.get("tasks").and_then(Value::as_array) {
                for task in arr {
                    let label = task.get("label").and_then(Value::as_str);
                    if let Some(lbl) = label {
                        if let Some(pos) = tasks
                            .iter()
                            .position(|t| t.get("label").and_then(Value::as_str) == Some(lbl))
                        {
                            tasks[pos] = task.clone();
                        } else {
                            tasks.push(task.clone());
                        }
                    } else {
                        tasks.push(task.clone());
                    }
                }
            }
        }
    }

    if !any_file {
        return Ok(None);
    }

    if mode == RtkTaskMode::Rtk {
        for task in &mut tasks {
            if let Some(obj) = task.as_object_mut() {
                if let Some(cmd) = obj.get("command").and_then(Value::as_str) {
                    if !cmd.starts_with("rtk ") {
                        let prefixed = format!("rtk {cmd}");
                        obj.insert("command".to_string(), Value::String(prefixed));
                    }
                }
            }
        }
    }

    let mut root = Map::new();
    root.insert("version".to_string(), Value::String("2.0.0".to_string()));
    root.insert("tasks".to_string(), Value::Array(tasks));
    Ok(Some(Value::Object(root)))
}

/// Compiles a deterministic UX-agnostic [`CodeProfileBundle`] for a canonical profile.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] if the profile does not exist, or
/// [`VspError::UsageOrConfig`] on resolution error.
pub fn compile_code_profile_bundle(
    catalog: &WorkspaceCatalog,
    profile_name: &str,
) -> VspResult<CodeProfileBundle> {
    let resolved = resolve_profile_recipe(
        catalog,
        profile_name,
        &[],
        &[],
        Some(UxPreset::Default),
        None,
    )?;
    let settings = compile_canonical_profile_settings(catalog, profile_name)?;
    let ext_entries = compile_profile_extensions(catalog, &resolved)?;
    let enabled = ext_entries
        .into_iter()
        .map(|entry| entry.identifier.id)
        .collect();

    Ok(CodeProfileBundle {
        name: profile_name.to_string(),
        settings,
        extensions: BundleExtensions { enabled },
    })
}

/// Formats any serializable value as canonical 2-space pretty JSON with a single trailing `\n`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if JSON serialization fails.
pub fn to_canonical_json_string<T: serde::Serialize>(value: &T) -> VspResult<String> {
    let mut s = serde_json::to_string_pretty(value)
        .map_err(|err| VspError::Operational(format!("JSON serialization error: {err}")))?;
    s.push('\n');
    Ok(s)
}

/// Computes the lowercase 64-character hex SHA-256 digest of a byte slice.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Emits deterministic `.code-profile` bundles for the requested profiles (or all 15
/// canonical profiles when `names` is empty) plus an `LC_ALL=C` sorted `SHA256SUMS` file.
///
/// Legacy profile names (such as `ai-profile-crisp`) resolve to their canonical profile
/// (`ai-copilot`).
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] if a requested profile name does not exist, or
/// [`VspError::Operational`] on filesystem errors.
pub fn export_profiles(
    catalog: &WorkspaceCatalog,
    names: &[String],
    out_dir: &Path,
    include_legacy_aliases: bool,
) -> VspResult<DistReport> {
    fs::create_dir_all(out_dir).map_err(|err| {
        VspError::Operational(format!(
            "failed to create output directory {}: {err}",
            out_dir.display()
        ))
    })?;

    let target_profiles: Vec<String> = if names.is_empty() {
        catalog.profiles.keys().cloned().collect()
    } else {
        let mut selected = Vec::new();
        for raw in names {
            let canonical = resolve_legacy_profile_name(raw)
                .map(|(c, _)| c.to_string())
                .unwrap_or_else(|| raw.clone());
            if !catalog.profiles.contains_key(&canonical) {
                return Err(VspError::TargetMissing(format!(
                    "unknown profile '{raw}'"
                )));
            }
            push_unique(&mut selected, &canonical);
        }
        selected
    };

    let mut emitted_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut canonical_bundles = Vec::new();
    let mut alias_bundles = Vec::new();

    for profile in &target_profiles {
        let bundle = compile_code_profile_bundle(catalog, profile)?;
        let json = to_canonical_json_string(&bundle)?;
        let bytes = json.into_bytes();
        let fname = format!("{profile}.code-profile");
        let dest = out_dir.join(&fname);
        fs::write(&dest, &bytes).map_err(|err| {
            VspError::Operational(format!("failed to write {}: {err}", dest.display()))
        })?;
        canonical_bundles.push(fname.clone());
        emitted_files.push((fname, bytes));
    }

    if include_legacy_aliases {
        for &(alias_file, canonical_file) in LEGACY_BUNDLE_ALIASES {
            if let Some((_, canonical_bytes)) =
                emitted_files.iter().find(|(name, _)| name == canonical_file)
            {
                let alias_bytes = canonical_bytes.clone();
                let dest = out_dir.join(alias_file);
                fs::write(&dest, &alias_bytes).map_err(|err| {
                    VspError::Operational(format!(
                        "failed to write legacy alias {}: {err}",
                        dest.display()
                    ))
                })?;
                alias_bundles.push(alias_file.to_string());
                emitted_files.push((alias_file.to_string(), alias_bytes));
            }
        }
    }

    // Sort strictly by filename in UTF-8/ASCII byte order (LC_ALL=C).
    emitted_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut checksums = Vec::with_capacity(emitted_files.len());
    let mut sums_buf = String::new();
    for (fname, bytes) in &emitted_files {
        let digest = sha256_hex(bytes);
        sums_buf.push_str(&digest);
        sums_buf.push_str("  ");
        sums_buf.push_str(fname);
        sums_buf.push('\n');
        checksums.push((digest, fname.clone()));
    }

    let sha256sums_path = out_dir.join("SHA256SUMS");
    fs::write(&sha256sums_path, sums_buf.as_bytes()).map_err(|err| {
        VspError::Operational(format!(
            "failed to write {}: {err}",
            sha256sums_path.display()
        ))
    })?;

    Ok(DistReport {
        out_dir: out_dir.to_path_buf(),
        canonical_bundles,
        alias_bundles,
        sha256sums_path,
        checksums,
    })
}

/// Builds the full deterministic release distribution in `out_dir`:
/// 15 canonical `.code-profile` bundles + 22 materialized legacy `.code-profile`
/// alias files + `LC_ALL=C` sorted `SHA256SUMS`.
///
/// # Errors
///
/// Returns [`VspError`] if compilation or filesystem output fails.
pub fn build_dist(catalog: &WorkspaceCatalog, out_dir: &Path) -> VspResult<DistReport> {
    export_profiles(catalog, &[], out_dir, true)
}

fn split_csv_items(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for entry in raw {
        for part in entry.split([',', '\n']) {
            let trimmed = part.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|x| x == item) {
        list.push(item.to_string());
    }
}

fn pair_conflicts(catalog: &WorkspaceCatalog, a: &str, b: &str) -> bool {
    let a_conf = catalog
        .modules
        .get(a)
        .is_some_and(|m| m.manifest.conflicts.iter().any(|c| c == b));
    let b_conf = catalog
        .modules
        .get(b)
        .is_some_and(|m| m.manifest.conflicts.iter().any(|c| c == a));
    a_conf || b_conf
}

fn dfs_visit(
    catalog: &WorkspaceCatalog,
    id: &str,
    visiting: &mut Vec<String>,
    requester: Option<&str>,
    excluded: &[String],
    visited: &mut Vec<String>,
) -> VspResult<()> {
    if !is_valid_module_id(id) {
        return Err(VspError::UsageOrConfig(format!(
            "invalid module id '{id}'"
        )));
    }
    if excluded.iter().any(|e| e == id) {
        if let Some(req) = requester {
            return Err(VspError::UsageOrConfig(format!(
                "module '{req}' requires excluded module '{id}'"
            )));
        }
        return Ok(());
    }
    if visiting.iter().any(|v| v == id) {
        return Err(VspError::UsageOrConfig(format!(
            "dependency cycle detected involving '{id}'"
        )));
    }
    if visited.iter().any(|v| v == id) {
        return Ok(());
    }

    let def = catalog
        .modules
        .get(id)
        .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{id}'")))?;

    visiting.push(id.to_string());
    for req in &def.manifest.requires {
        dfs_visit(catalog, req, visiting, Some(id), excluded, visited)?;
    }
    visiting.pop();
    visited.push(id.to_string());
    Ok(())
}

fn can_add_default_companion(
    catalog: &WorkspaceCatalog,
    candidate: &str,
    current_set: &[String],
    excluded: &[String],
) -> bool {
    if excluded.iter().any(|e| e == candidate) || current_set.iter().any(|m| m == candidate) {
        return false;
    }
    let mut closure = Vec::new();
    if dfs_visit(
        catalog,
        candidate,
        &mut Vec::new(),
        None,
        excluded,
        &mut closure,
    )
    .is_err()
    {
        return false;
    }
    for c in &closure {
        for m in current_set {
            if pair_conflicts(catalog, c, m) {
                return false;
            }
        }
    }
    true
}

fn module_sort_key(catalog: &WorkspaceCatalog, id: &str) -> VspResult<String> {
    let def = catalog
        .modules
        .get(id)
        .ok_or_else(|| VspError::UsageOrConfig(format!("unknown module '{id}'")))?;

    if id == "core-editor" {
        return Ok(format!("00:000:{id}"));
    }
    if def.manifest.kind == ModuleKind::Ux {
        return Ok(format!("40:000:{id}"));
    }
    if let Some(pos) = catalog.defaults.capabilities.iter().position(|d| d == id) {
        return Ok(format!("01:{:03}:{id}", pos + 1));
    }
    if def.manifest.kind == ModuleKind::Toolchain {
        return match &def.manifest.detect {
            None => {
                if id.ends_with("-base") {
                    Ok(format!("10:000:{id}"))
                } else {
                    Ok(format!("25:050:{id}"))
                }
            }
            Some(det) => Ok(format!("20:{:03}:{id}", det.priority)),
        };
    }
    Ok(format!("30:000:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_loader::load_legacy_catalog;
    use serde_json::json;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn order_extensions_by_group_follows_group_order() {
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let input = vec![
            "editorconfig.editorconfig".to_string(), // General
            "rust-lang.rust-analyzer".to_string(),   // Rust
            "github.copilot-chat".to_string(),       // AI
            "enkia.tokyo-night".to_string(),         // Themes
        ];
        let ordered = order_extensions_by_group(&catalog, &input);
        assert_eq!(
            ordered,
            vec![
                "github.copilot-chat",
                "enkia.tokyo-night",
                "rust-lang.rust-analyzer",
                "editorconfig.editorconfig",
            ]
        );
    }

    #[test]
    fn compile_tasks_supports_plain_and_rtk_modes() {
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let mods = vec!["rust".to_string()];

        let plain = compile_tasks(&catalog, RtkTaskMode::Plain, &mods)
            .expect("plain tasks")
            .expect("rust tasks");
        let rtk = compile_tasks(&catalog, RtkTaskMode::Rtk, &mods)
            .expect("rtk tasks")
            .expect("rust rtk tasks");

        let plain_cmd = plain["tasks"][0]["command"].as_str().expect("cmd");
        let rtk_cmd = rtk["tasks"][0]["command"].as_str().expect("rtk cmd");
        assert!(!plain_cmd.starts_with("rtk "));
        assert_eq!(rtk_cmd, format!("rtk {plain_cmd}"));
    }

    #[test]
    fn recursive_object_multiply_deep_merges_objects() {
        let mut lhs = Map::new();
        lhs.insert("a".to_string(), json!({"x": 1, "y": 2}));
        let mut rhs = Map::new();
        rhs.insert("a".to_string(), json!({"y": 3, "z": 4}));
        let out = recursive_object_multiply(&lhs, &rhs);
        assert_eq!(out["a"], json!({"x": 1, "y": 3, "z": 4}));
    }
}

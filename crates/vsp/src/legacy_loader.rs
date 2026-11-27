//! Offline reader for the legacy repository layout (`catalog/`, `modules/`, `profiles/`).
//!
//! Parses JSONC strictly via `jsonc-parser`, resolves catalog roots safely without
//! trusting arbitrary working directories, and validates directory/manifest identity
//! invariants during discovery.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::exit_codes::{VspError, VspResult};
pub use crate::jsonc::{
    parse_jsonc_settings_object, parse_jsonc_value, read_strict_json, read_strict_json_value,
};
use crate::model::{
    is_valid_module_id, DefaultsManifest, ExtensionCatalog, ExtensionPinsManifest,
    ModuleDefinition, ModuleKind, ModuleManifest, PinsLoadState, ProfileRecipe, PublishersManifest,
    SettingScopesManifest, WorkspaceCatalog,
};
use crate::provenance::check_path_contained;

/// Returns true if `dir` looks like a complete legacy catalog root containing
/// `catalog/extensions.json`, `modules/`, and `profiles/`.
#[must_use]
pub fn is_valid_legacy_root(dir: &Path) -> bool {
    dir.join("catalog/extensions.json").is_file()
        && dir.join("modules").is_dir()
        && dir.join("profiles").is_dir()
}

/// Resolves the catalog repository root safely.
///
/// Resolution priority:
/// 1. Explicit `--catalog <DIR>` CLI flag (missing path exits `2`; invalid layout exits `4`).
/// 2. `VSPCLI_ROOT` environment variable (when non-empty).
/// 3. Ancestors of the currently executing `vsp` binary (e.g. `<repo>/target/debug/vsp`).
/// 4. Current working directory (or its git/repo ancestor) ONLY when it contains the full
///    legacy layout (`catalog/extensions.json`, `modules/`, and `profiles/`).
///
/// This prevents accidentally loading an untrusted `catalog/extensions.json` from an
/// unrelated working directory when `--catalog` is omitted.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] if an explicit `--catalog` directory does not exist,
/// or [`VspError::UsageOrConfig`] if no valid catalog root can be located.
pub fn resolve_catalog_root(cli_catalog: Option<&Path>) -> VspResult<PathBuf> {
    if let Some(explicit) = cli_catalog {
        if !explicit.exists() {
            return Err(VspError::TargetMissing(format!(
                "catalog directory not found: {}",
                explicit.display()
            )));
        }
        let canon = fs::canonicalize(explicit).map_err(|err| {
            VspError::TargetMissing(format!(
                "cannot resolve catalog directory {}: {err}",
                explicit.display()
            ))
        })?;
        if !canon.join("catalog/extensions.json").is_file() {
            return Err(VspError::UsageOrConfig(format!(
                "invalid catalog root {} (missing catalog/extensions.json)",
                canon.display()
            )));
        }
        return Ok(canon);
    }

    if let Ok(env_root) = std::env::var("VSPCLI_ROOT") {
        let trimmed = env_root.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if !p.exists() {
                return Err(VspError::TargetMissing(format!(
                    "VSPCLI_ROOT directory not found: {}",
                    p.display()
                )));
            }
            let canon = fs::canonicalize(&p).map_err(|err| {
                VspError::TargetMissing(format!(
                    "cannot resolve VSPCLI_ROOT {}: {err}",
                    p.display()
                ))
            })?;
            if !canon.join("catalog/extensions.json").is_file() {
                return Err(VspError::UsageOrConfig(format!(
                    "VSPCLI_ROOT {} is missing catalog/extensions.json",
                    canon.display()
                )));
            }
            return Ok(canon);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let exe_canon = fs::canonicalize(&exe).unwrap_or(exe);
        for ancestor in exe_canon.ancestors().skip(1) {
            if is_valid_legacy_root(ancestor) {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_canon = fs::canonicalize(&cwd).unwrap_or(cwd);
        for ancestor in cwd_canon.ancestors() {
            if is_valid_legacy_root(ancestor) {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    Err(VspError::UsageOrConfig(
        "could not locate catalog root (pass --catalog <DIR> or set VSPCLI_ROOT)".to_string(),
    ))
}

/// Loads the complete legacy catalog (`catalog/`, `modules/`, `profiles/`) from `root`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] on module discovery/identity violations or
/// [`VspError::Operational`] on unreadable or malformed catalog JSON/JSONC files.
pub fn load_legacy_catalog(root: &Path) -> VspResult<WorkspaceCatalog> {
    let canon_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let catalog_dir = canon_root.join("catalog");

    // Each file is checked against the catalog root before it is read. A symbolic link or
    // a `..` path that leaves the root is refused (exit 5), as in the v2.2 loader.
    for name in [
        "defaults.json",
        "extensions.json",
        "publishers.json",
        "extension-pins.json",
        "setting-scopes.json",
    ] {
        verify_inside_root(&canon_root, &catalog_dir.join(name))?;
    }

    let defaults_path = catalog_dir.join("defaults.json");
    let defaults: DefaultsManifest = if defaults_path.is_file() {
        read_strict_json(&defaults_path)?
    } else {
        DefaultsManifest {
            version: 1,
            capabilities: Vec::new(),
            ux: "crisp".to_string(),
        }
    };

    let extensions_path = catalog_dir.join("extensions.json");
    if !extensions_path.is_file() {
        return Err(VspError::TargetMissing(format!(
            "missing catalog extensions manifest: {}",
            extensions_path.display()
        )));
    }
    let extensions: ExtensionCatalog = read_strict_json(&extensions_path)?;

    let publishers_path = catalog_dir.join("publishers.json");
    let publishers: PublishersManifest = if publishers_path.is_file() {
        read_strict_json(&publishers_path)?
    } else {
        PublishersManifest {
            version: 1,
            publishers: Vec::new(),
        }
    };

    let pins_path = std::env::var("ARTAGON_VSCODE_PINS_FILE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| catalog_dir.join("extension-pins.json"), PathBuf::from);
    let pins = load_pins_state(&pins_path);

    let scopes_path = catalog_dir.join("setting-scopes.json");
    let setting_scopes = if scopes_path.is_file() {
        Some(read_strict_json::<SettingScopesManifest>(&scopes_path)?)
    } else {
        None
    };

    let modules = discover_modules(&canon_root)?;
    let profiles = discover_profiles(&canon_root)?;

    Ok(WorkspaceCatalog {
        root: canon_root,
        defaults,
        // The legacy layout has no `blockedPublishers` list. Publishers of compromised and
        // removed extensions are still blocked.
        blocked_publishers: crate::model::derive_blocked_publishers(&extensions, &[]),
        extensions,
        publishers,
        pins,
        setting_scopes,
        modules,
        profiles,
        legacy_aliases: BTreeMap::new(),
        policy: None,
        retire_on_switch: BTreeMap::new(),
        lockfile: None,
        group_order: Vec::new(),
    })
}

/// Checks that `path` stays inside the catalog `root` after symbolic links and `..` are
/// resolved. A path that does not exist yet is accepted when its resolved location is
/// inside the root.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) when `path` leaves `root`.
fn verify_inside_root(root: &Path, path: &Path) -> VspResult<()> {
    check_path_contained(root, path).map(|_| ()).map_err(|err| {
        VspError::SafetyRefusal(format!(
            "legacy catalog file escapes the catalog root {}: {err}",
            root.display()
        ))
    })
}

/// Loads `extension-pins.json` into a [`PinsLoadState`] without silently ignoring errors.
#[must_use]
pub fn load_pins_state(path: &Path) -> PinsLoadState {
    if !path.is_file() {
        return PinsLoadState::Missing(path.to_path_buf());
    }
    match fs::read_to_string(path) {
        Err(err) => PinsLoadState::Invalid {
            path: path.to_path_buf(),
            error: err.to_string(),
        },
        Ok(raw) => match serde_json::from_str::<ExtensionPinsManifest>(&raw) {
            Ok(manifest) => PinsLoadState::Loaded(manifest),
            Err(err) => PinsLoadState::Invalid {
                path: path.to_path_buf(),
                error: err.to_string(),
            },
        },
    }
}

/// Discovers and loads all modules under `modules/{capabilities,toolchains,ux}/`.
///
/// Mirrors `discover_modules` in `scripts/lib/module-resolver.sh`, enforcing valid IDs,
/// `manifest.id == dirname`, and uniqueness across all module categories.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if a module directory is malformed, has an invalid
/// or mismatched ID, or duplicates another module ID.
pub fn discover_modules(root: &Path) -> VspResult<BTreeMap<String, ModuleDefinition>> {
    let mut modules = BTreeMap::new();

    for kind in [
        ModuleKind::Capability,
        ModuleKind::Toolchain,
        ModuleKind::Ux,
    ] {
        let kind_dir = root.join("modules").join(kind.dir_name());
        if !kind_dir.is_dir() {
            continue;
        }
        let mut subdirs: Vec<PathBuf> = fs::read_dir(&kind_dir)
            .map_err(|err| {
                VspError::Operational(format!("failed to list {}: {err}", kind_dir.display()))
            })?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();

        for dir in subdirs {
            let base = dir
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| {
                    VspError::UsageOrConfig(format!(
                        "non-UTF-8 module directory name: {}",
                        dir.display()
                    ))
                })?
                .to_string();

            let manifest_path = dir.join("module.json");
            verify_inside_root(root, &dir)?;
            verify_inside_root(root, &manifest_path)?;
            if !manifest_path.is_file() {
                return Err(VspError::UsageOrConfig(format!(
                    "missing module.json in {}",
                    dir.display()
                )));
            }
            if !is_valid_module_id(&base) {
                return Err(VspError::UsageOrConfig(format!(
                    "invalid module directory name '{base}' (must match ^[a-z0-9][a-z0-9-]*$)"
                )));
            }

            let manifest: ModuleManifest = read_strict_json(&manifest_path)?;
            if manifest.id != base {
                return Err(VspError::UsageOrConfig(format!(
                    "module.json id '{}' does not match directory name '{}' in {}",
                    manifest.id,
                    base,
                    manifest_path.display()
                )));
            }
            if modules.contains_key(&manifest.id) {
                return Err(VspError::UsageOrConfig(format!(
                    "duplicate module id '{}' across modules/*/",
                    manifest.id
                )));
            }

            // Fragment paths come from the manifest, so they are checked too.
            for rel in [
                &manifest.settings.workspace,
                &manifest.settings.profile,
                &manifest.tasks,
            ]
            .into_iter()
            .flatten()
            .filter(|rel| !rel.is_empty())
            {
                verify_inside_root(root, &dir.join(rel))?;
            }

            let workspace_settings = match &manifest.settings.workspace {
                Some(rel) if !rel.is_empty() => {
                    let p = dir.join(rel);
                    if p.is_file() {
                        Some(parse_jsonc_settings_object(&p)?)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let profile_settings = match &manifest.settings.profile {
                Some(rel) if !rel.is_empty() => {
                    let p = dir.join(rel);
                    if p.is_file() {
                        Some(parse_jsonc_settings_object(&p)?)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let tasks_doc = match &manifest.tasks {
                Some(rel) if !rel.is_empty() => {
                    let p = dir.join(rel);
                    if p.is_file() {
                        Some(read_strict_json_value(&p, false)?)
                    } else {
                        None
                    }
                }
                _ => None,
            };

            let id = manifest.id.clone();
            modules.insert(
                id,
                ModuleDefinition {
                    manifest,
                    dir,
                    workspace_settings,
                    profile_settings,
                    tasks_doc,
                },
            );
        }
    }

    Ok(modules)
}

/// Discovers and loads all canonical profile recipes under `profiles/*/profile.json`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if a profile recipe cannot be read or parsed.
pub fn discover_profiles(root: &Path) -> VspResult<BTreeMap<String, ProfileRecipe>> {
    let mut profiles = BTreeMap::new();
    let profiles_dir = root.join("profiles");
    if !profiles_dir.is_dir() {
        return Ok(profiles);
    }

    let mut subdirs: Vec<PathBuf> = fs::read_dir(&profiles_dir)
        .map_err(|err| {
            VspError::Operational(format!("failed to list {}: {err}", profiles_dir.display()))
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();

    for dir in subdirs {
        let recipe_path = dir.join("profile.json");
        verify_inside_root(root, &dir)?;
        verify_inside_root(root, &recipe_path)?;
        if !recipe_path.is_file() {
            continue;
        }
        let base = dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                VspError::Operational(format!("non-UTF-8 profile directory: {}", dir.display()))
            })?
            .to_string();
        let recipe: ProfileRecipe = read_strict_json(&recipe_path)?;
        profiles.insert(base, recipe);
    }

    Ok(profiles)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    const MODULE_JSON: &str = r#"{"version":1,"id":"demo","kind":"capability","description":"d"}"#;

    /// Builds a minimal legacy catalog root and returns `(tempdir, root, outside)`.
    fn minimal_root() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new().expect("tmp");
        let root = tmp.path().join("repo");
        let outside = tmp.path().join("outside");
        fs::create_dir_all(root.join("catalog")).expect("mkdir catalog");
        fs::create_dir_all(root.join("modules/capabilities/demo")).expect("mkdir module");
        fs::create_dir_all(root.join("profiles")).expect("mkdir profiles");
        fs::create_dir_all(&outside).expect("mkdir outside");
        fs::write(
            root.join("catalog/extensions.json"),
            r#"{"version":1,"extensions":{}}"#,
        )
        .expect("write extensions");
        fs::write(
            root.join("modules/capabilities/demo/module.json"),
            MODULE_JSON,
        )
        .expect("write module");
        (tmp, root, outside)
    }

    /// Opus L10: each legacy-layout file must stay inside the catalog root.
    #[test]
    fn legacy_loader_rejects_files_that_leave_the_catalog_root() {
        let (_tmp, root, _outside) = minimal_root();
        let catalog = load_legacy_catalog(&root).expect("clean root loads");
        assert!(catalog.modules.contains_key("demo"));
        assert!(catalog.legacy_aliases.is_empty());

        // Catalog file that is a symbolic link to a file outside the root.
        let (_tmp, root, outside) = minimal_root();
        fs::write(
            outside.join("publishers.json"),
            r#"{"version":1,"publishers":["evil"]}"#,
        )
        .expect("write outside");
        symlink(
            outside.join("publishers.json"),
            root.join("catalog/publishers.json"),
        )
        .expect("symlink");
        let err = load_legacy_catalog(&root).expect_err("linked catalog file");
        assert_eq!(err.exit_code().as_i32(), 5, "{err}");

        // Module directory that is a symbolic link to a directory outside the root.
        let (_tmp, root, outside) = minimal_root();
        fs::create_dir_all(outside.join("evil")).expect("mkdir evil");
        fs::write(
            outside.join("evil/module.json"),
            r#"{"version":1,"id":"evil","kind":"capability"}"#,
        )
        .expect("write evil module");
        symlink(outside.join("evil"), root.join("modules/capabilities/evil")).expect("symlink");
        let err = load_legacy_catalog(&root).expect_err("linked module dir");
        assert_eq!(err.exit_code().as_i32(), 5, "{err}");

        // Settings fragment path with `..` that leaves the root.
        let (_tmp, root, outside) = minimal_root();
        fs::write(outside.join("settings.jsonc"), r#"{"a.b": 1}"#).expect("write fragment");
        fs::write(
            root.join("modules/capabilities/demo/module.json"),
            r#"{"version":1,"id":"demo","kind":"capability",
                "settings":{"profile":"../../../../outside/settings.jsonc"}}"#,
        )
        .expect("write module");
        let err = load_legacy_catalog(&root).expect_err("fragment outside root");
        assert_eq!(err.exit_code().as_i32(), 5, "{err}");

        // Profile recipe directory that is a symbolic link to a directory outside the root.
        let (_tmp, root, outside) = minimal_root();
        fs::create_dir_all(outside.join("p")).expect("mkdir p");
        fs::write(outside.join("p/profile.json"), r#"{"name":"p"}"#).expect("write recipe");
        symlink(outside.join("p"), root.join("profiles/p")).expect("symlink");
        let err = load_legacy_catalog(&root).expect_err("linked profile dir");
        assert_eq!(err.exit_code().as_i32(), 5, "{err}");
    }
}

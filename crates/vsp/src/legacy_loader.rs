//! Offline reader for the legacy repository layout (`catalog/`, `modules/`, `profiles/`).
//!
//! Parses JSONC strictly via `jsonc-parser`, resolves catalog roots safely without
//! trusting arbitrary working directories, and validates directory/manifest identity
//! invariants during discovery.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use jsonc_parser::ParseOptions;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::exit_codes::{VspError, VspResult};
use crate::model::{
    is_valid_module_id, DefaultsManifest, ExtensionCatalog, ExtensionPinsManifest,
    ModuleDefinition, ModuleKind, ModuleManifest, PinsLoadState, ProfileRecipe, PublishersManifest,
    SettingScopesManifest, WorkspaceCatalog,
};

/// Parses a JSONC string strictly as a single JSON value.
///
/// Allows `//` and `/* */` comments and trailing commas while rejecting empty
/// documents, trailing tokens, and syntax errors.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if the input fails JSONC parsing or contains no value.
pub fn parse_jsonc_value(input: &str, label: &str) -> VspResult<Value> {
    let options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
    };
    let parsed = jsonc_parser::parse_to_serde_value(input, &options)
        .map_err(|err| VspError::Operational(format!("{label}: JSONC parse error: {err}")))?;
    parsed.ok_or_else(|| {
        VspError::Operational(format!("{label}: empty JSONC document (expected a value)"))
    })
}

/// Parses a JSONC file strictly as a single JSON object and strips `$schema` and `@extends`.
///
/// Matches `compiler_parse_jsonc_object` in `scripts/lib/compiler.sh`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if the file cannot be read, fails JSONC parsing,
/// or does not have an object root.
pub fn parse_jsonc_settings_object(path: &Path) -> VspResult<Map<String, Value>> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    let val = parse_jsonc_value(&raw, &path.display().to_string())?;
    let mut map = match val {
        Value::Object(m) => m,
        _ => {
            return Err(VspError::Operational(format!(
                "{}: expected a single JSON object",
                path.display()
            )));
        }
    };
    map.remove("$schema");
    map.remove("@extends");
    Ok(map)
}

/// Parses a strict `.json` file (comments and trailing commas disallowed) into a typed value.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if reading or JSON deserialization fails.
pub fn read_strict_json<T: DeserializeOwned>(path: &Path) -> VspResult<T> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    serde_json::from_str(&raw).map_err(|err| {
        VspError::Operational(format!("failed to parse strict JSON {}: {err}", path.display()))
    })
}

/// Parses a strict `.json` file into a raw [`Value`], verifying the expected root type.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if reading fails, JSON parsing fails, or the root
/// type does not match `expect_array`.
pub fn read_strict_json_value(path: &Path, expect_array: bool) -> VspResult<Value> {
    let raw = fs::read_to_string(path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", path.display()))
    })?;
    let val: Value = serde_json::from_str(&raw).map_err(|err| {
        VspError::Operational(format!("failed to parse strict JSON {}: {err}", path.display()))
    })?;
    if expect_array && !val.is_array() {
        return Err(VspError::Operational(format!(
            "{}: expected single JSON array",
            path.display()
        )));
    }
    if !expect_array && !val.is_object() {
        return Err(VspError::Operational(format!(
            "{}: expected single JSON object",
            path.display()
        )));
    }
    Ok(val)
}

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
        .map(PathBuf::from)
        .unwrap_or_else(|| catalog_dir.join("extension-pins.json"));
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
        extensions,
        publishers,
        pins,
        setting_scopes,
        modules,
        profiles,
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

    for kind in [ModuleKind::Capability, ModuleKind::Toolchain, ModuleKind::Ux] {
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

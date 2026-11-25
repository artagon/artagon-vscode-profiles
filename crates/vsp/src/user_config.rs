//! Personal user configuration (`${XDG_CONFIG_HOME:-~/.config}/vsp/config.jsonc`).
//!
//! The user configuration lets one person define personal defaults and custom profiles
//! without a fork of the catalog repository. It is JSONC with `version: 1`:
//!
//! ```jsonc
//! {
//!   "version": 1,
//!   "defaults": { "ux": "crisp", "ai": "copilot", "rtkTerminal": false },
//!   "profiles": {
//!     "my-rust": {
//!       "description": "Rust with my Git tools",
//!       "extends": "rust",
//!       "with": ["git-pro"],
//!       "without": [],
//!       "overrides": { "editor.fontSize": 14, "[rust]": { "editor.formatOnSave": true } },
//!       "extraExtensions": ["publisher.name"]
//!     }
//!   }
//! }
//! ```
//!
//! # Security invariants
//!
//! - **One source only.** [`load_user_config`] reads the explicit `--config <PATH>` file, or
//!   the XDG default file when it exists. It never reads `.vsp.toml`, `.vsp.jsonc`, or any
//!   other file from the current working directory.
//! - **No wildcard publisher trust.** `trust`, `publishers`, and `trustedPublishers` keys are
//!   rejected with exit `4`. Catalog-external extensions are allowed by exact ID only.
//! - **Dotted-key rule.** Each `overrides` key must be a literal VS Code setting ID (it
//!   contains `.`) or a `[language]` block. Bare keys such as `"chat"` are rejected with
//!   exit `4`, so a nested object cannot rebuild a denied setting path.
//! - **Autonomy deny-list.** [`check_autonomy_denylist`] runs on each override map. A hit is
//!   rejected with exit `5`, and no flag bypasses this check.
//! - **Exact-ID extra extensions.** `extraExtensions` entries must match `publisher.name`.
//!   Compromised and removed catalog IDs are rejected with exit `5`. Catalog-external IDs are
//!   accepted as Tier 3 [`ExtensionTrustTier::UserUnlocked`] and `vsp doctor` reports them.
//!
//! Personal `defaults` and `overrides` apply to personal targets only (named profiles and
//! the Default user profile). Team-shareable workspace files are never changed by them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::exit_codes::{VspError, VspResult};
use crate::legacy_loader::parse_jsonc_value;
use crate::model::{
    is_valid_module_id, resolve_legacy_profile_name, AiMode, ExtensionStatus, ProfileRecipe,
    UxPreset, WorkspaceCatalog,
};
use crate::policy::check_autonomy_denylist;
use crate::provenance::write_file_atomic;
use crate::resolver::{
    merge_json_object_into, recursive_object_multiply, resolve_profile_recipe, BundleAdditions,
};
use crate::vscode::resolve_xdg_config_home;
use crate::vsix::is_valid_extension_id_syntax;

/// Supported user configuration schema version (`"version": 1`).
pub const USER_CONFIG_VERSION: u64 = 1;

/// Stable label for catalog-external extensions that a user enabled by exact ID.
pub const USER_UNLOCKED_LABEL: &str = "user-unlocked";

/// Upper size limit for a user configuration file (1 MiB).
const MAX_USER_CONFIG_BYTES: u64 = 1024 * 1024;

/// Key names that would grant publisher-wide trust (compared after normalization).
const TRUST_KEYS: &[&str] = &["trust", "publishers", "trustedpublishers"];

/// Personal defaults from the `defaults` object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserDefaults {
    /// Default UX preset for personal targets (`defaults.ux`).
    pub ux: Option<UxPreset>,
    /// Default AI mode for personal targets (`defaults.ai`).
    pub ai: Option<AiMode>,
    /// `true` adds the `rtk-terminal` pack; `false` skips RTK wrapper installation.
    pub rtk_terminal: Option<bool>,
}

/// One custom profile from the `profiles` object.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserProfile {
    /// Profile name (the key in `profiles`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Catalog preset or pack that this profile extends.
    pub extends: Option<String>,
    /// Pack IDs to add.
    pub with: Vec<String>,
    /// Pack IDs to exclude.
    pub without: Vec<String>,
    /// Profile-scope setting overrides (literal setting IDs and `[language]` blocks).
    pub overrides: Map<String, Value>,
    /// Exact extension IDs to install in addition to the resolved packs.
    pub extra_extensions: Vec<String>,
}

/// Parsed and schema-validated user configuration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserConfig {
    /// File that the configuration was read from.
    pub path: PathBuf,
    /// Personal defaults.
    pub defaults: UserDefaults,
    /// Custom profiles indexed by name.
    pub profiles: BTreeMap<String, UserProfile>,
}

/// Supply-chain trust tier of one `extraExtensions` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ExtensionTrustTier {
    /// The catalog knows the extension. Publisher allowlist and VSIX pin checks apply.
    #[serde(rename = "catalog")]
    Catalog,
    /// Tier 3: catalog-external extension that the user enabled by exact ID.
    #[serde(rename = "user-unlocked")]
    UserUnlocked,
}

impl ExtensionTrustTier {
    /// Returns the stable label (`"catalog"` or `"user-unlocked"`).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::UserUnlocked => USER_UNLOCKED_LABEL,
        }
    }
}

/// One resolved `extraExtensions` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtraExtension {
    /// Lowercase extension ID (`publisher.name`).
    pub id: String,
    /// Trust tier of the extension.
    pub tier: ExtensionTrustTier,
}

/// Resolved data of one user profile after [`apply_user_config`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserProfileOverlay {
    /// Catalog preset or pack that the profile extends.
    pub extends: Option<String>,
    /// Validated profile-scope overrides (dotted-key rule and deny-list already applied).
    pub overrides: Map<String, Value>,
    /// Resolved extra extensions in declaration order.
    pub extra_extensions: Vec<ExtraExtension>,
}

/// Side data for user profiles that [`apply_user_config`] added to a catalog.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserOverlay {
    /// Resolved user profiles indexed by profile name.
    pub profiles: BTreeMap<String, UserProfileOverlay>,
}

impl UserOverlay {
    /// Returns `true` when `name` is a custom user profile.
    #[must_use]
    pub fn is_user_profile(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }

    /// Applies the overrides of user profile `name` as the last settings layer.
    ///
    /// The catalog compiler removes the managed UX keys (font, theme, and similar) and then
    /// applies the selected UX preset. A personal override of such a key must still win, so
    /// callers invoke this function after the UX step. Overridden keys are also removed
    /// from `delete_keys`. Unknown profile names change nothing.
    pub fn apply_final_overrides(
        &self,
        name: &str,
        desired_settings: &mut Map<String, Value>,
        delete_keys: &mut Vec<String>,
    ) {
        let Some(profile) = self.profiles.get(name) else {
            return;
        };
        if profile.overrides.is_empty() {
            return;
        }
        merge_json_object_into(desired_settings, profile.overrides.clone());
        delete_keys.retain(|key| !profile.overrides.contains_key(key));
    }

    /// Returns the bundle additions (overrides and extra extension IDs) of each user profile.
    #[must_use]
    pub fn bundle_additions(&self) -> BTreeMap<String, BundleAdditions> {
        self.profiles
            .iter()
            .map(|(name, p)| {
                (
                    name.clone(),
                    BundleAdditions {
                        settings: p.overrides.clone(),
                        extensions: p.extra_extensions.iter().map(|e| e.id.clone()).collect(),
                    },
                )
            })
            .collect()
    }

    /// Returns the resolved extra extensions of user profile `name` (empty when unknown).
    #[must_use]
    pub fn extras_for(&self, name: &str) -> &[ExtraExtension] {
        self.profiles
            .get(name)
            .map_or(&[], |p| p.extra_extensions.as_slice())
    }
}

/// Values that [`render_starter_config`] writes as active (uncommented) entries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StarterSeed {
    /// Active `defaults.ux` value.
    pub ux: Option<UxPreset>,
    /// Active `defaults.ai` value.
    pub ai: Option<AiMode>,
    /// Active `defaults.rtkTerminal` value.
    pub rtk_terminal: Option<bool>,
    /// Active custom profile entry.
    pub profile: Option<UserProfile>,
}

/// Returns the default user configuration path
/// (`${XDG_CONFIG_HOME:-~/.config}/vsp/config.jsonc`).
#[must_use]
pub fn default_user_config_path() -> PathBuf {
    resolve_xdg_config_home().join("vsp/config.jsonc")
}

/// Returns the explicit `--config <PATH>` value, or the default path when it is absent.
#[must_use]
pub fn resolve_user_config_path(cli_config: Option<&Path>) -> PathBuf {
    cli_config.map_or_else(default_user_config_path, Path::to_path_buf)
}

/// Loads the user configuration from exactly one location.
///
/// - With `cli_config`, only that file is read. A missing file is an error.
/// - Without `cli_config`, the XDG default file is read when it exists. `Ok(None)` is
///   returned when it does not exist.
///
/// Files in the current working directory (`.vsp.toml`, `.vsp.jsonc`) are never read.
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`): the explicit `--config` file does not exist.
/// - [`VspError::UsageOrConfig`] (exit `4`): unreadable, oversized, malformed, or
///   schema-invalid configuration, including wildcard trust keys and bare override keys.
/// - [`VspError::SafetyRefusal`] (exit `5`): an override matches the autonomy deny-list.
pub fn load_user_config(cli_config: Option<&Path>) -> VspResult<Option<UserConfig>> {
    let path = match cli_config {
        Some(explicit) => {
            if !explicit.is_file() {
                return Err(VspError::TargetMissing(format!(
                    "user config file not found: {} (from --config)",
                    explicit.display()
                )));
            }
            explicit.to_path_buf()
        }
        None => {
            let default_path = default_user_config_path();
            if !default_path.is_file() {
                return Ok(None);
            }
            default_path
        }
    };

    let meta = fs::metadata(&path).map_err(|err| {
        VspError::UsageOrConfig(format!("cannot read user config {}: {err}", path.display()))
    })?;
    if meta.len() > MAX_USER_CONFIG_BYTES {
        return Err(VspError::UsageOrConfig(format!(
            "user config {} is larger than {MAX_USER_CONFIG_BYTES} bytes",
            path.display()
        )));
    }
    let text = fs::read_to_string(&path).map_err(|err| {
        VspError::UsageOrConfig(format!("cannot read user config {}: {err}", path.display()))
    })?;
    parse_user_config(&text, &path).map(Some)
}

/// Parses and schema-validates user configuration text.
///
/// `path` is recorded in [`UserConfig::path`] and used as the label in error messages. This
/// function does not read the filesystem.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`): JSONC syntax error, wrong `version`, unknown
///   key, wildcard trust key, invalid name or ID syntax, or a bare `overrides` key.
/// - [`VspError::SafetyRefusal`] (exit `5`): an override matches the autonomy deny-list.
pub fn parse_user_config(text: &str, path: &Path) -> VspResult<UserConfig> {
    let label = path.display().to_string();
    let value = parse_jsonc_value(text, &label)
        .map_err(|err| VspError::UsageOrConfig(format!("invalid user config: {err}")))?;
    let Value::Object(root) = value else {
        return Err(config_error(&label, "", "expected a JSON object"));
    };

    reject_trust_keys(&root, &label, "")?;
    reject_unknown_keys(
        &root,
        &["$schema", "version", "defaults", "profiles"],
        &label,
        "",
    )?;

    match root.get("version").and_then(Value::as_u64) {
        Some(USER_CONFIG_VERSION) => {}
        _ => {
            return Err(config_error(
                &label,
                "version",
                &format!("must be the integer {USER_CONFIG_VERSION}"),
            ));
        }
    }

    let defaults = match root.get("defaults") {
        None | Some(Value::Null) => UserDefaults::default(),
        Some(Value::Object(obj)) => parse_defaults(obj, &label)?,
        Some(_) => return Err(config_error(&label, "defaults", "must be an object")),
    };

    let mut profiles = BTreeMap::new();
    match root.get("profiles") {
        None | Some(Value::Null) => {}
        Some(Value::Object(obj)) => {
            for (name, raw) in obj {
                let profile = parse_profile(name, raw, &label)?;
                profiles.insert(name.clone(), profile);
            }
        }
        Some(_) => return Err(config_error(&label, "profiles", "must be an object")),
    }

    // Structural validation (exit 4) is complete. Now run the deny-list (exit 5).
    for profile in profiles.values() {
        let where_ = format!("{label}: profiles.{}.overrides", profile.name);
        let hits = check_autonomy_denylist(&Value::Object(profile.overrides.clone()), &where_);
        if !hits.is_empty() {
            let summary: Vec<String> = hits.into_iter().map(|v| v.message).collect();
            return Err(VspError::SafetyRefusal(format!(
                "{where_}: autonomy/exec-override deny-list violation: {}",
                summary.join("; ")
            )));
        }
    }

    Ok(UserConfig {
        path: path.to_path_buf(),
        defaults,
        profiles,
    })
}

/// Adds each user profile to `catalog.profiles` as a [`ProfileRecipe`] and resolves its
/// extra extensions against the catalog.
///
/// After this call, `vsp list`, `show`, `diff`, `sync`, `export`, and `open` resolve user
/// profiles with the same code paths as catalog presets. Do not pass the changed catalog to
/// repository validation (`vsp dev validate`); use an unchanged catalog there.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`): a profile name collides with a catalog preset,
///   pack, legacy alias, or `default`; `extends`, `with`, or `without` names an unknown
///   module; the module selection does not resolve; or an extra extension is superseded
///   or retired.
/// - [`VspError::SafetyRefusal`] (exit `5`): an extra extension is compromised, removed, or
///   has an unrecognized catalog status.
pub fn apply_user_config(
    catalog: &mut WorkspaceCatalog,
    config: &UserConfig,
) -> VspResult<UserOverlay> {
    let label = config.path.display().to_string();
    let mut overlay = UserOverlay::default();
    let mut recipes: Vec<ProfileRecipe> = Vec::new();

    for (name, profile) in &config.profiles {
        let field = format!("profiles.{name}");
        if name == "default"
            || catalog.profiles.contains_key(name)
            || catalog.modules.contains_key(name)
            || resolve_legacy_profile_name(name).is_some()
        {
            return Err(config_error(
                &label,
                &field,
                "name collides with a catalog preset, pack, legacy alias, or 'default'",
            ));
        }

        let recipe = build_recipe(catalog, profile, &label)?;
        let extras = resolve_extra_extensions(catalog, profile, &label)?;
        overlay.profiles.insert(
            name.clone(),
            UserProfileOverlay {
                extends: profile.extends.clone(),
                overrides: profile.overrides.clone(),
                extra_extensions: extras,
            },
        );
        recipes.push(recipe);
    }

    for recipe in recipes {
        let name = recipe.name.clone();
        catalog.profiles.insert(name.clone(), recipe);
        // Resolve now so a DAG conflict is reported at load time, not during a later write.
        resolve_profile_recipe(catalog, &name, &[], &[], Some(UxPreset::Default), None).map_err(
            |err| {
                VspError::UsageOrConfig(format!(
                    "{label}: profiles.{name}: module selection does not resolve: {err}"
                ))
            },
        )?;
    }

    Ok(overlay)
}

/// Renders the commented starter configuration that `vsp init` writes.
///
/// Values in `seed` are written as active entries. All other entries are written as
/// comments so the result is valid and has no effect until the user edits it.
#[must_use]
pub fn render_starter_config(seed: &StarterSeed) -> String {
    let mut out = String::new();
    out.push_str(concat!(
        "// vsp personal configuration (schema version 1).\n",
        "//\n",
        "// Location: ${XDG_CONFIG_HOME:-~/.config}/vsp/config.jsonc (or --config <PATH>).\n",
        "// vsp reads only this file. It never reads .vsp.toml, .vsp.jsonc, or any other\n",
        "// configuration from a repository or the current directory.\n",
        "//\n",
        "// Security rules (vsp refuses a file that breaks them):\n",
        "//   - No publisher-wide trust. \"trust\", \"publishers\", and \"trustedPublishers\"\n",
        "//     keys are rejected. Add catalog-external extensions by exact ID only.\n",
        "//   - Each \"overrides\" key is a literal VS Code setting ID (it contains a dot)\n",
        "//     or a \"[language]\" block. Bare keys such as \"chat\" are rejected.\n",
        "//   - Overrides that match the autonomy deny-list are rejected.\n",
        "{\n",
        "  \"version\": 1,\n",
        "\n",
        "  // Personal defaults for named profiles and the Default user profile.\n",
        "  // Command-line flags (--ux, --ai, --no-ai, --no-rtk) take priority.\n",
        "  // Workspace targets are not changed by these defaults.\n",
        "  \"defaults\": {\n",
    ));

    let mut default_lines: Vec<(bool, String)> = Vec::new();
    match seed.ux {
        Some(ux) => default_lines.push((true, format!("\"ux\": \"{ux}\""))),
        None => default_lines.push((false, "\"ux\": \"crisp\"".to_string())),
    }
    match seed.ai {
        Some(ai) => default_lines.push((true, format!("\"ai\": \"{ai}\""))),
        None => default_lines.push((false, "\"ai\": \"full\"".to_string())),
    }
    match seed.rtk_terminal {
        Some(flag) => default_lines.push((true, format!("\"rtkTerminal\": {flag}"))),
        None => default_lines.push((false, "\"rtkTerminal\": false".to_string())),
    }
    let hints = [
        "\"crisp\" | \"retina\" | \"default\"",
        "\"full\" | \"copilot\" | \"off\"",
        "true adds the rtk-terminal pack; false skips RTK wrappers",
    ];
    let active_count = default_lines.iter().filter(|(active, _)| *active).count();
    let mut active_seen = 0usize;
    for ((active, text), hint) in default_lines.iter().zip(hints) {
        if *active {
            active_seen += 1;
            let comma = if active_seen < active_count { "," } else { "" };
            out.push_str(&format!("    {text}{comma} // {hint}\n"));
        } else {
            out.push_str(&format!("    // {text}, // {hint}\n"));
        }
    }
    out.push_str("  },\n\n");

    out.push_str(concat!(
        "  // Custom profiles. Use a name with `vsp sync -p <name>`, `vsp diff -p <name>`,\n",
        "  // `vsp show <name>`, or `vsp export <name>`.\n",
        "  //   extends          a catalog preset or pack (see `vsp list` and `vsp list packs`)\n",
        "  //   with / without   pack IDs to add or exclude\n",
        "  //   overrides        profile-scope settings (setting IDs and \"[language]\" blocks)\n",
        "  //   extraExtensions  exact \"publisher.name\" IDs; catalog-external IDs are\n",
        "  //                    installed as Tier 3 \"user-unlocked\" and shown by `vsp doctor`\n",
        "  \"profiles\": {\n",
    ));
    if let Some(profile) = &seed.profile {
        out.push_str(&render_profile_entry(profile));
    } else {
        out.push_str(concat!(
            "    // \"my-rust\": {\n",
            "    //   \"description\": \"Rust with my Git tools\",\n",
            "    //   \"extends\": \"rust\",\n",
            "    //   \"with\": [\"git-pro\"],\n",
            "    //   \"without\": [],\n",
            "    //   \"overrides\": {\n",
            "    //     \"editor.fontSize\": 14,\n",
            "    //     \"[rust]\": { \"editor.formatOnSave\": true }\n",
            "    //   },\n",
            "    //   \"extraExtensions\": [\"publisher.name\"]\n",
            "    // }\n",
        ));
    }
    out.push_str("  }\n}\n");
    out
}

/// Writes user configuration text to `path` atomically with mode `0600`.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): `path` exists and `force` is `false`, or
///   `path` is a directory or a symbolic link.
/// - [`VspError::Operational`] (exit `1`): the file cannot be examined or written.
pub fn write_user_config(path: &Path, content: &str, force: bool) -> VspResult<()> {
    check_user_config_target(path, force)?;
    write_file_atomic(path, content.as_bytes(), 0o600, true)
}

/// Checks that `vsp init` is allowed to write `path`. This function changes nothing.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): `path` exists and `force` is `false`, or
///   `path` is a directory or a symbolic link.
/// - [`VspError::Operational`] (exit `1`): the path cannot be examined.
pub fn check_user_config_target(path: &Path, force: bool) -> VspResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !force {
                return Err(VspError::SafetyRefusal(format!(
                    "refusing to overwrite existing user config {} (pass --force to replace it)",
                    path.display()
                )));
            }
            if !meta.is_file() {
                return Err(VspError::SafetyRefusal(format!(
                    "refusing to replace {}: it is not a regular file",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(VspError::Operational(format!(
            "cannot examine {}: {err}",
            path.display()
        ))),
    }
}

fn render_profile_entry(profile: &UserProfile) -> String {
    let mut fields: Vec<String> = Vec::new();
    if !profile.description.is_empty() {
        fields.push(format!(
            "\"description\": {}",
            Value::String(profile.description.clone())
        ));
    }
    if let Some(base) = &profile.extends {
        fields.push(format!("\"extends\": {}", Value::String(base.clone())));
    }
    fields.push(format!("\"with\": {}", string_array(&profile.with)));
    fields.push(format!("\"without\": {}", string_array(&profile.without)));
    fields.push(format!(
        "\"overrides\": {}",
        Value::Object(profile.overrides.clone())
    ));
    fields.push(format!(
        "\"extraExtensions\": {}",
        string_array(&profile.extra_extensions)
    ));

    let mut out = format!("    {}: {{\n", Value::String(profile.name.clone()));
    let last = fields.len().saturating_sub(1);
    for (idx, field) in fields.iter().enumerate() {
        let comma = if idx < last { "," } else { "" };
        out.push_str(&format!("      {field}{comma}\n"));
    }
    out.push_str("    }\n");
    out
}

fn string_array(items: &[String]) -> String {
    Value::Array(items.iter().cloned().map(Value::String).collect()).to_string()
}

fn config_error(label: &str, field: &str, message: &str) -> VspError {
    if field.is_empty() {
        VspError::UsageOrConfig(format!("invalid user config {label}: {message}"))
    } else {
        VspError::UsageOrConfig(format!("invalid user config {label}: {field}: {message}"))
    }
}

fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|c| *c != '_' && *c != '-')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn reject_trust_keys(obj: &Map<String, Value>, label: &str, field: &str) -> VspResult<()> {
    for key in obj.keys() {
        if TRUST_KEYS.contains(&normalize_key(key).as_str()) {
            let at = if field.is_empty() {
                key.clone()
            } else {
                format!("{field}.{key}")
            };
            return Err(config_error(
                label,
                &at,
                "publisher trust is not configurable; wildcard or publisher-wide trust is \
                 rejected and catalog-external extensions are allowed by exact ID only \
                 (use profiles.<name>.extraExtensions)",
            ));
        }
    }
    Ok(())
}

fn reject_unknown_keys(
    obj: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
    field: &str,
) -> VspResult<()> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            let at = if field.is_empty() {
                key.clone()
            } else {
                format!("{field}.{key}")
            };
            return Err(config_error(
                label,
                &at,
                &format!("unknown key (allowed: {})", allowed.join(", ")),
            ));
        }
    }
    Ok(())
}

fn parse_defaults(obj: &Map<String, Value>, label: &str) -> VspResult<UserDefaults> {
    reject_trust_keys(obj, label, "defaults")?;
    reject_unknown_keys(obj, &["ux", "ai", "rtkTerminal"], label, "defaults")?;

    let ux = match obj.get("ux") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "crisp" => UxPreset::Crisp,
            "retina" => UxPreset::Retina,
            "default" => UxPreset::Default,
            _ => {
                return Err(config_error(
                    label,
                    "defaults.ux",
                    "must be \"crisp\", \"retina\", or \"default\"",
                ));
            }
        }),
        Some(_) => return Err(config_error(label, "defaults.ux", "must be a string")),
    };
    let ai = match obj.get("ai") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "full" => AiMode::Full,
            "copilot" => AiMode::Copilot,
            "off" => AiMode::Off,
            _ => {
                return Err(config_error(
                    label,
                    "defaults.ai",
                    "must be \"full\", \"copilot\", or \"off\"",
                ));
            }
        }),
        Some(_) => return Err(config_error(label, "defaults.ai", "must be a string")),
    };
    let rtk_terminal = match obj.get("rtkTerminal") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => {
            return Err(config_error(
                label,
                "defaults.rtkTerminal",
                "must be a boolean",
            ));
        }
    };

    Ok(UserDefaults {
        ux,
        ai,
        rtk_terminal,
    })
}

fn parse_profile(name: &str, raw: &Value, label: &str) -> VspResult<UserProfile> {
    let field = format!("profiles.{name}");
    if !is_valid_module_id(name) || name.len() > 64 {
        return Err(config_error(
            label,
            &field,
            "profile name must match ^[a-z0-9][a-z0-9-]*$ and have at most 64 characters",
        ));
    }
    let Value::Object(obj) = raw else {
        return Err(config_error(label, &field, "must be an object"));
    };
    reject_trust_keys(obj, label, &field)?;
    reject_unknown_keys(
        obj,
        &[
            "description",
            "extends",
            "with",
            "without",
            "overrides",
            "extraExtensions",
        ],
        label,
        &field,
    )?;

    let description = match obj.get("description") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => {
            return Err(config_error(
                label,
                &format!("{field}.description"),
                "must be a string",
            ));
        }
    };

    let extends = match obj.get("extends") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if is_valid_module_id(s) => Some(s.clone()),
        Some(_) => {
            return Err(config_error(
                label,
                &format!("{field}.extends"),
                "must be the ID of one catalog preset or pack",
            ));
        }
    };

    let with = parse_module_list(obj.get("with"), label, &format!("{field}.with"))?;
    let without = parse_module_list(obj.get("without"), label, &format!("{field}.without"))?;
    if let Some(both) = with.iter().find(|id| without.contains(id)) {
        return Err(config_error(
            label,
            &field,
            &format!("'{both}' is in both \"with\" and \"without\""),
        ));
    }

    let overrides = match obj.get("overrides") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => {
            check_override_keys(map, label, &format!("{field}.overrides"), true)?;
            map.clone()
        }
        Some(_) => {
            return Err(config_error(
                label,
                &format!("{field}.overrides"),
                "must be an object",
            ));
        }
    };

    let extra_field = format!("{field}.extraExtensions");
    let mut extra_extensions: Vec<String> = Vec::new();
    match obj.get("extraExtensions") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            let mut seen = BTreeSet::new();
            for item in items {
                let Some(id) = item.as_str() else {
                    return Err(config_error(
                        label,
                        &extra_field,
                        "must contain strings only",
                    ));
                };
                if !is_exact_extension_id(id) {
                    return Err(config_error(
                        label,
                        &extra_field,
                        &format!(
                            "'{id}' is not an exact extension ID (expected publisher.name; \
                             wildcards and publisher-wide entries are rejected)"
                        ),
                    ));
                }
                let lc = id.to_ascii_lowercase();
                if seen.insert(lc.clone()) {
                    extra_extensions.push(lc);
                }
            }
        }
        Some(_) => return Err(config_error(label, &extra_field, "must be an array")),
    }

    Ok(UserProfile {
        name: name.to_string(),
        description,
        extends,
        with,
        without,
        overrides,
        extra_extensions,
    })
}

fn parse_module_list(raw: Option<&Value>, label: &str, field: &str) -> VspResult<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    match raw {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for item in items {
                match item.as_str() {
                    Some(id) if is_valid_module_id(id) => {
                        if !out.iter().any(|x| x == id) {
                            out.push(id.to_string());
                        }
                    }
                    _ => {
                        return Err(config_error(
                            label,
                            field,
                            "must contain pack IDs that match ^[a-z0-9][a-z0-9-]*$",
                        ));
                    }
                }
            }
        }
        Some(_) => return Err(config_error(label, field, "must be an array")),
    }
    Ok(out)
}

/// Returns `true` for a `[language]` block key such as `[rust]`.
fn is_language_block_key(key: &str) -> bool {
    key.len() > 2 && key.starts_with('[') && key.ends_with(']')
}

/// Returns `true` for a literal dotted VS Code setting ID such as `editor.fontSize`.
fn is_dotted_setting_key(key: &str) -> bool {
    key.contains('.')
        && !key.chars().any(char::is_whitespace)
        && key.split('.').all(|segment| !segment.is_empty())
}

fn check_override_keys(
    map: &Map<String, Value>,
    label: &str,
    field: &str,
    allow_language_blocks: bool,
) -> VspResult<()> {
    for (key, value) in map {
        if allow_language_blocks && is_language_block_key(key) {
            let Value::Object(inner) = value else {
                return Err(config_error(
                    label,
                    &format!("{field}.{key}"),
                    "a [language] block must be an object",
                ));
            };
            check_override_keys(inner, label, &format!("{field}.{key}"), false)?;
        } else if !is_dotted_setting_key(key) {
            return Err(config_error(
                label,
                &format!("{field}.{key}"),
                "override keys must be literal VS Code setting IDs that contain '.' (for \
                 example \"editor.fontSize\") or \"[language]\" blocks; bare single-segment \
                 keys are rejected",
            ));
        }
    }
    Ok(())
}

/// Returns `true` when `id` is exactly `publisher.name` with valid Marketplace syntax.
fn is_exact_extension_id(id: &str) -> bool {
    is_valid_extension_id_syntax(id) && id.matches('.').count() == 1
}

fn build_recipe(
    catalog: &WorkspaceCatalog,
    profile: &UserProfile,
    label: &str,
) -> VspResult<ProfileRecipe> {
    let field = format!("profiles.{}", profile.name);
    for (list_name, ids) in [("with", &profile.with), ("without", &profile.without)] {
        if let Some(unknown) = ids.iter().find(|id| !catalog.modules.contains_key(*id)) {
            return Err(config_error(
                label,
                &format!("{field}.{list_name}"),
                &format!("unknown pack '{unknown}' (run 'vsp list packs')"),
            ));
        }
    }

    let (use_defaults, base_modules, base_excludes, base_ux, base_ws, base_prof) =
        match profile.extends.as_deref() {
            None => (
                true,
                Vec::new(),
                Vec::new(),
                "default".to_string(),
                Map::new(),
                Map::new(),
            ),
            Some(base) => {
                if let Some(recipe) = catalog.profiles.get(base) {
                    let (ws, prof) = recipe.split_overrides();
                    (
                        recipe.use_defaults,
                        recipe.modules.clone(),
                        recipe.exclude_modules.clone(),
                        recipe.ux.clone(),
                        ws,
                        prof,
                    )
                } else if catalog.modules.contains_key(base) {
                    (
                        true,
                        vec![base.to_string()],
                        Vec::new(),
                        "default".to_string(),
                        Map::new(),
                        Map::new(),
                    )
                } else {
                    let hint = if resolve_legacy_profile_name(base).is_some() {
                        " (legacy alias names are not accepted; extend the canonical preset and \
                         set defaults.ux)"
                    } else {
                        " (run 'vsp list' and 'vsp list packs')"
                    };
                    return Err(config_error(
                        label,
                        &format!("{field}.extends"),
                        &format!("unknown catalog preset or pack '{base}'{hint}"),
                    ));
                }
            }
        };

    let mut modules = base_modules;
    for id in &profile.with {
        if !modules.iter().any(|m| m == id) {
            modules.push(id.clone());
        }
    }
    // An explicit "with" entry has priority over an exclusion from the base preset.
    let mut exclude_modules: Vec<String> = base_excludes
        .into_iter()
        .filter(|id| !profile.with.contains(id))
        .collect();
    for id in &profile.without {
        if !exclude_modules.iter().any(|m| m == id) {
            exclude_modules.push(id.clone());
        }
    }

    let mut overrides = Map::new();
    overrides.insert("workspace".to_string(), Value::Object(base_ws));
    overrides.insert(
        "profile".to_string(),
        Value::Object(recursive_object_multiply(&base_prof, &profile.overrides)),
    );

    let description = if profile.description.is_empty() {
        match &profile.extends {
            Some(base) => format!("User profile that extends '{base}'"),
            None => "User profile".to_string(),
        }
    } else {
        profile.description.clone()
    };

    Ok(ProfileRecipe {
        version: 1,
        name: profile.name.clone(),
        description,
        use_defaults,
        modules,
        exclude_modules,
        ux: base_ux,
        overrides: Value::Object(overrides),
    })
}

fn resolve_extra_extensions(
    catalog: &WorkspaceCatalog,
    profile: &UserProfile,
    label: &str,
) -> VspResult<Vec<ExtraExtension>> {
    let field = format!("profiles.{}.extraExtensions", profile.name);
    let mut out = Vec::with_capacity(profile.extra_extensions.len());
    for id in &profile.extra_extensions {
        if !is_exact_extension_id(id) {
            return Err(config_error(
                label,
                &field,
                &format!("'{id}' is not an exact extension ID (expected publisher.name)"),
            ));
        }
        let entry = match catalog.extension_record(id) {
            None => ExtraExtension {
                id: id.clone(),
                tier: ExtensionTrustTier::UserUnlocked,
            },
            Some(rec) => match &rec.status {
                ExtensionStatus::Active => ExtraExtension {
                    id: id.clone(),
                    tier: ExtensionTrustTier::Catalog,
                },
                ExtensionStatus::Superseded | ExtensionStatus::Retired => {
                    let hint = rec
                        .replaced_by
                        .as_deref()
                        .map(|rep| format!(" (use '{rep}')"))
                        .unwrap_or_default();
                    return Err(config_error(
                        label,
                        &field,
                        &format!(
                            "'{id}' is {} in the catalog and cannot be added{hint}",
                            rec.status.as_str()
                        ),
                    ));
                }
                ExtensionStatus::Compromised
                | ExtensionStatus::Removed
                | ExtensionStatus::Unknown(_) => {
                    return Err(VspError::SafetyRefusal(format!(
                        "user config {label}: {field}: extension '{id}' has catalog status \
                         '{}' and cannot be installed",
                        rec.status.as_str()
                    )));
                }
            },
        };
        out.push(entry);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit_codes::ExitCode;
    use crate::schema_loader::load_embedded;

    fn parse(text: &str) -> VspResult<UserConfig> {
        parse_user_config(text, Path::new("config.jsonc"))
    }

    fn exit_of<T: std::fmt::Debug>(res: VspResult<T>) -> ExitCode {
        res.expect_err("expected an error").exit_code()
    }

    #[test]
    fn parses_defaults_and_profile() {
        let cfg = parse(
            r#"{
              // comment
              "version": 1,
              "defaults": { "ux": "retina", "ai": "off", "rtkTerminal": true },
              "profiles": { "my-rust": { "extends": "rust", "with": ["git-pro"],
                "overrides": { "editor.fontSize": 14, "[rust]": { "editor.formatOnSave": true } },
                "extraExtensions": ["Acme.Tool", "acme.tool"] } },
            }"#,
        )
        .expect("valid config");
        assert_eq!(cfg.defaults.ux, Some(UxPreset::Retina));
        assert_eq!(cfg.defaults.ai, Some(AiMode::Off));
        assert_eq!(cfg.defaults.rtk_terminal, Some(true));
        let profile = &cfg.profiles["my-rust"];
        assert_eq!(profile.extends.as_deref(), Some("rust"));
        assert_eq!(profile.extra_extensions, vec!["acme.tool".to_string()]);
    }

    #[test]
    fn rejects_version_unknown_keys_and_trust_with_exit_4() {
        assert_eq!(exit_of(parse(r#"{}"#)), ExitCode::UsageOrConfig);
        assert_eq!(exit_of(parse(r#"{"version": 2}"#)), ExitCode::UsageOrConfig);
        assert_eq!(
            exit_of(parse(r#"{"version": 1, "extra": true}"#)),
            ExitCode::UsageOrConfig
        );
        for doc in [
            r#"{"version": 1, "trust": {"publishers": ["*"]}}"#,
            r#"{"version": 1, "publishers": ["acme"]}"#,
            r#"{"version": 1, "trustedPublishers": ["*"]}"#,
            r#"{"version": 1, "defaults": {"Trusted_Publishers": ["*"]}}"#,
            r#"{"version": 1, "profiles": {"p": {"trust": {"publishers": ["*"]}}}}"#,
        ] {
            let err = parse(doc).expect_err("trust key must be rejected");
            assert_eq!(err.exit_code(), ExitCode::UsageOrConfig, "{doc}");
            assert!(err.to_string().contains("exact ID only"), "{err}");
        }
    }

    #[test]
    fn rejects_bare_override_keys_with_exit_4() {
        for overrides in [
            r#"{"chat": {"tools": {"autoApprove": true}}}"#,
            r#"{"claudeCode": {"claudeProcessWrapper": "/tmp/x"}}"#,
            r#"{"[rust]": {"chat": {"tools.autoApprove": true}}}"#,
            r#"{"editor.": 1}"#,
            r#"{"[rust]": 1}"#,
        ] {
            let doc =
                format!(r#"{{"version": 1, "profiles": {{"p": {{"overrides": {overrides}}}}}}}"#);
            assert_eq!(exit_of(parse(&doc)), ExitCode::UsageOrConfig, "{overrides}");
        }
    }

    #[test]
    fn rejects_denylisted_overrides_with_exit_5() {
        for overrides in [
            r#"{"chat.tools.autoApprove": true}"#,
            r#"{"chat.tools.terminal.autoApprove": true}"#,
            r#"{"chat.tools": {"autoApprove": true}}"#,
            r#"{"claudeCode.claudeProcessWrapper": "/tmp/x"}"#,
            r#"{"claudeCode.allowDangerouslySkipPermissions": "true"}"#,
            r#"{"[rust]": {"chat.tools.autoApprove": true}}"#,
            r#"{"editor.custom": {"chat": {"tools": {"autoApprove": true}}}}"#,
            r#"{"claudeCode.initialPermissionMode": "bypassPermissions"}"#,
        ] {
            let doc =
                format!(r#"{{"version": 1, "profiles": {{"p": {{"overrides": {overrides}}}}}}}"#);
            assert_eq!(exit_of(parse(&doc)), ExitCode::SafetyRefusal, "{overrides}");
        }
    }

    #[test]
    fn rejects_wildcard_and_malformed_extension_ids_with_exit_4() {
        for id in [
            "acme.*",
            "*",
            "acme",
            "acme.tool.extra",
            "acme.tool@1.0.0",
            ".tool",
        ] {
            let doc = format!(
                r#"{{"version": 1, "profiles": {{"p": {{"extraExtensions": ["{id}"]}}}}}}"#
            );
            assert_eq!(exit_of(parse(&doc)), ExitCode::UsageOrConfig, "{id}");
        }
    }

    #[test]
    fn overlay_resolves_profile_and_classifies_extensions() {
        let mut catalog = load_embedded().expect("embedded catalog");
        let cfg = parse(
            r#"{"version": 1, "profiles": {"my-rust": {"extends": "rust", "with": ["git-pro"],
                "overrides": {"editor.fontSize": 14},
                "extraExtensions": ["zz-external.sample-tool", "rust-lang.rust-analyzer"]}}}"#,
        )
        .expect("valid config");
        let overlay = apply_user_config(&mut catalog, &cfg).expect("overlay");
        assert!(overlay.is_user_profile("my-rust"));
        let extras = overlay.extras_for("my-rust");
        assert_eq!(extras[0].tier, ExtensionTrustTier::UserUnlocked);
        assert_eq!(extras[1].tier, ExtensionTrustTier::Catalog);
        let additions = overlay.bundle_additions();
        assert_eq!(
            additions["my-rust"].extensions,
            vec![
                "zz-external.sample-tool".to_string(),
                "rust-lang.rust-analyzer".to_string()
            ]
        );
        assert_eq!(
            additions["my-rust"].settings["editor.fontSize"],
            Value::from(14)
        );

        // The override is the last layer: it replaces a value and cancels a deletion.
        let mut desired = Map::new();
        desired.insert("editor.fontSize".to_string(), Value::from(11));
        let mut delete_keys = vec!["editor.fontSize".to_string(), "other.key".to_string()];
        overlay.apply_final_overrides("my-rust", &mut desired, &mut delete_keys);
        assert_eq!(desired["editor.fontSize"], Value::from(14));
        assert_eq!(delete_keys, vec!["other.key".to_string()]);
        overlay.apply_final_overrides("unknown", &mut desired, &mut delete_keys);
        assert_eq!(desired.len(), 1);

        let resolved =
            resolve_profile_recipe(&catalog, "my-rust", &[], &[], Some(UxPreset::Default), None)
                .expect("resolve user profile");
        assert!(resolved.iter().any(|m| m == "rust"));
        assert!(resolved.iter().any(|m| m == "git-pro"));
        let (_, prof) = catalog.profiles["my-rust"].split_overrides();
        assert_eq!(prof["editor.fontSize"], Value::from(14));
    }

    #[test]
    fn overlay_rejects_collisions_unknown_modules_and_bad_extension_status() {
        let catalog = load_embedded().expect("embedded catalog");
        for (doc, expected) in [
            (
                r#"{"version": 1, "profiles": {"rust": {}}}"#,
                ExitCode::UsageOrConfig,
            ),
            (
                r#"{"version": 1, "profiles": {"default": {}}}"#,
                ExitCode::UsageOrConfig,
            ),
            (
                r#"{"version": 1, "profiles": {"p": {"extends": "no-such-preset"}}}"#,
                ExitCode::UsageOrConfig,
            ),
            (
                r#"{"version": 1, "profiles": {"p": {"with": ["no-such-pack"]}}}"#,
                ExitCode::UsageOrConfig,
            ),
            (
                r#"{"version": 1, "profiles": {"p": {"extends": "rust-profile-crisp"}}}"#,
                ExitCode::UsageOrConfig,
            ),
        ] {
            let cfg = parse(doc).expect("schema-valid config");
            let mut cat = catalog.clone();
            assert_eq!(
                exit_of(apply_user_config(&mut cat, &cfg)),
                expected,
                "{doc}"
            );
        }

        let blocked: Vec<String> = catalog
            .extensions
            .extensions
            .iter()
            .filter(|(id, rec)| {
                matches!(
                    rec.status,
                    ExtensionStatus::Compromised | ExtensionStatus::Removed
                ) && is_exact_extension_id(id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        assert!(
            !blocked.is_empty(),
            "catalog must list a compromised or removed extension"
        );
        for id in blocked {
            let doc = format!(
                r#"{{"version": 1, "profiles": {{"p": {{"extraExtensions": ["{id}"]}}}}}}"#
            );
            let cfg = parse(&doc).expect("schema-valid config");
            let mut cat = catalog.clone();
            assert_eq!(
                exit_of(apply_user_config(&mut cat, &cfg)),
                ExitCode::SafetyRefusal,
                "{id}"
            );
        }
    }

    #[test]
    fn starter_config_is_valid_with_and_without_seed() {
        let plain = render_starter_config(&StarterSeed::default());
        let cfg = parse(&plain).expect("plain starter parses");
        assert!(cfg.profiles.is_empty());
        assert_eq!(cfg.defaults, UserDefaults::default());

        let mut overrides = Map::new();
        overrides.insert("editor.fontSize".to_string(), Value::from(13));
        let seeded = render_starter_config(&StarterSeed {
            ux: Some(UxPreset::Retina),
            ai: Some(AiMode::Copilot),
            rtk_terminal: None,
            profile: Some(UserProfile {
                name: "my-rust".to_string(),
                description: "Mine \"quoted\"".to_string(),
                extends: Some("rust".to_string()),
                with: vec!["git-pro".to_string()],
                without: Vec::new(),
                overrides,
                extra_extensions: Vec::new(),
            }),
        });
        let cfg = parse(&seeded).expect("seeded starter parses");
        assert_eq!(cfg.defaults.ux, Some(UxPreset::Retina));
        assert_eq!(cfg.defaults.ai, Some(AiMode::Copilot));
        assert_eq!(cfg.profiles["my-rust"].with, vec!["git-pro".to_string()]);
        assert_eq!(cfg.profiles["my-rust"].description, "Mine \"quoted\"");
    }
}

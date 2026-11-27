//! Typed domain model for `vsp` catalogs, capability/toolchain/UX modules,
//! profile recipes, extensions, setting scopes, and legacy aliases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Authoritative extension group display/install order matching `install-extensions.sh`.
pub const GROUP_ORDER: &[&str] = &[
    "AI",
    "Themes",
    "DX",
    "CMake",
    "Java",
    "Rust",
    "Python",
    "Web",
    "Shell",
    "Markdown",
    "Quality",
    "Git",
    "Containers",
    "Collaboration",
    "General",
];

/// Top-level array setting keys merged by order-preserving set union instead of overwrite.
pub const COMPILER_UNION_KEYS: &[&str] = &[
    "shellcheck.exclude",
    "java.import.exclusions",
    "java.project.resourceFilters",
    "eslint.validate",
    "recommendations",
    "unwantedRecommendations",
];

/// Exact 15 symmetric managed UX keys shared by `crisp` and `retina` presets.
pub const EXPECTED_UX_KEYS: &[&str] = &[
    "editor.fontFamily",
    "editor.fontLigatures",
    "editor.fontSize",
    "editor.fontVariations",
    "editor.letterSpacing",
    "editor.lineHeight",
    "terminal.integrated.fontFamily",
    "terminal.integrated.fontSize",
    "window.density.editorTabHeight",
    "workbench.colorTheme",
    "workbench.fontAliasing",
    "workbench.iconTheme",
    "workbench.preferredDarkColorTheme",
    "workbench.productIconTheme",
    "workbench.tree.indent",
];

/// Authoritative 22 legacy `.code-profile` bundle alias pairs `(alias_filename, canonical_filename)`.
///
/// Security invariant: `ai-profile-{crisp,retina}.code-profile` ALWAYS map to
/// `ai-copilot.code-profile` (Copilot-only; never silently upgraded to the full AI suite).
pub const LEGACY_BUNDLE_ALIASES: &[(&str, &str)] = &[
    ("ai-profile-crisp.code-profile", "ai-copilot.code-profile"),
    ("ai-profile-retina.code-profile", "ai-copilot.code-profile"),
    ("ai-plus-crisp.code-profile", "ai-plus.code-profile"),
    ("ai-plus-retina.code-profile", "ai-plus.code-profile"),
    ("cpp-clangd-crisp.code-profile", "cpp-clangd.code-profile"),
    ("cpp-clangd-retina.code-profile", "cpp-clangd.code-profile"),
    (
        "cpp-intellisense-crisp.code-profile",
        "cpp-intellisense.code-profile",
    ),
    (
        "cpp-intellisense-retina.code-profile",
        "cpp-intellisense.code-profile",
    ),
    (
        "github-workflows-crisp.code-profile",
        "github-workflows.code-profile",
    ),
    (
        "github-workflows-retina.code-profile",
        "github-workflows.code-profile",
    ),
    ("java-gradle-crisp.code-profile", "java-gradle.code-profile"),
    (
        "java-gradle-retina.code-profile",
        "java-gradle.code-profile",
    ),
    ("java-maven-crisp.code-profile", "java-maven.code-profile"),
    ("java-maven-retina.code-profile", "java-maven.code-profile"),
    ("java-profile-crisp.code-profile", "java-maven.code-profile"),
    (
        "java-profile-retina.code-profile",
        "java-maven.code-profile",
    ),
    ("java-spring-crisp.code-profile", "java-spring.code-profile"),
    (
        "java-spring-retina.code-profile",
        "java-spring.code-profile",
    ),
    ("rust-profile-crisp.code-profile", "rust.code-profile"),
    ("rust-profile-retina.code-profile", "rust.code-profile"),
    ("web-astro-crisp.code-profile", "astro.code-profile"),
    ("web-astro-retina.code-profile", "astro.code-profile"),
];

/// Supported UX preset identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UxPreset {
    /// Crisp preset tuned for 1x external QHD displays.
    Crisp,
    /// Retina preset tuned for 2x HiDPI/Retina displays.
    Retina,
    /// UX-agnostic default preset (removes the 15 managed UX keys).
    Default,
}

impl UxPreset {
    /// Returns the canonical lowercase string identifier for this UX preset.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Crisp => "crisp",
            Self::Retina => "retina",
            Self::Default => "default",
        }
    }
}

impl fmt::Display for UxPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Supported AI composition modes for `--ai`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiMode {
    /// Full default AI suite (`ai-agents`: Antigravity, Claude Code, Codex, Copilot Chat).
    Full,
    /// Strict Copilot-only mode (`ai-copilot`).
    Copilot,
    /// AI disabled (excludes AI packs and sets `chat.disableAIFeatures = true`).
    Off,
}

impl AiMode {
    /// Returns the canonical lowercase string identifier for this AI mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Copilot => "copilot",
            Self::Off => "off",
        }
    }
}

impl fmt::Display for AiMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolves a legacy `<flavor>-{crisp,retina}` profile name into its canonical
/// `(canonical_profile, ux_preset)` pair.
///
/// Returns `None` for non-legacy names.
#[must_use]
pub fn resolve_legacy_profile_name(name: &str) -> Option<(&'static str, UxPreset)> {
    let (stem, ux) = if let Some(s) = name.strip_suffix("-crisp") {
        (s, UxPreset::Crisp)
    } else {
        (name.strip_suffix("-retina")?, UxPreset::Retina)
    };

    let canonical = match stem {
        "ai-profile" => "ai-copilot",
        "rust-profile" => "rust",
        "web-astro" => "astro",
        "java-profile" | "java-maven" => "java-maven",
        "ai-plus" => "ai-plus",
        "cpp-clangd" => "cpp-clangd",
        "cpp-intellisense" => "cpp-intellisense",
        "github-workflows" => "github-workflows",
        "java-gradle" => "java-gradle",
        "java-spring" => "java-spring",
        _ => return None,
    };

    Some((canonical, ux))
}

/// Validates a module identifier against `^[a-z0-9][a-z0-9-]*$`.
#[must_use]
pub fn is_valid_module_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_lowercase() || b.is_ascii_digit() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Complete offline catalog loaded from the legacy repository layout
/// (`catalog/`, `modules/`, and `profiles/`).
#[derive(Debug, Clone)]
pub struct WorkspaceCatalog {
    /// Canonical root directory from which the catalog was loaded.
    pub root: PathBuf,
    /// Default capabilities and UX preset (`catalog/defaults.json`).
    pub defaults: DefaultsManifest,
    /// Extension lifecycle registry (`catalog/extensions.json`).
    pub extensions: ExtensionCatalog,
    /// Publisher allowlist (`catalog/publishers.json`).
    pub publishers: PublishersManifest,
    /// VSIX SHA-256 pin manifest load state (`catalog/extension-pins.json`).
    pub pins: PinsLoadState,
    /// VS Code setting scope and schema registry (`catalog/setting-scopes.json`).
    pub setting_scopes: Option<SettingScopesManifest>,
    /// All discovered modules indexed by module ID (`modules/{capabilities,toolchains,ux}/`).
    pub modules: BTreeMap<String, ModuleDefinition>,
    /// All discovered canonical profile recipes indexed by profile name (`profiles/`).
    pub profiles: BTreeMap<String, ProfileRecipe>,
    /// Legacy aliases declared in `presets.jsonc` (`alias -> (canonical preset, UX preset)`).
    /// Empty for the legacy repository layout, which uses the built-in alias table only.
    pub legacy_aliases: BTreeMap<String, (String, UxPreset)>,
    /// Lowercase publishers that are never trusted: the `blockedPublishers` list of
    /// `catalog/extensions.jsonc`, plus the publishers of `compromised` and `removed`
    /// catalog extensions. See [`derive_blocked_publishers`] for the exact rule.
    pub blocked_publishers: BTreeSet<String>,
    /// Consolidated security and scope policy parsed from `catalog/policy.jsonc` (`Some` for
    /// v2.2 catalogs; `None` for the legacy repository layout).
    pub policy: Option<crate::schema_loader::PolicyManifest>,
    /// Declarative `retireOnSwitch` rules indexed by pack ID from v2.2 `packs/*.jsonc`.
    /// Empty for the legacy repository layout.
    pub retire_on_switch: BTreeMap<String, crate::schema_loader::PackRetireOnSwitch>,
    /// Parsed 4-tier per-platform VSIX lockfile from `vsp.lock.json` (`Some` for v2.2
    /// catalogs; `None` for the legacy repository layout).
    pub lockfile: Option<crate::schema_loader::LockfileManifest>,
    /// Authoritative extension group display/install order from `catalog/extensions.jsonc`.
    /// Empty for the legacy repository layout, which falls back to [`GROUP_ORDER`].
    pub group_order: Vec<String>,
}

/// Returns the lowercase publisher part of an extension ID (`publisher.name`).
#[must_use]
pub fn extension_publisher(ext_id: &str) -> String {
    ext_id
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Builds the blocked-publisher set of a catalog. All names are lowercase.
///
/// The set holds:
///
/// 1. Each name in `explicit` (the `blockedPublishers` list).
/// 2. Each publisher that owns a `compromised` extension. A publisher that shipped one
///    compromised extension is not trusted for any other extension ID.
/// 3. Each publisher that owns a `removed` extension, unless the same publisher also owns
///    an `active` catalog extension. `removed` is also the status of a deprecated
///    Marketplace listing (HTTP 404), so a publisher that the catalog still ships
///    (for example `vscjava`) stays trusted.
#[must_use]
pub fn derive_blocked_publishers(
    extensions: &ExtensionCatalog,
    explicit: &[String],
) -> BTreeSet<String> {
    // The publisher part of the ID is authoritative for `code --install-extension`. The
    // `publisher` field of the record is included in case the two differ.
    let publishers_of = |ext_id: &str, rec: &ExtensionRecord| -> BTreeSet<String> {
        [
            extension_publisher(ext_id),
            rec.publisher.trim().to_ascii_lowercase(),
        ]
        .into_iter()
        .filter(|name| !name.is_empty())
        .collect()
    };
    let with_status = |wanted: &ExtensionStatus| -> BTreeSet<String> {
        extensions
            .extensions
            .iter()
            .filter(|(_, rec)| rec.status == *wanted)
            .flat_map(|(ext_id, rec)| publishers_of(ext_id, rec))
            .collect()
    };

    let active = with_status(&ExtensionStatus::Active);
    let mut blocked: BTreeSet<String> = explicit
        .iter()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    blocked.extend(with_status(&ExtensionStatus::Compromised));
    blocked.extend(
        with_status(&ExtensionStatus::Removed)
            .into_iter()
            .filter(|name| !active.contains(name)),
    );
    blocked
}

impl WorkspaceCatalog {
    /// Returns `true` when this catalog was loaded from the consolidated v2.2 layout
    /// (`catalog/policy.jsonc`, `vsp.lock.json`, `presets.jsonc`, and `packs/`).
    #[must_use]
    pub const fn is_v2_layout(&self) -> bool {
        self.policy.is_some()
    }

    /// Returns `true` when the publisher of `ext_id` is in
    /// [`WorkspaceCatalog::blocked_publishers`].
    #[must_use]
    pub fn is_blocked_publisher(&self, ext_id: &str) -> bool {
        self.blocked_publishers
            .contains(&extension_publisher(ext_id))
    }

    /// Resolves a legacy alias name to `(canonical preset, UX preset)`.
    ///
    /// For v2.2 catalogs (`presets.jsonc`), [`WorkspaceCatalog::legacy_aliases`] is the single
    /// source of truth. The built-in `<flavor>-{crisp,retina}` table
    /// ([`resolve_legacy_profile_name`]) is used only as a fallback for the legacy repository
    /// layout.
    #[must_use]
    pub fn resolve_alias(&self, name: &str) -> Option<(String, UxPreset)> {
        if let Some((preset, ux)) = self.legacy_aliases.get(name) {
            return Some((preset.clone(), *ux));
        }
        if self.is_v2_layout() {
            return None;
        }
        resolve_legacy_profile_name(name).map(|(canonical, ux)| (canonical.to_string(), ux))
    }

    /// Returns `(alias_filename, canonical_filename)` `.code-profile` pairs for `dev dist`.
    ///
    /// Uses [`WorkspaceCatalog::legacy_aliases`] from `presets.jsonc` as the single source of
    /// truth for v2.2 catalogs, falling back to [`LEGACY_BUNDLE_ALIASES`] only for the legacy
    /// repository layout.
    #[must_use]
    pub fn bundle_alias_pairs(&self) -> Vec<(String, String)> {
        if self.is_v2_layout() {
            self.legacy_aliases
                .iter()
                .map(|(alias, (preset, _))| {
                    (
                        format!("{alias}.code-profile"),
                        format!("{preset}.code-profile"),
                    )
                })
                .collect()
        } else {
            LEGACY_BUNDLE_ALIASES
                .iter()
                .map(|&(alias, canonical)| (alias.to_string(), canonical.to_string()))
                .collect()
        }
    }

    /// Returns the authoritative extension group display/install order from
    /// `catalog/extensions.jsonc` (`groupOrder`), falling back to [`GROUP_ORDER`] for the
    /// legacy layout.
    #[must_use]
    pub fn effective_group_order(&self) -> Vec<&str> {
        if !self.group_order.is_empty() {
            self.group_order.iter().map(String::as_str).collect()
        } else {
            GROUP_ORDER.to_vec()
        }
    }

    /// Returns the sorted expected symmetric managed UX setting keys declared in
    /// `catalog/policy.jsonc` (`ux.managedKeys`), falling back to [`EXPECTED_UX_KEYS`] for the
    /// legacy layout.
    #[must_use]
    pub fn expected_ux_keys(&self) -> Vec<String> {
        if let Some(policy) = &self.policy {
            if !policy.ux.managed_keys.is_empty() {
                let mut keys = policy.ux.managed_keys.clone();
                keys.sort();
                keys.dedup();
                return keys;
            }
        }
        EXPECTED_UX_KEYS.iter().map(|s| (*s).to_string()).collect()
    }

    /// Builds the implicit preset of a toolchain pack: the `general` preset plus that pack.
    ///
    /// Returns `None` when `name` is an explicit preset, is not a pack, or is a pack of
    /// another kind. For example, `biome` gives `general + biome`.
    #[must_use]
    pub fn implicit_toolchain_recipe(&self, name: &str) -> Option<ProfileRecipe> {
        if self.profiles.contains_key(name) {
            return None;
        }
        let pack = self.modules.get(name)?;
        if pack.manifest.kind != ModuleKind::Toolchain {
            return None;
        }
        let base = self.profiles.get("general");
        let mut modules = base.map(|b| b.modules.clone()).unwrap_or_default();
        if !modules.iter().any(|m| m == name) {
            modules.push(name.to_string());
        }
        Some(ProfileRecipe {
            version: 1,
            name: name.to_string(),
            description: format!(
                "Implicit toolchain preset (general + {name}): {}",
                pack.manifest.description
            ),
            use_defaults: base.is_none_or(|b| b.use_defaults),
            modules,
            exclude_modules: base.map(|b| b.exclude_modules.clone()).unwrap_or_default(),
            ux: base.map_or_else(|| "default".to_string(), |b| b.ux.clone()),
            overrides: base.map_or_else(
                || serde_json::json!({ "workspace": {}, "profile": {} }),
                |b| b.overrides.clone(),
            ),
        })
    }

    /// Adds the implicit toolchain preset `name` to [`WorkspaceCatalog::profiles`] when
    /// `name` is not an explicit preset and names a toolchain pack. Returns `true` when a
    /// recipe was added.
    ///
    /// Commands call this only for a preset name that the user gave, so `vsp list` and
    /// `vsp dev dist` keep the explicit preset set.
    pub fn materialize_implicit_preset(&mut self, name: &str) -> bool {
        match self.implicit_toolchain_recipe(name) {
            Some(recipe) => {
                self.profiles.insert(name.to_string(), recipe);
                true
            }
            None => false,
        }
    }

    /// Looks up an extension catalog record case-insensitively.
    #[must_use]
    pub fn extension_record(&self, id: &str) -> Option<&ExtensionRecord> {
        let lc = id.to_ascii_lowercase();
        self.extensions.extensions.get(&lc)
    }

    /// Returns the catalog group for an extension ID, defaulting to `"General"`.
    #[must_use]
    pub fn extension_group(&self, id: &str) -> &str {
        self.extension_record(id)
            .map_or("General", |rec| rec.group.as_str())
    }

    /// Returns the sorted list of managed UX keys dynamically loaded from `crisp` (falling back
    /// to [`WorkspaceCatalog::expected_ux_keys`]).
    #[must_use]
    pub fn ux_managed_keys(&self) -> Vec<String> {
        self.modules
            .get("crisp")
            .and_then(|m| m.profile_settings.as_ref())
            .map_or_else(
                || self.expected_ux_keys(),
                |map| map.keys().cloned().collect(),
            )
    }

    /// Looks up a pin record case-insensitively when the pins manifest is loaded.
    #[must_use]
    pub fn lookup_pin(&self, id: &str) -> Option<&ExtensionPin> {
        let PinsLoadState::Loaded(manifest) = &self.pins else {
            return None;
        };
        let lc = id.to_ascii_lowercase();
        manifest
            .pins
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lc)
            .map(|(_, v)| v)
    }
}

/// Load status of `catalog/extension-pins.json` so validation can fail closed when missing.
#[derive(Debug, Clone)]
pub enum PinsLoadState {
    /// Successfully loaded and parsed pin manifest.
    Loaded(ExtensionPinsManifest),
    /// Pin manifest file does not exist or cannot be read.
    Missing(PathBuf),
    /// Pin manifest exists but failed JSON or schema parsing.
    Invalid {
        /// Path to the malformed pin manifest.
        path: PathBuf,
        /// Human-readable parse error detail.
        error: String,
    },
}

/// Parsed `catalog/defaults.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefaultsManifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Ordered default capability module IDs included when `useDefaults` is true.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Default UX preset name (e.g. `"crisp"`).
    #[serde(default = "default_ux_name")]
    pub ux: String,
}

fn default_version() -> u32 {
    1
}

fn default_ux_name() -> String {
    "crisp".to_string()
}

/// Lifecycle status of an extension in `catalog/extensions.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionStatus {
    /// Actively referenced by one or more modules.
    Active,
    /// Replaced by a newer active extension (`replacedBy`).
    Superseded,
    /// Removed from the Marketplace (e.g., HTTP 404).
    Removed,
    /// Security-compromised extension requiring unconditional Step 0 uninstall.
    Compromised,
    /// Retired/unmaintained extension no longer included in default profiles.
    Retired,
    /// Unrecognized status captured for strict validation reporting.
    #[serde(untagged)]
    Unknown(String),
}

impl ExtensionStatus {
    /// Returns the string label of this status.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Active => "active",
            Self::Superseded => "superseded",
            Self::Removed => "removed",
            Self::Compromised => "compromised",
            Self::Retired => "retired",
            Self::Unknown(s) => s.as_str(),
        }
    }

    /// Returns true if this status is one of the 5 recognized lifecycle states.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

/// Individual extension entry in `catalog/extensions.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionRecord {
    /// Expected lowercase publisher prefix matching the segment before `.`.
    pub publisher: String,
    /// Logical extension group (must appear in [`GROUP_ORDER`] when active).
    #[serde(default = "default_group")]
    pub group: String,
    /// Lifecycle status.
    pub status: ExtensionStatus,
    /// Whether the extension ships a universal platform-independent VSIX.
    #[serde(default)]
    pub universal: bool,
    /// Active replacement ID when `status == superseded`.
    #[serde(default)]
    pub replaced_by: Option<String>,
    /// Child extensions installed as part of an extension pack.
    #[serde(default)]
    pub pack_members: Vec<String>,
    /// Theme labels contributed by theme extensions.
    #[serde(default)]
    pub theme_labels: Vec<String>,
    /// Optional human-readable maintainer notes.
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_group() -> String {
    "General".to_string()
}

/// Parsed `catalog/extensions.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCatalog {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Map from extension ID to its lifecycle metadata.
    #[serde(default)]
    pub extensions: BTreeMap<String, ExtensionRecord>,
}

/// Parsed `catalog/publishers.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishersManifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Sorted unique lowercase allowlisted publishers.
    #[serde(default)]
    pub publishers: Vec<String>,
}

/// Individual VSIX SHA-256 pin record in `catalog/extension-pins.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionPin {
    /// Expected lowercase 64-hex SHA-256 digest of the VSIX archive.
    #[serde(default)]
    pub sha256: String,
    /// Pinned extension version string.
    #[serde(default)]
    pub version: String,
    /// Official Marketplace HTTPS VSIX download URL.
    #[serde(default)]
    pub vsix_url: String,
    /// Optional RFC 3339 verification timestamp.
    #[serde(default)]
    pub verified_at: Option<String>,
}

/// Parsed `catalog/extension-pins.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPinsManifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Optional explanatory note.
    #[serde(default)]
    pub note: Option<String>,
    /// Whether unpinned installs must fail closed.
    #[serde(default)]
    pub pin_required: bool,
    /// Pinned extension records indexed by lowercase extension ID.
    #[serde(default)]
    pub pins: BTreeMap<String, ExtensionPin>,
}

/// Individual setting specification in `catalog/setting-scopes.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingScopeSpec {
    /// Expected JSON value type (`string`, `boolean`, `integer`, `number`, `object`, `array`).
    #[serde(rename = "type")]
    pub value_type: String,
    /// VS Code setting scope (`workspace`, `profile`, `machine`, `application`).
    pub scope: String,
    /// Optional allowed string enum values.
    #[serde(rename = "enum", default)]
    pub allowed_enum: Option<Vec<String>>,
    /// Whether boolean values are also accepted for string-typed settings.
    #[serde(default)]
    pub allow_boolean: bool,
    /// Optional object value type constraint (e.g. `"integer"`).
    #[serde(rename = "valueType", default)]
    pub nested_value_type: Option<String>,
    /// Whether application-scoped keys are permitted in compiled profile fragments.
    #[serde(default)]
    pub allow_in_compiled_profile: Option<bool>,
    /// Optional advisory effective target (e.g. `"default-profile"`).
    #[serde(default)]
    pub effective_target: Option<String>,
}

/// Parsed `catalog/setting-scopes.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingScopesManifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Target VS Code baseline version string.
    #[serde(default)]
    pub vscode_version: String,
    /// Deprecated or removed VS Code setting keys forbidden across all fragments.
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
    /// Regex patterns for denied autonomy keys.
    #[serde(default)]
    pub autonomy_deny_patterns: Vec<String>,
    /// Keys forbidden anywhere in configuration regardless of value.
    #[serde(default)]
    pub autonomy_deny_presence: Vec<String>,
    /// Setting scope and type rules indexed by key name.
    #[serde(default)]
    pub settings: BTreeMap<String, SettingScopeSpec>,
}

/// Category of a composable module under `modules/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleKind {
    /// Cross-cutting developer capability (`modules/capabilities/`).
    Capability,
    /// Language or build-system toolchain (`modules/toolchains/`).
    Toolchain,
    /// Display/font/theme UX preset (`modules/ux/`).
    Ux,
}

impl ModuleKind {
    /// Returns the subdirectory name under `modules/` for this kind.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Capability => "capabilities",
            Self::Toolchain => "toolchains",
            Self::Ux => "ux",
        }
    }

    /// Returns the singular lowercase label for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capability => "capability",
            Self::Toolchain => "toolchain",
            Self::Ux => "ux",
        }
    }
}

impl fmt::Display for ModuleKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Extensions declared by a `module.json` manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleExtensions {
    /// Team-shareable workspace recommendation IDs.
    #[serde(default)]
    pub workspace: Vec<String>,
    /// Profile-scoped extension IDs.
    #[serde(default)]
    pub profile: Vec<String>,
    /// Unwanted/superseded extension IDs suppressed in workspace recommendations.
    #[serde(default)]
    pub unwanted: Vec<String>,
}

/// Relative settings fragment references declared in `module.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModuleSettingsRefs {
    /// Relative path to workspace settings fragment (e.g. `settings.workspace.jsonc`).
    #[serde(default)]
    pub workspace: Option<String>,
    /// Relative path to profile settings fragment (e.g. `settings.profile.jsonc`).
    #[serde(default)]
    pub profile: Option<String>,
}

/// Rule combination mode for module auto-detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectMatchMode {
    /// Module matches when any rule succeeds.
    #[default]
    Any,
    /// Module matches only when every rule succeeds.
    All,
}

/// Individual data-driven detection rule inside `module.json`'s `detect.rules[]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DetectRule {
    /// Matches if a single relative file path exists as a regular file.
    File {
        /// Relative file path inside workspace root.
        path: String,
    },
    /// Matches if any of the listed relative paths exists as a regular file.
    AnyFile {
        /// Candidate relative file paths.
        paths: Vec<String>,
    },
    /// Matches if all `paths` exist and (when non-empty) at least one `any_extra` file exists.
    AllFiles {
        /// Required relative file paths.
        paths: Vec<String>,
        /// Optional extra paths where at least one must exist.
        #[serde(default)]
        any_extra: Vec<String>,
    },
    /// Matches if any regular file matching `pattern` exists under `dir` within `max_depth`.
    Glob {
        /// Filename glob pattern (e.g. `*.bats`, `*.yml`).
        pattern: String,
        /// Optional relative subdirectory to search (defaults to `.`).
        #[serde(default = "default_dot")]
        dir: String,
        /// Maximum directory depth (defaults to `3`).
        #[serde(default = "default_glob_depth")]
        max_depth: usize,
    },
    /// Matches if `present` is a regular file and `absent` does not exist.
    FileAndAbsent {
        /// Relative path that must exist as a file.
        present: String,
        /// Relative path that must NOT exist.
        #[serde(default)]
        absent: String,
    },
    /// Matches if any manifest in `paths` exists AND a file matching `glob` under `search_dir`
    /// contains the literal substring `pattern`.
    FileAndGrep {
        /// Candidate manifest paths (at least one must exist).
        paths: Vec<String>,
        /// Relative subdirectory to scan (defaults to `.`).
        #[serde(default = "default_dot")]
        search_dir: String,
        /// Filename glob pattern (defaults to `*`).
        #[serde(default = "default_star")]
        glob: String,
        /// Literal substring to search inside matching files.
        pattern: String,
    },
}

fn default_dot() -> String {
    ".".to_string()
}

fn default_star() -> String {
    "*".to_string()
}

fn default_glob_depth() -> usize {
    3
}

fn default_detect_priority() -> u32 {
    50
}

/// Auto-detection specification inside `module.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectSpec {
    /// Ascending priority order (lower values evaluate and sort earlier).
    #[serde(default = "default_detect_priority")]
    pub priority: u32,
    /// Whether this detector only activates if no primary detector matched.
    #[serde(default)]
    pub fallback: bool,
    /// Rule aggregation mode (`any` or `all`).
    #[serde(rename = "match", default)]
    pub match_mode: DetectMatchMode,
    /// Module IDs superseded when this detector matches.
    #[serde(default)]
    pub supersedes: Vec<String>,
    /// Rules evaluated against a target workspace directory.
    #[serde(default)]
    pub rules: Vec<DetectRule>,
}

/// Raw JSON schema of `modules/<kind>/<id>/module.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleManifest {
    /// Manifest format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Unique module identifier (must equal directory basename).
    pub id: String,
    /// Module kind (`capability`, `toolchain`, or `ux`).
    pub kind: ModuleKind,
    /// Human-readable summary of the module.
    #[serde(default)]
    pub description: String,
    /// Hard transitive dependencies.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Conditional default companions suppressed if excluded or conflicting.
    #[serde(default)]
    pub default_with: Vec<String>,
    /// Mutually exclusive module IDs (must be declared symmetrically).
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Declared workspace, profile, and unwanted extensions.
    #[serde(default)]
    pub extensions: ModuleExtensions,
    /// Relative paths to JSONC settings fragments.
    #[serde(default)]
    pub settings: ModuleSettingsRefs,
    /// Optional relative path to a VS Code `tasks.json` fragment.
    #[serde(default)]
    pub tasks: Option<String>,
    /// Optional data-driven workspace detection specification.
    #[serde(default)]
    pub detect: Option<DetectSpec>,
}

/// Loaded module manifest together with its resolved directory and parsed fragments.
#[derive(Debug, Clone)]
pub struct ModuleDefinition {
    /// Parsed `module.json` manifest.
    pub manifest: ModuleManifest,
    /// Physical directory containing `module.json` and sibling fragments.
    pub dir: PathBuf,
    /// Parsed workspace settings fragment with `$schema`/`@extends` stripped, if declared.
    pub workspace_settings: Option<Map<String, Value>>,
    /// Parsed profile settings fragment with `$schema`/`@extends` stripped, if declared.
    pub profile_settings: Option<Map<String, Value>>,
    /// Parsed tasks JSON document, if declared.
    pub tasks_doc: Option<Value>,
}

/// Parsed `profiles/<name>/profile.json` recipe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileRecipe {
    /// Recipe format version.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Canonical profile name (matches directory basename).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Whether `catalog/defaults.json` capabilities are prepended to root modules.
    #[serde(default = "default_true")]
    pub use_defaults: bool,
    /// Explicitly requested module IDs.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Excluded module IDs (can be overridden by CLI `--with`).
    #[serde(default)]
    pub exclude_modules: Vec<String>,
    /// Recipe default UX preset (`default`, `crisp`, or `retina`).
    #[serde(default = "default_recipe_ux")]
    pub ux: String,
    /// Raw overrides JSON value supporting both `{workspace, profile}` and flat object shapes.
    #[serde(default)]
    pub overrides: Value,
}

fn default_true() -> bool {
    true
}

fn default_recipe_ux() -> String {
    "default".to_string()
}

impl ProfileRecipe {
    /// Extracts `(workspace_overrides, profile_overrides)` matching `scripts/lib/compiler.sh`.
    #[must_use]
    pub fn split_overrides(&self) -> (Map<String, Value>, Map<String, Value>) {
        match &self.overrides {
            Value::Object(map) => {
                if map.contains_key("workspace") || map.contains_key("profile") {
                    let ws = map
                        .get("workspace")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    let prof = map
                        .get("profile")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    (ws, prof)
                } else {
                    (Map::new(), map.clone())
                }
            }
            _ => (Map::new(), Map::new()),
        }
    }
}

/// VS Code `extensions.json` element shape (`{ "identifier": { "id": "..." } }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsCodeExtensionEntry {
    /// Nested identifier wrapper required by VS Code profile `extensions.json`.
    pub identifier: VsCodeExtensionIdentifier,
}

/// Nested extension identifier wrapper inside [`VsCodeExtensionEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsCodeExtensionIdentifier {
    /// Extension identifier string (`publisher.name`).
    pub id: String,
}

/// Compiled `.vscode/extensions.json` structure for workspace targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExtensionsFile {
    /// Ordered, case-insensitively deduplicated workspace extension recommendations.
    pub recommendations: Vec<String>,
    /// Ordered, case-insensitively deduplicated unwanted extension recommendations.
    pub unwanted_recommendations: Vec<String>,
}

/// Exported `.code-profile` bundle JSON structure matching `exports/*.code-profile` byte-for-byte.
///
/// Field declaration order (`name`, `settings`, `extensions`) is intentionally preserved
/// during serialization to match tracked bundles produced by `scripts/export-profiles.sh`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeProfileBundle {
    /// Canonical profile name.
    pub name: String,
    /// UX-agnostic merged settings object with lexicographically sorted keys.
    pub settings: Map<String, Value>,
    /// Bundle extensions block.
    pub extensions: BundleExtensions,
}

/// Extensions section inside [`CodeProfileBundle`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundleExtensions {
    /// Ordered enabled extension IDs.
    pub enabled: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Opus R2-H1: the blocked set has the explicit list, the publishers of compromised
    /// extensions, and the publishers of removed extensions that own no active extension.
    #[test]
    fn blocked_publishers_follow_the_three_rules() {
        let record = |publisher: &str, status: ExtensionStatus| ExtensionRecord {
            publisher: publisher.to_string(),
            group: "General".to_string(),
            status,
            universal: true,
            replaced_by: None,
            pack_members: Vec::new(),
            theme_labels: Vec::new(),
            notes: None,
        };
        let extensions = ExtensionCatalog {
            version: 1,
            extensions: [
                ("shipped.tool", "shipped", ExtensionStatus::Active),
                ("shipped.old-tool", "shipped", ExtensionStatus::Removed),
                ("gone.tool", "gone", ExtensionStatus::Removed),
                ("Evil.Tool", "evil", ExtensionStatus::Compromised),
                ("evil.other", "evil", ExtensionStatus::Active),
                ("retired.tool", "retired", ExtensionStatus::Retired),
                ("old.tool", "old", ExtensionStatus::Superseded),
            ]
            .into_iter()
            .map(|(id, publisher, status)| (id.to_string(), record(publisher, status)))
            .collect(),
        };

        let blocked = derive_blocked_publishers(&extensions, &[" Listed ".to_string()]);
        let expected: BTreeSet<String> = ["evil", "gone", "listed"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(blocked, expected);

        assert_eq!(extension_publisher("JSCearcy.Rust-Doc"), "jscearcy");
        assert_eq!(extension_publisher("no-dot"), "no-dot");
        assert_eq!(extension_publisher(""), "");
    }

    #[test]
    fn module_id_validation_matches_regex() {
        assert!(is_valid_module_id("core-editor"));
        assert!(is_valid_module_id("0-test"));
        assert!(is_valid_module_id("a"));
        assert!(!is_valid_module_id(""));
        assert!(!is_valid_module_id("-leading"));
        assert!(!is_valid_module_id("UpperCase"));
        assert!(!is_valid_module_id("under_score"));
        assert!(!is_valid_module_id("../escape"));
    }

    #[test]
    fn legacy_profile_resolution_preserves_ai_copilot_invariant() {
        assert_eq!(
            resolve_legacy_profile_name("ai-profile-crisp"),
            Some(("ai-copilot", UxPreset::Crisp))
        );
        assert_eq!(
            resolve_legacy_profile_name("ai-profile-retina"),
            Some(("ai-copilot", UxPreset::Retina))
        );
        assert_eq!(
            resolve_legacy_profile_name("rust-profile-crisp"),
            Some(("rust", UxPreset::Crisp))
        );
        assert_eq!(
            resolve_legacy_profile_name("web-astro-retina"),
            Some(("astro", UxPreset::Retina))
        );
        assert_eq!(
            resolve_legacy_profile_name("java-profile-crisp"),
            Some(("java-maven", UxPreset::Crisp))
        );
        assert_eq!(resolve_legacy_profile_name("rust"), None);
        assert_eq!(resolve_legacy_profile_name("unknown-crisp"), None);
        assert_eq!(LEGACY_BUNDLE_ALIASES.len(), 22);
    }

    #[test]
    fn profile_recipe_split_overrides_handles_nested_and_flat_forms() {
        let nested = ProfileRecipe {
            version: 1,
            name: "test".to_string(),
            description: String::new(),
            use_defaults: true,
            modules: Vec::new(),
            exclude_modules: Vec::new(),
            ux: "default".to_string(),
            overrides: json!({
                "workspace": { "a": 1 },
                "profile": { "b": 2 }
            }),
        };
        let (ws, prof) = nested.split_overrides();
        assert_eq!(ws.get("a"), Some(&json!(1)));
        assert_eq!(prof.get("b"), Some(&json!(2)));

        let flat = ProfileRecipe {
            overrides: json!({ "chat.editor.fontSize": 14 }),
            ..nested
        };
        let (ws2, prof2) = flat.split_overrides();
        assert!(ws2.is_empty());
        assert_eq!(prof2.get("chat.editor.fontSize"), Some(&json!(14)));
    }
}

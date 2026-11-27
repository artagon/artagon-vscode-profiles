//! Consolidated v2.2 schema loader supporting both build-time embedded snapshots
//! and explicit external catalog directories.
//!
//! Loads the consolidated layout:
//! - `catalog/extensions.jsonc` (extensions registry, defaults, group order, blocked publishers)
//! - `catalog/policy.jsonc` (VS Code setting scopes, forbidden keys, nested-safe autonomy rules,
//!   Default-profile expectations, UX managed keys, and Tier 1 privileged extension IDs)
//! - `vsp.lock.json` (4-tier per-platform VSIX supply-chain lockfile)
//! - `presets.jsonc` (canonical presets with inheritance, 22 legacy aliases, and implicit
//!   toolchain preset support)
//! - `packs/*.jsonc` and `packs/rtk-terminal/{rtk-terminal.jsonc,scripts/...}`
//!
//! Both [`load_embedded`] and [`load_from_dir`] execute the exact same strict
//! parser and validator ([`load_from_snapshot`]) and map into the existing
//! [`crate::model`] domain structs without loss of metadata or fidelity.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::exit_codes::{VspError, VspResult};
use crate::jsonc::parse_jsonc_bytes;
use crate::model::{
    is_valid_module_id, DefaultsManifest, DetectSpec, ExtensionCatalog, ExtensionPin,
    ExtensionPinsManifest, ExtensionRecord, ExtensionStatus, ModuleDefinition, ModuleExtensions,
    ModuleKind, ModuleManifest, ModuleSettingsRefs, PinsLoadState, ProfileRecipe,
    PublishersManifest, SettingScopeSpec, SettingScopesManifest, UxPreset, WorkspaceCatalog,
};
use crate::policy::{is_valid_marketplace_vsix_url, is_valid_sha256_hex};
use crate::resolver::recursive_object_multiply;

mod embedded_snapshot {
    include!(concat!(env!("OUT_DIR"), "/embedded_snapshot.rs"));
}

/// Supported consolidated schema format version (`v2.2.0` layout).
pub const SCHEMA_VERSION: u32 = 2;

/// Virtual root label stored on [`WorkspaceCatalog::root`] when loaded from the
/// embedded compile-time snapshot.
pub const EMBEDDED_ROOT_LABEL: &str = "<embedded>";

/// Required top-level manifest paths inside every v2.2 snapshot.
const REQUIRED_SNAPSHOT_FILES: &[&str] = &[
    "catalog/extensions.jsonc",
    "catalog/policy.jsonc",
    "presets.jsonc",
    "vsp.lock.json",
];

/// Declarative settings and extension keys retired when switching to a mutually exclusive pack.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRetireOnSwitch {
    /// Setting keys owned by rival pack(s) to retire when this pack is activated.
    #[serde(default)]
    pub settings: Vec<String>,
    /// Extension IDs owned by rival pack(s) to retire when this pack is activated.
    #[serde(default)]
    pub extensions: Vec<String>,
}

impl PackRetireOnSwitch {
    /// Returns `true` when neither settings nor extensions are declared for retirement.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.extensions.is_empty()
    }
}

/// Nested-safe autonomy and execution-override deny-list configuration from `catalog/policy.jsonc`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyDenyListPolicy {
    /// Regex patterns matched against flat keys and joined nested dot-paths.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Execution-override keys denied anywhere regardless of value or nesting.
    #[serde(default)]
    pub presence_keys: Vec<String>,
    /// Execution-override key prefixes denied anywhere regardless of value or nesting.
    #[serde(default)]
    pub presence_key_prefixes: Vec<String>,
    /// Boolean keys denied when evaluated to `true` (typed or string `"true"`).
    #[serde(default)]
    pub deny_true_keys: Vec<String>,
    /// Key-to-forbidden-string-values mappings checked case-insensitively.
    #[serde(default)]
    pub deny_values: BTreeMap<String, Vec<String>>,
    /// Substrings forbidden anywhere inside string setting values (case-insensitive).
    #[serde(default)]
    pub deny_value_substrings: Vec<String>,
}

/// Expected APPLICATION-scoped security setting audited in VS Code's Default profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultProfileExpectation {
    /// Exact expected JSON value (type and value are both enforced).
    pub expected: Value,
    /// Expected JSON type name (`string`, `boolean`, etc.).
    #[serde(rename = "type")]
    pub value_type: String,
    /// Security rationale shown in audit diagnostics.
    #[serde(default)]
    pub reason: String,
}

/// UX symmetry policy block inside `catalog/policy.jsonc`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UxPolicy {
    /// Exact symmetric managed UX setting keys shared by `crisp` and `retina`.
    #[serde(default)]
    pub managed_keys: Vec<String>,
}

/// Parsed `catalog/policy.jsonc` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyManifest {
    /// Schema format version (must be [`SCHEMA_VERSION`]).
    pub version: u32,
    /// Target VS Code baseline version string (semver).
    pub vscode_version: String,
    /// Deprecated or removed VS Code setting keys forbidden across all packs and profiles.
    #[serde(default)]
    pub forbidden_keys: Vec<String>,
    /// Nested-safe autonomy and execution-override deny-list rules.
    #[serde(default)]
    pub autonomy_deny_list: AutonomyDenyListPolicy,
    /// Expected Default-profile security settings (`telemetry.telemetryLevel`, etc.).
    #[serde(default)]
    pub default_profile_expectations: BTreeMap<String, DefaultProfileExpectation>,
    /// UX managed key policy.
    #[serde(default)]
    pub ux: UxPolicy,
    /// Privileged extension IDs guaranteed Tier 1 hash-locking.
    #[serde(default)]
    pub tier1_privileged: Vec<String>,
    /// Authoritative VS Code setting scope and schema rules.
    #[serde(default)]
    pub scopes: BTreeMap<String, SettingScopeSpec>,
}

/// Verification state of an individual [`LockEntry`] inside `vsp.lock.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockEntryStatus {
    /// SHA-256 VSIX digest and Marketplace URL have been verified and recorded.
    Verified,
    /// Entry is structurally locked to its tier and awaiting online `vsp dev pins --refresh`.
    PendingRefresh,
}

/// Per-platform target VSIX digest entry inside a Tier 1 [`LockEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockTargetDigest {
    /// Expected lowercase 64-hex SHA-256 digest of the VSIX package.
    pub sha256: String,
    /// Official Visual Studio Marketplace HTTPS VSIX download URL.
    pub vsix_url: String,
}

/// Individual extension lock record in `vsp.lock.json` (Tiers 1, 2, or 3).
///
/// Tier 0 (`compromised` and `removed`) extensions remain exclusively in
/// `catalog/extensions.jsonc` for unconditional Step 0 uninstall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockEntry {
    /// Security lock tier (`1` = hash-locked, `2` = version-locked, `3` = unlocked declarative).
    pub tier: u8,
    /// Whether the extension distributes a platform-independent universal VSIX.
    #[serde(default)]
    pub universal: Option<bool>,
    /// Verification state (`verified` vs `pending_refresh`) for Tiers 1 and 2.
    #[serde(default)]
    pub status: Option<LockEntryStatus>,
    /// Locked extension version string (semver when verified).
    #[serde(default)]
    pub version: Option<String>,
    /// Universal VSIX lowercase 64-hex SHA-256 digest (Tier 1 verified universal entries).
    #[serde(default)]
    pub sha256: Option<String>,
    /// Official Marketplace HTTPS VSIX download URL (Tier 1 verified universal entries).
    #[serde(default)]
    pub vsix_url: Option<String>,
    /// RFC 3339 verification timestamp (Tier 1 verified entries).
    #[serde(default)]
    pub verified_at: Option<String>,
    /// Per-platform VSIX digests keyed by platform target (`universal`, `darwin-arm64`, etc.).
    #[serde(default)]
    pub targets: BTreeMap<String, LockTargetDigest>,
}

/// Parsed 4-tier per-platform VSIX lockfile (`vsp.lock.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockfileManifest {
    /// Schema format version (must be [`SCHEMA_VERSION`]).
    pub version: u32,
    /// Optional human-readable documentation note.
    #[serde(default)]
    pub note: Option<String>,
    /// Whether unpinned installs must fail closed.
    #[serde(default)]
    pub pin_required: bool,
    /// Supported target platform labels.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Explicit tier entries covering all active extensions in `catalog/extensions.jsonc`.
    #[serde(default)]
    pub extensions: BTreeMap<String, LockEntry>,
}

/// Legacy `<flavor>-{crisp,retina}` alias target declared in `presets.jsonc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyAliasEntry {
    /// Target canonical preset name.
    pub preset: String,
    /// Associated legacy UX preset hint.
    pub ux: UxPreset,
}

/// Result of resolving a preset name, legacy alias, or implicit toolchain preset.
#[derive(Debug, Clone)]
pub struct ResolvedPreset {
    /// Resolved canonical or synthesized [`ProfileRecipe`].
    pub recipe: ProfileRecipe,
    /// Optional UX preset implied by a legacy alias.
    pub ux_hint: Option<UxPreset>,
    /// `true` when synthesized implicitly as `general + <toolchain>` from a toolchain pack ID.
    pub is_implicit_toolchain: bool,
    /// Original legacy alias name if resolved via `legacyAliases`.
    pub alias_source: Option<String>,
}

/// Complete validated v2.2 schema bundle together with its mapped [`WorkspaceCatalog`].
#[derive(Debug, Clone)]
pub struct LoadedSchema {
    /// Domain catalog mapped 1-to-1 into existing [`crate::model`] structs.
    pub catalog: WorkspaceCatalog,
    /// Authoritative extension group display/install order from `catalog/extensions.jsonc`.
    pub group_order: Vec<String>,
    /// Blocked publishers subtracted when deriving allowed publishers.
    pub blocked_publishers: Vec<String>,
    /// Parsed unified security and scope policy from `catalog/policy.jsonc`.
    pub policy: PolicyManifest,
    /// Parsed 4-tier per-platform VSIX lockfile from `vsp.lock.json`.
    pub lockfile: LockfileManifest,
    /// All declared legacy aliases from `presets.jsonc`.
    pub legacy_aliases: BTreeMap<String, LegacyAliasEntry>,
    /// Declarative `retireOnSwitch` rules indexed by pack ID.
    pub retire_on_switch: BTreeMap<String, PackRetireOnSwitch>,
    /// Raw embedded/loaded script bytes indexed by `pack_id -> relative_path` (e.g. `rtk-terminal`).
    pub pack_scripts: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawExtensionsFile {
    version: u32,
    defaults: DefaultsManifest,
    #[serde(default)]
    group_order: Vec<String>,
    #[serde(default)]
    blocked_publishers: Vec<String>,
    #[serde(default)]
    extensions: BTreeMap<String, ExtensionRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPackSettings {
    #[serde(default)]
    workspace: Value,
    #[serde(default)]
    profile: Value,
}

impl Default for RawPackSettings {
    fn default() -> Self {
        Self {
            workspace: Value::Null,
            profile: Value::Null,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPackFile {
    version: u32,
    id: String,
    kind: ModuleKind,
    #[serde(default)]
    description: String,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    default_with: Vec<String>,
    #[serde(default)]
    conflicts: Vec<String>,
    #[serde(default)]
    extensions: ModuleExtensions,
    #[serde(default)]
    settings: RawPackSettings,
    #[serde(default)]
    tasks: Value,
    #[serde(default)]
    detect: Option<DetectSpec>,
    #[serde(default)]
    retire_on_switch: PackRetireOnSwitch,
    #[serde(default)]
    scripts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPresetEntry {
    #[serde(default)]
    description: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    use_defaults: Option<bool>,
    #[serde(default)]
    packs: Vec<String>,
    #[serde(default)]
    exclude_packs: Vec<String>,
    #[serde(default)]
    ux: Option<String>,
    #[serde(default)]
    overrides: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPresetsFile {
    version: u32,
    #[serde(default)]
    presets: BTreeMap<String, RawPresetEntry>,
    #[serde(default)]
    legacy_aliases: BTreeMap<String, LegacyAliasEntry>,
}

/// Returns the compile-time embedded v2.2 snapshot entries in deterministic `LC_ALL=C` order.
#[must_use]
pub fn embedded_snapshot_files() -> &'static [(&'static str, &'static [u8])] {
    embedded_snapshot::EMBEDDED_SNAPSHOT_FILES
}

/// Returns all embedded RTK shell script assets (`packs/rtk-terminal/scripts/...`)
/// in deterministic `LC_ALL=C` order.
#[must_use]
pub fn embedded_rtk_scripts() -> Vec<(&'static str, &'static [u8])> {
    embedded_snapshot_files()
        .iter()
        .copied()
        .filter(|(path, _)| path.starts_with("packs/rtk-terminal/scripts/"))
        .collect()
}

/// Returns `true` when `dir` contains the complete consolidated v2.2 layout
/// (`catalog/extensions.jsonc`, `catalog/policy.jsonc`, `presets.jsonc`,
/// `vsp.lock.json`, and `packs/`).
#[must_use]
pub fn is_valid_v2_root(dir: &Path) -> bool {
    REQUIRED_SNAPSHOT_FILES
        .iter()
        .all(|rel| dir.join(rel).is_file())
        && dir.join("packs").is_dir()
}

/// Derives the `LC_ALL=C`-sorted publisher allowlist dynamically as
/// `{ ext.publisher | ext.status == Active } \ blocked_publishers`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if any active extension belongs to a
/// publisher listed in `blocked_publishers`.
pub fn derive_allowed_publishers(
    extensions: &ExtensionCatalog,
    blocked_publishers: &[String],
) -> VspResult<PublishersManifest> {
    let blocked: BTreeSet<String> = blocked_publishers
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect();

    let mut allowed: BTreeSet<String> = BTreeSet::new();
    for (ext_id, rec) in &extensions.extensions {
        if rec.status == ExtensionStatus::Active {
            let pub_lc = rec.publisher.to_ascii_lowercase();
            if blocked.contains(&pub_lc) {
                return Err(VspError::SafetyRefusal(format!(
                    "active extension '{ext_id}' belongs to blocked publisher '{pub_lc}'"
                )));
            }
            allowed.insert(pub_lc);
        }
    }

    Ok(PublishersManifest {
        version: 1,
        publishers: allowed.into_iter().collect(),
    })
}

/// Builds a zero-copy borrowed index over the compile-time embedded v2.2 snapshot entries.
fn embedded_snapshot_index() -> VspResult<BTreeMap<&'static str, &'static [u8]>> {
    let mut files = BTreeMap::new();
    for &(path, bytes) in embedded_snapshot_files() {
        if files.insert(path, bytes).is_some() {
            return Err(VspError::UsageOrConfig(format!(
                "duplicate path '{path}' in embedded v2.2 snapshot"
            )));
        }
    }
    Ok(files)
}

/// Loads and validates the compile-time embedded v2.2 snapshot into a [`LoadedSchema`].
///
/// Works identically regardless of the current working directory or disk contents and avoids
/// copying static snapshot buffers onto the heap.
///
/// # Errors
///
/// Returns [`VspError`] if the embedded snapshot violates any v2.2 schema invariant.
pub fn load_embedded_schema() -> VspResult<LoadedSchema> {
    let files = embedded_snapshot_index()?;
    load_schema_from_slices(PathBuf::from(EMBEDDED_ROOT_LABEL), &files)
}

/// Loads the compile-time embedded v2.2 snapshot directly into a [`WorkspaceCatalog`],
/// retaining `policy`, `retire_on_switch`, `lockfile`, and `group_order` without copying
/// static snapshot file buffers or cloning manifest payloads.
///
/// # Errors
///
/// Returns [`VspError`] if the embedded snapshot fails strict validation.
pub fn load_embedded() -> VspResult<WorkspaceCatalog> {
    let files = embedded_snapshot_index()?;
    load_catalog_from_slices(PathBuf::from(EMBEDDED_ROOT_LABEL), &files, false).map(|p| p.catalog)
}

fn read_external_v2_snapshot(root: &Path) -> VspResult<(PathBuf, BTreeMap<String, Vec<u8>>)> {
    if !root.exists() {
        return Err(VspError::TargetMissing(format!(
            "catalog directory not found: {}",
            root.display()
        )));
    }
    if !root.is_dir() {
        return Err(VspError::UsageOrConfig(format!(
            "catalog path is not a directory: {}",
            root.display()
        )));
    }
    let canon_root = fs::canonicalize(root).map_err(|err| {
        VspError::TargetMissing(format!(
            "cannot resolve catalog directory {}: {err}",
            root.display()
        ))
    })?;

    let mut files = BTreeMap::new();

    for &rel in REQUIRED_SNAPSHOT_FILES {
        let abs = canon_root.join(rel);
        if !abs.exists() {
            return Err(VspError::UsageOrConfig(format!(
                "incomplete v2.2 catalog at {}: missing required file '{rel}'",
                canon_root.display()
            )));
        }
        let canon = verify_inside_root(&canon_root, &abs)?;
        if !canon.is_file() {
            return Err(VspError::UsageOrConfig(format!(
                "expected regular file at '{rel}' in {}",
                canon_root.display()
            )));
        }
        let bytes = fs::read(&canon).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", canon.display()))
        })?;
        files.insert(rel.to_string(), bytes);
    }

    let packs_dir = canon_root.join("packs");
    if !packs_dir.exists() {
        return Err(VspError::UsageOrConfig(format!(
            "incomplete v2.2 catalog at {}: missing required directory 'packs/'",
            canon_root.display()
        )));
    }
    let canon_packs = verify_inside_root(&canon_root, &packs_dir)?;
    if !canon_packs.is_dir() {
        return Err(VspError::UsageOrConfig(format!(
            "'packs' is not a directory in {}",
            canon_root.display()
        )));
    }
    read_dir_tree_into_snapshot(&canon_root, &canon_packs, &mut files)?;

    Ok((canon_root, files))
}

/// Loads and validates an explicit external v2.2 catalog directory into a [`LoadedSchema`].
///
/// Never falls back to the embedded snapshot when files are missing or invalid.
/// Rejects symlinks or paths escaping `root` with [`VspError::SafetyRefusal`] (exit `5`).
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`): `root` does not exist or is not a directory.
/// - [`VspError::UsageOrConfig`] (exit `4`): required v2.2 files/directories are missing or invalid.
/// - [`VspError::SafetyRefusal`] (exit `5`): symlink or path escapes `root`, or security invariant fails.
/// - [`VspError::Operational`] (exit `1`): IO or JSON/JSONC syntax error.
pub fn load_schema_from_dir(root: &Path) -> VspResult<LoadedSchema> {
    let (canon_root, files) = read_external_v2_snapshot(root)?;
    load_from_snapshot(canon_root, &files)
}

/// Loads an explicit external v2.2 catalog directory directly into a [`WorkspaceCatalog`],
/// retaining `policy`, `retire_on_switch`, `lockfile`, and `group_order`.
///
/// Never falls back to the embedded snapshot.
///
/// # Errors
///
/// Returns [`VspError`] on missing directory, incomplete layout, out-of-root escape, or
/// schema validation failure.
pub fn load_from_dir(root: &Path) -> VspResult<WorkspaceCatalog> {
    let (canon_root, files) = read_external_v2_snapshot(root)?;
    let borrowed: BTreeMap<&str, &[u8]> = files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    load_catalog_from_slices(canon_root, &borrowed, false).map(|p| p.catalog)
}

/// Loads a [`WorkspaceCatalog`] using the authoritative v2.2 CLI resolution policy:
///
/// 1. When `--catalog <DIR>` or non-empty `VSPCLI_ROOT` is explicitly provided:
///    - Rejects missing paths with [`VspError::TargetMissing`] (exit `2`) and non-directories
///      with [`VspError::UsageOrConfig`] (exit `4`).
///    - Detects v2.2 layout (`presets.jsonc` / `packs/`) vs legacy layout
///      (`catalog/extensions.json`, `modules/`, `profiles/`) and loads strictly from that
///      directory. **NEVER** falls back to the embedded snapshot on an explicit external dir.
/// 2. When neither `--catalog` nor `VSPCLI_ROOT` is provided:
///    - Loads directly from [`load_embedded`] (**NEVER** auto-loads an untrusted catalog from
///      the current working directory).
/// 3. Honors an explicit non-empty `ARTAGON_VSCODE_PINS_FILE` environment override for
///    pin manifest resolution (preserving offline test fixtures and fail-closed pin checks).
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] (exit `2`), [`VspError::UsageOrConfig`] (exit `4`),
/// [`VspError::SafetyRefusal`] (exit `5`), or [`VspError::Operational`] (exit `1`).
pub fn load_catalog_auto(cli_catalog: Option<&Path>) -> VspResult<WorkspaceCatalog> {
    let explicit_dir: Option<PathBuf> = if let Some(p) = cli_catalog {
        Some(p.to_path_buf())
    } else {
        std::env::var("VSPCLI_ROOT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    };

    let mut catalog = if let Some(dir) = explicit_dir {
        load_explicit_catalog_dir(&dir)?
    } else {
        load_embedded()?
    };

    if let Ok(override_pins) = std::env::var("ARTAGON_VSCODE_PINS_FILE") {
        let trimmed = override_pins.trim();
        if !trimmed.is_empty() {
            catalog.pins = crate::legacy_loader::load_pins_state(Path::new(trimmed));
        }
    }

    Ok(catalog)
}

/// Loads strictly from an explicit external directory, auto-detecting v2.2 layout
/// (`presets.jsonc` / `packs/`) vs legacy layout (`catalog/extensions.json` + `modules/` + `profiles/`).
///
/// Never falls back to the compile-time embedded snapshot.
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`) when `dir` does not exist.
/// - [`VspError::UsageOrConfig`] (exit `4`) when `dir` is not a directory, is incomplete, or
///   matches neither v2.2 nor legacy layout.
/// - [`VspError::SafetyRefusal`] (exit `5`) on symlink escapes or security invariant failures.
/// - [`VspError::Operational`] (exit `1`) on unreadable or malformed JSON/JSONC.
pub fn load_explicit_catalog_dir(dir: &Path) -> VspResult<WorkspaceCatalog> {
    if !dir.exists() {
        return Err(VspError::TargetMissing(format!(
            "catalog directory not found: {}",
            dir.display()
        )));
    }
    if !dir.is_dir() {
        return Err(VspError::UsageOrConfig(format!(
            "catalog path is not a directory: {}",
            dir.display()
        )));
    }

    let has_v2_markers = is_valid_v2_root(dir)
        || dir.join("presets.jsonc").exists()
        || dir.join("packs").exists()
        || dir.join("catalog/extensions.jsonc").exists()
        || dir.join("catalog/policy.jsonc").exists();

    if has_v2_markers {
        return load_from_dir(dir);
    }

    if crate::legacy_loader::is_valid_legacy_root(dir)
        || dir.join("catalog/extensions.json").is_file()
    {
        return crate::legacy_loader::load_legacy_catalog(dir);
    }

    Err(VspError::UsageOrConfig(format!(
        "invalid catalog root {}: expected v2.2 layout (presets.jsonc + packs/) or legacy layout (catalog/extensions.json + modules/ + profiles/)",
        dir.display()
    )))
}

/// Resolves a physical repository checkout directory for maintainer file operations
/// (`vsp dev hooks`, `vsp dev docs`), supporting both v2.2 and legacy repository layouts.
///
/// # Errors
///
/// Returns [`VspError::TargetMissing`] (exit `2`) if an explicit directory does not exist,
/// or [`VspError::UsageOrConfig`] (exit `4`) if no repository root can be located.
pub fn resolve_repo_dir(cli_catalog: Option<&Path>) -> VspResult<PathBuf> {
    let explicit: Option<PathBuf> = if let Some(p) = cli_catalog {
        Some(p.to_path_buf())
    } else {
        std::env::var("VSPCLI_ROOT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    };

    if let Some(dir) = explicit {
        if !dir.exists() {
            return Err(VspError::TargetMissing(format!(
                "repository directory not found: {}",
                dir.display()
            )));
        }
        return fs::canonicalize(&dir).map_err(|err| {
            VspError::TargetMissing(format!(
                "cannot resolve repository directory {}: {err}",
                dir.display()
            ))
        });
    }

    let is_repo = |d: &Path| {
        is_valid_v2_root(d)
            || crate::legacy_loader::is_valid_legacy_root(d)
            || (d.join("README.md").is_file() && d.join("docs/rust.md").is_file())
    };

    if let Ok(cwd) = std::env::current_dir() {
        let canon = fs::canonicalize(&cwd).unwrap_or(cwd);
        for ancestor in canon.ancestors() {
            if is_repo(ancestor) {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let canon = fs::canonicalize(&exe).unwrap_or(exe);
        for ancestor in canon.ancestors().skip(1) {
            if is_repo(ancestor) {
                return Ok(ancestor.to_path_buf());
            }
        }
    }

    Err(VspError::UsageOrConfig(
        "could not locate repository root (pass --catalog <DIR> or run inside the repository)"
            .to_string(),
    ))
}

struct LoadedCatalogParts {
    catalog: WorkspaceCatalog,
    blocked_publishers: Vec<String>,
    legacy_aliases: BTreeMap<String, LegacyAliasEntry>,
    pack_scripts: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
}

/// Shared strict parser and validator powering both [`load_embedded_schema`] and
/// [`load_schema_from_dir`].
///
/// Validates:
/// - Safe relative snapshot paths (rejects leading `/`, `..`, `\`, or NUL bytes)
/// - Presence of all 4 required manifests and at least one pack under `packs/`
/// - Exact schema version (`2`) on every manifest and pack
/// - Unique, valid pack IDs (`id == filename`) and absence of a dummy `packs/default.jsonc`
/// - Dynamic publisher derivation (`active \ blockedPublishers`) with blocked-active refusal
/// - Strict JSON parsing and 100% active extension coverage of 4-tier `vsp.lock.json`
/// - Verified Tier 1 SHA-256 hex, Marketplace HTTPS URL, and semver validation
/// - Acyclic preset `extends` inheritance and `ai-profile-{crisp,retina} -> ai-copilot` invariant
/// - Clean virtual `default` UX synthesis (`{}` profile settings) with symmetric UX conflicts.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] (exit `4`) on incomplete/invalid schemas,
/// [`VspError::SafetyRefusal`] (exit `5`) on path-escape or security invariant violations,
/// or [`VspError::Operational`] (exit `1`) on JSON/JSONC syntax errors.
pub fn load_from_snapshot(
    root_label: PathBuf,
    files: &BTreeMap<String, Vec<u8>>,
) -> VspResult<LoadedSchema> {
    let borrowed: BTreeMap<&str, &[u8]> = files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    load_schema_from_slices(root_label, &borrowed)
}

fn load_schema_from_slices(
    root_label: PathBuf,
    files: &BTreeMap<&str, &[u8]>,
) -> VspResult<LoadedSchema> {
    let parts = load_catalog_from_slices(root_label, files, true)?;
    let policy = parts.catalog.policy.clone().ok_or_else(|| {
        VspError::Operational("internal error: loaded v2.2 catalog missing policy".to_string())
    })?;
    let lockfile = parts.catalog.lockfile.clone().ok_or_else(|| {
        VspError::Operational("internal error: loaded v2.2 catalog missing lockfile".to_string())
    })?;
    let group_order = parts.catalog.group_order.clone();
    let retire_on_switch = parts.catalog.retire_on_switch.clone();
    Ok(LoadedSchema {
        catalog: parts.catalog,
        group_order,
        blocked_publishers: parts.blocked_publishers,
        policy,
        lockfile,
        legacy_aliases: parts.legacy_aliases,
        retire_on_switch,
        pack_scripts: parts.pack_scripts,
    })
}

fn load_catalog_from_slices(
    root_label: PathBuf,
    files: &BTreeMap<&str, &[u8]>,
    collect_scripts: bool,
) -> VspResult<LoadedCatalogParts> {
    // 1. Validate every relative path key is strictly inside the snapshot root.
    for &rel in files.keys() {
        validate_relative_snapshot_path(rel)?;
    }

    // 2. Require all 4 consolidated root manifests.
    for &req in REQUIRED_SNAPSHOT_FILES {
        if !files.contains_key(req) {
            return Err(VspError::UsageOrConfig(format!(
                "incomplete v2.2 snapshot: missing required file '{req}'"
            )));
        }
    }

    // 3. Parse catalog/extensions.jsonc.
    let ext_bytes = files
        .get("catalog/extensions.jsonc")
        .copied()
        .ok_or_else(|| {
            VspError::UsageOrConfig(
                "incomplete v2.2 snapshot: missing required file 'catalog/extensions.jsonc'"
                    .to_string(),
            )
        })?;
    let raw_exts: RawExtensionsFile = parse_jsonc_bytes(ext_bytes, "catalog/extensions.jsonc")?;
    require_schema_version(raw_exts.version, "catalog/extensions.jsonc")?;
    if raw_exts.group_order.is_empty() {
        return Err(VspError::UsageOrConfig(
            "catalog/extensions.jsonc: groupOrder must not be empty".to_string(),
        ));
    }
    for bp in &raw_exts.blocked_publishers {
        if bp.is_empty() || bp != &bp.to_ascii_lowercase() {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: blockedPublishers entry '{bp}' must be non-empty lowercase"
            )));
        }
    }
    for (ext_id, rec) in &raw_exts.extensions {
        if ext_id != &ext_id.to_ascii_lowercase() {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: extension key '{ext_id}' must be lowercase"
            )));
        }
        let prefix = ext_id.split('.').next().unwrap_or("");
        if rec.publisher != prefix {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: publisher '{}' does not match prefix '{prefix}' for '{ext_id}'",
                rec.publisher
            )));
        }
        if !rec.status.is_valid() {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: unrecognized status '{}' for '{ext_id}'",
                rec.status.as_str()
            )));
        }
        if rec.status == ExtensionStatus::Active
            && !raw_exts
                .group_order
                .iter()
                .any(|g| g.eq_ignore_ascii_case(&rec.group))
        {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: active extension '{ext_id}' group '{}' missing from groupOrder",
                rec.group
            )));
        }
    }

    let extensions_catalog = ExtensionCatalog {
        version: 1,
        extensions: raw_exts.extensions,
    };
    let publishers = derive_allowed_publishers(&extensions_catalog, &raw_exts.blocked_publishers)?;

    // 4. Parse catalog/policy.jsonc.
    let policy_bytes = files.get("catalog/policy.jsonc").copied().ok_or_else(|| {
        VspError::UsageOrConfig(
            "incomplete v2.2 snapshot: missing required file 'catalog/policy.jsonc'".to_string(),
        )
    })?;
    let policy: PolicyManifest = parse_jsonc_bytes(policy_bytes, "catalog/policy.jsonc")?;
    require_schema_version(policy.version, "catalog/policy.jsonc")?;
    if policy.vscode_version.is_empty() || semver::Version::parse(&policy.vscode_version).is_err() {
        return Err(VspError::UsageOrConfig(format!(
            "catalog/policy.jsonc: invalid semver vscodeVersion '{}'",
            policy.vscode_version
        )));
    }
    if policy.ux.managed_keys.is_empty() {
        return Err(VspError::UsageOrConfig(
            "catalog/policy.jsonc: ux.managedKeys must not be empty".to_string(),
        ));
    }
    validate_default_profile_expectations(&policy)?;

    let setting_scopes = SettingScopesManifest {
        version: 1,
        vscode_version: policy.vscode_version.clone(),
        forbidden_keys: policy.forbidden_keys.clone(),
        autonomy_deny_patterns: policy.autonomy_deny_list.patterns.clone(),
        autonomy_deny_presence: policy.autonomy_deny_list.presence_keys.clone(),
        settings: policy.scopes.clone(),
    };

    // 5. Discover and parse packs/** (single-file packs/<id>.jsonc & directory packs/<id>/<id>.jsonc).
    let (mut modules, retire_on_switch, pack_scripts) =
        parse_packs_from_snapshot(&root_label, files, collect_scripts)?;

    // Synthesize virtual `default` UX preset (`{}` profile settings, zero dummy file on disk)
    // and ensure symmetric UX conflict declarations with all loaded UX packs.
    let ux_pack_ids: Vec<String> = modules
        .values()
        .filter(|m| m.manifest.kind == ModuleKind::Ux)
        .map(|m| m.manifest.id.clone())
        .collect();
    for ux_id in &ux_pack_ids {
        if let Some(def) = modules.get_mut(ux_id) {
            if !def.manifest.conflicts.iter().any(|c| c == "default") {
                def.manifest.conflicts.push("default".to_string());
            }
        }
    }
    modules.insert(
        "default".to_string(),
        ModuleDefinition {
            manifest: ModuleManifest {
                version: 1,
                id: "default".to_string(),
                kind: ModuleKind::Ux,
                description:
                    "UX-agnostic default preset (removes all 15 managed UX keys on explicit apply)"
                        .to_string(),
                requires: vec!["core-editor".to_string()],
                default_with: Vec::new(),
                conflicts: ux_pack_ids,
                extensions: ModuleExtensions::default(),
                settings: ModuleSettingsRefs::default(),
                tasks: None,
                detect: None,
            },
            dir: root_label.join("packs"),
            workspace_settings: None,
            profile_settings: Some(Map::new()),
            tasks_doc: None,
        },
    );

    // 6. Parse strict JSON 4-tier vsp.lock.json and validate 100% active coverage.
    let lock_bytes = files.get("vsp.lock.json").copied().ok_or_else(|| {
        VspError::UsageOrConfig(
            "incomplete v2.2 snapshot: missing required file 'vsp.lock.json'".to_string(),
        )
    })?;
    let lockfile: LockfileManifest = serde_json::from_slice(lock_bytes).map_err(|err| {
        VspError::Operational(format!("failed to parse strict JSON vsp.lock.json: {err}"))
    })?;
    require_schema_version(lockfile.version, "vsp.lock.json")?;
    let pins_manifest = validate_and_map_lockfile(
        &lockfile,
        &extensions_catalog,
        &raw_exts.defaults,
        &modules,
        &policy,
    )?;

    // 7. Parse presets.jsonc, resolve `extends` inheritance, and validate legacyAliases.
    let presets_bytes = files.get("presets.jsonc").copied().ok_or_else(|| {
        VspError::UsageOrConfig(
            "incomplete v2.2 snapshot: missing required file 'presets.jsonc'".to_string(),
        )
    })?;
    let raw_presets: RawPresetsFile = parse_jsonc_bytes(presets_bytes, "presets.jsonc")?;
    require_schema_version(raw_presets.version, "presets.jsonc")?;
    let profiles = resolve_presets_map(&raw_presets, &modules)?;
    validate_legacy_aliases(&raw_presets.legacy_aliases, &profiles)?;

    let catalog = WorkspaceCatalog {
        root: root_label,
        defaults: DefaultsManifest {
            version: 1,
            capabilities: raw_exts.defaults.capabilities,
            ux: raw_exts.defaults.ux,
        },
        blocked_publishers: crate::model::derive_blocked_publishers(
            &extensions_catalog,
            &raw_exts.blocked_publishers,
        ),
        extensions: extensions_catalog,
        publishers,
        pins: PinsLoadState::Loaded(pins_manifest),
        setting_scopes: Some(setting_scopes),
        modules,
        profiles,
        legacy_aliases: raw_presets
            .legacy_aliases
            .iter()
            .map(|(alias, entry)| (alias.clone(), (entry.preset.clone(), entry.ux)))
            .collect(),
        policy: Some(policy),
        retire_on_switch,
        lockfile: Some(lockfile),
        group_order: raw_exts.group_order,
    };

    Ok(LoadedCatalogParts {
        catalog,
        blocked_publishers: raw_exts.blocked_publishers,
        legacy_aliases: raw_presets.legacy_aliases,
        pack_scripts,
    })
}

/// Resolves a preset name via:
/// 1. `legacyAliases` in `presets.jsonc` (e.g. `ai-profile-crisp` -> `ai-copilot` + `Crisp`),
/// 2. Explicit canonical presets in `presets.jsonc`, or
/// 3. Implicit toolchain presets (any [`ModuleKind::Toolchain`] pack ID not explicitly
///    overridden resolves as `general + <toolchain>`).
#[must_use]
pub fn resolve_preset_or_toolchain(schema: &LoadedSchema, name: &str) -> Option<ResolvedPreset> {
    if let Some(alias) = schema.legacy_aliases.get(name) {
        let recipe = schema.catalog.profiles.get(&alias.preset)?.clone();
        return Some(ResolvedPreset {
            recipe,
            ux_hint: Some(alias.ux),
            is_implicit_toolchain: false,
            alias_source: Some(name.to_string()),
        });
    }

    if let Some(recipe) = schema.catalog.profiles.get(name) {
        return Some(ResolvedPreset {
            recipe: recipe.clone(),
            ux_hint: None,
            is_implicit_toolchain: false,
            alias_source: None,
        });
    }

    // The same builder backs the CLI (`--preset <toolchain>`), so both give one recipe.
    let recipe = schema.catalog.implicit_toolchain_recipe(name)?;

    Some(ResolvedPreset {
        recipe,
        ux_hint: None,
        is_implicit_toolchain: true,
        alias_source: None,
    })
}

/// Returns the sorted list of Tier 0 blocked extension IDs (`compromised` and `removed`)
/// that must be uninstalled unconditionally in Step 0.
#[must_use]
pub fn tier0_blocked_ids(schema: &LoadedSchema) -> Vec<String> {
    schema
        .catalog
        .extensions
        .extensions
        .iter()
        .filter(|(_, rec)| {
            matches!(
                rec.status,
                ExtensionStatus::Compromised | ExtensionStatus::Removed
            )
        })
        .map(|(id, _)| id.clone())
        .collect()
}

fn require_schema_version(actual: u32, label: &str) -> VspResult<()> {
    if actual != SCHEMA_VERSION {
        return Err(VspError::UsageOrConfig(format!(
            "{label}: unsupported schema version {actual} (expected {SCHEMA_VERSION})"
        )));
    }
    Ok(())
}

fn validate_relative_snapshot_path(rel: &str) -> VspResult<()> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.contains('\\')
        || rel.contains('\0')
        || rel.ends_with('/')
    {
        return Err(VspError::SafetyRefusal(format!(
            "rejected unsafe snapshot path: '{rel}'"
        )));
    }
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(VspError::SafetyRefusal(format!(
                "rejected out-of-root or malformed snapshot path: '{rel}'"
            )));
        }
    }
    Ok(())
}

fn verify_inside_root(canon_root: &Path, candidate: &Path) -> VspResult<PathBuf> {
    let canon = fs::canonicalize(candidate).map_err(|err| {
        VspError::Operational(format!(
            "cannot resolve path {}: {err}",
            candidate.display()
        ))
    })?;
    if !canon.starts_with(canon_root) {
        return Err(VspError::SafetyRefusal(format!(
            "path escapes catalog root {}: {}",
            canon_root.display(),
            candidate.display()
        )));
    }
    Ok(canon)
}

fn read_dir_tree_into_snapshot(
    canon_root: &Path,
    dir: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> VspResult<()> {
    let mut children: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|err| VspError::Operational(format!("failed to list {}: {err}", dir.display())))?
        .map(|res| {
            res.map(|e| e.path()).map_err(|err| {
                VspError::Operational(format!("failed to read entry in {}: {err}", dir.display()))
            })
        })
        .collect::<VspResult<Vec<_>>>()?;
    children.sort();

    for child in children {
        let Some(name) = child.file_name().and_then(|n| n.to_str()) else {
            return Err(VspError::UsageOrConfig(format!(
                "non-UTF-8 path inside catalog: {}",
                child.display()
            )));
        };
        if name == ".DS_Store" {
            continue;
        }
        let canon = verify_inside_root(canon_root, &child)?;
        if canon.is_dir() {
            read_dir_tree_into_snapshot(canon_root, &canon, files)?;
        } else if canon.is_file() {
            let rel = canon
                .strip_prefix(canon_root)
                .map_err(|_| {
                    VspError::SafetyRefusal(format!(
                        "path escapes catalog root: {}",
                        canon.display()
                    ))
                })?
                .components()
                .map(|c| {
                    c.as_os_str().to_str().ok_or_else(|| {
                        VspError::UsageOrConfig(format!(
                            "non-UTF-8 component in {}",
                            canon.display()
                        ))
                    })
                })
                .collect::<VspResult<Vec<_>>>()?
                .join("/");
            let bytes = fs::read(&canon).map_err(|err| {
                VspError::Operational(format!("failed to read {}: {err}", canon.display()))
            })?;
            files.insert(rel, bytes);
        }
    }
    Ok(())
}

fn parse_optional_settings_object(
    value: Value,
    label: &str,
    scope: &str,
) -> VspResult<Option<Map<String, Value>>> {
    match value {
        Value::Null => Ok(None),
        Value::Object(mut map) => {
            map.remove("$schema");
            map.remove("@extends");
            Ok(Some(map))
        }
        _ => Err(VspError::UsageOrConfig(format!(
            "{label}: settings.{scope} must be a JSON object or null"
        ))),
    }
}

fn parse_optional_tasks_object(value: Value, label: &str) -> VspResult<Option<Value>> {
    match value {
        Value::Null => Ok(None),
        obj @ Value::Object(_) => Ok(Some(obj)),
        _ => Err(VspError::UsageOrConfig(format!(
            "{label}: tasks must be a JSON object or null"
        ))),
    }
}

type ParsedPacksResult = (
    BTreeMap<String, ModuleDefinition>,
    BTreeMap<String, PackRetireOnSwitch>,
    BTreeMap<String, BTreeMap<String, Vec<u8>>>,
);

fn parse_packs_from_snapshot(
    root_label: &Path,
    files: &BTreeMap<&str, &[u8]>,
    collect_scripts: bool,
) -> VspResult<ParsedPacksResult> {
    let mut pack_manifests: Vec<(&str, &str, &str, PathBuf, &[u8])> = Vec::new();
    let mut dir_pack_assets: BTreeMap<&str, BTreeMap<String, &[u8]>> = BTreeMap::new();

    for (&rel, &bytes) in files {
        let Some(sub) = rel.strip_prefix("packs/") else {
            continue;
        };
        let parts: Vec<&str> = sub.split('/').collect();
        match parts.as_slice() {
            [file] => {
                let Some(stem) = file.strip_suffix(".jsonc") else {
                    return Err(VspError::UsageOrConfig(format!(
                        "unexpected non-.jsonc file directly under packs/: '{rel}'"
                    )));
                };
                pack_manifests.push((stem, rel, file, root_label.join("packs"), bytes));
            }
            [dir_id, file] if file.ends_with(".jsonc") => {
                let stem = file.strip_suffix(".jsonc").unwrap_or("");
                if stem != *dir_id {
                    return Err(VspError::UsageOrConfig(format!(
                        "directory pack manifest '{rel}' must be named '{dir_id}.jsonc'"
                    )));
                }
                pack_manifests.push((
                    dir_id,
                    rel,
                    file,
                    root_label.join("packs").join(dir_id),
                    bytes,
                ));
            }
            [dir_id, rest @ ..] => {
                if rest.last().is_some_and(|f| f.ends_with(".jsonc")) {
                    return Err(VspError::UsageOrConfig(format!(
                        "unexpected nested .jsonc pack file: '{rel}'"
                    )));
                }
                let asset_rel = rest.join("/");
                dir_pack_assets
                    .entry(dir_id)
                    .or_default()
                    .insert(asset_rel, bytes);
            }
            [] => {}
        }
    }

    if pack_manifests.is_empty() {
        return Err(VspError::UsageOrConfig(
            "incomplete v2.2 snapshot: no pack manifests found under packs/".to_string(),
        ));
    }

    let mut modules = BTreeMap::new();
    let mut retire_on_switch = BTreeMap::new();
    let mut pack_scripts = BTreeMap::new();

    for (expected_id, rel_path, manifest_file, pack_dir, bytes) in pack_manifests {
        if expected_id == "default" {
            return Err(VspError::UsageOrConfig(
                "packs/default.jsonc is forbidden: 'default' UX is virtual (no UX pack file)"
                    .to_string(),
            ));
        }
        if !is_valid_module_id(expected_id) {
            return Err(VspError::UsageOrConfig(format!(
                "invalid pack identifier '{expected_id}' in '{rel_path}' (must match ^[a-z0-9][a-z0-9-]*$)"
            )));
        }

        let raw: RawPackFile = parse_jsonc_bytes(bytes, rel_path)?;
        require_schema_version(raw.version, rel_path)?;
        if raw.id != expected_id {
            return Err(VspError::UsageOrConfig(format!(
                "{rel_path}: pack id '{}' does not match filename '{expected_id}'",
                raw.id
            )));
        }
        if modules.contains_key(&raw.id) {
            return Err(VspError::UsageOrConfig(format!(
                "duplicate pack id '{}' detected at '{rel_path}'",
                raw.id
            )));
        }

        let workspace_settings =
            parse_optional_settings_object(raw.settings.workspace, rel_path, "workspace")?;
        let profile_settings =
            parse_optional_settings_object(raw.settings.profile, rel_path, "profile")?;
        let tasks_doc = parse_optional_tasks_object(raw.tasks, rel_path)?;

        if !raw.scripts.is_empty() {
            let assets = dir_pack_assets.get(raw.id.as_str()).ok_or_else(|| {
                VspError::UsageOrConfig(format!(
                    "{rel_path}: declares scripts {:?} but no directory assets exist under packs/{}/",
                    raw.scripts, raw.id
                ))
            })?;
            let mut scripts_map = BTreeMap::new();
            for script_rel in &raw.scripts {
                validate_relative_snapshot_path(script_rel)?;
                let &data = assets.get(script_rel).ok_or_else(|| {
                    VspError::UsageOrConfig(format!(
                        "{rel_path}: declared script '{script_rel}' missing from packs/{}/",
                        raw.id
                    ))
                })?;
                if collect_scripts {
                    scripts_map.insert(script_rel.clone(), data.to_vec());
                }
            }
            if collect_scripts {
                pack_scripts.insert(raw.id.clone(), scripts_map);
            }
        }

        if !raw.retire_on_switch.is_empty() {
            retire_on_switch.insert(raw.id.clone(), raw.retire_on_switch);
        }

        let settings_refs = ModuleSettingsRefs {
            workspace: workspace_settings
                .as_ref()
                .map(|_| manifest_file.to_string()),
            profile: profile_settings.as_ref().map(|_| manifest_file.to_string()),
        };
        let tasks_ref = tasks_doc.as_ref().map(|_| manifest_file.to_string());

        let manifest = ModuleManifest {
            version: 1,
            id: raw.id.clone(),
            kind: raw.kind,
            description: raw.description,
            requires: raw.requires,
            default_with: raw.default_with,
            conflicts: raw.conflicts,
            extensions: raw.extensions,
            settings: settings_refs,
            tasks: tasks_ref,
            detect: raw.detect,
        };

        modules.insert(
            raw.id,
            ModuleDefinition {
                manifest,
                dir: pack_dir,
                workspace_settings,
                profile_settings,
                tasks_doc,
            },
        );
    }

    Ok((modules, retire_on_switch, pack_scripts))
}

fn validate_default_profile_expectations(policy: &PolicyManifest) -> VspResult<()> {
    for (key, expected_type) in [
        ("telemetry.telemetryLevel", "string"),
        ("security.workspace.trust.untrustedFiles", "string"),
        ("extensions.autoUpdate", "boolean"),
    ] {
        let Some(exp) = policy.default_profile_expectations.get(key) else {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/policy.jsonc: defaultProfileExpectations missing required key '{key}'"
            )));
        };
        if exp.value_type != expected_type {
            return Err(VspError::UsageOrConfig(format!(
                "catalog/policy.jsonc: defaultProfileExpectations['{key}'].type must be '{expected_type}'"
            )));
        }
    }
    Ok(())
}

fn validate_and_map_lockfile(
    lockfile: &LockfileManifest,
    extensions: &ExtensionCatalog,
    defaults: &DefaultsManifest,
    modules: &BTreeMap<String, ModuleDefinition>,
    policy: &PolicyManifest,
) -> VspResult<ExtensionPinsManifest> {
    let mut required_tier1: BTreeSet<String> = BTreeSet::new();
    for cap_id in &defaults.capabilities {
        let def = modules.get(cap_id).ok_or_else(|| {
            VspError::UsageOrConfig(format!(
                "catalog/extensions.jsonc: default capability '{cap_id}' not found in packs/"
            ))
        })?;
        for ext_id in def
            .manifest
            .extensions
            .workspace
            .iter()
            .chain(def.manifest.extensions.profile.iter())
        {
            required_tier1.insert(ext_id.to_ascii_lowercase());
        }
    }
    for (ext_id, rec) in &extensions.extensions {
        if rec.status == ExtensionStatus::Active && rec.group.eq_ignore_ascii_case("AI") {
            required_tier1.insert(ext_id.clone());
        }
    }
    for priv_id in &policy.tier1_privileged {
        required_tier1.insert(priv_id.to_ascii_lowercase());
    }

    for (k, entry) in &lockfile.extensions {
        if k != &k.to_ascii_lowercase() {
            return Err(VspError::UsageOrConfig(format!(
                "vsp.lock.json: non-lowercase extension key '{k}'"
            )));
        }
        if !matches!(entry.tier, 1..=3) {
            return Err(VspError::UsageOrConfig(format!(
                "vsp.lock.json: invalid tier {} for '{k}' (must be 1, 2, or 3; Tier 0 lives in catalog/extensions.jsonc)",
                entry.tier
            )));
        }
        let Some(rec) = extensions.extensions.get(k) else {
            return Err(VspError::UsageOrConfig(format!(
                "vsp.lock.json: unknown extension '{k}' missing from catalog/extensions.jsonc"
            )));
        };
        if rec.status != ExtensionStatus::Active {
            return Err(VspError::UsageOrConfig(format!(
                "vsp.lock.json: non-active extension '{k}' (status={}) must not appear in lockfile",
                rec.status.as_str()
            )));
        }
    }

    let mut pins: BTreeMap<String, ExtensionPin> = BTreeMap::new();

    for (ext_id, rec) in &extensions.extensions {
        if rec.status != ExtensionStatus::Active {
            continue;
        }
        let entry = lockfile.extensions.get(ext_id).ok_or_else(|| {
            VspError::UsageOrConfig(format!(
                "vsp.lock.json: active extension '{ext_id}' is missing an explicit tier entry"
            ))
        })?;

        if required_tier1.contains(ext_id) && entry.tier != 1 {
            return Err(VspError::UsageOrConfig(format!(
                "vsp.lock.json: privileged/default/AI extension '{ext_id}' must be tier 1 (found tier {})",
                entry.tier
            )));
        }

        if entry.tier == 1 {
            let has_universal_digest = entry.sha256.is_some() || entry.vsix_url.is_some();
            if entry.universal == Some(false) && has_universal_digest {
                return Err(VspError::UsageOrConfig(format!(
                    "vsp.lock.json: platform-only Tier 1 pin '{ext_id}' (universal: false) must not declare top-level sha256 or vsixUrl; use per-platform targets"
                )));
            }
            let has_digest = has_universal_digest || !entry.targets.is_empty();
            if entry.status == Some(LockEntryStatus::Verified) || has_digest {
                let ver = entry.version.as_deref().unwrap_or("");
                if ver.is_empty() || semver::Version::parse(ver).is_err() {
                    return Err(VspError::UsageOrConfig(format!(
                        "vsp.lock.json: verified Tier 1 pin '{ext_id}' has invalid semver version '{ver}'"
                    )));
                }
                // Each per-platform target is validated for both record kinds.
                for (platform, target) in &entry.targets {
                    if !is_valid_sha256_hex(&target.sha256) {
                        return Err(VspError::UsageOrConfig(format!(
                            "vsp.lock.json: target '{platform}' for '{ext_id}' has invalid sha256"
                        )));
                    }
                    if !is_valid_marketplace_vsix_url(&target.vsix_url) {
                        return Err(VspError::SafetyRefusal(format!(
                            "vsp.lock.json: target '{platform}' for '{ext_id}' has invalid Marketplace vsixUrl"
                        )));
                    }
                }

                // Universal records (`universal: true` or unset) and platform-only records
                // (`universal: false`) are validated separately:
                // - a universal record needs a top-level sha256 + vsixUrl;
                // - a platform-only record forbids top-level sha256/vsixUrl (checked above),
                //   forbids a `'universal'` target, and needs at least one per-platform target.
                if entry.universal == Some(false) {
                    if entry.targets.is_empty() {
                        return Err(VspError::UsageOrConfig(format!(
                            "vsp.lock.json: verified platform-only Tier 1 pin '{ext_id}' has no per-platform targets"
                        )));
                    }
                    if entry.targets.contains_key("universal") {
                        return Err(VspError::UsageOrConfig(format!(
                            "vsp.lock.json: platform-only Tier 1 pin '{ext_id}' (universal: false) must not declare a 'universal' target"
                        )));
                    }
                } else {
                    let sha = entry.sha256.as_deref().unwrap_or("");
                    let url = entry.vsix_url.as_deref().unwrap_or("");
                    if !is_valid_sha256_hex(sha) {
                        return Err(VspError::UsageOrConfig(format!(
                            "vsp.lock.json: verified Tier 1 pin '{ext_id}' has invalid sha256 '{sha}'"
                        )));
                    }
                    if !is_valid_marketplace_vsix_url(url) {
                        return Err(VspError::SafetyRefusal(format!(
                            "vsp.lock.json: verified Tier 1 pin '{ext_id}' has invalid Marketplace vsixUrl '{url}'"
                        )));
                    }
                    // The flat pin manifest holds universal digests only. A platform-only
                    // record is resolved per platform from the lockfile at install time.
                    pins.insert(
                        ext_id.clone(),
                        ExtensionPin {
                            sha256: sha.to_string(),
                            version: ver.to_string(),
                            vsix_url: url.to_string(),
                            verified_at: entry.verified_at.clone(),
                        },
                    );
                }
            }
        }
    }

    Ok(ExtensionPinsManifest {
        version: 1,
        note: lockfile.note.clone(),
        pin_required: lockfile.pin_required,
        pins,
    })
}

fn resolve_presets_map(
    raw: &RawPresetsFile,
    modules: &BTreeMap<String, ModuleDefinition>,
) -> VspResult<BTreeMap<String, ProfileRecipe>> {
    let mut resolved = BTreeMap::new();
    for name in raw.presets.keys() {
        let mut visiting = Vec::new();
        let recipe = resolve_single_preset(name, raw, &mut visiting, &mut resolved)?;
        for pack_id in recipe.modules.iter().chain(recipe.exclude_modules.iter()) {
            if !modules.contains_key(pack_id) {
                return Err(VspError::UsageOrConfig(format!(
                    "presets.jsonc: preset '{name}' references unknown pack '{pack_id}'"
                )));
            }
        }
        resolved.insert(name.clone(), recipe);
    }
    Ok(resolved)
}

fn resolve_single_preset(
    name: &str,
    raw: &RawPresetsFile,
    visiting: &mut Vec<String>,
    cache: &mut BTreeMap<String, ProfileRecipe>,
) -> VspResult<ProfileRecipe> {
    if let Some(existing) = cache.get(name) {
        return Ok(existing.clone());
    }
    if visiting.iter().any(|v| v == name) {
        return Err(VspError::UsageOrConfig(format!(
            "presets.jsonc: inheritance cycle detected involving preset '{name}'"
        )));
    }
    let entry = raw.presets.get(name).ok_or_else(|| {
        VspError::UsageOrConfig(format!("presets.jsonc: unknown parent preset '{name}'"))
    })?;

    visiting.push(name.to_string());

    let parent = match &entry.extends {
        Some(parent_name) => Some(resolve_single_preset(parent_name, raw, visiting, cache)?),
        None => None,
    };

    visiting.pop();

    let use_defaults = entry
        .use_defaults
        .unwrap_or_else(|| parent.as_ref().is_none_or(|p| p.use_defaults));

    let mut modules = parent
        .as_ref()
        .map(|p| p.modules.clone())
        .unwrap_or_default();
    for pack in &entry.packs {
        if !modules.iter().any(|m| m == pack) {
            modules.push(pack.clone());
        }
    }

    let mut exclude_modules = parent
        .as_ref()
        .map(|p| p.exclude_modules.clone())
        .unwrap_or_default();
    for ex in &entry.exclude_packs {
        if !exclude_modules.iter().any(|m| m == ex) {
            exclude_modules.push(ex.clone());
        }
    }

    let ux = entry.ux.clone().unwrap_or_else(|| {
        parent
            .as_ref()
            .map_or_else(|| "default".to_string(), |p| p.ux.clone())
    });

    let (parent_ws, parent_prof) = parent
        .as_ref()
        .map(ProfileRecipe::split_overrides)
        .unwrap_or_default();
    let (child_ws, child_prof) = split_raw_overrides(&entry.overrides, name)?;
    let merged_ws = recursive_object_multiply(&parent_ws, &child_ws);
    let merged_prof = recursive_object_multiply(&parent_prof, &child_prof);

    let mut ov_root = Map::new();
    ov_root.insert("workspace".to_string(), Value::Object(merged_ws));
    ov_root.insert("profile".to_string(), Value::Object(merged_prof));

    let recipe = ProfileRecipe {
        version: 1,
        name: name.to_string(),
        description: entry.description.clone(),
        use_defaults,
        modules,
        exclude_modules,
        ux,
        overrides: Value::Object(ov_root),
    };
    cache.insert(name.to_string(), recipe.clone());
    Ok(recipe)
}

fn split_raw_overrides(
    overrides: &Value,
    preset_name: &str,
) -> VspResult<(Map<String, Value>, Map<String, Value>)> {
    match overrides {
        Value::Null => Ok((Map::new(), Map::new())),
        Value::Object(map) => {
            if map.is_empty() {
                Ok((Map::new(), Map::new()))
            } else if map.contains_key("workspace") || map.contains_key("profile") {
                let ws = match map.get("workspace") {
                    None | Some(Value::Null) => Map::new(),
                    Some(Value::Object(m)) => m.clone(),
                    Some(_) => {
                        return Err(VspError::UsageOrConfig(format!(
                            "presets.jsonc: preset '{preset_name}' overrides.workspace must be an object"
                        )));
                    }
                };
                let prof = match map.get("profile") {
                    None | Some(Value::Null) => Map::new(),
                    Some(Value::Object(m)) => m.clone(),
                    Some(_) => {
                        return Err(VspError::UsageOrConfig(format!(
                            "presets.jsonc: preset '{preset_name}' overrides.profile must be an object"
                        )));
                    }
                };
                Ok((ws, prof))
            } else {
                Ok((Map::new(), map.clone()))
            }
        }
        _ => Err(VspError::UsageOrConfig(format!(
            "presets.jsonc: preset '{preset_name}' overrides must be an object"
        ))),
    }
}

fn validate_legacy_aliases(
    aliases: &BTreeMap<String, LegacyAliasEntry>,
    profiles: &BTreeMap<String, ProfileRecipe>,
) -> VspResult<()> {
    for (alias, target) in aliases {
        if !profiles.contains_key(&target.preset) {
            return Err(VspError::UsageOrConfig(format!(
                "presets.jsonc: legacyAlias '{alias}' points to unknown preset '{}'",
                target.preset
            )));
        }
    }
    for copilot_alias in ["ai-profile-crisp", "ai-profile-retina"] {
        if let Some(target) = aliases.get(copilot_alias) {
            if target.preset != "ai-copilot" {
                return Err(VspError::SafetyRefusal(format!(
                    "presets.jsonc: security invariant violated — '{copilot_alias}' MUST map to 'ai-copilot', found '{}'",
                    target.preset
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_path_validator_rejects_escapes() {
        assert!(validate_relative_snapshot_path("packs/rust.jsonc").is_ok());
        assert!(validate_relative_snapshot_path("packs/rtk-terminal/scripts/zsh/.zshrc").is_ok());
        assert!(validate_relative_snapshot_path("/etc/passwd").is_err());
        assert!(validate_relative_snapshot_path("../escape.jsonc").is_err());
        assert!(validate_relative_snapshot_path("packs/../escape.jsonc").is_err());
        assert!(validate_relative_snapshot_path("packs\\rust.jsonc").is_err());
        assert!(validate_relative_snapshot_path("").is_err());
    }
}

//! Maintainer operations backing `vsp dev pins`, `vsp dev compat`, `vsp dev docs`, and
//! `vsp dev hooks`.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{ExtensionPinsManifest, PinsLoadState, UxPreset, WorkspaceCatalog};
use crate::policy::{is_valid_marketplace_vsix_url, is_valid_sha256_hex};
use crate::provenance::{now_rfc3339_utc, write_file_atomic};
use crate::resolver::{
    compile_profile_extensions, resolve_profile_recipe, to_canonical_json_string,
};
use crate::schema_loader::LockEntryStatus;
use crate::vscode::{find_binary, find_code_binary, run_code_command};
use crate::vsix::{
    allow_test_pin_urls, build_marketplace_http_agent, build_vsix_http_agent, catalog_is_embedded,
    download_and_cache_vsix_with_agent, load_lockfile_strict, pins_file_override_path,
    query_marketplace_extension_versions, query_marketplace_universal_pin_with_agent,
    resolve_vsix_cache_dir, validate_vsix_url,
};

/// Default VS Code Marketplace extension query endpoint.
pub const DEFAULT_MARKETPLACE_API_URL: &str =
    "https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery";

const RUST_POLICY_BEGIN: &str = "<!-- BEGIN RUST-DOCS-POLICY -->";
const RUST_POLICY_END: &str = "<!-- END RUST-DOCS-POLICY -->";
const README_MATRIX_BEGIN: &str = "<!-- BEGIN GENERATED-MATRIX -->";
const README_MATRIX_END: &str = "<!-- END GENERATED-MATRIX -->";

const AGENT_DOC_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CODEX.md", "GEMINI.md"];

const DEFAULT_PIN_IDS: &[&str] = &[
    "catppuccin.catppuccin-vsc",
    "catppuccin.catppuccin-vsc-icons",
    "editorconfig.editorconfig",
    "enkia.tokyo-night",
    "miguelsolorio.fluent-icons",
    "pkief.material-icon-theme",
    "pkief.material-product-icons",
    "tamasfe.even-better-toml",
];

/// Options controlling [`run_dev_pins_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DevPinsOptions {
    /// Validate lockfile / pins manifest (`--check`).
    pub check: bool,
    /// Refresh lockfile / pins manifest from Marketplace (`--refresh`).
    pub refresh: bool,
    /// Reject any `pending_refresh` (or Tier 2 missing version) entry during `--check`.
    pub strict_release_gate: bool,
    /// Preview planned changes with zero file or cache writes (`--dry-run`).
    pub dry_run: bool,
    /// Emit machine-readable JSON output.
    pub json: bool,
    /// Suppress informational stdout output.
    pub quiet: bool,
}

/// Options controlling [`run_dev_docs_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DevDocsOptions {
    /// Check documentation blocks for drift (`--check`).
    pub check: bool,
    /// Rewrite out-of-sync documentation blocks (`--write`).
    pub write: bool,
    /// Preview planned documentation updates with zero file writes (`--dry-run`).
    pub dry_run: bool,
    /// Emit machine-readable JSON output.
    pub json: bool,
    /// Suppress informational stdout output.
    pub quiet: bool,
}

/// Options controlling [`run_dev_hooks_with_options`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DevHooksOptions {
    /// Preview hook configuration without executing `git config` (`--dry-run`).
    pub dry_run: bool,
    /// Emit machine-readable JSON output.
    pub json: bool,
    /// Suppress informational stdout output.
    pub quiet: bool,
}

/// Refreshes Tier 1 universal, Tier 1 per-platform (`universal: false`), and Tier 2 version
/// lock entries of a v2.2 `vsp.lock.json` in place using shared HTTP agents and warming the
/// XDG VSIX cache (or performing zero writes when `opts.dry_run` is `true`).
fn refresh_lockfile(
    catalog_root: &Path,
    lock_path: &Path,
    opts: DevPinsOptions,
) -> VspResult<ExitCode> {
    let api_url = marketplace_api_url();
    let allow_test = allow_test_pin_urls();
    let cache_dir = (!opts.dry_run).then(|| resolve_vsix_cache_dir(None));
    refresh_lockfile_with_endpoint(
        catalog_root,
        lock_path,
        opts,
        &api_url,
        allow_test,
        cache_dir.as_deref(),
    )
}

fn refresh_lockfile_with_endpoint(
    catalog_root: &Path,
    lock_path: &Path,
    opts: DevPinsOptions,
    api_url: &str,
    allow_test: bool,
    cache_dir: Option<&Path>,
) -> VspResult<ExitCode> {
    let original = fs::read(lock_path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", lock_path.display()))
    })?;
    let mut doc: Value = serde_json::from_slice(&original).map_err(|err| {
        VspError::Operational(format!("malformed {}: {err}", lock_path.display()))
    })?;

    let lock_platforms: Vec<String> = doc
        .get("platforms")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .filter(|p| !p.eq_ignore_ascii_case("universal"))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let entries = doc
        .get_mut("extensions")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            VspError::Operational(format!(
                "{} has no 'extensions' object",
                lock_path.display()
            ))
        })?;

    let mp_agent = build_marketplace_http_agent();
    let vsix_agent = build_vsix_http_agent(allow_test);
    let effective_cache = if opts.dry_run { None } else { cache_dir };
    let now = now_rfc3339_utc();

    let mut refreshed_t1_universal: Vec<String> = Vec::new();
    let mut refreshed_t1_native: Vec<String> = Vec::new();
    let mut refreshed_t2: Vec<String> = Vec::new();
    let mut skipped_native: Vec<String> = Vec::new();

    for (ext_id, entry) in entries.iter_mut() {
        let Some(obj) = entry.as_object_mut() else {
            continue;
        };
        let tier = obj.get("tier").and_then(Value::as_u64);
        match tier {
            Some(1) => {
                let is_universal = obj.get("universal").and_then(Value::as_bool) == Some(true);
                if is_universal {
                    let versions =
                        query_marketplace_extension_versions(&mp_agent, ext_id, api_url)?;
                    let chosen = versions
                        .into_iter()
                        .find(|v| {
                            !v.is_prerelease
                                && v.target_platform
                                    .as_deref()
                                    .is_none_or(|tp| tp.eq_ignore_ascii_case("universal"))
                                && v.vsix_url.is_some()
                        })
                        .ok_or_else(|| {
                            VspError::IncompleteObservation(format!(
                                "no stable universal VSIX version found on Marketplace for '{ext_id}'"
                            ))
                        })?;
                    let vsix_url = chosen.vsix_url.unwrap_or_default();
                    validate_vsix_url(&vsix_url, allow_test)?;
                    let sha256 = download_and_cache_vsix_with_agent(
                        &vsix_agent,
                        &vsix_url,
                        ext_id,
                        effective_cache,
                        allow_test,
                    )?;
                    if !opts.quiet && !opts.json {
                        println!("pinned {}@{} ({})", ext_id, chosen.version, sha256);
                    }
                    obj.insert("status".to_string(), Value::String("verified".to_string()));
                    obj.insert("version".to_string(), Value::String(chosen.version));
                    obj.insert("sha256".to_string(), Value::String(sha256.clone()));
                    obj.insert("vsixUrl".to_string(), Value::String(vsix_url.clone()));
                    obj.insert("verifiedAt".to_string(), Value::String(now.clone()));
                    let mut targets = obj
                        .get("targets")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    targets.insert(
                        "universal".to_string(),
                        serde_json::json!({ "sha256": sha256, "vsixUrl": vsix_url }),
                    );
                    obj.insert("targets".to_string(), Value::Object(targets));
                    refreshed_t1_universal.push(ext_id.clone());
                } else {
                    // Platform-specific Tier 1 (`universal: false`): query per-platform targets.
                    let versions =
                        query_marketplace_extension_versions(&mp_agent, ext_id, api_url)?;
                    let mut new_targets = serde_json::Map::new();
                    let mut selected_version: Option<String> = None;

                    for plat in &lock_platforms {
                        let matched = versions.iter().find(|v| {
                            !v.is_prerelease
                                && v.target_platform
                                    .as_deref()
                                    .is_some_and(|tp| tp.eq_ignore_ascii_case(plat))
                                && v.vsix_url.is_some()
                        });
                        if let Some(ver_info) = matched {
                            if let Some(vsix_url) = ver_info.vsix_url.as_deref() {
                                validate_vsix_url(vsix_url, allow_test)?;
                                let sha = download_and_cache_vsix_with_agent(
                                    &vsix_agent,
                                    vsix_url,
                                    ext_id,
                                    effective_cache,
                                    allow_test,
                                )?;
                                if selected_version.is_none() {
                                    selected_version = Some(ver_info.version.clone());
                                }
                                new_targets.insert(
                                    plat.clone(),
                                    serde_json::json!({ "sha256": sha, "vsixUrl": vsix_url }),
                                );
                            }
                        }
                    }

                    if let Some(ver) = selected_version {
                        // Astra A16 invariant: non-universal entries must never declare flat
                        // top-level `sha256` or `vsixUrl`.
                        obj.remove("sha256");
                        obj.remove("vsixUrl");
                        obj.insert("status".to_string(), Value::String("verified".to_string()));
                        obj.insert("version".to_string(), Value::String(ver.clone()));
                        obj.insert("verifiedAt".to_string(), Value::String(now.clone()));
                        obj.insert("targets".to_string(), Value::Object(new_targets));
                        if !opts.quiet && !opts.json {
                            println!("pinned native {ext_id}@{ver}");
                        }
                        refreshed_t1_native.push(ext_id.clone());
                    } else {
                        skipped_native.push(ext_id.clone());
                    }
                }
            }
            Some(2) => {
                // Tier 2 version locking: find the latest stable semver version from Marketplace.
                let versions = query_marketplace_extension_versions(&mp_agent, ext_id, api_url)?;
                if let Some(ver_info) = versions
                    .iter()
                    .find(|v| !v.is_prerelease && semver::Version::parse(&v.version).is_ok())
                {
                    obj.insert("status".to_string(), Value::String("verified".to_string()));
                    obj.insert(
                        "version".to_string(),
                        Value::String(ver_info.version.clone()),
                    );
                    obj.insert("verifiedAt".to_string(), Value::String(now.clone()));
                    if !opts.quiet && !opts.json {
                        println!("locked Tier 2 {}@{}", ext_id, ver_info.version);
                    }
                    refreshed_t2.push(ext_id.clone());
                }
            }
            _ => {}
        }
    }

    let mut all_refreshed: Vec<String> = Vec::new();
    all_refreshed.extend(refreshed_t1_universal.iter().cloned());
    all_refreshed.extend(refreshed_t1_native.iter().cloned());
    all_refreshed.extend(refreshed_t2.iter().cloned());

    if !opts.dry_run {
        let content = to_canonical_json_string(&doc)?;
        write_file_atomic(lock_path, content.as_bytes(), 0o644, true)?;

        // Reload through the real loader. A lock that the loader rejects must not stay on disk.
        if !allow_test {
            if let Err(err) = crate::schema_loader::load_schema_from_dir(catalog_root) {
                write_file_atomic(lock_path, &original, 0o644, true)?;
                return Err(VspError::Operational(format!(
                    "refreshed {} failed verification and was restored: {err}",
                    lock_path.display()
                )));
            }
        }
    }

    if opts.json {
        let payload = serde_json::json!({
            "status": if opts.dry_run { "dry_run" } else { "refreshed" },
            "dryRun": opts.dry_run,
            "path": lock_path.display().to_string(),
            "count": all_refreshed.len(),
            "refreshed": all_refreshed,
            "refreshedTier1Universal": refreshed_t1_universal,
            "refreshedTier1PlatformSpecific": refreshed_t1_native,
            "refreshedTier2": refreshed_t2,
            "skippedPlatformSpecific": skipped_native,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else if !opts.quiet {
        let verb = if opts.dry_run { "Would write" } else { "Wrote" };
        println!(
            "{verb} {} ({} Tier 1 universal, {} Tier 1 platform-specific, {} Tier 2 refreshed; {} platform-specific unchanged)",
            lock_path.display(),
            refreshed_t1_universal.len(),
            refreshed_t1_native.len(),
            refreshed_t2.len(),
            skipped_native.len()
        );
    }
    Ok(ExitCode::Ok)
}

fn marketplace_api_url() -> String {
    env::var("VSP_MARKETPLACE_API_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MARKETPLACE_API_URL.to_string())
}

/// Validates (`--check`) or refreshes (`--refresh`) `vsp.lock.json` (v2.2 layout) or
/// `catalog/extension-pins.json` (legacy layout).
///
/// Forwards to [`run_dev_pins_with_options`] with `dry_run = false` and
/// `strict_release_gate = false`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if both `--check` and `--refresh` are set,
/// [`VspError::Operational`] on pin format violations, or [`VspError::IncompleteObservation`]
/// (exit `7`) if the Marketplace API fails during `--refresh`.
pub fn run_dev_pins(
    catalog: &WorkspaceCatalog,
    check: bool,
    refresh: bool,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    run_dev_pins_with_options(
        catalog,
        DevPinsOptions {
            check,
            refresh,
            strict_release_gate: false,
            dry_run: false,
            json,
            quiet,
        },
    )
}

/// Validates (`--check`) or refreshes (`--refresh`) `vsp.lock.json` or
/// `catalog/extension-pins.json` with explicit [`DevPinsOptions`] (supporting `dry_run` and
/// `strict_release_gate`).
///
/// When `opts.dry_run` is `true`, performs ZERO file writes and ZERO VSIX cache writes.
///
/// # Errors
///
/// Same as [`run_dev_pins`].
pub fn run_dev_pins_with_options(
    catalog: &WorkspaceCatalog,
    opts: DevPinsOptions,
) -> VspResult<ExitCode> {
    if opts.check && opts.refresh {
        return Err(VspError::UsageOrConfig(
            "vsp dev pins: cannot combine --check and --refresh".to_string(),
        ));
    }

    if opts.refresh {
        let env_pins = pins_file_override_path();
        let pins_path = if let Some(path) = env_pins {
            path
        } else {
            if catalog_is_embedded(catalog) {
                return Err(VspError::UsageOrConfig(
                    "vsp dev pins --refresh requires --catalog <REPO_DIR> when using embedded defaults"
                        .to_string(),
                ));
            }
            // A v2.2 catalog is locked by `vsp.lock.json`. Refresh that file: the
            // loader never reads `catalog/extension-pins.json` in this layout.
            let lock_path = catalog.root.join("vsp.lock.json");
            if lock_path.is_file() {
                return refresh_lockfile(&catalog.root, &lock_path, opts);
            }
            catalog.root.join("catalog/extension-pins.json")
        };

        let mut target_ids: BTreeSet<String> =
            DEFAULT_PIN_IDS.iter().map(|id| (*id).to_string()).collect();
        if let PinsLoadState::Loaded(existing) = &catalog.pins {
            for k in existing.pins.keys() {
                target_ids.insert(k.to_ascii_lowercase());
            }
        }

        let api_url = marketplace_api_url();
        let allow_test = allow_test_pin_urls();
        let mp_agent = build_marketplace_http_agent();
        let vsix_agent = build_vsix_http_agent(allow_test);
        let cache_dir = (!opts.dry_run).then(|| resolve_vsix_cache_dir(None));
        let now = now_rfc3339_utc();
        let mut pins = BTreeMap::new();
        for ext_id in &target_ids {
            let mut pin = query_marketplace_universal_pin_with_agent(&mp_agent, ext_id, &api_url)?;
            let digest = download_and_cache_vsix_with_agent(
                &vsix_agent,
                &pin.vsix_url,
                ext_id,
                cache_dir.as_deref(),
                allow_test,
            )?;
            pin.sha256 = digest;
            pin.verified_at = Some(now.clone());
            if !opts.quiet && !opts.json {
                println!("pinned {}@{} ({})", ext_id, pin.version, pin.sha256);
            }
            pins.insert(ext_id.clone(), pin);
        }

        let loaded_pin_required = match &catalog.pins {
            PinsLoadState::Loaded(existing) => existing.pin_required,
            _ => false,
        };
        let manifest = ExtensionPinsManifest {
            version: 1,
            note: Some(
                "Install-time verified SHA-256 VSIX pins for universal core/UX extensions. Refresh via vsp dev pins --refresh."
                    .to_string(),
            ),
            pin_required: loaded_pin_required,
            pins,
        };

        if !opts.dry_run {
            let content = to_canonical_json_string(&manifest)?;
            write_file_atomic(&pins_path, content.as_bytes(), 0o644, true)?;
        }

        if opts.json {
            let payload = serde_json::json!({
                "status": if opts.dry_run { "dry_run" } else { "refreshed" },
                "dryRun": opts.dry_run,
                "path": pins_path.display().to_string(),
                "count": manifest.pins.len(),
            });
            print!("{}", to_canonical_json_string(&payload)?);
        } else if !opts.quiet {
            let verb = if opts.dry_run { "Would write" } else { "Wrote" };
            println!("{verb} {}", pins_path.display());
        }
        return Ok(ExitCode::Ok);
    }

    // Default or `--check`: validate v2.2 `vsp.lock.json` when using v2.2 layout without an
    // explicit `ARTAGON_VSCODE_PINS_FILE` override; otherwise validate flat pins manifest.
    if catalog.is_v2_layout() && pins_file_override_path().is_none() {
        return check_v2_lockfile(catalog, opts);
    }

    match &catalog.pins {
        PinsLoadState::Loaded(manifest) => {
            let mut errors = Vec::new();
            for (k, pin) in &manifest.pins {
                if k != &k.to_ascii_lowercase() {
                    errors.push(format!("non-lowercase pin key: {k}"));
                }
                if !is_valid_sha256_hex(&pin.sha256) {
                    errors.push(format!("invalid sha256 for pin {k}"));
                }
                if !is_valid_marketplace_vsix_url(&pin.vsix_url) {
                    errors.push(format!("invalid vsix_url for pin {k}: {}", pin.vsix_url));
                }
                if pin.version.is_empty() || semver::Version::parse(&pin.version).is_err() {
                    errors.push(format!(
                        "invalid semver version for pin {k}: {}",
                        pin.version
                    ));
                }
            }

            let source_label = pins_file_override_path().map_or_else(
                || "catalog/extension-pins.json".to_string(),
                |p| p.display().to_string(),
            );

            if opts.json {
                let payload = serde_json::json!({
                    "source": source_label,
                    "status": if errors.is_empty() { "ok" } else { "failed" },
                    "count": manifest.pins.len(),
                    "pinRequired": manifest.pin_required,
                    "errors": errors,
                });
                print!("{}", to_canonical_json_string(&payload)?);
            } else if errors.is_empty() {
                if !opts.quiet {
                    println!(
                        "{source_label}: {} valid pinned extension(s)",
                        manifest.pins.len()
                    );
                }
            } else {
                for e in &errors {
                    eprintln!("FAILED: {e}");
                }
            }

            if errors.is_empty() {
                Ok(ExitCode::Ok)
            } else {
                Ok(ExitCode::Operational)
            }
        }
        PinsLoadState::Missing(path) => Err(VspError::Operational(format!(
            "missing extension pins manifest: {}",
            path.display()
        ))),
        PinsLoadState::Invalid { path, error } => Err(VspError::Operational(format!(
            "invalid extension pins manifest {}: {error}",
            path.display()
        ))),
    }
}

fn check_v2_lockfile(catalog: &WorkspaceCatalog, opts: DevPinsOptions) -> VspResult<ExitCode> {
    let lockfile = load_lockfile_strict(catalog)?.ok_or_else(|| {
        VspError::Operational("missing vsp.lock.json in v2.2 catalog".to_string())
    })?;

    let mut errors = Vec::new();
    let mut verified_count = 0usize;
    let mut pending_t1 = 0usize;
    let mut pending_t2 = 0usize;

    for (k, entry) in &lockfile.extensions {
        if k != &k.to_ascii_lowercase() {
            errors.push(format!("vsp.lock.json: non-lowercase key: {k}"));
        }
        match entry.tier {
            1 => {
                if entry.status == Some(LockEntryStatus::PendingRefresh) {
                    pending_t1 += 1;
                    if opts.strict_release_gate {
                        errors.push(format!(
                            "vsp.lock.json: Tier 1 entry '{k}' is pending_refresh (blocked by release gate)"
                        ));
                    }
                    continue;
                }
                if entry.status != Some(LockEntryStatus::Verified) {
                    errors.push(format!(
                        "vsp.lock.json: Tier 1 entry '{k}' missing verified/pending_refresh status"
                    ));
                    continue;
                }
                verified_count += 1;
                if entry
                    .version
                    .as_deref()
                    .is_none_or(|v| semver::Version::parse(v).is_err())
                {
                    errors.push(format!(
                        "vsp.lock.json: invalid semver version for Tier 1 '{k}': {:?}",
                        entry.version
                    ));
                }
                if entry.universal == Some(false) {
                    if entry.sha256.is_some() || entry.vsix_url.is_some() {
                        errors.push(format!(
                            "vsp.lock.json: non-universal Tier 1 '{k}' must not declare top-level sha256/vsixUrl"
                        ));
                    }
                    if entry.targets.is_empty() {
                        errors.push(format!(
                            "vsp.lock.json: verified non-universal Tier 1 '{k}' has no platform targets"
                        ));
                    }
                } else {
                    let sha = entry.sha256.as_deref().unwrap_or("");
                    let url = entry.vsix_url.as_deref().unwrap_or("");
                    if !is_valid_sha256_hex(sha) {
                        errors.push(format!("vsp.lock.json: invalid sha256 for Tier 1 '{k}'"));
                    }
                    if !is_valid_marketplace_vsix_url(url) {
                        errors.push(format!(
                            "vsp.lock.json: invalid vsixUrl for Tier 1 '{k}': {url}"
                        ));
                    }
                }
                for (plat, tgt) in &entry.targets {
                    if !is_valid_sha256_hex(&tgt.sha256) {
                        errors.push(format!(
                            "vsp.lock.json: invalid sha256 for Tier 1 '{k}' target '{plat}'"
                        ));
                    }
                    if !is_valid_marketplace_vsix_url(&tgt.vsix_url) {
                        errors.push(format!(
                            "vsp.lock.json: invalid vsixUrl for Tier 1 '{k}' target '{plat}': {}",
                            tgt.vsix_url
                        ));
                    }
                }
            }
            2 => {
                if entry.status == Some(LockEntryStatus::PendingRefresh)
                    || entry.version.as_deref().is_none_or(str::is_empty)
                {
                    pending_t2 += 1;
                    if opts.strict_release_gate {
                        errors.push(format!(
                            "vsp.lock.json: Tier 2 entry '{k}' is pending_refresh / unpinned (blocked by release gate)"
                        ));
                    }
                    continue;
                }
                verified_count += 1;
                if let Some(ver) = entry.version.as_deref() {
                    if semver::Version::parse(ver).is_err() {
                        errors.push(format!(
                            "vsp.lock.json: invalid semver version for Tier 2 '{k}': {ver}"
                        ));
                    }
                }
            }
            3 => {}
            other => {
                errors.push(format!("vsp.lock.json: unexpected tier {other} for '{k}'"));
            }
        }
    }

    if opts.json {
        let payload = serde_json::json!({
            "source": "vsp.lock.json",
            "status": if errors.is_empty() { "ok" } else { "failed" },
            "count": lockfile.extensions.len(),
            "verified": verified_count,
            "pendingTier1": pending_t1,
            "pendingTier2": pending_t2,
            "pinRequired": lockfile.pin_required,
            "strictReleaseGate": opts.strict_release_gate,
            "errors": errors,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else if errors.is_empty() {
        if !opts.quiet {
            println!(
                "vsp.lock.json: {} valid lock entry(ies) ({verified_count} verified, {pending_t1} pending Tier 1, {pending_t2} pending Tier 2)",
                lockfile.extensions.len()
            );
        }
    } else {
        for e in &errors {
            eprintln!("FAILED: {e}");
        }
    }

    if errors.is_empty() {
        Ok(ExitCode::Ok)
    } else {
        Ok(ExitCode::Operational)
    }
}

/// Options for `vsp dev compat`.
#[derive(Debug, Clone, Default)]
pub struct CompatOptions {
    /// Audit all canonical profiles in the catalog.
    pub all: bool,
    /// Optional newline-separated or JSON array extensions file.
    pub extensions_file: Option<PathBuf>,
    /// Override detected VS Code semver (e.g. `1.147.0`).
    pub vscode_version: Option<String>,
    /// Specific profile names to audit.
    pub profiles: Vec<String>,
}

/// Single audited extension entry emitted by `vsp dev compat --json`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatEntry {
    /// Extension identifier (`publisher.name`).
    pub id: String,
    /// Audit status (`compatible`, `incompatible`, or `NOT_FOUND`).
    pub status: String,
    /// Selected Marketplace version, if found.
    pub version: Option<String>,
    /// Declared `engines.vscode` range, if found.
    pub engine: Option<String>,
    /// Whether the selected version is a pre-release.
    pub prerelease: bool,
}

/// Summary counts for `vsp dev compat`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatSummary {
    /// Total unique extension IDs audited.
    pub total: usize,
    /// Number of compatible extensions.
    pub compatible: usize,
    /// Number of incompatible extensions.
    pub incompatible: usize,
    /// Number of extensions missing from Marketplace.
    pub not_found: usize,
}

/// Complete structured report emitted by `vsp dev compat`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatReport {
    /// Target VS Code semver evaluated against.
    pub vscode_version: String,
    /// Aggregate summary counts.
    pub summary: CompatSummary,
    /// Per-extension compatibility findings.
    pub extensions: Vec<CompatEntry>,
}

/// Executes `vsp dev compat` against the VS Code Marketplace API.
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`): unknown profile or missing extensions file.
/// - [`VspError::IncompleteObservation`] (exit `7`): unable to determine VS Code version or
///   Marketplace verification unavailable.
pub fn run_dev_compat(
    catalog: &WorkspaceCatalog,
    opts: &CompatOptions,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    let vscode_ver = resolve_target_vscode_version(catalog, opts.vscode_version.as_deref())?;
    let audit_ids = collect_compat_extension_ids(catalog, opts)?;

    let api_url = marketplace_api_url();
    let agent = build_marketplace_http_agent();

    let mut mp_extensions: Vec<Value> = Vec::new();
    for chunk in audit_ids.chunks(40) {
        let resp = query_marketplace_compat_batch(&agent, &api_url, chunk, 529)?;
        let arr = resp
            .pointer("/results/0/extensions")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                VspError::IncompleteObservation(
                    "compat: Marketplace API response has no results[0].extensions array (verification incomplete)"
                        .to_string(),
                )
            })?;
        mp_extensions.extend(arr.iter().cloned());
    }

    let mut entries = Vec::with_capacity(audit_ids.len());
    let mut compatible = 0usize;
    let mut incompatible = 0usize;
    let mut not_found = 0usize;

    for id in &audit_ids {
        let id_lc = id.to_ascii_lowercase();
        let found = mp_extensions.iter().find(|ext| {
            let p = ext
                .pointer("/publisher/publisherName")
                .and_then(Value::as_str)
                .unwrap_or("");
            let n = ext
                .get("extensionName")
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("{p}.{n}").eq_ignore_ascii_case(&id_lc)
        });

        let Some(ext_obj) = found else {
            not_found += 1;
            entries.push(CompatEntry {
                id: id.clone(),
                status: "NOT_FOUND".to_string(),
                version: None,
                engine: None,
                prerelease: false,
            });
            continue;
        };

        let Some((mut ver, mut eng, mut pre)) =
            pick_extension_version(ext_obj).map_err(|why| incomplete_engine_evidence(id, &why))?
        else {
            not_found += 1;
            entries.push(CompatEntry {
                id: id.clone(),
                status: "NOT_FOUND".to_string(),
                version: None,
                engine: None,
                prerelease: false,
            });
            continue;
        };

        if pre {
            let fb_resp =
                query_marketplace_compat_batch(&agent, &api_url, std::slice::from_ref(id), 17)?;
            let fb_ext = fb_resp
                .pointer("/results/0/extensions")
                .and_then(Value::as_array)
                .and_then(|arr| {
                    arr.iter().find(|e| {
                        let p = e
                            .pointer("/publisher/publisherName")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let n = e.get("extensionName").and_then(Value::as_str).unwrap_or("");
                        format!("{p}.{n}").eq_ignore_ascii_case(&id_lc)
                    })
                })
                .ok_or_else(|| {
                    VspError::IncompleteObservation(format!(
                        "compat: verification unavailable for '{id}' (missing stable history)"
                    ))
                })?;
            if let Some((fb_ver, fb_eng, false)) = pick_extension_version(fb_ext)
                .map_err(|why| incomplete_engine_evidence(id, &why))?
            {
                ver = fb_ver;
                eng = fb_eng;
                pre = false;
            }
        }

        let is_compat = semver_satisfies(&vscode_ver, &eng);
        let status = if is_compat {
            compatible += 1;
            "compatible"
        } else {
            incompatible += 1;
            "incompatible"
        };
        entries.push(CompatEntry {
            id: id.clone(),
            status: status.to_string(),
            version: Some(ver),
            engine: Some(eng),
            prerelease: pre,
        });
    }

    let report = CompatReport {
        vscode_version: vscode_ver,
        summary: CompatSummary {
            total: audit_ids.len(),
            compatible,
            incompatible,
            not_found,
        },
        extensions: entries,
    };

    if json {
        print!("{}", to_canonical_json_string(&report)?);
    } else if !quiet {
        println!(
            "VS Code version: {} | total={} compatible={} incompatible={} not_found={}",
            report.vscode_version,
            report.summary.total,
            report.summary.compatible,
            report.summary.incompatible,
            report.summary.not_found
        );
        for item in &report.extensions {
            if item.status != "compatible" {
                eprintln!(
                    "  {}: {} (version={}, engine={})",
                    item.status,
                    item.id,
                    item.version.as_deref().unwrap_or("none"),
                    item.engine.as_deref().unwrap_or("none")
                );
            }
        }
    }

    if report.summary.incompatible > 0 || report.summary.not_found > 0 {
        Ok(ExitCode::Operational)
    } else {
        Ok(ExitCode::Ok)
    }
}

fn resolve_target_vscode_version(
    catalog: &WorkspaceCatalog,
    override_ver: Option<&str>,
) -> VspResult<String> {
    if let Some(raw) = override_ver {
        return parse_clean_semver(raw);
    }
    if let Some(code_bin) = find_code_binary() {
        let mut cmd = Command::new(&code_bin);
        cmd.arg("--version");
        if let Ok(out) = run_code_command(&mut cmd, "code --version") {
            if out.success {
                if let Some(first) = out.stdout.lines().next() {
                    if let Ok(clean) = parse_clean_semver(first.trim()) {
                        return Ok(clean);
                    }
                }
            }
        }
    }
    if let Some(scopes) = &catalog.setting_scopes {
        if !scopes.vscode_version.is_empty() {
            if let Ok(clean) = parse_clean_semver(&scopes.vscode_version) {
                return Ok(clean);
            }
        }
    }
    Err(VspError::IncompleteObservation(
        "compat: unable to determine VS Code version (install 'code' or pass --vscode-version)"
            .to_string(),
    ))
}

fn parse_clean_semver(raw: &str) -> VspResult<String> {
    let base = raw
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");
    let parsed = semver::Version::parse(base)
        .map_err(|_| VspError::UsageOrConfig(format!("invalid VS Code semver version: '{raw}'")))?;
    Ok(format!(
        "{}.{}.{}",
        parsed.major, parsed.minor, parsed.patch
    ))
}

fn collect_compat_extension_ids(
    catalog: &WorkspaceCatalog,
    opts: &CompatOptions,
) -> VspResult<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    let mut push_ci = |id: &str| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return;
        }
        if seen.insert(trimmed.to_ascii_lowercase()) {
            ids.push(trimmed.to_string());
        }
    };

    if let Some(file) = &opts.extensions_file {
        if !file.is_file() {
            return Err(VspError::TargetMissing(format!(
                "extensions file not found: {}",
                file.display()
            )));
        }
        let raw = fs::read_to_string(file).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", file.display()))
        })?;
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    push_ci(s);
                } else if let Some(s) = item.pointer("/identifier/id").and_then(Value::as_str) {
                    push_ci(s);
                }
            }
        } else {
            for line in raw.lines() {
                let no_comment = line.split('#').next().unwrap_or("").trim();
                push_ci(no_comment);
            }
        }
    }

    let target_profiles: Vec<String> =
        if opts.all || (opts.profiles.is_empty() && opts.extensions_file.is_none()) {
            catalog.profiles.keys().cloned().collect()
        } else {
            opts.profiles.clone()
        };

    for prof in &target_profiles {
        if !catalog.profiles.contains_key(prof) {
            return Err(VspError::TargetMissing(format!("unknown profile '{prof}'")));
        }
        let resolved =
            resolve_profile_recipe(catalog, prof, &[], &[], Some(UxPreset::Default), None)?;
        let exts = compile_profile_extensions(catalog, &resolved)?;
        for entry in exts {
            push_ci(&entry.identifier.id);
        }
    }

    Ok(ids)
}

fn query_marketplace_compat_batch(
    agent: &ureq::Agent,
    api_url: &str,
    ids: &[String],
    flags: u32,
) -> VspResult<Value> {
    let criteria: Vec<Value> = ids
        .iter()
        .map(|id| serde_json::json!({ "filterType": 7, "value": id }))
        .collect();
    let payload = serde_json::json!({
        "filters": [{
            "criteria": criteria,
            "pageNumber": 1,
            "pageSize": 100
        }],
        "assetTypes": [],
        "flags": flags
    });
    let mut resp = agent
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json;api-version=3.0-preview.1")
        .send_json(&payload)
        .map_err(|err| {
            VspError::IncompleteObservation(format!(
                "compat: Marketplace API request failed: {err}"
            ))
        })?;
    let val: Value = resp.body_mut().read_json().map_err(|err| {
        VspError::IncompleteObservation(format!(
            "compat: Marketplace API returned malformed JSON: {err}"
        ))
    })?;
    if !val.get("results").is_some_and(Value::is_array) {
        return Err(VspError::IncompleteObservation(
            "compat: Marketplace API response missing results array".to_string(),
        ));
    }
    Ok(val)
}

fn incomplete_engine_evidence(ext_id: &str, why: &str) -> VspError {
    VspError::IncompleteObservation(format!(
        "compat: verification unavailable for '{ext_id}' ({why})"
    ))
}

fn pick_extension_version(ext_obj: &Value) -> Result<Option<(String, String, bool)>, String> {
    let Some(versions) = ext_obj.get("versions").and_then(Value::as_array) else {
        return Ok(None);
    };
    if versions.is_empty() {
        return Ok(None);
    }
    let is_pre = |v: &Value| {
        v.get("properties")
            .and_then(Value::as_array)
            .is_some_and(|props| {
                props.iter().any(|p| {
                    p.get("key").and_then(Value::as_str)
                        == Some("Microsoft.VisualStudio.Code.PreRelease")
                        && p.get("value").and_then(Value::as_str) == Some("true")
                })
            })
    };
    let (chosen, pre) = match versions.iter().find(|v| !is_pre(v)) {
        Some(stable) => (stable, false),
        None => (&versions[0], true),
    };
    let ver = chosen
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| "version entry has no version string".to_string())?
        .to_string();
    let engine = chosen
        .get("properties")
        .and_then(Value::as_array)
        .and_then(|props| {
            props.iter().find(|p| {
                p.get("key").and_then(Value::as_str) == Some("Microsoft.VisualStudio.Code.Engine")
            })
        })
        .and_then(|p| p.get("value").and_then(Value::as_str))
        .filter(|range| !range.trim().is_empty())
        .ok_or_else(|| format!("version {ver} has no Microsoft.VisualStudio.Code.Engine property"))?
        .to_string();
    Ok(Some((ver, engine, pre)))
}

/// Evaluates whether `vscode_ver` (`major.minor.patch`) satisfies a VS Code `engines.vscode`
/// semver range expression via [`semver::Version`] and [`semver::VersionReq`] (supporting `||`,
/// space-AND clauses, `^`, `~`, `>=`, `<=`, `>`, `<`, `=`, bare exact versions, `*`, and `x`/`X`
/// wildcards).
#[must_use]
pub fn semver_satisfies(vscode_ver: &str, range: &str) -> bool {
    let trimmed = range.trim();
    if trimmed.is_empty() || matches!(trimmed, "*" | "x" | "X") {
        return true;
    }
    let clean_ver = vscode_ver
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");
    let Ok(ver) = semver::Version::parse(clean_ver) else {
        return false;
    };

    // Collapse whitespace immediately following operators (e.g. `>= 1.80.0` -> `>=1.80.0`).
    let mut norm = trimmed.to_string();
    for op in [">=", "<=", ">", "<", "^", "~", "="] {
        let pat = format!("{op} ");
        while norm.contains(&pat) {
            norm = norm.replace(&pat, op);
        }
    }

    norm.split("||").any(|branch| {
        let clauses: Vec<String> = branch
            .split_whitespace()
            .filter_map(normalize_vscode_semver_clause)
            .collect();
        let raw_count = branch.split_whitespace().count();
        if raw_count == 0 || clauses.len() != raw_count {
            return false;
        }
        semver::VersionReq::parse(&clauses.join(", ")).is_ok_and(|req| req.matches(&ver))
    })
}

/// Normalizes a single VS Code / npm comparator token into [`semver::VersionReq`] syntax.
fn normalize_vscode_semver_clause(raw: &str) -> Option<String> {
    let clause = raw.trim();
    if clause.is_empty() {
        return None;
    }
    if matches!(clause, "*" | "x" | "X") {
        return Some("*".to_string());
    }
    let (op, rest) = [">=", "<=", ">", "<", "^", "~", "="]
        .into_iter()
        .find_map(|prefix| clause.strip_prefix(prefix).map(|r| (prefix, r)))
        .unwrap_or(("", clause));

    let body = rest.trim_start_matches('v').split('-').next().unwrap_or("");
    if body.is_empty() {
        return None;
    }
    if matches!(body, "*" | "x" | "X") {
        return Some("*".to_string());
    }

    let has_wild = body.split('.').any(|p| matches!(p, "*" | "x" | "X"));
    if op.is_empty() {
        if has_wild {
            Some(body.to_string())
        } else if body.split('.').count() == 3 {
            // Bare `1.80.0` in VS Code / npm means exact match, whereas Cargo's VersionReq
            // treats bare `1.80.0` as caret `^1.80.0`.
            Some(format!("={body}"))
        } else {
            // Partial bare version (`1` or `1.80`) means wildcard (`1.*` / `1.80.*`).
            Some(format!("{body}.*"))
        }
    } else if op == "=" && has_wild {
        Some(body.to_string())
    } else {
        Some(format!("{op}{body}"))
    }
}

/// Checks (`--check`, default) or updates (`--write`) both:
/// 1. The Rust docs policy sentinel block (`<!-- BEGIN/END RUST-DOCS-POLICY -->`) extracted
///    from §12 of `docs/rust.md` across `AGENTS.md`, `CLAUDE.md`, `CODEX.md`, and `GEMINI.md`.
/// 2. The generated Profile & Pack Matrix sentinel block (`<!-- BEGIN/END GENERATED-MATRIX -->`)
///    in `README.md`.
///
/// Forwards to [`run_dev_docs_with_options`] with `dry_run = false`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if both `--check` and `--write` are specified, or
/// [`VspError::Operational`] on missing source files or malformed marker pairs.
pub fn run_dev_docs(
    catalog: &WorkspaceCatalog,
    check: bool,
    write: bool,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    run_dev_docs_with_options(
        catalog,
        DevDocsOptions {
            check,
            write,
            dry_run: false,
            json,
            quiet,
        },
    )
}

/// Checks or updates repository documentation blocks with explicit [`DevDocsOptions`].
///
/// When `opts.dry_run` is `true`, performs ZERO file writes while reporting every file that
/// would be updated.
///
/// # Errors
///
/// Same as [`run_dev_docs`].
pub fn run_dev_docs_with_options(
    catalog: &WorkspaceCatalog,
    opts: DevDocsOptions,
) -> VspResult<ExitCode> {
    if opts.check && opts.write {
        return Err(VspError::UsageOrConfig(
            "vsp dev docs: cannot combine --check and --write".to_string(),
        ));
    }
    let write_mode = opts.write;
    let should_write = write_mode && !opts.dry_run;
    let mut drifts = Vec::new();
    let mut updated = Vec::new();
    let docs_root = if catalog_is_embedded(catalog) {
        crate::schema_loader::resolve_repo_dir(None)?
    } else {
        catalog.root.clone()
    };

    let rust_doc_path = docs_root.join("docs/rust.md");
    let readme_path = docs_root.join("README.md");
    for (label, path) in [
        ("docs/rust.md", &rust_doc_path),
        ("README.md", &readme_path),
    ] {
        if !path.is_file() {
            return Err(VspError::Operational(format!(
                "vsp dev docs: required source {label} is missing in {}",
                docs_root.display()
            )));
        }
    }

    // 1. Rust docs policy sentinel sync/check across agent docs.
    if rust_doc_path.is_file() {
        let policy_body = extract_rust_policy_block(&rust_doc_path)?;
        let expected_block = format!("{RUST_POLICY_BEGIN}\n{policy_body}\n{RUST_POLICY_END}");

        if write_mode {
            for rel in AGENT_DOC_FILES {
                let target = docs_root.join(rel);
                if target.is_file() {
                    let content = fs::read_to_string(&target).map_err(|err| {
                        VspError::Operational(format!("failed to read {}: {err}", target.display()))
                    })?;
                    if let MarkerState::Malformed(reason) =
                        inspect_marker_pair(&content, RUST_POLICY_BEGIN, RUST_POLICY_END)
                    {
                        return Err(VspError::Operational(format!(
                            "{}: {reason}; refusing to rewrite",
                            target.display()
                        )));
                    }
                }
            }
        }

        for rel in AGENT_DOC_FILES {
            let target = docs_root.join(rel);
            if !target.is_file() {
                if write_mode {
                    if should_write {
                        let out = format!("{expected_block}\n");
                        write_file_atomic(&target, out.as_bytes(), 0o644, false)?;
                    }
                    updated.push((*rel).to_string());
                } else {
                    drifts.push(format!("{rel} missing"));
                }
                continue;
            }
            let content = fs::read_to_string(&target).map_err(|err| {
                VspError::Operational(format!("failed to read {}: {err}", target.display()))
            })?;
            match inspect_marker_pair(&content, RUST_POLICY_BEGIN, RUST_POLICY_END) {
                MarkerState::Valid(current_block) => {
                    if current_block != expected_block {
                        if write_mode {
                            if should_write {
                                let new_content = replace_marker_block(
                                    &content,
                                    RUST_POLICY_BEGIN,
                                    RUST_POLICY_END,
                                    &expected_block,
                                );
                                write_file_atomic(&target, new_content.as_bytes(), 0o644, false)?;
                            }
                            updated.push((*rel).to_string());
                        } else {
                            drifts.push(format!(
                                "{rel} RUST-DOCS-POLICY block out of sync with docs/rust.md §12"
                            ));
                        }
                    }
                }
                MarkerState::None => {
                    if write_mode {
                        if should_write {
                            let mut new_content = content;
                            if !new_content.ends_with('\n') {
                                new_content.push('\n');
                            }
                            new_content.push('\n');
                            new_content.push_str(&expected_block);
                            new_content.push('\n');
                            write_file_atomic(&target, new_content.as_bytes(), 0o644, false)?;
                        }
                        updated.push((*rel).to_string());
                    } else {
                        drifts.push(format!("{rel} lacks RUST-DOCS-POLICY sentinels"));
                    }
                }
                MarkerState::Malformed(reason) => {
                    drifts.push(format!("{rel}: {reason}"));
                }
            }
        }
    }

    // 2. README.md generated matrix block sync/check.
    if readme_path.is_file() {
        let expected_matrix = render_readme_matrix_block(catalog)?;
        let readme_content = fs::read_to_string(&readme_path).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", readme_path.display()))
        })?;
        match inspect_marker_pair(&readme_content, README_MATRIX_BEGIN, README_MATRIX_END) {
            MarkerState::Valid(current_block) => {
                if current_block != expected_matrix {
                    if write_mode {
                        if should_write {
                            let new_content = replace_marker_block(
                                &readme_content,
                                README_MATRIX_BEGIN,
                                README_MATRIX_END,
                                &expected_matrix,
                            );
                            write_file_atomic(&readme_path, new_content.as_bytes(), 0o644, false)?;
                        }
                        updated.push("README.md".to_string());
                    } else {
                        drifts.push("README.md GENERATED-MATRIX block is out of sync".to_string());
                    }
                }
            }
            MarkerState::None => {
                drifts.push("README.md lacks BEGIN/END GENERATED-MATRIX sentinels".to_string());
            }
            MarkerState::Malformed(reason) => {
                drifts.push(format!("README.md: {reason}"));
            }
        }
    }

    let mode_label = if opts.dry_run {
        "dry_run"
    } else if write_mode {
        "write"
    } else {
        "check"
    };

    if opts.json {
        let payload = serde_json::json!({
            "mode": mode_label,
            "dryRun": opts.dry_run,
            "inSync": drifts.is_empty(),
            "updated": updated,
            "drifts": drifts,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else if !opts.quiet {
        if write_mode {
            if updated.is_empty() {
                println!("ok: all docs already in sync");
            } else if opts.dry_run {
                println!("would update: {}", updated.join(", "));
            } else {
                println!("updated: {}", updated.join(", "));
            }
        } else if drifts.is_empty() {
            println!("ok: RUST-DOCS-POLICY and README.md matrix blocks are in sync");
        } else {
            for d in &drifts {
                eprintln!("drift: {d}");
            }
        }
    }

    if drifts.is_empty() {
        Ok(ExitCode::Ok)
    } else {
        Ok(ExitCode::Operational)
    }
}

/// Extracts §12 (`# 12. Docs Policy for AI Agents`) inner ` ```md ... ``` ` block from `docs/rust.md`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `rust.md` cannot be read or contains an empty §12 block.
pub fn extract_rust_policy_block(rust_md_path: &Path) -> VspResult<String> {
    let raw = fs::read_to_string(rust_md_path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", rust_md_path.display()))
    })?;
    let mut found_section = false;
    let mut in_block = false;
    let mut inner_fence = false;
    let mut lines = Vec::new();

    for line in raw.lines() {
        if !found_section {
            if line.starts_with("# 12. Docs Policy for AI Agents") {
                found_section = true;
            }
            continue;
        }
        if !in_block {
            if line == "```md" {
                in_block = true;
            }
            continue;
        }
        if line.starts_with("```")
            && line.len() > 3
            && line[3..].bytes().all(|b| b.is_ascii_lowercase())
        {
            inner_fence = true;
            lines.push(line);
            continue;
        }
        if line == "```" {
            if inner_fence {
                inner_fence = false;
                lines.push(line);
                continue;
            }
            break;
        }
        lines.push(line);
    }

    if lines.is_empty() {
        return Err(VspError::Operational(format!(
            "empty or missing §12 Docs Policy block in {}",
            rust_md_path.display()
        )));
    }
    Ok(lines.join("\n"))
}

/// Renders the deterministic `<!-- BEGIN GENERATED-MATRIX --> ... <!-- END GENERATED-MATRIX -->`
/// block labeling `Packs` cleanly and referencing bundle filenames without dead raw-branch
/// `exports/*.code-profile` markdown links.
///
/// # Errors
///
/// Returns [`VspError`] if any profile fails module resolution.
pub fn render_readme_matrix_block(catalog: &WorkspaceCatalog) -> VspResult<String> {
    let mut out = String::new();
    out.push_str(README_MATRIX_BEGIN);
    out.push('\n');
    let _ = writeln!(
        out,
        "### Profiles ({} Compiled Recipes)\n",
        catalog.profiles.len()
    );
    out.push_str("| Profile | Description | Included Packs | Extensions | Bundle Artifact |\n");
    out.push_str("| --- | --- | --- | ---: | --- |\n");

    for (prof_name, recipe) in &catalog.profiles {
        let resolved =
            resolve_profile_recipe(catalog, prof_name, &[], &[], Some(UxPreset::Default), None)?;
        let exts = compile_profile_extensions(catalog, &resolved)?;
        let packs_str = resolved.join(", ");
        let bundle_cell = format!("`{prof_name}.code-profile`");
        let _ = writeln!(
            out,
            "| `{prof_name}` | {} | `{packs_str}` | {} | {bundle_cell} |",
            recipe.description,
            exts.len()
        );
    }

    out.push_str("\n### Packs (`packs/`)\n\n");
    out.push_str("| Kind | Pack ID | Description | Requires | Conflicts |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");

    for kind in [
        crate::model::ModuleKind::Capability,
        crate::model::ModuleKind::Toolchain,
        crate::model::ModuleKind::Ux,
    ] {
        for def in catalog.modules.values() {
            if def.manifest.kind != kind {
                continue;
            }
            let reqs = if def.manifest.requires.is_empty() {
                "—".to_string()
            } else {
                def.manifest.requires.join(", ")
            };
            let confs = if def.manifest.conflicts.is_empty() {
                "—".to_string()
            } else {
                def.manifest.conflicts.join(", ")
            };
            let _ = writeln!(
                out,
                "| `{}` | `{}` | {} | {reqs} | {confs} |",
                def.manifest.kind.as_str(),
                def.manifest.id,
                def.manifest.description
            );
        }
    }

    out.push_str(README_MATRIX_END);
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
enum MarkerState {
    None,
    Valid(String),
    Malformed(String),
}

fn inspect_marker_pair(content: &str, begin_mark: &str, end_mark: &str) -> MarkerState {
    let b_raw_tok = begin_mark
        .trim_start_matches("<!-- ")
        .trim_end_matches(" -->");
    let e_raw_tok = end_mark
        .trim_start_matches("<!-- ")
        .trim_end_matches(" -->");

    let mut b_cnt = 0usize;
    let mut e_cnt = 0usize;
    let mut b_raw = 0usize;
    let mut e_raw = 0usize;
    let mut b_line = 0usize;
    let mut e_line = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let lineno = idx + 1;
        let bc = line.matches(begin_mark).count();
        if bc > 0 {
            if b_line == 0 {
                b_line = lineno;
            }
            b_cnt += bc;
        }
        let ec = line.matches(end_mark).count();
        if ec > 0 {
            if e_line == 0 {
                e_line = lineno;
            }
            e_cnt += ec;
        }
        b_raw += line.matches(b_raw_tok).count();
        e_raw += line.matches(e_raw_tok).count();
    }

    if b_raw != b_cnt || e_raw != e_cnt {
        return MarkerState::Malformed(format!(
            "corrupted sentinel (exact begin={b_cnt} end={e_cnt}, raw begin={b_raw} end={e_raw})"
        ));
    }
    if b_cnt == 0 && e_cnt == 0 {
        return MarkerState::None;
    }
    if b_cnt == 1 && e_cnt == 1 && b_line < e_line {
        let block: Vec<&str> = content
            .lines()
            .skip(b_line - 1)
            .take(e_line - b_line + 1)
            .collect();
        return MarkerState::Valid(block.join("\n"));
    }
    if b_cnt > 0 && e_cnt == 0 {
        return MarkerState::Malformed(format!("unmatched BEGIN marker (begin={b_cnt}, end=0)"));
    }
    if b_cnt == 0 && e_cnt > 0 {
        return MarkerState::Malformed(format!("unmatched END marker (begin=0, end={e_cnt})"));
    }
    if b_cnt != 1 || e_cnt != 1 {
        return MarkerState::Malformed(format!(
            "multiple policy markers (begin={b_cnt}, end={e_cnt})"
        ));
    }
    MarkerState::Malformed(format!(
        "marker order invalid (begin line {b_line} >= end line {e_line})"
    ))
}

fn replace_marker_block(
    content: &str,
    begin_mark: &str,
    end_mark: &str,
    replacement: &str,
) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.contains(begin_mark) {
            in_block = true;
            out.push(replacement);
            continue;
        }
        if line.contains(end_mark) {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Configures `git config core.hooksPath scripts/git-hooks` idempotently.
///
/// Forwards to [`run_dev_hooks_with_options`] with `dry_run = false`.
/// Refuses to overwrite an existing non-default `core.hooksPath` with
/// [`VspError::SafetyRefusal`] (exit `5`).
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if a different `core.hooksPath` is already
/// configured, or [`VspError::Operational`] if `git` fails or a hook script is not executable.
pub fn run_dev_hooks(repo_root: &Path, json: bool, quiet: bool) -> VspResult<ExitCode> {
    run_dev_hooks_with_options(
        repo_root,
        DevHooksOptions {
            dry_run: false,
            json,
            quiet,
        },
    )
}

/// Configures `git config core.hooksPath scripts/git-hooks` with explicit [`DevHooksOptions`].
///
/// When `opts.dry_run` is `true`, validates the hook scripts on disk and reports the planned
/// configuration without invoking `git config`.
///
/// # Errors
///
/// Same as [`run_dev_hooks`].
pub fn run_dev_hooks_with_options(repo_root: &Path, opts: DevHooksOptions) -> VspResult<ExitCode> {
    let target = "scripts/git-hooks";

    // Verify the hook BEFORE any `git config` write or success report.
    verify_hooks_usable(&repo_root.join(target))?;

    if opts.dry_run {
        if opts.json {
            print!(
                "{}",
                to_canonical_json_string(&serde_json::json!({
                    "status": "dry_run",
                    "dryRun": true,
                    "hooksPath": target,
                }))?
            );
        } else if !opts.quiet {
            println!("vsp dev hooks (dry-run): would set core.hooksPath = {target}");
        }
        return Ok(ExitCode::Ok);
    }

    let git_bin = find_binary("git")
        .ok_or_else(|| VspError::Operational("git executable not found on PATH".to_string()))?;
    let get_out = Command::new(&git_bin)
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .map_err(|err| VspError::Operational(format!("failed to invoke git: {err}")))?;

    let current = if get_out.status.success() {
        String::from_utf8_lossy(&get_out.stdout).trim().to_string()
    } else {
        String::new()
    };

    if current.is_empty() {
        let set_out = Command::new(&git_bin)
            .arg("-C")
            .arg(repo_root)
            .args(["config", "core.hooksPath", target])
            .output()
            .map_err(|err| VspError::Operational(format!("failed to invoke git config: {err}")))?;
        if !set_out.status.success() {
            return Err(VspError::Operational(format!(
                "git config core.hooksPath failed: {}",
                String::from_utf8_lossy(&set_out.stderr).trim()
            )));
        }
        if opts.json {
            print!(
                "{}",
                to_canonical_json_string(&serde_json::json!({
                    "status": "installed",
                    "hooksPath": target,
                }))?
            );
        } else if !opts.quiet {
            println!("vsp dev hooks: set core.hooksPath = {target}");
        }
        return Ok(ExitCode::Ok);
    }

    if current == target {
        if opts.json {
            print!(
                "{}",
                to_canonical_json_string(&serde_json::json!({
                    "status": "already_installed",
                    "hooksPath": target,
                }))?
            );
        } else if !opts.quiet {
            println!("vsp dev hooks: already installed (core.hooksPath = {target})");
        }
        return Ok(ExitCode::Ok);
    }

    Err(VspError::SafetyRefusal(format!(
        "refusing to overwrite existing core.hooksPath = {current} (expected unset or '{target}')"
    )))
}

fn verify_hooks_usable(hooks_dir: &Path) -> VspResult<()> {
    if !hooks_dir.is_dir() {
        return Err(VspError::Operational(format!(
            "hooks directory {} is missing; core.hooksPath was not changed",
            hooks_dir.display()
        )));
    }
    let pre_commit = hooks_dir.join("pre-commit");
    if !pre_commit.is_file() {
        return Err(VspError::Operational(format!(
            "required hook {} is missing; core.hooksPath was not changed",
            pre_commit.display()
        )));
    }
    fs::File::open(&pre_commit).map_err(|err| {
        VspError::Operational(format!(
            "hook {} is not readable: {err}",
            pre_commit.display()
        ))
    })?;

    let entries = fs::read_dir(hooks_dir).map_err(|err| {
        VspError::Operational(format!(
            "cannot list hooks directory {}: {err}",
            hooks_dir.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            VspError::Operational(format!(
                "cannot list hooks directory {}: {err}",
                hooks_dir.display()
            ))
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .map_err(|err| {
                    VspError::Operational(format!("cannot examine {}: {err}", path.display()))
                })?
                .permissions()
                .mode();
            if mode & 0o111 == 0 {
                return Err(VspError::Operational(format!(
                    "hook {} is not executable; run 'chmod +x {}'",
                    path.display(),
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_loader::load_from_dir;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn semver_range_evaluator_matches_vscode_engines() {
        // Caret semantics (`^`)
        assert!(semver_satisfies("1.147.0", "^1.80.0"));
        assert!(!semver_satisfies("1.79.0", "^1.80.0"));
        assert!(!semver_satisfies("2.0.0", "^1.80.0"));
        assert!(semver_satisfies("0.8.5", "^0.8.0"));
        assert!(!semver_satisfies("0.9.0", "^0.8.0"));
        assert!(semver_satisfies("0.0.3", "^0.0.3"));
        assert!(!semver_satisfies("0.0.4", "^0.0.3"));
        assert!(semver_satisfies("1.85.0", "^v1.80.0-insider"));

        // Tilde semantics (`~`)
        assert!(semver_satisfies("1.80.9", "~1.80.0"));
        assert!(!semver_satisfies("1.81.0", "~1.80.0"));
        assert!(!semver_satisfies("1.79.9", "~1.80.0"));

        // Compound space-AND and OR (`||`)
        assert!(semver_satisfies("1.147.0", ">= 1.80.0 <2.0.0"));
        assert!(!semver_satisfies("2.0.0", ">= 1.80.0 <2.0.0"));
        assert!(semver_satisfies("1.147.0", "^1.99.0 || ^1.100.0"));
        assert!(!semver_satisfies("1.50.0", "^1.99.0 || ^1.100.0"));

        // Wildcards (`*`, `x`, `X`) and bare exact vs partial versions
        assert!(semver_satisfies("1.147.0", "*"));
        assert!(semver_satisfies("1.147.0", "x"));
        assert!(semver_satisfies("1.147.0", "1.x"));
        assert!(semver_satisfies("1.147.0", "1.X"));
        assert!(semver_satisfies("1.80.5", "1.80.x"));
        assert!(!semver_satisfies("1.81.0", "1.80.x"));
        assert!(semver_satisfies("1.80.5", "=1.80.x"));
        assert!(semver_satisfies("1.80.0", "1.80.0"));
        assert!(!semver_satisfies("1.80.1", "1.80.0"));
        assert!(semver_satisfies("1.80.5", "1.80"));
        assert!(!semver_satisfies("1.81.0", "1.80"));

        // Prerelease VS Code version + invalid inputs
        assert!(semver_satisfies("1.147.0-insider", "^1.80.0"));
        assert!(!semver_satisfies("not-a-version", "^1.80.0"));
        assert!(!semver_satisfies("1.147.0", "not-a-range"));
    }

    /// Astra A03: missing engine metadata is missing evidence, not `*`.
    #[test]
    fn missing_engine_metadata_is_not_treated_as_compatible() {
        let with_engine = serde_json::json!({"versions": [{"version": "1.2.3", "properties": [
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.80.0"}]}]});
        assert_eq!(
            pick_extension_version(&with_engine),
            Ok(Some(("1.2.3".to_string(), "^1.80.0".to_string(), false)))
        );
        let explicit_star = serde_json::json!({"versions": [{"version": "1.0.0", "properties": [
            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "*"}]}]});
        assert!(matches!(
            pick_extension_version(&explicit_star),
            Ok(Some(_))
        ));

        for missing in [
            serde_json::json!({"versions": [{"version": "1.2.3"}]}),
            serde_json::json!({"versions": [{"version": "1.2.3", "properties": []}]}),
            serde_json::json!({"versions": [{"version": "1.2.3", "properties": [
                {"key": "Microsoft.VisualStudio.Code.Engine", "value": " "}]}]}),
            serde_json::json!({"versions": [{"properties": []}]}),
        ] {
            assert!(pick_extension_version(&missing).is_err(), "{missing}");
        }
        assert_eq!(
            pick_extension_version(&serde_json::json!({"versions": []})),
            Ok(None)
        );
        assert_eq!(
            incomplete_engine_evidence("a.b", "x").exit_code(),
            ExitCode::IncompleteObservation
        );
    }

    /// Astra A04: the hook is verified before `core.hooksPath` is written, and `dry_run`
    /// never mutates git config.
    #[cfg(unix)]
    #[test]
    fn dev_hooks_requires_executable_pre_commit_before_git_config() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().expect("tmp");
        let repo = tmp.path();
        let init = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["init", "-q"])
            .status()
            .expect("git init");
        assert!(init.success());
        let hooks_path = || {
            let out = Command::new("git")
                .arg("-C")
                .arg(repo)
                .args(["config", "--get", "core.hooksPath"])
                .output()
                .expect("git config");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        assert_eq!(
            run_dev_hooks(repo, false, true)
                .expect_err("missing dir")
                .exit_code(),
            ExitCode::Operational
        );
        let hooks = repo.join("scripts/git-hooks");
        fs::create_dir_all(&hooks).expect("mkdir hooks");
        assert!(run_dev_hooks(repo, false, true).is_err());
        let pre_commit = hooks.join("pre-commit");
        fs::write(&pre_commit, "#!/bin/sh\nexit 0\n").expect("write hook");
        fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(run_dev_hooks(repo, false, true).is_err());
        assert_eq!(
            hooks_path(),
            "",
            "core.hooksPath must not be set by a refused run"
        );

        // Usable hook: dry-run succeeds with ZERO git config mutations.
        fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o755)).expect("chmod");
        assert_eq!(
            run_dev_hooks_with_options(
                repo,
                DevHooksOptions {
                    dry_run: true,
                    json: false,
                    quiet: true,
                }
            )
            .expect("dry-run hooks"),
            ExitCode::Ok
        );
        assert_eq!(hooks_path(), "", "dry-run must not set core.hooksPath");

        assert_eq!(
            run_dev_hooks(repo, false, true).expect("install"),
            ExitCode::Ok
        );
        assert_eq!(hooks_path(), "scripts/git-hooks");
        assert_eq!(
            run_dev_hooks(repo, false, true).expect("again"),
            ExitCode::Ok
        );

        fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(run_dev_hooks(repo, false, true).is_err());
    }

    /// Astra A05: a missing source document fails the docs check.
    #[test]
    fn dev_docs_check_fails_when_a_source_document_is_missing() {
        let mut catalog = load_from_dir(&repo_root()).expect("catalog");
        let tmp = tempfile::TempDir::new().expect("tmp");
        catalog.root = tmp.path().to_path_buf();
        let err = run_dev_docs(&catalog, true, false, false, true).expect_err("no sources");
        assert_eq!(err.exit_code(), ExitCode::Operational);
        assert!(err.to_string().contains("docs/rust.md"));

        fs::create_dir_all(tmp.path().join("docs")).expect("mkdir docs");
        fs::copy(
            repo_root().join("docs/rust.md"),
            tmp.path().join("docs/rust.md"),
        )
        .expect("copy rust.md");
        let err = run_dev_docs(&catalog, true, false, false, true).expect_err("no README");
        assert!(err.to_string().contains("README.md"));
    }

    /// SOL-08 + CARBON-09: `dev docs` renders `Packs` without dead `exports/*.code-profile`
    /// markdown links, honors `dry_run = true` with zero writes, and passes `--check` after `--write`.
    #[test]
    fn dev_docs_round_trip_renders_packs_and_honors_dry_run() {
        let root = repo_root();
        let mut catalog = load_from_dir(&root).expect("v2 catalog");
        let tmp = tempfile::TempDir::new().expect("tmp");
        fs::create_dir_all(tmp.path().join("docs")).expect("mkdir docs");
        fs::copy(root.join("docs/rust.md"), tmp.path().join("docs/rust.md")).expect("copy rust.md");
        fs::copy(root.join("README.md"), tmp.path().join("README.md")).expect("copy README.md");
        for doc in AGENT_DOC_FILES {
            fs::copy(root.join(doc), tmp.path().join(doc)).expect("copy agent doc");
        }
        catalog.root = tmp.path().to_path_buf();

        let readme_before = fs::read_to_string(tmp.path().join("README.md")).expect("read before");

        // Dry-run write must perform zero writes.
        let dry = run_dev_docs_with_options(
            &catalog,
            DevDocsOptions {
                check: false,
                write: true,
                dry_run: true,
                json: false,
                quiet: true,
            },
        )
        .expect("dry-run docs");
        assert_eq!(dry, ExitCode::Ok);
        assert_eq!(
            fs::read_to_string(tmp.path().join("README.md")).expect("read after dry"),
            readme_before,
            "dry-run must not modify README.md"
        );

        // Real `--write` updates README.md to the new Packs matrix without dead links.
        let wrote = run_dev_docs(&catalog, false, true, false, true).expect("docs write");
        assert_eq!(wrote, ExitCode::Ok);
        let readme_after = fs::read_to_string(tmp.path().join("README.md")).expect("read after");
        assert!(readme_after.contains("### Packs (`packs/`)"));
        assert!(readme_after
            .contains("| Profile | Description | Included Packs | Extensions | Bundle Artifact |"));
        assert!(!readme_after.contains("](exports/"));

        // Follow-up `--check` passes cleanly.
        let checked = run_dev_docs(&catalog, true, false, false, true).expect("docs check");
        assert_eq!(checked, ExitCode::Ok);
    }

    /// CARBON-09: `dev pins --refresh` with `dry_run = true` performs ZERO file writes and
    /// ZERO VSIX cache writes.
    #[test]
    fn dev_pins_refresh_dry_run_performs_zero_writes() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let tmp = tempfile::TempDir::new().expect("tmp");
        let lock_path = tmp.path().join("vsp.lock.json");
        let cache_dir = tmp.path().join("cache");
        let initial = serde_json::json!({
            "version": 2,
            "pinRequired": false,
            "platforms": ["darwin-arm64", "universal"],
            "extensions": {
                "editorconfig.editorconfig": {
                    "tier": 1,
                    "universal": true,
                    "status": "pending_refresh",
                    "targets": {}
                }
            }
        })
        .to_string();
        fs::write(&lock_path, &initial).expect("write lock");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw);
                    if let Some(head_end) = text.find("\r\n\r\n") {
                        let cl = text[..head_end]
                            .lines()
                            .find_map(|line| {
                                let (k, v) = line.split_once(':')?;
                                k.eq_ignore_ascii_case("content-length")
                                    .then(|| v.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if raw.len() >= head_end + 4 + cl {
                            break;
                        }
                    }
                }
                let req = String::from_utf8_lossy(&raw);
                let (ct, body) = if req.starts_with("POST") {
                    let payload = serde_json::json!({"results":[{"extensions":[{"versions":[{
                        "version": "1.0.0",
                        "properties": [],
                        "files": [{"assetType":"Microsoft.VisualStudio.Services.VSIXPackage","source":format!("http://{addr}/p.vsix")}]
                    }]}]}]});
                    ("application/json", payload.to_string().into_bytes())
                } else {
                    ("application/octet-stream", b"dry-run-bytes".to_vec())
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });

        let code = refresh_lockfile_with_endpoint(
            tmp.path(),
            &lock_path,
            DevPinsOptions {
                check: false,
                refresh: true,
                strict_release_gate: false,
                dry_run: true,
                json: false,
                quiet: true,
            },
            &format!("http://{addr}/query"),
            true,
            Some(&cache_dir),
        )
        .expect("dry-run refresh");
        assert_eq!(code, ExitCode::Ok);
        assert_eq!(
            fs::read_to_string(&lock_path).expect("read lock"),
            initial,
            "dry-run refresh must not modify vsp.lock.json"
        );
        assert!(
            !cache_dir.exists(),
            "dry-run refresh must not create or write VSIX cache files"
        );
    }
}

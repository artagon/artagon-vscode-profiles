//! Read-only environment, catalog, lockfile, Default-profile security, user-configuration,
//! and parallelized live VS Code auditor powering `vsp doctor [--strict]`.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::thread;

use serde::Serialize;
use serde_json::Value;

use crate::exit_codes::{ExitCode, VspError};
use crate::jsonc::parse_jsonc_value;
use crate::model::{ExtensionStatus, WorkspaceCatalog};
use crate::policy::validate_repository;
use crate::provenance::{load_profile_provenance, ProfileProvenance};
use crate::schema_loader::LockEntryStatus;
use crate::user_config::{ExtensionTrustTier, UserConfig, UserOverlay, USER_UNLOCKED_LABEL};
use crate::vscode::{
    find_code_binary, list_installed_extensions, list_storage_profiles, resolve_vscode_store,
    resolve_xdg_config_home, InstalledExtension,
};
use crate::vsix::{load_lockfile_strict, pins_file_override_path, MAX_VSIX_WORKERS};

const OPTIONAL_RTK_NOT_ENABLED_MSG: &str = "optional rtk-terminal wrappers not enabled";

/// Severity of an individual [`DoctorCheck`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckSeverity {
    /// Check passed cleanly.
    Pass,
    /// Advisory warning (treated as failure under `--strict`).
    Warn,
    /// Hard operational, catalog, or security failure.
    Fail,
}

/// Single diagnostic finding emitted by [`run_doctor`].
#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    /// Severity (`pass`, `warn`, or `fail`).
    pub severity: CheckSeverity,
    /// Logical check category (e.g. `catalog`, `lockfile`, `default-profile`, `extensions`, `rtk`).
    pub category: String,
    /// Human-readable diagnostic message.
    pub message: String,
}

/// Aggregated diagnostic report produced by [`run_doctor`] / [`run_doctor_with_options`].
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    /// Ordered list of diagnostic checks executed.
    pub checks: Vec<DoctorCheck>,
    /// Number of `pass` checks.
    pub passes: usize,
    /// Number of `warn` checks.
    pub warns: usize,
    /// Number of `fail` checks.
    pub fails: usize,
    /// `true` when external observation (e.g., `code --list-extensions`) failed.
    pub incomplete_observation: bool,
    /// Whether `--strict` mode was requested.
    pub strict: bool,
    /// Number of Tier 1 lock entries in `vsp.lock.json` currently marked `pending_refresh`
    /// (informational count; does not by itself fail `--strict`).
    pub pending_tier1_pins: usize,
    /// Number of Tier 2 lock entries in `vsp.lock.json` currently lacking a verified version
    /// (`pending_refresh` / `version: null`; informational count).
    pub pending_tier2_pins: usize,
    /// Catalog-external extensions that the user configuration enables by exact ID
    /// (Tier 3 `user-unlocked`), sorted and unique. Omitted from JSON when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub user_unlocked_extensions: Vec<String>,
}

impl DoctorReport {
    /// Computes the authoritative [`ExitCode`] for this audit report:
    /// - `7` (`IncompleteObservation`) when any live observation failed
    /// - `1` (`Operational`) when any check failed, or when `strict` and `warns > 0`
    /// - `0` (`Ok`) when clean.
    #[must_use]
    pub const fn exit_code(&self) -> ExitCode {
        if self.incomplete_observation {
            ExitCode::IncompleteObservation
        } else if self.fails > 0 || (self.strict && self.warns > 0) {
            ExitCode::Operational
        } else {
            ExitCode::Ok
        }
    }

    fn record(&mut self, severity: CheckSeverity, category: &str, message: impl Into<String>) {
        match severity {
            CheckSeverity::Pass => self.passes += 1,
            CheckSeverity::Warn => self.warns += 1,
            CheckSeverity::Fail => self.fails += 1,
        }
        self.checks.push(DoctorCheck {
            severity,
            category: category.to_string(),
            message: message.into(),
        });
    }
}

/// Runs the full read-only `vsp doctor` audit.
///
/// Equivalent to [`run_doctor_with_options`] with `user_config = None`. Callers that also
/// invoke [`audit_user_config`] afterward automatically have `defaults.rtkTerminal = true`
/// reflected in the `rtk` check.
///
/// Audits:
/// 1. Repository catalog, module DAG, security deny-list, pins, and UX symmetry.
/// 2. Unified fail-closed `vsp.lock.json` lockfile integrity and informational pending pin
///    counts (`pending_tier1_pins`, `pending_tier2_pins`), plus active
///    `ARTAGON_VSCODE_PINS_FILE` override detection.
/// 3. Default-profile APPLICATION-scope security keys derived from
///    `catalog.policy.default_profile_expectations` layered on the built-in floor
///    (`telemetry.telemetryLevel == "off"`,
///    `security.workspace.trust.untrustedFiles == "prompt"`, and exact JSON boolean
///    `extensions.autoUpdate == false`).
/// 4. Conditional XDG RTK shell wrapper audit: emits `PASS` (`optional rtk-terminal wrappers
///    not enabled`) when wrappers are absent unless `user_config.defaults.rtk_terminal == true`
///    or any synced profile provenance includes `rtk-terminal`.
/// 5. Parallelized live VS Code extension scan across `default` and all profiles in
///    `storage.json` on a bounded 4-worker pool ([`MAX_VSIX_WORKERS`]): flags compromised
///    extensions as `FAIL`, pin version drift and provenance bypasses (`--allow-unlocked`,
///    publisher allowlist bypass, `ARTAGON_VSCODE_PINS_FILE` override, unpinned Tier 2 IDs) as
///    `WARN`, and failed `code --list-extensions` probes or missing `code`/store as
///    `incomplete_observation = true` (exit `7`).
#[must_use]
pub fn run_doctor(
    catalog: &WorkspaceCatalog,
    vscode_data_dir: Option<&Path>,
    strict: bool,
) -> DoctorReport {
    run_doctor_with_options(catalog, vscode_data_dir, strict, None)
}

/// Runs the full read-only `vsp doctor` audit with optional pre-loaded personal [`UserConfig`].
#[must_use]
pub fn run_doctor_with_options(
    catalog: &WorkspaceCatalog,
    vscode_data_dir: Option<&Path>,
    strict: bool,
    user_config: Option<&UserConfig>,
) -> DoctorReport {
    let mut report = DoctorReport {
        checks: Vec::new(),
        passes: 0,
        warns: 0,
        fails: 0,
        incomplete_observation: false,
        strict,
        pending_tier1_pins: 0,
        pending_tier2_pins: 0,
        user_unlocked_extensions: Vec::new(),
    };

    // 1. Catalog & repository integrity.
    let validation = validate_repository(catalog);
    if validation.is_ok() {
        report.record(
            CheckSeverity::Pass,
            "catalog",
            format!(
                "catalog, modules, profiles, pins, and security invariants valid ({} files)",
                validation.checked_files
            ),
        );
    } else {
        report.record(
            CheckSeverity::Fail,
            "catalog",
            format!(
                "repository validation reported {} error(s) (run 'vsp dev validate' for details)",
                validation.violations.len()
            ),
        );
    }

    // 2. Unified fail-closed `vsp.lock.json` audit + ARTAGON_VSCODE_PINS_FILE override warning.
    let pins_override = pins_file_override_path();
    if let Some(ref override_path) = pins_override {
        report.record(
            CheckSeverity::Warn,
            "pins",
            format!(
                "ARTAGON_VSCODE_PINS_FILE={} overrides catalog vsp.lock.json (supply-chain lock verification overridden)",
                override_path.display()
            ),
        );
    }

    let mut locked_versions: BTreeMap<String, String> = BTreeMap::new();
    match load_lockfile_strict(catalog) {
        Err(err) => {
            report.record(
                CheckSeverity::Fail,
                "lockfile",
                format!("vsp.lock.json is unreadable or malformed: {err}"),
            );
        }
        Ok(Some(lockfile)) => {
            let mut pending_t1 = 0usize;
            let mut pending_t2 = 0usize;
            for (id, entry) in &lockfile.extensions {
                match entry.tier {
                    1 if entry.status == Some(LockEntryStatus::PendingRefresh) => {
                        pending_t1 += 1;
                    }
                    2 if entry.status == Some(LockEntryStatus::PendingRefresh)
                        || entry.version.as_deref().is_none_or(str::is_empty) =>
                    {
                        pending_t2 += 1;
                    }
                    _ => {}
                }
                if pins_override.is_none() && entry.status == Some(LockEntryStatus::Verified) {
                    if let Some(ver) = entry.version.as_deref().filter(|v| !v.is_empty()) {
                        locked_versions.insert(id.to_ascii_lowercase(), ver.to_string());
                    }
                }
            }
            report.pending_tier1_pins = pending_t1;
            report.pending_tier2_pins = pending_t2;
            report.record(
                CheckSeverity::Pass,
                "lockfile",
                format!(
                    "vsp.lock.json valid ({} entries; pending_tier1_pins={pending_t1}, pending_tier2_pins={pending_t2})",
                    lockfile.extensions.len()
                ),
            );
        }
        Ok(None) => {}
    }

    // 3. VS Code store resolution & Default profile APPLICATION-scope security keys.
    let store = match resolve_vscode_store(vscode_data_dir, true) {
        Ok(s) => Some(s),
        Err(err) => {
            // The live profile scan cannot run against an unresolved store, so the audit
            // is incomplete (exit 7), not only failed.
            report.incomplete_observation = true;
            report.record(
                CheckSeverity::Fail,
                "store",
                format!("audit incomplete — VS Code store configuration mismatch: {err}"),
            );
            resolve_vscode_store(vscode_data_dir, false).ok()
        }
    };

    if let Some(ref st) = store {
        let user_settings = st.user_dir.join("settings.json");
        audit_default_profile_settings(catalog, &user_settings, &mut report);
    }

    // Collect profile names + provenance upfront so both the conditional RTK wrapper check
    // and the parallelized live extension scan share one consistent view.
    let mut scan_profiles = vec!["default".to_string()];
    let mut storage_enum_err: Option<String> = None;
    if let Some(ref st) = store {
        match list_storage_profiles(&st.user_dir) {
            Ok(named) => scan_profiles.extend(named),
            Err(err) => {
                storage_enum_err = Some(err.to_string());
            }
        }
    }

    let provenances: Vec<Option<ProfileProvenance>> = match &store {
        Some(st) => scan_profiles
            .iter()
            .map(|prof| load_profile_provenance(&st.user_dir, prof))
            .collect(),
        None => vec![None; scan_profiles.len()],
    };

    // 4. Conditional XDG RTK wrappers presence (supports both v2.2 `vsp/rtk` and legacy
    //    `artagon-vscode/rtk`). Only WARN when `defaults.rtkTerminal == true` or any synced
    //    profile provenance includes `rtk-terminal`.
    let rtk_profile_requiring = scan_profiles
        .iter()
        .zip(&provenances)
        .find_map(|(prof, prov)| {
            prov.as_ref()
                .filter(|p| p.resolved_modules.iter().any(|m| m == "rtk-terminal"))
                .map(|_| prof.as_str())
        });
    let rtk_enabled_in_config = user_config.is_some_and(|c| c.defaults.rtk_terminal == Some(true));
    audit_rtk_wrappers(&mut report, rtk_enabled_in_config, rtk_profile_requiring);

    // 5. Live VS Code extension scan across Default + all registered profiles (parallelized
    //    across at most MAX_VSIX_WORKERS = 4 workers with deterministic result ordering).
    match find_code_binary() {
        None => {
            // Without `code`, installed extensions cannot be observed. A compromised
            // extension could be present, so this is an incomplete audit (exit 7) and never
            // a warning or a clean exit.
            report.incomplete_observation = true;
            report.record(
                CheckSeverity::Fail,
                "code",
                "audit incomplete — VS Code 'code' CLI not on PATH (live profile inspection was not done)",
            );
        }
        Some(code_bin) => {
            report.record(
                CheckSeverity::Pass,
                "code",
                format!("tool available: {}", code_bin.display()),
            );

            if let Some(err_msg) = storage_enum_err {
                report.incomplete_observation = true;
                report.record(
                    CheckSeverity::Fail,
                    "storage",
                    format!(
                        "storage.json is malformed; could not enumerate named VS Code profiles: {err_msg}"
                    ),
                );
            }

            if let Some(ref st) = store {
                let scan_results = scan_profiles_parallel(&code_bin, st, &scan_profiles);

                for ((prof, prov_opt), list_res) in scan_profiles
                    .iter()
                    .zip(provenances.iter())
                    .zip(scan_results)
                {
                    if let Some(prov) = prov_opt {
                        if prov.publisher_trust_bypass {
                            report.record(
                                CheckSeverity::Warn,
                                "provenance",
                                format!(
                                    "profile '{}' was last synced with ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 (publisher allowlist bypassed at {})",
                                    prof, prov.last_sync_at
                                ),
                            );
                        }
                        if prov.unlocked_bypass {
                            report.record(
                                CheckSeverity::Warn,
                                "provenance",
                                format!(
                                    "profile '{}' was last synced with --allow-unlocked (unverified VSIX state recorded at {})",
                                    prof, prov.last_sync_at
                                ),
                            );
                        }
                        if prov.pins_file_override {
                            report.record(
                                CheckSeverity::Warn,
                                "provenance",
                                format!(
                                    "profile '{}' was synced with ARTAGON_VSCODE_PINS_FILE override (re-run --repair-pins without the override to clear; recorded at {})",
                                    prof, prov.last_sync_at
                                ),
                            );
                        }
                        if !prov.tier2_unpinned.is_empty() {
                            report.record(
                                CheckSeverity::Warn,
                                "provenance",
                                format!(
                                    "profile '{}' has {} unpinned Tier 2 extension(s) ({}) installed without a verified version lock (recorded at {})",
                                    prof,
                                    prov.tier2_unpinned.len(),
                                    prov.tier2_unpinned.join(", "),
                                    prov.last_sync_at
                                ),
                            );
                        }
                    }

                    match list_res {
                        Err(err) => {
                            report.incomplete_observation = true;
                            report.record(CheckSeverity::Fail, "extensions", err.to_string());
                        }
                        Ok(installed) => {
                            for ext in installed {
                                if let Some(rec) = catalog.extension_record(&ext.id) {
                                    if rec.status == ExtensionStatus::Compromised {
                                        // A write to a `String` cannot fail.
                                        let mut hint = String::from("code");
                                        if let Some(ref dr) = st.data_root {
                                            let _ =
                                                write!(hint, " --user-data-dir '{}'", dr.display());
                                        }
                                        if prof != "default" {
                                            let _ = write!(hint, " --profile '{prof}'");
                                        }
                                        let _ = write!(hint, " --uninstall-extension {}", ext.id);
                                        report.record(
                                            CheckSeverity::Fail,
                                            "security",
                                            format!(
                                                "SECURITY: compromised extension '{}' installed in profile '{}' (remove via: {hint})",
                                                ext.id, prof
                                            ),
                                        );
                                    }
                                }
                                let expected_pin_ver = catalog
                                    .lookup_pin(&ext.id)
                                    .filter(|p| !p.version.is_empty())
                                    .map(|p| p.version.as_str())
                                    .or_else(|| {
                                        locked_versions
                                            .get(&ext.id.to_ascii_lowercase())
                                            .map(String::as_str)
                                    });
                                if let (Some(inst_ver), Some(pin_ver)) =
                                    (&ext.version, expected_pin_ver)
                                {
                                    if pin_ver != *inst_ver {
                                        report.record(
                                            CheckSeverity::Warn,
                                            "pins",
                                            format!(
                                                "profile '{}': pinned extension '{}' version drift (installed={}, pinned={})",
                                                prof, ext.id, inst_ver, pin_ver
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    report
}

fn scan_profiles_parallel(
    code_bin: &Path,
    store: &crate::vscode::VsCodeStore,
    profiles: &[String],
) -> Vec<crate::exit_codes::VspResult<Vec<InstalledExtension>>> {
    if profiles.len() <= 1 {
        return profiles
            .iter()
            .map(|p| list_installed_extensions(code_bin, store, p))
            .collect();
    }

    let worker_count = profiles.len().min(MAX_VSIX_WORKERS);
    let queue: Mutex<VecDeque<(usize, &str)>> = Mutex::new(
        profiles
            .iter()
            .enumerate()
            .map(|(i, p)| (i, p.as_str()))
            .collect(),
    );
    let mut slots: Vec<Option<crate::exit_codes::VspResult<Vec<InstalledExtension>>>> =
        (0..profiles.len()).map(|_| None).collect();
    let results: Mutex<Vec<(usize, crate::exit_codes::VspResult<Vec<InstalledExtension>>)>> =
        Mutex::new(Vec::with_capacity(profiles.len()));

    thread::scope(|scope| {
        let workers: Vec<_> = (0..worker_count)
            .map(|_| {
                let queue_ref = &queue;
                let results_ref = &results;
                scope.spawn(move || loop {
                    let next = queue_ref
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    let Some((idx, prof)) = next else {
                        break;
                    };
                    let outcome = list_installed_extensions(code_bin, store, prof);
                    results_ref
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((idx, outcome));
                })
            })
            .collect();
        for worker in workers {
            let _ = worker.join();
        }
    });

    for (idx, outcome) in results.into_inner().unwrap_or_else(PoisonError::into_inner) {
        if let Some(slot) = slots.get_mut(idx) {
            *slot = Some(outcome);
        }
    }

    slots
        .into_iter()
        .enumerate()
        .map(|(idx, slot)| {
            slot.unwrap_or_else(|| {
                Err(VspError::IncompleteObservation(format!(
                    "audit incomplete — doctor scan worker missing result for profile '{}'",
                    profiles.get(idx).map_or("unknown", String::as_str)
                )))
            })
        })
        .collect()
}

fn audit_rtk_wrappers(
    report: &mut DoctorReport,
    rtk_enabled_in_config: bool,
    rtk_profile_requiring: Option<&str>,
) {
    let xdg_base = resolve_xdg_config_home();
    audit_rtk_wrappers_at(
        report,
        &xdg_base,
        rtk_enabled_in_config,
        rtk_profile_requiring,
    );
}

fn audit_rtk_wrappers_at(
    report: &mut DoctorReport,
    xdg_base: &Path,
    rtk_enabled_in_config: bool,
    rtk_profile_requiring: Option<&str>,
) {
    let vsp_rtk = xdg_base.join("vsp/rtk");
    let legacy_rtk = xdg_base.join("artagon-vscode/rtk");
    let has_rtk = |dir: &Path| {
        dir.join("rtk-init.bash").is_file()
            && dir.join("rtk-init.fish").is_file()
            && dir.join("rtk-init.zsh").is_file()
    };
    if has_rtk(&vsp_rtk) {
        report.record(
            CheckSeverity::Pass,
            "rtk",
            format!("XDG RTK wrappers installed at {}", vsp_rtk.display()),
        );
    } else if has_rtk(&legacy_rtk) {
        report.record(
            CheckSeverity::Pass,
            "rtk",
            format!("XDG RTK wrappers installed at {}", legacy_rtk.display()),
        );
    } else if let Some(prof) = rtk_profile_requiring {
        report.record(
            CheckSeverity::Warn,
            "rtk",
            format!(
                "XDG RTK wrappers not yet installed at {} (profile '{prof}' enables rtk-terminal)",
                vsp_rtk.display()
            ),
        );
    } else if rtk_enabled_in_config {
        report.record(
            CheckSeverity::Warn,
            "rtk",
            format!(
                "XDG RTK wrappers not yet installed at {} (user config sets defaults.rtkTerminal = true)",
                vsp_rtk.display()
            ),
        );
    } else {
        report.record(CheckSeverity::Pass, "rtk", OPTIONAL_RTK_NOT_ENABLED_MSG);
    }
}

/// Adds the personal user configuration findings to `report`.
///
/// - `PASS` when no user configuration exists, or when the loaded configuration is valid.
/// - `WARN` for each catalog-external extension that a custom profile enables by exact ID
///   (Tier 3 `user-unlocked`). These IDs are also recorded in
///   [`DoctorReport::user_unlocked_extensions`]. With `--strict`, a warning fails the audit.
/// - If `config` sets `defaults.rtkTerminal = true` and XDG RTK wrappers were not installed,
///   upgrades the optional `rtk` `PASS` check recorded by [`run_doctor`] to `WARN`.
///
/// `config` must be the configuration that produced `overlay`. An invalid configuration is
/// rejected before this function runs (exit `4` or `5`), so it is not reported here.
pub fn audit_user_config(
    report: &mut DoctorReport,
    config_path: &Path,
    config: Option<&UserConfig>,
    overlay: &UserOverlay,
) {
    let Some(cfg) = config else {
        report.record(
            CheckSeverity::Pass,
            "user-config",
            format!(
                "no user config at {} (built-in defaults in use)",
                config_path.display()
            ),
        );
        return;
    };

    if cfg.defaults.rtk_terminal == Some(true) {
        if let Some(rtk_check) = report.checks.iter_mut().find(|c| {
            c.category == "rtk"
                && c.severity == CheckSeverity::Pass
                && c.message == OPTIONAL_RTK_NOT_ENABLED_MSG
        }) {
            let vsp_rtk = resolve_xdg_config_home().join("vsp/rtk");
            rtk_check.severity = CheckSeverity::Warn;
            rtk_check.message = format!(
                "XDG RTK wrappers not yet installed at {} (user config sets defaults.rtkTerminal = true)",
                vsp_rtk.display()
            );
            report.passes = report.passes.saturating_sub(1);
            report.warns += 1;
        }
    }

    report.record(
        CheckSeverity::Pass,
        "user-config",
        format!(
            "user config valid: {} ({} custom profile(s))",
            cfg.path.display(),
            cfg.profiles.len()
        ),
    );

    let mut unlocked: Vec<String> = Vec::new();
    for (name, profile) in &overlay.profiles {
        for extra in &profile.extra_extensions {
            if extra.tier == ExtensionTrustTier::UserUnlocked {
                report.record(
                    CheckSeverity::Warn,
                    "user-config",
                    format!(
                        "{USER_UNLOCKED_LABEL} (Tier 3): profile '{name}' enables catalog-external extension '{}' by exact ID (no publisher allowlist entry and no VSIX pin verification)",
                        extra.id
                    ),
                );
                unlocked.push(extra.id.clone());
            }
        }
    }
    unlocked.sort();
    unlocked.dedup();
    report.user_unlocked_extensions = unlocked;
}

/// Records a personal user configuration that cannot be loaded or applied as a `FAIL`
/// check in `report`.
///
/// The audit does not stop on this error: `vsp doctor` records it and still reports every
/// other check (compromised extensions, pin drift, provenance). A stale configuration,
/// for example one that names an extension which the catalog later marked `compromised`,
/// must never hide those findings.
pub fn audit_user_config_error(report: &mut DoctorReport, config_path: &Path, error: &VspError) {
    report.record(
        CheckSeverity::Fail,
        "user-config",
        format!(
            "user config {} cannot be used (exit {} when a command needs it): {error}",
            config_path.display(),
            error.exit_code().as_i32()
        ),
    );
}

fn json_value_type(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn audit_default_profile_settings(
    catalog: &WorkspaceCatalog,
    settings_path: &Path,
    report: &mut DoctorReport,
) {
    if !settings_path.is_file() {
        report.record(
            CheckSeverity::Warn,
            "default-profile",
            format!(
                "Default profile settings.json not found: {}",
                settings_path.display()
            ),
        );
        return;
    }

    let raw = match fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(err) => {
            report.record(
                CheckSeverity::Warn,
                "default-profile",
                format!(
                    "Default profile settings.json unreadable ({}): {err}",
                    settings_path.display()
                ),
            );
            return;
        }
    };

    let Ok(Value::Object(obj)) = parse_jsonc_value(&raw, &settings_path.display().to_string())
    else {
        report.record(
            CheckSeverity::Warn,
            "default-profile",
            format!(
                "Default profile settings.json could not be parsed as a JSON object: {}",
                settings_path.display()
            ),
        );
        return;
    };

    // Merge built-in security floor with declarative `catalog/policy.jsonc`
    // `defaultProfileExpectations` (policy can add expectations and refine rationale, never
    // weaken the built-in 3-key floor).
    let mut expectations: Vec<(String, Value, String, String)> = vec![
        (
            "telemetry.telemetryLevel".to_string(),
            Value::String("off".to_string()),
            "string".to_string(),
            "application-scope requires \"off\" in Default profile".to_string(),
        ),
        (
            "security.workspace.trust.untrustedFiles".to_string(),
            Value::String("prompt".to_string()),
            "string".to_string(),
            "application-scope requires \"prompt\" in Default profile".to_string(),
        ),
        (
            "extensions.autoUpdate".to_string(),
            Value::Bool(false),
            "boolean".to_string(),
            "application-scope requires boolean false to prevent auto-updates advancing install-time VSIX pins".to_string(),
        ),
    ];

    if let Some(policy) = &catalog.policy {
        for (key, exp) in &policy.default_profile_expectations {
            if let Some(existing) = expectations.iter_mut().find(|(k, _, _, _)| k == key) {
                if !exp.reason.is_empty() && existing.1 == exp.expected {
                    existing.3 = format!(
                        "application-scope requires {} ({})",
                        exp.expected, exp.reason
                    );
                }
            } else {
                let reason = if exp.reason.is_empty() {
                    format!("policy requires {} {}", exp.value_type, exp.expected)
                } else {
                    format!(
                        "policy requires {} {} ({})",
                        exp.value_type, exp.expected, exp.reason
                    )
                };
                expectations.push((
                    key.clone(),
                    exp.expected.clone(),
                    exp.value_type.clone(),
                    reason,
                ));
            }
        }
    }

    for (key, expected, _expected_type, reason) in expectations {
        match obj.get(&key) {
            Some(actual) if actual == &expected => {
                let expected_disp = expected
                    .as_str()
                    .map_or_else(|| expected.to_string(), ToString::to_string);
                report.record(
                    CheckSeverity::Pass,
                    "default-profile",
                    format!("Default profile {key} == {expected_disp}"),
                );
            }
            Some(actual) => {
                let vtype = json_value_type(actual);
                let actual_disp = if expected.is_string() {
                    actual
                        .as_str()
                        .map_or_else(|| actual.to_string(), ToString::to_string)
                } else {
                    actual.to_string()
                };
                report.record(
                    CheckSeverity::Warn,
                    "default-profile",
                    format!(
                        "Default profile ({}) has {key}={actual_disp} (type={vtype}; {reason})",
                        settings_path.display()
                    ),
                );
            }
            None => {
                report.record(
                    CheckSeverity::Warn,
                    "default-profile",
                    format!(
                        "Default profile ({}) has {key}=MISSING (type=missing; {reason})",
                        settings_path.display()
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn empty_report() -> DoctorReport {
        DoctorReport {
            checks: Vec::new(),
            passes: 0,
            warns: 0,
            fails: 0,
            incomplete_observation: false,
            strict: true,
            pending_tier1_pins: 0,
            pending_tier2_pins: 0,
            user_unlocked_extensions: Vec::new(),
        }
    }

    #[test]
    fn rtk_audit_is_optional_pass_by_default_and_warns_only_when_enabled() {
        let tmp = TempDir::new().expect("tempdir");

        let mut clean = empty_report();
        audit_rtk_wrappers_at(&mut clean, tmp.path(), false, None);
        assert_eq!(clean.warns, 0);
        assert_eq!(clean.passes, 1);
        assert_eq!(clean.checks[0].message, OPTIONAL_RTK_NOT_ENABLED_MSG);

        let mut cfg_enabled = empty_report();
        audit_rtk_wrappers_at(&mut cfg_enabled, tmp.path(), true, None);
        assert_eq!(cfg_enabled.warns, 1);
        assert_eq!(cfg_enabled.checks[0].severity, CheckSeverity::Warn);
        assert!(cfg_enabled.checks[0]
            .message
            .contains("defaults.rtkTerminal = true"));

        let mut prof_enabled = empty_report();
        audit_rtk_wrappers_at(&mut prof_enabled, tmp.path(), false, Some("rust"));
        assert_eq!(prof_enabled.warns, 1);
        assert!(prof_enabled.checks[0]
            .message
            .contains("profile 'rust' enables rtk-terminal"));
    }
}

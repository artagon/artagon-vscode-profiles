//! Read-only environment, catalog, Default-profile security, user-configuration, and live
//! VS Code auditor powering `vsp doctor [--strict]`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::exit_codes::ExitCode;
use crate::legacy_loader::parse_jsonc_value;
use crate::model::{ExtensionStatus, WorkspaceCatalog};
use crate::policy::validate_repository;
use crate::provenance::load_profile_provenance;
use crate::schema_loader::{
    embedded_snapshot_files, LockEntryStatus, LockfileManifest, EMBEDDED_ROOT_LABEL,
};
use crate::user_config::{ExtensionTrustTier, UserConfig, UserOverlay, USER_UNLOCKED_LABEL};
use crate::vscode::{
    find_code_binary, list_installed_extensions, list_storage_profiles, resolve_vscode_store,
    resolve_xdg_config_home,
};

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
    /// Logical check category (e.g. `catalog`, `default-profile`, `extensions`, `rtk`).
    pub category: String,
    /// Human-readable diagnostic message.
    pub message: String,
}

/// Aggregated diagnostic report produced by [`run_doctor`].
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
/// Audits:
/// 1. Repository catalog, module DAG, security deny-list, pins, and UX symmetry.
/// 2. XDG RTK shell wrapper installation state.
/// 3. Default-profile APPLICATION-scope security keys (`telemetry.telemetryLevel == "off"`,
///    `security.workspace.trust.untrustedFiles == "prompt"`, and exact JSON boolean
///    `extensions.autoUpdate == false` — warning on missing, `true`, `"false"`, or any
///    non-boolean value, and failing under `--strict`).
/// 4. Live VS Code extension scan across `default` and all profiles in `storage.json`:
///    flags compromised extensions as `FAIL`, pin version drift as `WARN`, and failed
///    `code --list-extensions` probes as `incomplete_observation = true` (exit `7`) while
///    continuing to scan remaining profiles. A missing `code` CLI or an unresolved VS Code
///    store also sets `incomplete_observation = true` (exit `7`): the audit never ends with
///    a warning or exit `0` when it could not observe the installed extensions. Recorded
///    `--allow-unlocked` and publisher-allowlist bypasses are reported as `WARN`.
#[must_use]
pub fn run_doctor(
    catalog: &WorkspaceCatalog,
    vscode_data_dir: Option<&Path>,
    strict: bool,
) -> DoctorReport {
    let mut report = DoctorReport {
        checks: Vec::new(),
        passes: 0,
        warns: 0,
        fails: 0,
        incomplete_observation: false,
        strict,
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

    // 2. XDG RTK wrappers presence (supports both v2.2 `vsp/rtk` and legacy `artagon-vscode/rtk`).
    let xdg_base = resolve_xdg_config_home();
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
    } else {
        report.record(
            CheckSeverity::Warn,
            "rtk",
            format!("XDG RTK wrappers not yet installed at {}", vsp_rtk.display()),
        );
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
        audit_default_profile_settings(&user_settings, &mut report);
    }

    // 4. Live VS Code extension scan across Default + all registered profiles.
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

            if let Some(ref st) = store {
                let mut scan_profiles = vec!["default".to_string()];
                match list_storage_profiles(&st.user_dir) {
                    Ok(named) => scan_profiles.extend(named),
                    Err(err) => {
                        report.incomplete_observation = true;
                        report.record(
                            CheckSeverity::Fail,
                            "storage",
                            format!(
                                "storage.json is malformed; could not enumerate named VS Code profiles: {err}"
                            ),
                        );
                    }
                }

                let locked_versions = load_verified_lock_versions(catalog);
                for prof in scan_profiles {
                    if let Some(prov) = load_profile_provenance(&st.user_dir, &prof) {
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
                    }
                    match list_installed_extensions(&code_bin, st, &prof) {
                        Err(err) => {
                            report.incomplete_observation = true;
                            report.record(CheckSeverity::Fail, "extensions", err.to_string());
                        }
                        Ok(installed) => {
                            for ext in installed {
                                if let Some(rec) = catalog.extension_record(&ext.id) {
                                    if rec.status == ExtensionStatus::Compromised {
                                        let mut hint = String::from("code");
                                        if let Some(ref dr) = st.data_root {
                                            hint.push_str(&format!(
                                                " --user-data-dir '{}'",
                                                dr.display()
                                            ));
                                        }
                                        if prof != "default" {
                                            hint.push_str(&format!(" --profile '{prof}'"));
                                        }
                                        hint.push_str(&format!(" --uninstall-extension {}", ext.id));
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

/// Adds the personal user configuration findings to `report`.
///
/// - `PASS` when no user configuration exists, or when the loaded configuration is valid.
/// - `WARN` for each catalog-external extension that a custom profile enables by exact ID
///   (Tier 3 `user-unlocked`). These IDs are also recorded in
///   [`DoctorReport::user_unlocked_extensions`]. With `--strict`, a warning fails the audit.
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

fn load_verified_lock_versions(catalog: &WorkspaceCatalog) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if env::var("ARTAGON_VSCODE_PINS_FILE")
        .ok()
        .is_some_and(|s| !s.trim().is_empty())
    {
        return out;
    }

    let raw_bytes: Option<Vec<u8>> = if catalog.root == Path::new(EMBEDDED_ROOT_LABEL) {
        embedded_snapshot_files()
            .iter()
            .find(|(p, _)| *p == "vsp.lock.json")
            .map(|(_, b)| b.to_vec())
    } else {
        let lock_path = catalog.root.join("vsp.lock.json");
        if lock_path.is_file() {
            fs::read(&lock_path).ok()
        } else {
            None
        }
    };

    let Some(bytes) = raw_bytes else {
        return out;
    };
    let Ok(manifest) = serde_json::from_slice::<LockfileManifest>(&bytes) else {
        return out;
    };
    for (id, entry) in manifest.extensions {
        if entry.status == Some(LockEntryStatus::Verified) {
            if let Some(ver) = entry.version.filter(|v| !v.is_empty()) {
                out.insert(id.to_ascii_lowercase(), ver);
            }
        }
    }
    out
}

fn audit_default_profile_settings(settings_path: &Path, report: &mut DoctorReport) {
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

    let obj = match parse_jsonc_value(&raw, &settings_path.display().to_string()) {
        Ok(Value::Object(m)) => m,
        _ => {
            report.record(
                CheckSeverity::Warn,
                "default-profile",
                format!(
                    "Default profile settings.json could not be parsed as a JSON object: {}",
                    settings_path.display()
                ),
            );
            return;
        }
    };

    // telemetry.telemetryLevel == "off"
    match obj.get("telemetry.telemetryLevel").and_then(Value::as_str) {
        Some("off") => report.record(
            CheckSeverity::Pass,
            "default-profile",
            "Default profile telemetry.telemetryLevel == off",
        ),
        other => report.record(
            CheckSeverity::Warn,
            "default-profile",
            format!(
                "Default profile ({}) has telemetry.telemetryLevel={} (application-scope requires \"off\" in Default profile)",
                settings_path.display(),
                other.unwrap_or("MISSING")
            ),
        ),
    }

    // security.workspace.trust.untrustedFiles == "prompt"
    match obj
        .get("security.workspace.trust.untrustedFiles")
        .and_then(Value::as_str)
    {
        Some("prompt") => report.record(
            CheckSeverity::Pass,
            "default-profile",
            "Default profile security.workspace.trust.untrustedFiles == prompt",
        ),
        other => report.record(
            CheckSeverity::Warn,
            "default-profile",
            format!(
                "Default profile ({}) has security.workspace.trust.untrustedFiles={} (application-scope requires \"prompt\" in Default profile)",
                settings_path.display(),
                other.unwrap_or("MISSING")
            ),
        ),
    }

    // extensions.autoUpdate === boolean false (strict JSON type + value!)
    match obj.get("extensions.autoUpdate") {
        Some(Value::Bool(false)) => report.record(
            CheckSeverity::Pass,
            "default-profile",
            "Default profile extensions.autoUpdate == false",
        ),
        Some(val) => {
            let vtype = match val {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            report.record(
                CheckSeverity::Warn,
                "default-profile",
                format!(
                    "Default profile ({}) has extensions.autoUpdate={} (type={vtype}; application-scope requires boolean false to prevent auto-updates advancing install-time VSIX pins)",
                    settings_path.display(),
                    val
                ),
            );
        }
        None => report.record(
            CheckSeverity::Warn,
            "default-profile",
            format!(
                "Default profile ({}) has extensions.autoUpdate=MISSING (type=missing; application-scope requires boolean false to prevent auto-updates advancing install-time VSIX pins)",
                settings_path.display()
            ),
        ),
    }
}

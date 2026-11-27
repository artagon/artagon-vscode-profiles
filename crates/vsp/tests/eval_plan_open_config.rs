//! Wave 2B remediation tests covering `plan.rs`, `open.rs`, `user_config.rs`,
//! `compat_vspcli.rs`, and `self_ops.rs`:
//!
//! - `FABLE-01`: unregistered profile diff succeeds with `observation_complete == true`
//!   even when `code --list-extensions --profile=<name>` exits 1 (`Profile '<name>' not found.`).
//! - `ASTRA-02`: `compute_plan_with_extras(..., prune: true / Unmanaged)` emits exact
//!   `ExtensionChangeKind::Uninstall` entries for retired, switch-retired, and extra catalog
//!   extensions while sparing unknown third-party extensions.
//! - `OPUS-06`: `ExtensionChange.lock_status` and `Plan.would_refuse` are populated from
//!   `catalog.lockfile` for the host target platform and honor `allow_unlocked`.
//! - `CARBON-05` / `CARBON-07`: `catalog.retire_on_switch` retires stale rival pack settings
//!   and nested code actions, and `SettingChange` exposes `current`/`desired` diff values.
//! - `CARBON-04`: `run_use` unambiguously selects the named profile even when a same-named
//!   directory exists in cwd, and `run_open` suggests `vsp use <name>` / `vsp open -p <name>`.
//! - `SOL-06`: `load_user_config_scoped` skips malformed default config under `SkipDefault`,
//!   warns under `FallbackOnDefaultError`, and always fails closed on explicit `--config`.
//! - `SOL-04`: legacy `vspcli reconcile` help text and `--as`/`--extensions-only` translation.
//! - `FABLE-12`: `self_ops` dry-run preview leaves disk untouched and refuses foreign/dangling links.

#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde_json::{json, Value};
use tempfile::TempDir;

use vsp::compat_vspcli::{translate, usage_text, Translation};
use vsp::exit_codes::ExitCode;
use vsp::open::{run_open, run_use, OpenOptions};
use vsp::plan::{
    compute_plan, compute_plan_with_extras, ExtensionChangeKind, PlanExtras, PlanOptions,
    SettingChangeKind,
};
use vsp::schema_loader::{load_embedded, LockEntryStatus};
use vsp::self_ops::{self, LinkAction};
use vsp::sync::PruneMode;
use vsp::user_config::{
    apply_user_config, load_user_config_scoped, parse_user_config, UserConfigScope,
};
use vsp::vsix::current_lock_platform;

/// Serializes tests that mutate process-wide environment variables or cwd.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved_vars: Vec<(&'static str, Option<String>)>,
    saved_cwd: Option<PathBuf>,
}

impl EnvGuard {
    fn new(vars: &[&'static str]) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let saved_vars = vars.iter().map(|&k| (k, env::var(k).ok())).collect();
        Self {
            _lock: lock,
            saved_vars,
            saved_cwd: None,
        }
    }

    fn set(&self, key: &str, value: impl AsRef<std::ffi::OsStr>) {
        env::set_var(key, value);
    }

    fn remove(&self, key: &str) {
        env::remove_var(key);
    }

    fn set_cwd(&mut self, dir: &Path) {
        if self.saved_cwd.is_none() {
            self.saved_cwd = Some(env::current_dir().expect("current_dir"));
        }
        env::set_current_dir(dir).expect("set_current_dir");
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(orig) = &self.saved_cwd {
            let _ = env::set_current_dir(orig);
        }
        for (k, orig) in &self.saved_vars {
            match orig {
                Some(v) => env::set_var(k, v),
                None => env::remove_var(k),
            }
        }
    }
}

fn write_executable(path: &Path, script: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir script parent");
    }
    fs::write(path, script).expect("write script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod +x");
}

fn seed_storage(data_dir: &Path, profiles_json: &str) {
    let gs = data_dir.join("User/globalStorage");
    fs::create_dir_all(&gs).expect("mkdir globalStorage");
    fs::write(gs.join("storage.json"), profiles_json).expect("write storage.json");
}

#[test]
fn fable01_unregistered_profile_diff_succeeds_with_realistic_code_error() {
    let guard = EnvGuard::new(&[
        "PATH",
        "XDG_STATE_HOME",
        "ARTAGON_VSCODE_PINS_FILE",
        "ARTAGON_VSCODE_ALLOW_UNLOCKED",
    ]);
    let tmp = TempDir::new().expect("tmp");
    let data_dir = tmp.path().join("data");
    let bin_dir = tmp.path().join("bin");
    seed_storage(&data_dir, r#"{"userDataProfiles":[]}"#);

    // Realistic VS Code CLI behavior: fails exit 1 when `--profile=rust` is not registered.
    write_executable(
        &bin_dir.join("code"),
        "#!/bin/sh\necho \"Profile 'rust' not found.\" >&2\nexit 1\n",
    );
    guard.set("PATH", &bin_dir);
    guard.set("XDG_STATE_HOME", tmp.path().join("state"));
    guard.remove("ARTAGON_VSCODE_PINS_FILE");
    guard.remove("ARTAGON_VSCODE_ALLOW_UNLOCKED");

    let catalog = load_embedded().expect("embedded catalog");

    // 1. Unregistered profile via `--preset rust` succeeds with complete observation.
    let plan_preset = compute_plan(
        &catalog,
        &PlanOptions {
            preset: Some("rust".to_string()),
            vscode_data_dir: Some(data_dir.clone()),
            ..Default::default()
        },
    )
    .expect("compute_plan --preset rust");
    assert!(
        plan_preset.observation_complete,
        "unregistered profile must report observation_complete=true: warnings={:?}",
        plan_preset.warnings
    );
    assert!(plan_preset.has_drift);
    assert!(
        !plan_preset.settings_changes.is_empty(),
        "desired settings must be planned"
    );
    assert!(
        !plan_preset.extension_changes.is_empty(),
        "desired extensions must be planned"
    );
    assert!(plan_preset
        .extension_changes
        .iter()
        .any(|c| c.id == "rust-lang.rust-analyzer"
            && c.action == ExtensionChangeKind::EnsureInstalled));

    // 2. Unregistered profile when `code` is completely absent from PATH also succeeds.
    guard.set("PATH", tmp.path().join("empty-bin"));
    let plan_no_code = compute_plan(
        &catalog,
        &PlanOptions {
            profile: Some("rust".to_string()),
            vscode_data_dir: Some(data_dir.clone()),
            ..Default::default()
        },
    )
    .expect("compute_plan -p rust without code");
    assert!(plan_no_code.observation_complete);
    assert!(!plan_no_code.settings_changes.is_empty());
    assert!(!plan_no_code.extension_changes.is_empty());

    // 3. Control: once `"rust"` IS registered in storage.json, a failing `code` CLI MUST still
    //    produce `observation_complete == false` (fail-closed exit 7).
    guard.set("PATH", &bin_dir);
    seed_storage(
        &data_dir,
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}]}"#,
    );
    let plan_registered_fail = compute_plan(
        &catalog,
        &PlanOptions {
            profile: Some("rust".to_string()),
            vscode_data_dir: Some(data_dir),
            ..Default::default()
        },
    )
    .expect("compute_plan registered profile");
    assert!(
        !plan_registered_fail.observation_complete,
        "registered profile with failing code must be incomplete"
    );
    assert!(!plan_registered_fail.has_drift);
}

#[test]
fn astra02_plan_with_prune_emits_exact_uninstall_entries() {
    let guard = EnvGuard::new(&[
        "PATH",
        "XDG_STATE_HOME",
        "ARTAGON_VSCODE_PINS_FILE",
        "ARTAGON_VSCODE_ALLOW_UNLOCKED",
    ]);
    let tmp = TempDir::new().expect("tmp");
    let data_dir = tmp.path().join("data");
    let bin_dir = tmp.path().join("bin");
    seed_storage(
        &data_dir,
        r#"{"userDataProfiles":[{"name":"rust","location":"rust1111"},{"name":"biome","location":"bio2222"}]}"#,
    );

    // Installed set contains:
    // - panicbit.cargo (catalog status: retired)
    // - ms-python.python (catalog status: active, unmanaged for rust)
    // - dbaeumer.vscode-eslint (retireOnSwitch rival for biome)
    // - unknownpub.third-party-theme (non-catalog user extension, never pruned)
    // Use POSIX shell builtin `printf` (PATH holds only `bin_dir`, not `/bin`).
    let installed_script = concat!(
        "#!/bin/sh\n",
        "printf '%s\\n' \\\n",
        "  'panicbit.cargo@0.1.0' \\\n",
        "  'ms-python.python@2026.1.0' \\\n",
        "  'dbaeumer.vscode-eslint@3.0.10' \\\n",
        "  'unknownpub.third-party-theme@1.0.0'\n",
        "exit 0\n",
    );
    write_executable(&bin_dir.join("code"), installed_script);
    guard.set("PATH", &bin_dir);
    guard.set("XDG_STATE_HOME", tmp.path().join("state"));
    guard.remove("ARTAGON_VSCODE_PINS_FILE");
    guard.remove("ARTAGON_VSCODE_ALLOW_UNLOCKED");

    let mut catalog = load_embedded().expect("embedded catalog");

    let rust_opts = PlanOptions {
        profile: Some("rust".to_string()),
        vscode_data_dir: Some(data_dir.clone()),
        ..Default::default()
    };

    // Without prune: none of retired/unmanaged/third-party are scheduled for Uninstall.
    let no_prune = compute_plan_with_extras(&catalog, &rust_opts, &PlanExtras::default())
        .expect("plan without prune");
    let uninstall_ids = |plan: &vsp::plan::Plan| -> Vec<String> {
        plan.extension_changes
            .iter()
            .filter(|c| c.action == ExtensionChangeKind::Uninstall)
            .map(|c| c.id.clone())
            .collect()
    };
    assert!(
        uninstall_ids(&no_prune).is_empty(),
        "no uninstalls without prune: {:?}",
        uninstall_ids(&no_prune)
    );

    // `prune: true` (default Retired mode): emits exact Uninstall for retired catalog extension.
    let prune_retired = compute_plan_with_extras(
        &catalog,
        &rust_opts,
        &PlanExtras {
            prune: true,
            ..Default::default()
        },
    )
    .expect("plan with prune=true");
    assert!(
        prune_retired.observation_complete,
        "observation must be complete: {:?}",
        prune_retired.warnings
    );
    let retired_uninstalls = uninstall_ids(&prune_retired);
    assert_eq!(retired_uninstalls, vec!["panicbit.cargo".to_string()]);
    assert!(prune_retired
        .extension_changes
        .iter()
        .filter(|c| c.action == ExtensionChangeKind::Uninstall)
        .all(|c| c.lock_status.is_none()));

    // `prune_mode: Some(PruneMode::Unmanaged)`: emits retired + extra active catalog extensions,
    // while keeping non-catalog third-party extensions untouched.
    let prune_unmanaged = compute_plan_with_extras(
        &catalog,
        &rust_opts,
        &PlanExtras {
            prune: true,
            prune_mode: Some(PruneMode::Unmanaged),
            allow_unlocked: false,
        },
    )
    .expect("plan with prune=unmanaged");
    let unmanaged_uninstalls = uninstall_ids(&prune_unmanaged);
    assert!(unmanaged_uninstalls.contains(&"panicbit.cargo".to_string()));
    assert!(unmanaged_uninstalls.contains(&"ms-python.python".to_string()));
    assert!(unmanaged_uninstalls.contains(&"dbaeumer.vscode-eslint".to_string()));
    assert!(
        !unmanaged_uninstalls.contains(&"unknownpub.third-party-theme".to_string()),
        "unknown non-catalog extension must never be pruned"
    );

    // Pack switch with `retireOnSwitch.extensions`: `biome` with `prune: true` retires
    // `dbaeumer.vscode-eslint` even in default Retired mode.
    catalog.materialize_implicit_preset("biome");
    let biome_plan = compute_plan_with_extras(
        &catalog,
        &PlanOptions {
            profile: Some("biome".to_string()),
            vscode_data_dir: Some(data_dir),
            ..Default::default()
        },
        &PlanExtras {
            prune: true,
            ..Default::default()
        },
    )
    .expect("biome plan with prune=true");
    let biome_uninstalls = uninstall_ids(&biome_plan);
    assert!(biome_uninstalls.contains(&"panicbit.cargo".to_string()));
    assert!(
        biome_uninstalls.contains(&"dbaeumer.vscode-eslint".to_string()),
        "retireOnSwitch rival extension must be pruned: {biome_uninstalls:?}"
    );
    assert!(!biome_uninstalls.contains(&"ms-python.python".to_string()));
}

#[test]
fn opus06_plan_populates_lock_status_and_would_refuse_and_honors_allow_unlocked() {
    let guard = EnvGuard::new(&[
        "PATH",
        "XDG_STATE_HOME",
        "ARTAGON_VSCODE_PINS_FILE",
        "ARTAGON_VSCODE_ALLOW_UNLOCKED",
        "ARTAGON_VSCODE_BYPASS_PINS",
    ]);
    let tmp = TempDir::new().expect("tmp");
    let data_dir = tmp.path().join("data");
    seed_storage(&data_dir, r#"{"userDataProfiles":[]}"#);
    guard.set("PATH", tmp.path().join("empty-bin"));
    guard.set("XDG_STATE_HOME", tmp.path().join("state"));
    guard.remove("ARTAGON_VSCODE_PINS_FILE");
    guard.remove("ARTAGON_VSCODE_ALLOW_UNLOCKED");
    guard.remove("ARTAGON_VSCODE_BYPASS_PINS");

    let mut catalog = load_embedded().expect("embedded catalog");
    let host_platform = current_lock_platform();

    // Configure a Tier 2 entry with an explicit verified semver version and ensure we have
    // verified Tier 1, pending Tier 1, version Tier 2, unlocked Tier 3, and user-unlocked extras.
    if let Some(lockfile) = catalog.lockfile.as_mut() {
        if let Some(entry) = lockfile.extensions.get_mut("anthropic.claude-code") {
            entry.tier = 1;
            entry.universal = Some(false);
            entry.status = Some(LockEntryStatus::PendingRefresh);
            entry.version = None;
            entry.sha256 = None;
            entry.vsix_url = None;
            entry.targets.clear();
        }
        if let Some(entry) = lockfile.extensions.get_mut("tamasfe.even-better-toml") {
            entry.tier = 2;
            entry.status = Some(LockEntryStatus::Verified);
            entry.version = Some("0.21.2".to_string());
        }
        // Verify host-platform lookup on a native Tier 1 entry vs universal verified entry.
        if let Some(entry) = lockfile.extensions.get_mut("rust-lang.rust-analyzer") {
            entry.tier = 1;
            entry.universal = Some(false);
            entry.status = Some(LockEntryStatus::Verified);
            entry.targets.clear();
            entry.targets.insert(
                host_platform.to_string(),
                vsp::schema_loader::LockTargetDigest {
                    sha256: "a".repeat(64),
                    vsix_url:
                        "https://marketplace.visualstudio.com/_apis/public/gallery/publishers/rust-lang/vsextensions/rust-analyzer/0.3.2000/vspackage"
                            .to_string(),
                },
            );
        }
    }

    // Add a custom user profile with a Tier 3 catalog-external `user-unlocked` extra extension.
    let cfg = parse_user_config(
        r#"{
          "version": 1,
          "profiles": {
            "my-rust-lock": {
              "extends": "rust",
              "extraExtensions": ["zz-external.sample-tool"]
            }
          }
        }"#,
        Path::new("config.jsonc"),
    )
    .expect("parse user config");
    let overlay = apply_user_config(&mut catalog, &cfg).expect("apply user config");

    let opts = PlanOptions {
        profile: Some("my-rust-lock".to_string()),
        with_modules: vec!["themes-pack".to_string()],
        vscode_data_dir: Some(data_dir.clone()),
        user_overlay: overlay,
        ..Default::default()
    };

    // Strict preview (`allow_unlocked: false`): pending Tier 1 entries populate `would_refuse`.
    let strict = compute_plan_with_extras(&catalog, &opts, &PlanExtras::default())
        .expect("strict lock plan");
    assert!(strict.observation_complete);
    let status_of = |plan: &vsp::plan::Plan, id: &str| -> Option<String> {
        plan.extension_changes
            .iter()
            .find(|c| c.id.eq_ignore_ascii_case(id))
            .and_then(|c| c.lock_status.clone())
    };
    assert_eq!(
        status_of(&strict, "catppuccin.catppuccin-vsc").as_deref(),
        Some("verified"),
        "universal verified Tier 1 pin"
    );
    assert_eq!(
        status_of(&strict, "rust-lang.rust-analyzer").as_deref(),
        Some("verified"),
        "host-platform verified native Tier 1 pin"
    );
    assert_eq!(
        status_of(&strict, "anthropic.claude-code").as_deref(),
        Some("pending"),
        "pending_refresh Tier 1 pin"
    );
    assert_eq!(
        status_of(&strict, "tamasfe.even-better-toml").as_deref(),
        Some("version"),
        "version-locked Tier 2 pin"
    );
    assert_eq!(
        status_of(&strict, "zz-external.sample-tool").as_deref(),
        Some("user-unlocked"),
        "catalog-external exact-ID extra"
    );
    // At least one declarative Tier 3 extension (e.g. a theme/grammar) is marked `"unlocked"`.
    assert!(
        strict
            .extension_changes
            .iter()
            .any(|c| c.lock_status.as_deref() == Some("unlocked")),
        "expected at least one Tier 3 'unlocked' extension in plan"
    );
    assert!(
        strict
            .would_refuse
            .iter()
            .any(|r| r.contains("anthropic.claude-code")),
        "pending Tier 1 pin must be listed in would_refuse: {:?}",
        strict.would_refuse
    );
    assert!(
        !strict
            .would_refuse
            .iter()
            .any(|r| r.contains("catppuccin.catppuccin-vsc")),
        "verified Tier 1 pin must not be in would_refuse"
    );

    // JSON output carries `lock_status` and `would_refuse`.
    let strict_json: Value = serde_json::to_value(&strict).expect("serialize strict plan");
    assert!(strict_json["would_refuse"].is_array());
    assert!(strict_json["extension_changes"]
        .as_array()
        .expect("ext array")
        .iter()
        .any(|c| c["id"] == "catppuccin.catppuccin-vsc" && c["lock_status"] == "verified"));

    // Unlocked preview (`allow_unlocked: true`): clears pending entries from `would_refuse`
    // while keeping verified entries marked `"verified"`.
    let unlocked = compute_plan_with_extras(
        &catalog,
        &opts,
        &PlanExtras {
            prune: false,
            prune_mode: None,
            allow_unlocked: true,
        },
    )
    .expect("unlocked lock plan");
    assert!(
        unlocked.would_refuse.is_empty(),
        "allow_unlocked=true must clear pending entries from would_refuse: {:?}",
        unlocked.would_refuse
    );
    assert_eq!(
        status_of(&unlocked, "catppuccin.catppuccin-vsc").as_deref(),
        Some("verified")
    );
    assert_eq!(
        status_of(&unlocked, "rust-lang.rust-analyzer").as_deref(),
        Some("verified")
    );
    assert_eq!(
        status_of(&unlocked, "anthropic.claude-code").as_deref(),
        Some("pending")
    );
    let unlocked_json: Value = serde_json::to_value(&unlocked).expect("serialize unlocked plan");
    assert!(
        unlocked_json.get("would_refuse").is_none(),
        "empty would_refuse must be omitted in JSON"
    );

    // Tier 0 blocked extension is marked `"tier0-blocked"` and stays in `would_refuse` even
    // when `allow_unlocked == true`.
    let mut blocked_catalog = catalog.clone();
    blocked_catalog
        .blocked_publishers
        .insert("catppuccin".to_string());
    let blocked_plan = compute_plan_with_extras(
        &blocked_catalog,
        &PlanOptions {
            profile: Some("rust".to_string()),
            vscode_data_dir: Some(data_dir),
            ..Default::default()
        },
        &PlanExtras {
            prune: false,
            prune_mode: None,
            allow_unlocked: true,
        },
    )
    .expect("blocked publisher plan");
    assert_eq!(
        status_of(&blocked_plan, "catppuccin.catppuccin-vsc").as_deref(),
        Some("tier0-blocked")
    );
    assert!(
        blocked_plan
            .would_refuse
            .iter()
            .any(|r| r.contains("catppuccin.catppuccin-vsc")),
        "tier0-blocked must never be cleared by allow_unlocked"
    );
}

#[test]
fn carbon05_and_carbon07_retire_on_switch_and_setting_diff_values() {
    let catalog = load_embedded().expect("embedded catalog");
    let ws = TempDir::new().expect("workspace tmp");

    // Create a Biome workspace that still carries stale ESLint settings and codeActionsOnSave.
    fs::write(ws.path().join("biome.json"), "{}\n").expect("write biome.json");
    fs::create_dir_all(ws.path().join(".vscode")).expect("mkdir .vscode");
    fs::write(
        ws.path().join(".vscode/settings.json"),
        serde_json::to_string_pretty(&json!({
            "eslint.workingDirectories": [{ "mode": "auto" }],
            "eslint.validate": ["typescript"],
            "editor.formatOnSave": false,
            "[typescript]": {
                "editor.defaultFormatter": "biomejs.biome",
                "editor.formatOnSave": true,
                "editor.codeActionsOnSave": {
                    "source.fixAll.biome": "explicit",
                    "source.organizeImports.biome": "explicit",
                    "source.fixAll.eslint": "explicit"
                }
            }
        }))
        .expect("json"),
    )
    .expect("write settings.json");

    let plan = compute_plan(
        &catalog,
        &PlanOptions {
            workspace_path: Some(ws.path().to_path_buf()),
            ..Default::default()
        },
    )
    .expect("compute workspace plan");
    assert!(plan.observation_complete);
    assert!(plan.has_drift);

    // Top-level rival keys from `catalog.retire_on_switch["biome"].settings` are removed
    // with `current` populated and `desired == None`.
    for retired_key in ["eslint.workingDirectories", "eslint.validate"] {
        let change = plan
            .settings_changes
            .iter()
            .find(|c| c.key == retired_key)
            .unwrap_or_else(|| panic!("expected Remove for retired switch key '{retired_key}'"));
        assert_eq!(change.action, SettingChangeKind::Remove);
        assert!(change.current.is_some());
        assert!(change.desired.is_none());
        let diff_lines = change.format_diff_lines();
        assert_eq!(diff_lines.len(), 1);
        assert!(diff_lines[0].starts_with(&format!("- {retired_key}: ")));
    }

    // Nested `[typescript].editor.codeActionsOnSave.source.fixAll.eslint` triggers an Update
    // with both `current` and `desired` values exposed (`CARBON-07`).
    let ts_change = plan
        .settings_changes
        .iter()
        .find(|c| c.key == "[typescript]")
        .expect("expected Update for [typescript] due to stale eslint codeAction");
    assert_eq!(ts_change.action, SettingChangeKind::Update);
    assert!(ts_change.current.is_some());
    assert!(ts_change.desired.is_some());
    let ts_lines = ts_change.format_diff_lines();
    assert_eq!(ts_lines.len(), 2);
    assert!(ts_lines[0].starts_with("- [typescript]: "));
    assert!(ts_lines[1].starts_with("+ [typescript]: "));

    // Also verify `Add` exposes `current == None` and `desired == Some(...)`.
    let add_change = plan
        .settings_changes
        .iter()
        .find(|c| c.action == SettingChangeKind::Add)
        .expect("expected at least one Add setting change");
    assert!(add_change.current.is_none());
    assert!(add_change.desired.is_some());
    let add_lines = add_change.format_diff_lines();
    assert_eq!(add_lines.len(), 1);
    assert!(add_lines[0].starts_with(&format!("+ {}: ", add_change.key)));

    // Verify another `retireOnSwitch` pair (`java-maven` retiring `gradle.autoDetect`).
    let java_ws = TempDir::new().expect("java ws");
    fs::write(java_ws.path().join("pom.xml"), "<project/>\n").expect("write pom.xml");
    fs::create_dir_all(java_ws.path().join(".vscode")).expect("mkdir .vscode");
    fs::write(
        java_ws.path().join(".vscode/settings.json"),
        r#"{"gradle.autoDetect": "on"}"#,
    )
    .expect("write java settings");
    let java_plan = compute_plan(
        &catalog,
        &PlanOptions {
            workspace_path: Some(java_ws.path().to_path_buf()),
            ..Default::default()
        },
    )
    .expect("java maven plan");
    assert!(java_plan
        .settings_changes
        .iter()
        .any(|c| c.key == "gradle.autoDetect" && c.action == SettingChangeKind::Remove));
}

#[test]
fn carbon04_run_use_selects_profile_over_same_named_cwd_dir_and_run_open_hints() {
    let mut guard = EnvGuard::new(&["VSCODE_USER_DIR", "VSCODE_USER_DATA_DIR"]);
    guard.remove("VSCODE_USER_DIR");
    guard.remove("VSCODE_USER_DATA_DIR");

    let tmp = TempDir::new().expect("tmp");
    let cwd = tmp.path().join("cwd");
    let same_named_shell_dir = cwd.join("shell");
    let real_ws = cwd.join("my-project");
    let data_dir = tmp.path().join("data");
    fs::create_dir_all(&same_named_shell_dir).expect("mkdir cwd/shell");
    fs::create_dir_all(&real_ws).expect("mkdir cwd/my-project");
    fs::create_dir_all(data_dir.join("User")).expect("mkdir data/User");
    guard.set_cwd(&cwd);

    let mut catalog = load_embedded().expect("embedded catalog");
    let cfg = parse_user_config(
        r#"{"version": 1, "profiles": {"my-custom-profile": {"extends": "rust"}}}"#,
        Path::new("config.jsonc"),
    )
    .expect("parse config");
    let overlay = apply_user_config(&mut catalog, &cfg).expect("apply user config");

    let base_opts = OpenOptions {
        dry_run: true,
        quiet: true,
        vscode_data_dir: Some(data_dir.clone()),
        user_overlay: overlay.clone(),
        ..Default::default()
    };

    // 1. `run_use("rust", None, ...)` selects profile `"rust"` with no workspace path.
    let rust_report = run_use(&catalog, "rust", None, &base_opts).expect("run_use rust");
    assert!(rust_report.dry_run);
    assert_eq!(rust_report.profile.as_deref(), Some("rust"));
    assert_eq!(rust_report.path, None);
    assert!(rust_report.command.iter().any(|a| a == "--profile=rust"));
    assert!(!rust_report.command.iter().any(|a| a == "--"));

    // 2. `run_use("shell", None, ...)` inside cwd containing `./shell/` strictly selects
    //    profile `"shell"` instead of opening `./shell`.
    let shell_report = run_use(&catalog, "shell", None, &base_opts).expect("run_use shell");
    assert_eq!(shell_report.profile.as_deref(), Some("shell"));
    assert_eq!(shell_report.path, None);
    assert!(shell_report.command.iter().any(|a| a == "--profile=shell"));
    assert!(
        !shell_report
            .command
            .iter()
            .any(|a| a == "--" || a == "shell"),
        "run_use must never open ./shell as a path: {:?}",
        shell_report.command
    );

    // 3. `run_use("shell", Some(&real_ws), ...)` selects profile `"shell"` AND opens `real_ws`.
    let shell_with_ws =
        run_use(&catalog, "shell", Some(&real_ws), &base_opts).expect("run_use shell + ws");
    assert_eq!(shell_with_ws.profile.as_deref(), Some("shell"));
    assert_eq!(
        shell_with_ws.path.as_deref(),
        Some(real_ws.to_str().expect("utf8"))
    );
    assert!(shell_with_ws.command.iter().any(|a| a == "--profile=shell"));
    assert!(shell_with_ws.command.iter().any(|a| a == "--"));

    // 4. `run_open` with nonexistent path matching a catalog preset suggests `vsp use` / `vsp open -p`.
    let err_preset = run_open(
        &catalog,
        &OpenOptions {
            path: Some(PathBuf::from("rust")),
            profile: None,
            ..base_opts.clone()
        },
    )
    .expect_err("nonexistent ./rust should fail with helpful hint");
    assert_eq!(err_preset.exit_code(), ExitCode::TargetMissing);
    let msg = err_preset.to_string();
    assert!(msg.contains("vsp use rust"), "missing vsp use hint: {msg}");
    assert!(
        msg.contains("vsp open -p rust"),
        "missing vsp open -p hint: {msg}"
    );

    // 5. `run_open` with nonexistent path matching a custom user profile also suggests `vsp use`.
    let err_user = run_open(
        &catalog,
        &OpenOptions {
            path: Some(PathBuf::from("my-custom-profile")),
            profile: None,
            ..base_opts
        },
    )
    .expect_err("nonexistent user profile path should fail with hint");
    assert_eq!(err_user.exit_code(), ExitCode::TargetMissing);
    assert!(err_user.to_string().contains("vsp use my-custom-profile"));
}

#[test]
fn sol06_scoped_user_config_skips_malformed_default_but_explicit_fails_closed() {
    let guard = EnvGuard::new(&["XDG_CONFIG_HOME", "HOME"]);
    let tmp = TempDir::new().expect("tmp");
    let xdg_config = tmp.path().join("xdg-config");
    let default_cfg = xdg_config.join("vsp/config.jsonc");
    fs::create_dir_all(default_cfg.parent().expect("parent")).expect("mkdir vsp config");
    fs::write(&default_cfg, "{ malformed jsonc !!").expect("write malformed default");
    fs::set_permissions(&default_cfg, fs::Permissions::from_mode(0o600)).expect("chmod 600");

    guard.set("HOME", tmp.path().join("home"));
    guard.set("XDG_CONFIG_HOME", &xdg_config);

    // 1. `SkipDefault` succeeds with `config: None` even though default config is malformed.
    let skipped = load_user_config_scoped(None, UserConfigScope::SkipDefault)
        .expect("SkipDefault must ignore malformed default config");
    assert!(skipped.config.is_none());
    assert!(skipped.warnings.is_empty());

    // 2. `FallbackOnDefaultError` succeeds with `config: None` and records a warning.
    let fallback = load_user_config_scoped(None, UserConfigScope::FallbackOnDefaultError)
        .expect("FallbackOnDefaultError must tolerate malformed default config");
    assert!(fallback.config.is_none());
    assert!(
        !fallback.warnings.is_empty(),
        "expected warning on malformed default config"
    );

    // 3. `Required` fails closed (exit 4) on malformed default config.
    let req_err = load_user_config_scoped(None, UserConfigScope::Required)
        .expect_err("Required must fail closed on malformed default");
    assert_eq!(req_err.exit_code(), ExitCode::UsageOrConfig);

    // 4. Explicit `--config PATH` ALWAYS fails closed across all scopes:
    //    - Missing file -> exit 2 (TargetMissing)
    //    - Malformed JSONC -> exit 4 (UsageOrConfig)
    //    - Autonomy deny-list violation -> exit 5 (SafetyRefusal)
    let missing = tmp.path().join("does-not-exist.jsonc");
    let malformed = tmp.path().join("bad-explicit.jsonc");
    fs::write(&malformed, "{ broken").expect("write bad explicit");
    fs::set_permissions(&malformed, fs::Permissions::from_mode(0o600)).expect("chmod 600");

    let denied = tmp.path().join("denied-explicit.jsonc");
    fs::write(
        &denied,
        r#"{"version": 1, "profiles": {"p": {"overrides": {"chat.tools.autoApprove": true}}}}"#,
    )
    .expect("write denied explicit");
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).expect("chmod 600");

    for scope in [
        UserConfigScope::SkipDefault,
        UserConfigScope::Required,
        UserConfigScope::FallbackOnDefaultError,
    ] {
        let e2 = load_user_config_scoped(Some(&missing), scope)
            .expect_err("explicit missing config must fail closed");
        assert_eq!(e2.exit_code(), ExitCode::TargetMissing, "scope={scope:?}");

        let e4 = load_user_config_scoped(Some(&malformed), scope)
            .expect_err("explicit malformed config must fail closed");
        assert_eq!(e4.exit_code(), ExitCode::UsageOrConfig, "scope={scope:?}");

        let e5 = load_user_config_scoped(Some(&denied), scope)
            .expect_err("explicit denied config must fail closed");
        assert_eq!(e5.exit_code(), ExitCode::SafetyRefusal, "scope={scope:?}");
    }
}

#[test]
fn sol04_compat_reconcile_help_and_translation_accuracy() {
    let help = usage_text();
    assert!(
        help.contains("vsp sync -p <name> --extensions-only [--preset <recipe>]"),
        "reconcile help text must document --extensions-only and --preset: {help}"
    );

    let sub_help = translate(&["reconcile".to_string(), "--help".to_string()]);
    match sub_help {
        Translation::Usage(text) => {
            assert!(text.contains("vsp sync -p <name> --extensions-only [--preset <recipe>]"));
        }
        other => panic!("expected Usage, got {other:?}"),
    }

    let translated = translate(&[
        "reconcile".to_string(),
        "--profile".to_string(),
        "work".to_string(),
        "--as".to_string(),
        "rust".to_string(),
        "--prune-retired".to_string(),
    ]);
    match translated {
        Translation::Vsp { argv, .. } => {
            assert_eq!(
                argv,
                vec![
                    "sync",
                    "-p",
                    "work",
                    "--preset",
                    "rust",
                    "--prune-retired",
                    "--extensions-only",
                ]
            );
        }
        other => panic!("expected Vsp translation, got {other:?}"),
    }
}

#[test]
fn fable12_self_ops_dry_run_preview_and_foreign_symlink_safety() {
    let tmp = TempDir::new().expect("tmp");
    let bin_dir = tmp.path().join("bin");

    // 1. Dry-run install on nonexistent bin_dir reports planned links and target without touching disk.
    let preview = self_ops::install(Some(&bin_dir), true, false, true).expect("dry-run install");
    assert!(preview.dry_run);
    assert!(preview.target.is_some());
    assert_eq!(
        preview.planned_paths(),
        vec![bin_dir.join("vsp"), bin_dir.join("vspcli")]
    );
    assert!(preview
        .links
        .iter()
        .all(|l| l.action == LinkAction::Created));
    assert!(
        !bin_dir.exists(),
        "dry_run install must not create bin_dir or symlinks"
    );

    // 2. Foreign symlink is refused with exit 5 even in dry_run without --force.
    fs::create_dir_all(&bin_dir).expect("mkdir bin_dir");
    let foreign_bin = tmp.path().join("other-vsp");
    fs::write(&foreign_bin, b"#!/bin/sh\n").expect("write foreign");
    symlink(&foreign_bin, bin_dir.join("vsp")).expect("symlink foreign");

    let err_foreign = self_ops::install(Some(&bin_dir), false, false, true)
        .expect_err("foreign symlink must be refused");
    assert_eq!(err_foreign.exit_code(), ExitCode::SafetyRefusal);
    assert_eq!(
        fs::read_link(bin_dir.join("vsp")).expect("read link"),
        foreign_bin
    );

    // 3. Dangling symlink is also refused with exit 5 without --force.
    fs::remove_file(bin_dir.join("vsp")).expect("rm link");
    let missing_target = tmp.path().join("gone-vsp");
    symlink(&missing_target, bin_dir.join("vsp")).expect("symlink dangling");
    let err_dangling = self_ops::install(Some(&bin_dir), false, false, true)
        .expect_err("dangling symlink must be refused");
    assert_eq!(err_dangling.exit_code(), ExitCode::SafetyRefusal);

    // 4. Dry-run uninstall skips foreign symlink without modifying it.
    let uninstall_preview =
        self_ops::uninstall(Some(&bin_dir), false, true).expect("dry-run uninstall");
    assert!(uninstall_preview.dry_run);
    assert_eq!(uninstall_preview.links[0].action, LinkAction::Skipped);
    assert_eq!(
        fs::read_link(bin_dir.join("vsp")).expect("read link"),
        missing_target
    );
}

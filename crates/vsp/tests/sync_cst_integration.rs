//! Comprehensive integration tests for Phase 2.2:
//! - Lossless JSONC CST merge comment preservation across repeated sync, `biome` <-> `ts-eslint-prettier`,
//!   `retina` <-> `crisp` <-> `default` UX switches, and `--ai full` -> `copilot` -> `off` switches
//! - Byte-identical `vsp diff` == `vsp sync --dry-run` (JSON and text)
//! - Step 0 compromised-first ordering even when installs fail, plus `--all --compromised-only` sweep
//! - Offline `--from <bundle>` import (`SHA256SUMS`, all 5 N1 payloads exit `5`, `--ux retina`,
//!   `--ai off`, `ai-profile-* -> ai-copilot` invariant, theme failure fallback)
//! - Existing profile `storage.json` fast-path vs new profile running-VS-Code exit `5` guard
//! - Full exit code contract (`0`, `2`, `3`, `4`, `5`, `6`, `7`), VSIX pins, `--repair-pins`,
//!   `--prune=unmanaged`, `vsp open` (`use`) + `--sync`, and `vsp dev {pins,docs,hooks,compat}`.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

use vsp::legacy_loader::parse_jsonc_value;
use vsp::resolver::sha256_hex;
use vsp::storage::acquire_advisory_lock;
use vsp::sync::{profile_sync_lock_path, workspace_sync_lock_path};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/vsp lives two levels below repo root")
        .to_path_buf()
}

fn vsp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vsp"))
}

/// Builds a hermetic `vsp` command.
///
/// `HOME` and every XDG directory point into a per-process sandbox under
/// `CARGO_TARGET_TMPDIR`, so a host `~/.config/vsp/config.jsonc`, host provenance state, or
/// a host VSIX cache can never change a test result. A no-op `code` stub is first on
/// `PATH`, so a test never runs a real VS Code CLI. A test that needs another `code`
/// behavior sets `PATH` itself.
fn vsp_cmd() -> Command {
    static SANDBOX: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let sandbox = SANDBOX.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("sync-cst-sandbox-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["home", "config", "state", "cache", "bin"] {
            fs::create_dir_all(dir.join(sub)).expect("create sandbox dir");
        }
        #[cfg(unix)]
        write_executable_stub(&dir.join("bin"), "code", "#!/bin/sh\nexit 0\n");
        dir
    });
    let host_path = std::env::var("PATH").unwrap_or_default();
    let mut cmd = Command::new(vsp_bin());
    cmd.env("HOME", sandbox.join("home"))
        .env("XDG_CONFIG_HOME", sandbox.join("config"))
        .env("XDG_STATE_HOME", sandbox.join("state"))
        .env("XDG_CACHE_HOME", sandbox.join("cache"))
        .env(
            "PATH",
            format!("{}:{host_path}", sandbox.join("bin").display()),
        )
        .env_remove("VSP_COMPAT_VSPCLI")
        .env_remove("XDG_BIN_HOME");
    cmd
}

#[cfg(unix)]
fn write_executable_stub(dir: &Path, name: &str, script: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    fs::write(&p, script).expect("write stub");
    let mut perms = fs::metadata(&p).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&p, perms).expect("chmod");
    p
}

fn write_empty_pins_file(dir: &Path) -> PathBuf {
    let p = dir.join("pins.json");
    fs::write(&p, "{\"version\":1,\"pins\":{}}\n").expect("write empty pins");
    p
}

#[test]
fn cst_workspace_preserves_all_comments_across_repeated_sync_and_biome_eslint_switch() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let ws = TempDir::new().expect("ws");
    let xdg_state = TempDir::new().expect("xdg state");

    // Astro + TypeScript workspace markers.
    fs::write(ws.path().join("package.json"), "{}\n").expect("pkg");
    fs::write(ws.path().join("tsconfig.json"), "{}\n").expect("tsconfig");
    fs::write(ws.path().join("astro.config.mjs"), "export default {};\n").expect("astro");

    let vscode_dir = ws.path().join(".vscode");
    fs::create_dir_all(&vscode_dir).expect("mkdir .vscode");
    let initial_settings = r#"// Leading workspace file header comment
{
  /* Block comment above user setting */
  "user.teamCustomFlag": true, // inline user comment
  // Comment above [typescript] block
  "[typescript]": {
    // Keep this user sub-key comment inside [typescript]
    "editor.tabSize": 4, // inline user sub-key comment
    /* Interior block comment */
    "editor.defaultFormatter": "esbenp.prettier-vscode"
  }
}
"#;
    fs::write(vscode_dir.join("settings.json"), initial_settings).expect("write settings");

    let run_ws_sync = |extra_args: &[&str]| {
        let mut cmd = vsp_cmd();
        cmd.args([
            "--catalog",
            catalog_arg,
            "sync",
            ws.path().to_str().expect("utf8"),
        ]);
        cmd.args(extra_args);
        cmd.env("XDG_STATE_HOME", xdg_state.path());
        cmd.output().expect("run vsp sync workspace")
    };

    // 1. Sync with default Astro stack (`ts-eslint-prettier`).
    let s1 = run_ws_sync(&[]);
    assert_eq!(
        s1.status.code(),
        Some(0),
        "sync 1 failed: {}",
        String::from_utf8_lossy(&s1.stderr)
    );
    // Repeat sync (idempotency check).
    let s2 = run_ws_sync(&[]);
    assert_eq!(s2.status.code(), Some(0));

    let after_eslint = fs::read_to_string(vscode_dir.join("settings.json")).expect("read");
    for comment in [
        "// Leading workspace file header comment",
        "/* Block comment above user setting */",
        "// inline user comment",
        "// Comment above [typescript] block",
        "// Keep this user sub-key comment inside [typescript]",
        "// inline user sub-key comment",
        "/* Interior block comment */",
    ] {
        assert!(
            after_eslint.contains(comment),
            "lost comment '{comment}' after ts-eslint-prettier sync:\n{after_eslint}"
        );
    }
    let v_eslint = parse_jsonc_value(&after_eslint, "after_eslint").expect("valid jsonc");
    assert_eq!(v_eslint["user.teamCustomFlag"], true);
    assert_eq!(v_eslint["[typescript]"]["editor.tabSize"], 4);
    assert_eq!(
        v_eslint["[typescript]"]["editor.codeActionsOnSave"]["source.fixAll.eslint"],
        "explicit"
    );
    assert!(v_eslint.get("eslint.validate").is_some());

    // diff --check after sync MUST exit 0 (converged even with user sub-key in [typescript]!).
    let check_clean = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            "--check",
            ws.path().to_str().expect("utf8"),
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .output()
        .expect("diff --check clean");
    assert_eq!(
        check_clean.status.code(),
        Some(0),
        "expected converged diff --check=0, stdout={}",
        String::from_utf8_lossy(&check_clean.stdout)
    );

    // 2. Switch to Biome (`--with biome --without ts-eslint-prettier`).
    let s_biome = run_ws_sync(&["--with", "biome", "--without", "ts-eslint-prettier"]);
    assert_eq!(
        s_biome.status.code(),
        Some(0),
        "biome switch failed: {}",
        String::from_utf8_lossy(&s_biome.stderr)
    );
    let after_biome = fs::read_to_string(vscode_dir.join("settings.json")).expect("read");
    for comment in [
        "// Leading workspace file header comment",
        "/* Block comment above user setting */",
        "// inline user comment",
        "// Comment above [typescript] block",
        "// Keep this user sub-key comment inside [typescript]",
        "// inline user sub-key comment",
        "/* Interior block comment */",
    ] {
        assert!(
            after_biome.contains(comment),
            "lost comment '{comment}' after biome switch:\n{after_biome}"
        );
    }
    let v_biome = parse_jsonc_value(&after_biome, "after_biome").expect("valid jsonc");
    assert_eq!(v_biome["user.teamCustomFlag"], true);
    assert_eq!(v_biome["[typescript]"]["editor.tabSize"], 4);
    assert_eq!(
        v_biome["[typescript]"]["editor.defaultFormatter"],
        "biomejs.biome"
    );
    assert_eq!(
        v_biome["[typescript]"]["editor.codeActionsOnSave"]["source.fixAll.biome"],
        "explicit"
    );
    assert!(
        v_biome["[typescript]"]["editor.codeActionsOnSave"]
            .get("source.fixAll.eslint")
            .is_none(),
        "stale eslint codeAction must be retired on biome switch"
    );
    assert!(
        v_biome.get("eslint.validate").is_none(),
        "retired eslint.validate key must be removed on biome switch"
    );

    // 3. Switch back from Biome -> ts-eslint-prettier!
    let s_back = run_ws_sync(&["--with", "ts-eslint-prettier", "--without", "biome"]);
    assert_eq!(s_back.status.code(), Some(0));
    let after_back = fs::read_to_string(vscode_dir.join("settings.json")).expect("read");
    for comment in [
        "// Leading workspace file header comment",
        "/* Block comment above user setting */",
        "// inline user comment",
        "// Comment above [typescript] block",
        "// Keep this user sub-key comment inside [typescript]",
        "// inline user sub-key comment",
        "/* Interior block comment */",
    ] {
        assert!(
            after_back.contains(comment),
            "lost comment '{comment}' switching biome -> ts-eslint-prettier:\n{after_back}"
        );
    }
    let v_back = parse_jsonc_value(&after_back, "after_back").expect("valid jsonc");
    assert_eq!(v_back["[typescript]"]["editor.tabSize"], 4);
    assert!(v_back["[typescript]"]["editor.codeActionsOnSave"]
        .get("source.fixAll.biome")
        .is_none());
    assert_eq!(
        v_back["[typescript]"]["editor.codeActionsOnSave"]["source.fixAll.eslint"],
        "explicit"
    );
}

#[cfg(unix)]
#[test]
fn cst_profile_preserves_comments_across_ux_and_ai_switches_and_skips_storage_for_existing_profile()
{
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("vscode data");
    let stub_bin = TempDir::new().expect("stub bin");
    let xdg_state = TempDir::new().expect("xdg state");
    let xdg_config = TempDir::new().expect("xdg config");
    let pins_dir = TempDir::new().expect("pins");
    let empty_pins = write_empty_pins_file(pins_dir.path());

    // Pre-register an EXISTING profile `rust` in storage.json with comments in its settings.json.
    let user_dir = fake_data.path().join("User");
    let gs_dir = user_dir.join("globalStorage");
    let prof_dir = user_dir.join("profiles/abcd1234");
    fs::create_dir_all(&gs_dir).expect("mkdir gs");
    fs::create_dir_all(&prof_dir).expect("mkdir prof");
    let storage_path = gs_dir.join("storage.json");
    let original_storage =
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}],"unrelatedKey":"preserve"}"#;
    fs::write(&storage_path, original_storage).expect("write storage");

    let initial_prof_settings = r#"// Profile banner comment
{
  /* Personal user setting comment */
  "my.personalKey": "untouched", // inline personal comment
  // Managed setting comment
  "editor.fontSize": 11 // inline font comment
}
"#;
    let prof_settings_path = prof_dir.join("settings.json");
    fs::write(&prof_settings_path, initial_prof_settings).expect("write prof settings");

    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\nif [ \"$1\" = \"--user-data-dir\" ]; then shift 2; fi\ncase \"$*\" in\n  *--list-extensions*) exit 0 ;;\n  *) exit 0 ;;\nesac\n",
    );

    let run_prof_sync = |extra: &[&str]| {
        let mut cmd = vsp_cmd();
        cmd.args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "rust",
        ]);
        cmd.args(extra);
        // VS Code IS RUNNING (`ARTAGON_VSCODE_TEST_RUNNING=1`), yet existing profile MUST
        // succeed because storage.json is NOT written!
        cmd.env("ARTAGON_VSCODE_TEST_RUNNING", "1");
        cmd.env("ARTAGON_VSCODE_PINS_FILE", &empty_pins);
        cmd.env("XDG_STATE_HOME", xdg_state.path());
        cmd.env("XDG_CONFIG_HOME", xdg_config.path());
        cmd.env("PATH", stub_bin.path());
        cmd.output().expect("run profile sync")
    };

    // 1. Sync with `--ux retina --ai full --with rtk-terminal`.
    let r1 = run_prof_sync(&["--ux", "retina", "--ai", "full", "--with", "rtk-terminal"]);
    assert_eq!(
        r1.status.code(),
        Some(0),
        "existing profile sync failed while VS Code running: {}",
        String::from_utf8_lossy(&r1.stderr)
    );
    // storage.json MUST be byte-identical (skipped write completely!).
    assert_eq!(
        fs::read_to_string(&storage_path).expect("storage"),
        original_storage
    );
    // XDG RTK wrappers written under ${XDG_CONFIG_HOME}/vsp/rtk/.
    assert!(xdg_config.path().join("vsp/rtk/rtk-init.bash").is_file());
    assert!(xdg_config.path().join("vsp/rtk/rtk-init.fish").is_file());
    assert!(xdg_config.path().join("vsp/rtk/rtk-init.zsh").is_file());
    assert!(xdg_config.path().join("vsp/rtk/zsh/.zshrc").is_file());

    let s_retina = fs::read_to_string(&prof_settings_path).expect("read retina");
    let v_retina = parse_jsonc_value(&s_retina, "retina").expect("valid jsonc");
    assert_eq!(v_retina["workbench.colorTheme"], "Catppuccin Mocha");
    assert_eq!(v_retina["my.personalKey"], "untouched");
    assert!(v_retina.get("claudeCode.preferredLocation").is_some());

    // 2. Switch `--ux crisp --ai copilot`.
    let r2 = run_prof_sync(&["--ux", "crisp", "--ai", "copilot"]);
    assert_eq!(
        r2.status.code(),
        Some(0),
        "crisp/copilot switch failed: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    let s_crisp = fs::read_to_string(&prof_settings_path).expect("read crisp");
    let v_crisp = parse_jsonc_value(&s_crisp, "crisp").expect("valid jsonc");
    assert_eq!(v_crisp["workbench.colorTheme"], "Tokyo Night");
    assert!(
        v_crisp.get("claudeCode.preferredLocation").is_none(),
        "claudeCode key must retire when switching --ai full -> --ai copilot"
    );

    // 3. Switch `--ux default --ai off`.
    let r3 = run_prof_sync(&["--ux", "default", "--ai", "off"]);
    assert_eq!(
        r3.status.code(),
        Some(0),
        "default/ai-off switch failed: {}",
        String::from_utf8_lossy(&r3.stderr)
    );
    let s_default = fs::read_to_string(&prof_settings_path).expect("read default");
    for comment in [
        "// Profile banner comment",
        "/* Personal user setting comment */",
        "// inline personal comment",
        "// Managed setting comment",
    ] {
        assert!(
            s_default.contains(comment),
            "lost comment '{comment}' across UX/AI switches:\n{s_default}"
        );
    }
    let v_default = parse_jsonc_value(&s_default, "default").expect("valid jsonc");
    assert_eq!(v_default["my.personalKey"], "untouched");
    assert!(
        v_default.get("workbench.colorTheme").is_none(),
        "UX keys must retire on --ux default"
    );
    assert!(v_default.get("editor.fontSize").is_none());
    assert_eq!(v_default["chat.disableAIFeatures"], true);
    assert!(v_default.get("claudeCode.preferredLocation").is_none());

    // 4. Creating a BRAND-NEW profile while ARTAGON_VSCODE_TEST_RUNNING=1 MUST refuse with exit 5!
    let new_running = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "brand-new-profile",
            "--preset",
            "rust",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "1")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("new profile while running");
    assert_eq!(
        new_running.status.code(),
        Some(5),
        "expected exit 5 creating new profile while VS Code running, stderr={}",
        String::from_utf8_lossy(&new_running.stderr)
    );
    // storage.json STILL untouched!
    assert_eq!(
        fs::read_to_string(&storage_path).expect("storage"),
        original_storage
    );

    // 5. Creating the new profile with ARTAGON_VSCODE_TEST_RUNNING=0 succeeds and updates storage.json!
    let new_idle = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "brand-new-profile",
            "--preset",
            "rust",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("new profile while idle");
    assert_eq!(
        new_idle.status.code(),
        Some(0),
        "new profile creation failed: {}",
        String::from_utf8_lossy(&new_idle.stderr)
    );
    let updated_storage: Value =
        serde_json::from_str(&fs::read_to_string(&storage_path).expect("read storage"))
            .expect("parse storage");
    assert!(updated_storage["userDataProfiles"]
        .as_array()
        .expect("arr")
        .iter()
        .any(|p| p["name"] == "brand-new-profile"));
}

#[test]
fn diff_and_sync_dry_run_are_byte_identical_for_json_and_text() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let ws = TempDir::new().expect("ws");
    let fake_data = TempDir::new().expect("data");
    fs::write(ws.path().join("Cargo.toml"), "[package]\nname=\"demo\"\n").expect("cargo");

    let cases: Vec<(Vec<&str>, i32)> = vec![
        (vec![ws.path().to_str().expect("utf8")], 0),
        (vec!["-p", "rust", "--ux", "retina", "--allow-unlocked"], 0),
        // Without `--allow-unlocked`, pending embedded Tier 1 locks populate `would_refuse`,
        // so `diff` exits 0 while `sync --dry-run` predicts the apply refusal with exit 5
        // and emits byte-identical stdout!
        (vec!["-p", "rust", "--ux", "retina"], 5),
        (vec!["--user", "--ux", "crisp", "--ai", "off"], 0),
    ];

    for (selector, expected_dry_run_exit) in cases {
        for flag_mode in [&["--json"][..], &[][..]] {
            let mut diff_cmd = vsp_cmd();
            diff_cmd
                .args([
                    "--catalog",
                    catalog_arg,
                    "--vscode-data-dir",
                    fake_data.path().to_str().expect("utf8"),
                ])
                .args(flag_mode)
                .arg("diff")
                .args(&selector);
            let diff_out = diff_cmd.output().expect("run diff");
            assert_eq!(diff_out.status.code(), Some(0));

            let mut sync_cmd = vsp_cmd();
            sync_cmd
                .args([
                    "--catalog",
                    catalog_arg,
                    "--vscode-data-dir",
                    fake_data.path().to_str().expect("utf8"),
                    "--dry-run",
                ])
                .args(flag_mode)
                .arg("sync")
                .args(&selector);
            let sync_out = sync_cmd.output().expect("run sync --dry-run");
            assert_eq!(sync_out.status.code(), Some(expected_dry_run_exit));

            assert_eq!(
                diff_out.stdout, sync_out.stdout,
                "vsp diff and vsp sync --dry-run stdout differed for selector={selector:?} mode={flag_mode:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn step0_uninstalls_compromised_extension_before_any_install_even_when_installs_fail() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("data");
    let stub_bin = TempDir::new().expect("stub");
    let log_dir = TempDir::new().expect("log");
    let xdg_state = TempDir::new().expect("state");
    let pins_dir = TempDir::new().expect("pins");
    let empty_pins = write_empty_pins_file(pins_dir.path());
    let log_file = log_dir.path().join("code.log");

    // Pre-register `rust` profile and another profile `extra` for sweep testing.
    let user_dir = fake_data.path().join("User");
    fs::create_dir_all(user_dir.join("globalStorage")).expect("gs");
    fs::write(
        user_dir.join("globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"aaaa1111"},{"name":"extra","location":"bbbb2222"}]}"#,
    )
    .expect("storage");

    let stub_script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  *--list-extensions*)\n    echo 'jscearcy.rust-doc-viewer@4.2.1'\n    exit 0\n    ;;\n  *--uninstall-extension*)\n    exit 0\n    ;;\n  *--install-extension*)\n    echo 'simulated network failure' >&2\n    exit 1\n    ;;\nesac\nexit 0\n",
        log_file.display()
    );
    write_executable_stub(stub_bin.path(), "code", &stub_script);

    // 1. Profile sync: installs fail, YET Step 0 uninstall MUST run FIRST!
    let out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "rust",
            "--ux",
            "retina",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("run step0 test");
    assert_eq!(
        out.status.code(),
        Some(6),
        "mid-apply failed installs after Step 0/Step 4 mutations must exit 6 (PartialApply), stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err_text = String::from_utf8_lossy(&out.stderr);
    assert!(err_text.contains("partial apply"), "{err_text}");
    assert!(
        err_text.contains("simulated network failure"),
        "child stderr must be preserved in partial-apply summary: {err_text}"
    );

    let log_lines: Vec<String> = fs::read_to_string(&log_file)
        .expect("read log")
        .lines()
        .map(ToString::to_string)
        .collect();
    let uninstall_pos = log_lines
        .iter()
        .position(|l| l.contains("--uninstall-extension jscearcy.rust-doc-viewer"))
        .expect("Step 0 must uninstall jscearcy.rust-doc-viewer");
    let first_install_pos = log_lines
        .iter()
        .position(|l| l.contains("--install-extension"))
        .expect("install should be attempted after Step 0");
    assert!(
        uninstall_pos < first_install_pos,
        "Step 0 uninstall (line {uninstall_pos}) MUST precede first install (line {first_install_pos}):\n{log_lines:#?}"
    );

    // Theme failure fallback: because catppuccin theme ext failed install, dangling theme key stripped!
    let prof_settings: Value = parse_jsonc_value(
        &fs::read_to_string(user_dir.join("profiles/aaaa1111/settings.json")).expect("settings"),
        "settings",
    )
    .expect("jsonc");
    assert!(
        prof_settings.get("workbench.colorTheme").is_none(),
        "theme failure fallback must strip dangling workbench.colorTheme"
    );

    // 2. Global emergency sweep `--all --compromised-only` sweeps Default + every registered profile!
    fs::write(&log_file, "").expect("clear log");
    let sweep = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "--all",
            "--compromised-only",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("run sweep");
    assert_eq!(
        sweep.status.code(),
        Some(0),
        "sweep failed: {}",
        String::from_utf8_lossy(&sweep.stderr)
    );
    let sweep_log = fs::read_to_string(&log_file).expect("read sweep log");
    assert!(sweep_log.contains("--uninstall-extension jscearcy.rust-doc-viewer"));
    assert!(sweep_log.contains("--profile=rust --uninstall-extension jscearcy.rust-doc-viewer"));
    assert!(sweep_log.contains("--profile=extra --uninstall-extension jscearcy.rust-doc-viewer"));
    assert!(!sweep_log.contains("--install-extension"));
}

#[cfg(unix)]
#[test]
fn offline_from_bundle_verifies_sha256sums_blocks_n1_payloads_and_preserves_ai_copilot_invariant() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("data");
    let stub_bin = TempDir::new().expect("stub");
    let dist_dir = TempDir::new().expect("dist");
    let xdg_state = TempDir::new().expect("state");
    let pins_dir = TempDir::new().expect("pins");
    let empty_pins = write_empty_pins_file(pins_dir.path());
    let log_file = pins_dir.path().join("code-bundle.log");

    let stub_script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n",
        log_file.display()
    );
    write_executable_stub(stub_bin.path(), "code", &stub_script);

    // Build deterministic dist bundles + SHA256SUMS.
    let dist = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "dev",
            "dist",
            "--out",
            dist_dir.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("dev dist");
    assert_eq!(dist.status.code(), Some(0));

    // 1. Offline import of `rust.code-profile` with `--ux retina --ai off` succeeds!
    let rust_bundle = dist_dir.path().join("rust.code-profile");
    let imp = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "--from",
            rust_bundle.to_str().expect("utf8"),
            "-p",
            "rust-offline",
            "--ux",
            "retina",
            "--ai",
            "off",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("import rust offline");
    assert_eq!(
        imp.status.code(),
        Some(0),
        "offline bundle import failed: {}",
        String::from_utf8_lossy(&imp.stderr)
    );

    let storage: Value = serde_json::from_str(
        &fs::read_to_string(fake_data.path().join("User/globalStorage/storage.json"))
            .expect("storage"),
    )
    .expect("parse storage");
    let loc = storage["userDataProfiles"]
        .as_array()
        .expect("arr")
        .iter()
        .find(|p| p["name"] == "rust-offline")
        .expect("rust-offline registered")["location"]
        .as_str()
        .expect("loc");
    let settings: Value = parse_jsonc_value(
        &fs::read_to_string(
            fake_data
                .path()
                .join(format!("User/profiles/{loc}/settings.json")),
        )
        .expect("settings"),
        "settings",
    )
    .expect("jsonc");
    assert_eq!(settings["workbench.colorTheme"], "Catppuccin Mocha");
    assert_eq!(settings["chat.disableAIFeatures"], true);
    assert!(settings.get("claudeCode.preferredLocation").is_none());

    let log_str = fs::read_to_string(&log_file).expect("log");
    assert!(!log_str.contains("anthropic.claude-code"));
    assert!(!log_str.contains("github.copilot-chat"));

    // 2. Legacy `ai-profile-retina.code-profile` preserves `ai-profile-* -> ai-copilot` invariant!
    fs::write(&log_file, "").expect("clear log");
    let ai_alias_bundle = dist_dir.path().join("ai-profile-retina.code-profile");
    let imp_ai = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "--from",
            ai_alias_bundle.to_str().expect("utf8"),
            "-p",
            "ai-profile-retina",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("import ai-profile-retina");
    assert_eq!(imp_ai.status.code(), Some(0));
    let storage2: Value = serde_json::from_str(
        &fs::read_to_string(fake_data.path().join("User/globalStorage/storage.json"))
            .expect("storage"),
    )
    .expect("parse");
    assert!(storage2["userDataProfiles"]
        .as_array()
        .expect("arr")
        .iter()
        .any(|p| p["name"] == "ai-copilot"));
    let ai_log = fs::read_to_string(&log_file).expect("log");
    assert!(ai_log.contains("github.copilot-chat"));
    assert!(!ai_log.contains("anthropic.claude-code"));
    assert!(!ai_log.contains("google.google-antigravity"));
    assert!(!ai_log.contains("openai.chatgpt"));

    // 3. Tampered bundle failing SHA256SUMS MUST exit 5!
    let bad_sum_dir = TempDir::new().expect("bad sum");
    let bad_bundle = bad_sum_dir.path().join("tampered.code-profile");
    fs::write(
        &bad_bundle,
        r#"{"name":"tampered","settings":{},"extensions":{"enabled":[]}}"#,
    )
    .expect("write");
    fs::write(
        bad_sum_dir.path().join("SHA256SUMS"),
        format!("{}  tampered.code-profile\n", "0".repeat(64)),
    )
    .expect("write sums");
    let bad_sum_res = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "--from",
            bad_bundle.to_str().expect("utf8"),
            "-p",
            "tampered",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("PATH", stub_bin.path())
        .output()
        .expect("tampered checksum");
    assert_eq!(bad_sum_res.status.code(), Some(5));

    // 4. All 5 N1 nested/string/case-variation autonomy payloads MUST exit 5 with ZERO mutation!
    let n1_payloads = [
        json!({"chat.tools.autoApprove": true}),
        json!({"chat": {"tools": {"autoApprove": true}}}),
        json!({"chat": {"tools.terminal.autoApprove": true}}),
        json!({"claudeCode": {"claudeProcessWrapper": "/tmp/x"}}),
        json!({"claudeCode.allowDangerouslySkipPermissions": "true"}),
    ];
    for (idx, payload) in n1_payloads.iter().enumerate() {
        let p_dir = TempDir::new().expect("n1 dir");
        let p_file = p_dir.path().join(format!("n1-{idx}.code-profile"));
        let doc = json!({
            "name": format!("n1-{idx}"),
            "settings": payload,
            "extensions": {"enabled": []}
        });
        fs::write(&p_file, serde_json::to_string_pretty(&doc).expect("ser")).expect("write");

        let res = vsp_cmd()
            .args([
                "--catalog",
                catalog_arg,
                "--vscode-data-dir",
                fake_data.path().to_str().expect("utf8"),
                "sync",
                "--from",
                p_file.to_str().expect("utf8"),
                "-p",
                &format!("n1-prof-{idx}"),
            ])
            .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
            .env("PATH", stub_bin.path())
            .output()
            .expect("run n1 import");
        assert_eq!(
            res.status.code(),
            Some(5),
            "N1 payload #{idx} must exit 5, stderr={}",
            String::from_utf8_lossy(&res.stderr)
        );
    }
}

#[cfg(unix)]
#[test]
fn exit_codes_lock_contention_6_missing_2_drift_3_invalid_4_list_fail_7_and_guards_5() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let ws = TempDir::new().expect("ws");
    let fake_data = TempDir::new().expect("data");
    let xdg_state = TempDir::new().expect("state");
    let stub_bin = TempDir::new().expect("stub");
    let pins_dir = TempDir::new().expect("pins");
    let empty_pins = write_empty_pins_file(pins_dir.path());

    fs::write(ws.path().join("Cargo.toml"), "[package]\nname=\"demo\"\n").expect("cargo");

    // Exit 6: Lock contention on workspace target lock!
    let ws_canon = fs::canonicalize(ws.path()).expect("canon");
    // Point lock to the same XDG_STATE_HOME lock path via env override.
    let ws_lock_path = xdg_state.path().join("vsp/locks").join(format!(
        "ws-{}.lock",
        sha256_hex(ws_canon.to_string_lossy().as_bytes())
    ));
    let ws_guard = acquire_advisory_lock(&ws_lock_path, Duration::from_secs(1)).expect("lock");
    let lock_busy = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "sync",
            ws.path().to_str().expect("utf8"),
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("VSP_LOCK_TIMEOUT_MS", "60")
        .output()
        .expect("run contended sync");
    assert_eq!(
        lock_busy.status.code(),
        Some(6),
        "expected exit 6 on lock contention, stderr={}",
        String::from_utf8_lossy(&lock_busy.stderr)
    );
    drop(ws_guard);

    // Silence unused helper warning by checking path helper parity.
    assert!(workspace_sync_lock_path(&ws_canon).file_name().is_some());
    assert!(
        profile_sync_lock_path(&fake_data.path().join("User"), "rust")
            .file_name()
            .is_some()
    );

    // Exit 2: missing workspace target.
    let missing_out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "sync",
            "/nonexistent/vsp/workspace",
        ])
        .output()
        .expect("missing ws");
    assert_eq!(missing_out.status.code(), Some(2));

    // Exit 3: diff --check drift.
    let drift_out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            "--check",
            ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("drift check");
    assert_eq!(drift_out.status.code(), Some(3));

    // Exit 4: invalid / permanently retired `--allow-comment-loss` flag and missing selector.
    let retired_flag = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "sync",
            "--allow-comment-loss",
            ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("retired flag");
    assert_eq!(retired_flag.status.code(), Some(4));

    let missing_sel = vsp_cmd()
        .args(["--catalog", catalog_arg, "sync"])
        .output()
        .expect("missing sel");
    assert_eq!(missing_sel.status.code(), Some(4));

    // Exit 7: failed `code --list-extensions` during profile sync on an existing registered profile.
    let reg_gs = fake_data.path().join("User/globalStorage");
    fs::create_dir_all(&reg_gs).expect("mkdir gs");
    fs::write(
        reg_gs.join("storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}]}"#,
    )
    .expect("register rust profile");
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'ipc failed' >&2\nexit 1\n",
    );
    let list_fail = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "rust",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("code list fail");
    assert_eq!(
        list_fail.status.code(),
        Some(7),
        "expected exit 7 on failed code --list-extensions, stderr={}",
        String::from_utf8_lossy(&list_fail.stderr)
    );

    // Exit 5: non-TTY `--prune=unmanaged` without `-y/--yes`.
    write_executable_stub(stub_bin.path(), "code", "#!/bin/sh\nexit 0\n");
    let prune_refuse = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "rust",
            "--prune=unmanaged",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("prune unmanaged without -y");
    assert_eq!(prune_refuse.status.code(), Some(5));

    // Exit 5: symlink escape in workspace `.vscode`.
    let outside = TempDir::new().expect("outside");
    let escape_ws = TempDir::new().expect("escape ws");
    fs::write(
        escape_ws.path().join("Cargo.toml"),
        "[package]\nname=\"x\"\n",
    )
    .expect("cargo");
    std::os::unix::fs::symlink(outside.path(), escape_ws.path().join(".vscode")).expect("symlink");
    let escape_res = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "sync",
            escape_ws.path().to_str().expect("utf8"),
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .output()
        .expect("symlink escape");
    assert_eq!(escape_res.status.code(), Some(5));
}

#[cfg(unix)]
#[test]
fn repair_pins_downgrade_requires_yes_and_open_sync_aborts_on_failure() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("data");
    let stub_bin = TempDir::new().expect("stub");
    let xdg_state = TempDir::new().expect("state");
    let xdg_cache = TempDir::new().expect("cache");
    let fixture_dir = TempDir::new().expect("fixture");

    // Create local VSIX fixture + pin file with version 0.18.2 while stub reports installed 0.19.0 (downgrade!).
    let vsix_fixture = fixture_dir.path().join("editorconfig.vsix");
    let vsix_bytes = b"vsix-zip-bytes-0.18.2";
    fs::write(&vsix_fixture, vsix_bytes).expect("write vsix");
    let vsix_sha = sha256_hex(vsix_bytes);
    let custom_pins = fixture_dir.path().join("pins.json");
    fs::write(
        &custom_pins,
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "pins": {
                "editorconfig.editorconfig": {
                    "version": "0.18.2",
                    "sha256": vsix_sha,
                    "vsix_url": format!("file://{}", vsix_fixture.display())
                }
            }
        }))
        .expect("ser pins"),
    )
    .expect("write pins");

    let code_log = fixture_dir.path().join("code-repair.log");
    let stub_script = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  *--list-extensions*)\n    echo 'editorconfig.editorconfig@0.19.0'\n    exit 0\n    ;;\nesac\nexit 0\n",
        code_log.display()
    );
    write_executable_stub(stub_bin.path(), "code", &stub_script);

    // 1. Non-TTY `--repair-pins` downgrade without `--yes` MUST exit 5!
    let refuse = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "general-minimal",
            "--repair-pins",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &custom_pins)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CACHE_HOME", xdg_cache.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("repair downgrade without yes");
    assert_eq!(
        refuse.status.code(),
        Some(5),
        "expected exit 5 for downgrade without --yes, stderr={}",
        String::from_utf8_lossy(&refuse.stderr)
    );
    assert!(String::from_utf8_lossy(&refuse.stderr).contains("downgrade"));

    // 2. With `--yes`, stages & verifies VSIX into XDG cache and installs with `--force`!
    let ok = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "--yes",
            "sync",
            "-p",
            "general-minimal",
            "--repair-pins",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &custom_pins)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CACHE_HOME", xdg_cache.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("repair downgrade with yes");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "expected exit 0 for downgrade with --yes, stderr={}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let log_text = fs::read_to_string(&code_log).expect("read log");
    assert!(log_text.contains("--force"));
    assert!(xdg_cache
        .path()
        .join(format!(
            "vsp/vsix/editorconfig.editorconfig-{vsix_sha}.vsix"
        ))
        .is_file());

    // 3. `vsp use <PROFILE> <PATH> --sync` and `vsp open <PATH> --sync` abort launch when sync fails!
    fs::write(&code_log, "").expect("clear log");
    let empty_ws = TempDir::new().expect("empty ws");
    let use_fail = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "use",
            "general-minimal",
            empty_ws.path().to_str().expect("utf8"),
            "--sync",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("use --sync empty ws");
    assert_eq!(use_fail.status.code(), Some(3));
    assert!(
        fs::read_to_string(&code_log).expect("log").is_empty(),
        "code must NOT be launched when --sync fails"
    );
    let open_fail = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "open",
            empty_ws.path().to_str().expect("utf8"),
            "--sync",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("open --sync empty ws");
    assert_eq!(open_fail.status.code(), Some(3));
    assert!(
        fs::read_to_string(&code_log).expect("log").is_empty(),
        "code must NOT be launched when --sync fails"
    );

    // 4. `vsp open` on a valid Rust workspace with `-p general-minimal --sync` converges and launches!
    let rust_ws = TempDir::new().expect("rust ws");
    fs::write(
        rust_ws.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\n",
    )
    .expect("cargo");
    let open_ok = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "open",
            rust_ws.path().to_str().expect("utf8"),
            "-p",
            "general-minimal",
            "--sync",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &custom_pins)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CACHE_HOME", xdg_cache.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("open --sync ok");
    assert_eq!(
        open_ok.status.code(),
        Some(0),
        "open --sync failed: {}",
        String::from_utf8_lossy(&open_ok.stderr)
    );
    assert!(rust_ws.path().join(".vscode/settings.json").is_file());
    let launch_log = fs::read_to_string(&code_log).expect("log");
    assert!(launch_log.contains(&format!(
        "--user-data-dir {} --profile=general-minimal -- {}",
        fake_data.path().display(),
        rust_ws.path().display()
    )));
}

#[test]
fn dev_ops_pins_docs_hooks_and_compat_mock_marketplace() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");

    // 1. `vsp dev pins --check` passes cleanly on tracked repo and cites `vsp.lock.json` (SOL-08).
    let pins_ok = vsp_cmd()
        .args(["--catalog", catalog_arg, "dev", "pins", "--check"])
        .output()
        .expect("dev pins --check");
    assert_eq!(pins_ok.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&pins_ok.stdout).contains("vsp.lock.json"),
        "dev pins --check on v2.2 catalog must cite vsp.lock.json, stdout={}",
        String::from_utf8_lossy(&pins_ok.stdout)
    );

    // 2. `vsp dev docs --write` + `--check` passes cleanly on a temp repo copy with updated
    //    `Packs` matrix format (SOL-08; Wave 3 regenerates tracked README.md).
    let docs_tmp = TempDir::new().expect("docs tmp");
    fs::create_dir_all(docs_tmp.path().join("docs")).expect("mkdir docs");
    fs::create_dir_all(docs_tmp.path().join("catalog")).expect("mkdir catalog");
    fs::create_dir_all(docs_tmp.path().join("packs")).expect("mkdir packs");
    for rel in [
        "docs/rust.md",
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "CODEX.md",
        "GEMINI.md",
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), docs_tmp.path().join(rel)).expect("copy repo file");
    }
    fn copy_tree(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).expect("mkdir dst");
        for entry in fs::read_dir(src).expect("read src").flatten() {
            let p = entry.path();
            let t = dst.join(entry.file_name());
            if p.is_dir() {
                copy_tree(&p, &t);
            } else {
                fs::copy(&p, &t).expect("copy");
            }
        }
    }
    copy_tree(&root.join("packs"), &docs_tmp.path().join("packs"));
    let docs_arg = docs_tmp.path().to_str().expect("utf8");
    let docs_write = vsp_cmd()
        .args(["--catalog", docs_arg, "dev", "docs", "--write"])
        .output()
        .expect("dev docs --write");
    assert_eq!(
        docs_write.status.code(),
        Some(0),
        "dev docs --write failed: stdout={} stderr={}",
        String::from_utf8_lossy(&docs_write.stdout),
        String::from_utf8_lossy(&docs_write.stderr)
    );
    let docs_ok = vsp_cmd()
        .args(["--catalog", docs_arg, "dev", "docs", "--check"])
        .output()
        .expect("dev docs --check");
    assert_eq!(
        docs_ok.status.code(),
        Some(0),
        "dev docs --check failed: stdout={} stderr={}",
        String::from_utf8_lossy(&docs_ok.stdout),
        String::from_utf8_lossy(&docs_ok.stderr)
    );

    // 3. `vsp dev compat` against a local mock HTTP Marketplace server!
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let addr = listener.local_addr().expect("addr");
    let server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut req_buf = [0u8; 4096];
            let _ = stream.read(&mut req_buf);
            let body = serde_json::to_string(&json!({
                "results": [{
                    "extensions": [{
                        "publisher": { "publisherName": "editorconfig" },
                        "extensionName": "editorconfig",
                        "versions": [{
                            "version": "0.18.2",
                            "properties": [
                                { "key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.80.0" }
                            ]
                        }]
                    }]
                }]
            }))
            .expect("ser");
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let ext_tmp = TempDir::new().expect("ext tmp");
    let ext_file = ext_tmp.path().join("exts.txt");
    fs::write(&ext_file, "editorconfig.editorconfig\n").expect("write exts");

    let compat = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--json",
            "dev",
            "compat",
            "--extensions-file",
            ext_file.to_str().expect("utf8"),
            "--vscode-version",
            "1.147.0",
        ])
        .env("VSP_MARKETPLACE_API_URL", format!("http://{addr}/query"))
        .output()
        .expect("dev compat");
    let _ = server.join();
    assert_eq!(
        compat.status.code(),
        Some(0),
        "dev compat failed: stderr={}",
        String::from_utf8_lossy(&compat.stderr)
    );
    let compat_json: Value = serde_json::from_slice(&compat.stdout).expect("compat json");
    assert_eq!(compat_json["summary"]["compatible"], 1);
    assert_eq!(compat_json["summary"]["incompatible"], 0);
}

#[cfg(unix)]
#[test]
fn vsix_off_host_redirect_rejected_exit_5_and_allow_unlocked_recorded_in_xdg_state_and_doctor() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("data");
    let stub_bin = TempDir::new().expect("stub");
    let xdg_state = TempDir::new().expect("state");
    let xdg_cache = TempDir::new().expect("cache");
    let xdg_config = TempDir::new().expect("config");
    let fixture_dir = TempDir::new().expect("fixture");

    write_executable_stub(stub_bin.path(), "code", "#!/bin/sh\nexit 0\n");

    // 1. Local HTTP server returning 302 redirect to an off-host URL (`https://evil.example/...`) -> MUST exit 5!
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let redir_server = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 302 Found\r\nLocation: https://evil.example/hijack.vsix\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let redir_pins = fixture_dir.path().join("redir-pins.json");
    fs::write(
        &redir_pins,
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "pins": {
                "editorconfig.editorconfig": {
                    "version": "0.18.2",
                    "sha256": "a".repeat(64),
                    "vsix_url": format!("http://{addr}/editorconfig.vsix")
                }
            }
        }))
        .expect("ser"),
    )
    .expect("write redir pins");

    let redir_out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "general-minimal",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &redir_pins)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CACHE_HOME", xdg_cache.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("run redirect test");
    let _ = redir_server.join();
    assert_eq!(
        redir_out.status.code(),
        Some(5),
        "off-host VSIX redirect must fail closed with exit 5, stderr={}",
        String::from_utf8_lossy(&redir_out.stderr)
    );

    // 2. Missing pins manifest fails closed with exit 5 unless `--allow-unlocked` is passed!
    let missing_pins = fixture_dir.path().join("does-not-exist-pins.json");
    let fail_closed = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "general-minimal",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &missing_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("missing pins fail closed");
    assert_eq!(fail_closed.status.code(), Some(5));

    // 3. Explicit `--allow-unlocked` prints warning to stderr, succeeds, records `unlockedBypass: true`
    //    in XDG provenance, and triggers a provenance warning in `vsp doctor`!
    let unlocked_ok = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "general-minimal",
            "--allow-unlocked",
        ])
        .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
        .env("ARTAGON_VSCODE_PINS_FILE", &missing_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CONFIG_HOME", xdg_config.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("allow unlocked sync");
    assert_eq!(
        unlocked_ok.status.code(),
        Some(0),
        "allow-unlocked sync failed: {}",
        String::from_utf8_lossy(&unlocked_ok.stderr)
    );
    assert!(String::from_utf8_lossy(&unlocked_ok.stderr).contains("WARNING"));

    let prov_dir = xdg_state.path().join("vsp/profiles");
    let prov_files: Vec<PathBuf> = fs::read_dir(&prov_dir)
        .expect("ls prov")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    assert!(!prov_files.is_empty());
    let has_unlocked_receipt = prov_files.iter().any(|p| {
        fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .is_some_and(|v| v["profile"] == "general-minimal" && v["unlockedBypass"] == true)
    });
    assert!(
        has_unlocked_receipt,
        "expected XDG profile provenance with unlockedBypass=true"
    );

    let doc_out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CONFIG_HOME", xdg_config.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("doctor after unlocked");
    assert!(
        String::from_utf8_lossy(&doc_out.stdout).contains("--allow-unlocked"),
        "doctor should report --allow-unlocked provenance warning, stdout={}",
        String::from_utf8_lossy(&doc_out.stdout)
    );
}

#[cfg(unix)]
#[test]
fn user_default_sync_preserves_comments_and_satisfies_doctor_strict() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("data");
    let stub_bin = TempDir::new().expect("stub");
    let xdg_state = TempDir::new().expect("state");
    let xdg_config = TempDir::new().expect("config");

    let user_dir = fake_data.path().join("User");
    fs::create_dir_all(&user_dir).expect("mkdir User");
    let user_settings_path = user_dir.join("settings.json");
    fs::write(
        &user_settings_path,
        r#"// User Default settings banner
{
  /* Personal user key */
  "my.userFlag": "keep", // inline user note
  "extensions.autoUpdate": true
}
"#,
    )
    .expect("write default settings");

    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'editorconfig.editorconfig@0.18.2'\nexit 0\n",
    );

    let sync_user = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "--user",
            "--with",
            "rtk-terminal",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CONFIG_HOME", xdg_config.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("sync --user");
    assert_eq!(
        sync_user.status.code(),
        Some(0),
        "sync --user failed: {}",
        String::from_utf8_lossy(&sync_user.stderr)
    );

    let merged = fs::read_to_string(&user_settings_path).expect("read");
    assert!(merged.contains("// User Default settings banner"));
    assert!(merged.contains("/* Personal user key */"));
    assert!(merged.contains("// inline user note"));
    let parsed = parse_jsonc_value(&merged, "user settings").expect("valid jsonc");
    assert_eq!(parsed["my.userFlag"], "keep");
    assert_eq!(parsed["telemetry.telemetryLevel"], "off");
    assert_eq!(parsed["security.workspace.trust.untrustedFiles"], "prompt");
    assert_eq!(parsed["extensions.autoUpdate"], false);

    // Doctor --strict MUST pass cleanly with both Default security keys and XDG vsp/rtk wrappers!
    let doc_strict = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
            "--strict",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("XDG_CONFIG_HOME", xdg_config.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("doctor --strict");
    assert_eq!(
        doc_strict.status.code(),
        Some(0),
        "expected doctor --strict=0 after sync --user --with rtk-terminal, stdout={}",
        String::from_utf8_lossy(&doc_strict.stdout)
    );
}

#[cfg(unix)]
#[test]
fn dev_hooks_compat_unreachable_exit_7_ps_fail_closed_and_data_dir_symlink_escape_exit_5() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");

    // 1. `run_dev_hooks`: idempotent install & exit 5 refusal on foreign `core.hooksPath`.
    let tmp_repo = TempDir::new().expect("git repo");
    let init = Command::new("git")
        .arg("-C")
        .arg(tmp_repo.path())
        .arg("init")
        .output()
        .expect("git init");
    assert!(init.status.success());

    // Astra A04: a repository without a usable `scripts/git-hooks/pre-commit` is refused
    // BEFORE `core.hooksPath` is written.
    let refused = vsp::dev_ops::run_dev_hooks(tmp_repo.path(), false, true)
        .expect_err("hooks without pre-commit");
    assert_eq!(refused.exit_code().as_i32(), 1);
    let hooks_dir = tmp_repo.path().join("scripts/git-hooks");
    fs::create_dir_all(&hooks_dir).expect("mkdir hooks");
    write_executable_stub(&hooks_dir, "pre-commit", "#!/bin/sh\nexit 0\n");

    let h1 = vsp::dev_ops::run_dev_hooks(tmp_repo.path(), false, true).expect("hooks 1");
    assert_eq!(h1.as_i32(), 0);
    let h2 = vsp::dev_ops::run_dev_hooks(tmp_repo.path(), false, true).expect("hooks 2");
    assert_eq!(h2.as_i32(), 0);

    let cfg = Command::new("git")
        .arg("-C")
        .arg(tmp_repo.path())
        .args(["config", "core.hooksPath", ".husky"])
        .output()
        .expect("set foreign hooks");
    assert!(cfg.status.success());
    let h3 = vsp::dev_ops::run_dev_hooks(tmp_repo.path(), false, true)
        .expect_err("must refuse overwriting foreign hooksPath");
    assert_eq!(h3.exit_code().as_i32(), 5);

    // 2. `vsp dev compat` unreachable Marketplace API exits 7 (IncompleteObservation).
    let dead_listener = TcpListener::bind("127.0.0.1:0").expect("bind dead");
    let dead_addr = dead_listener.local_addr().expect("addr");
    drop(dead_listener);
    let ext_tmp = TempDir::new().expect("ext");
    let ext_file = ext_tmp.path().join("exts.txt");
    fs::write(&ext_file, "editorconfig.editorconfig\n").expect("write");
    let compat_down = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "dev",
            "compat",
            "--extensions-file",
            ext_file.to_str().expect("utf8"),
            "--vscode-version",
            "1.147.0",
        ])
        .env(
            "VSP_MARKETPLACE_API_URL",
            format!("http://{dead_addr}/query"),
        )
        .output()
        .expect("compat dead");
    assert_eq!(compat_down.status.code(), Some(7));

    // 3. Failing `ps` scan treated as busy (fail-closed -> exit 5) when creating new profile!
    let stub_bin = TempDir::new().expect("stub");
    let fake_data = TempDir::new().expect("data");
    let xdg_state = TempDir::new().expect("state");
    let pins_dir = TempDir::new().expect("pins");
    let empty_pins = write_empty_pins_file(pins_dir.path());
    write_executable_stub(stub_bin.path(), "code", "#!/bin/sh\nexit 0\n");
    write_executable_stub(stub_bin.path(), "ps", "#!/bin/sh\nexit 1\n");

    let ps_fail = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "sync",
            "-p",
            "ps-fail-new-prof",
            "--preset",
            "rust",
        ])
        .env_remove("ARTAGON_VSCODE_TEST_RUNNING")
        .env("ARTAGON_VSCODE_PINS_FILE", &empty_pins)
        .env("XDG_STATE_HOME", xdg_state.path())
        .env("PATH", stub_bin.path())
        .output()
        .expect("ps fail test");
    assert_eq!(
        ps_fail.status.code(),
        Some(5),
        "failed ps scan must fail-closed as busy (exit 5), stderr={}",
        String::from_utf8_lossy(&ps_fail.stderr)
    );

    // 4. Symlink escape on `--vscode-data-dir/User` MUST refuse with exit 5!
    let escaped_root = TempDir::new().expect("escaped root");
    let outside = TempDir::new().expect("outside");
    std::os::unix::fs::symlink(outside.path(), escaped_root.path().join("User")).expect("symlink");
    let data_escape = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            escaped_root.path().to_str().expect("utf8"),
            "sync",
            "--user",
        ])
        .env("XDG_STATE_HOME", xdg_state.path())
        .output()
        .expect("data dir escape");
    assert_eq!(data_escape.status.code(), Some(5));
}

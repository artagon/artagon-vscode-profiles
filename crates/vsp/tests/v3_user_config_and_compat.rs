//! v3.0.0 integration tests: personal user configuration, `vsp init`, `vsp self`, dynamic
//! completions, and the legacy `vspcli` compatibility mode.
//!
//! Each test runs the real `vsp` binary in a hermetic environment: `HOME` and all XDG
//! directories point to a temporary directory, and `PATH` holds only a stub `code` CLI.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

/// Catalog-external extension ID used as the Tier 3 `user-unlocked` sample.
const EXTERNAL_EXT: &str = "zz-external.sample-tool";

/// Extension that the catalog marks as compromised.
const COMPROMISED_EXT: &str = "jscearcy.rust-doc-viewer";

fn vsp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vsp"))
}

/// Hermetic directories for one test.
struct Sandbox {
    root: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = TempDir::new().expect("sandbox root");
        for dir in [
            "home",
            "config",
            "state",
            "cache",
            "bin",
            "cwd",
            "data/User/globalStorage",
        ] {
            fs::create_dir_all(root.path().join(dir)).expect("mkdir sandbox dir");
        }
        fs::write(
            root.path().join("data/User/globalStorage/storage.json"),
            r#"{"userDataProfiles":[]}"#,
        )
        .expect("seed storage.json");
        fs::write(
            root.path().join("pins.json"),
            "{\"version\":1,\"pins\":{}}\n",
        )
        .expect("write empty pins");
        Self { root }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn data_dir(&self) -> String {
        self.path("data").to_str().expect("utf8").to_string()
    }

    fn default_config_path(&self) -> PathBuf {
        self.path("config/vsp/config.jsonc")
    }

    /// Writes the user configuration at the XDG default path.
    fn write_default_config(&self, text: &str) {
        let path = self.default_config_path();
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir config");
        fs::write(path, text).expect("write user config");
    }

    /// Writes a named configuration file outside the default path (for `--config`).
    fn write_config_file(&self, name: &str, text: &str) -> String {
        let path = self.path(name);
        fs::write(&path, text).expect("write config file");
        path.to_str().expect("utf8").to_string()
    }

    /// Installs a stub `code` CLI that appends its arguments to `code.log`.
    fn install_code_stub(&self) -> PathBuf {
        let log = self.path("code.log");
        let script = format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n", log.display());
        let stub = self.path("bin/code");
        fs::write(&stub, script).expect("write code stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");
        log
    }

    /// Installs a stub `code` CLI that fails each invocation.
    fn install_failing_code_stub(&self) {
        let stub = self.path("bin/code");
        fs::write(&stub, "#!/bin/sh\necho 'boom' >&2\nexit 9\n").expect("write code stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");
    }

    fn command(&self, program: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(self.path("cwd"))
            .env_clear()
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("PATH", self.path("bin"))
            .env("ARTAGON_VSCODE_TEST_RUNNING", "0")
            .env("ARTAGON_VSCODE_PINS_FILE", self.path("pins.json"));
        cmd
    }

    fn vsp(&self, args: &[&str]) -> Output {
        self.command(&vsp_bin())
            .args(args)
            .output()
            .expect("run vsp")
    }

    /// Runs `vsp` in legacy compatibility mode through `VSP_COMPAT_VSPCLI=1`.
    fn vspcli(&self, args: &[&str]) -> Output {
        self.command(&vsp_bin())
            .env("VSP_COMPAT_VSPCLI", "1")
            .env("VSCODE_USER_DATA_DIR", self.path("data"))
            .args(args)
            .output()
            .expect("run vsp in compat mode")
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_exit(out: &Output, expected: i32, context: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{context}: expected exit {expected}\nstdout={}\nstderr={}",
        stdout(out),
        stderr(out)
    );
}

fn json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", stdout(out)))
}

fn custom_profile_config() -> String {
    format!(
        r#"// personal config
{{
  "version": 1,
  "profiles": {{
    "my-rust": {{
      "description": "Rust with my Git tools",
      "extends": "rust",
      "with": ["git-pro"],
      "overrides": {{
        "editor.fontSize": 17,
        "[rust]": {{ "editor.formatOnSave": true }}
      }},
      "extraExtensions": ["{EXTERNAL_EXT}"]
    }}
  }}
}}
"#
    )
}

/// Finds the `settings.json` of the single named profile that a sync created.
fn created_profile_settings(sb: &Sandbox) -> Value {
    let profiles_dir = sb.path("data/User/profiles");
    let mut found: Vec<PathBuf> = fs::read_dir(&profiles_dir)
        .expect("profiles dir")
        .filter_map(Result::ok)
        .map(|e| e.path().join("settings.json"))
        .filter(|p| p.is_file())
        .collect();
    assert_eq!(found.len(), 1, "expected exactly one profile settings file");
    let text = fs::read_to_string(found.remove(0)).expect("read profile settings");
    vsp::legacy_loader::parse_jsonc_value(&text, "profile settings").expect("valid JSONC")
}

#[test]
fn custom_user_profile_lists_shows_diffs_syncs_and_exports_without_repo_fork() {
    let sb = Sandbox::new();
    sb.write_default_config(&custom_profile_config());
    let code_log = sb.install_code_stub();
    let data = sb.data_dir();

    // list: the custom profile appears next to the catalog presets.
    let list = sb.vsp(&["list"]);
    assert_exit(&list, 0, "list");
    let names = stdout(&list);
    assert!(names.lines().any(|l| l == "my-rust"), "{names}");
    assert!(names.lines().any(|l| l == "rust"), "{names}");

    let list_json = sb.vsp(&["list", "--json"]);
    assert_exit(&list_json, 0, "list --json");
    let items = json(&list_json);
    let mine = items
        .as_array()
        .expect("array")
        .iter()
        .find(|i| i["id"] == "my-rust")
        .expect("my-rust listed");
    assert_eq!(mine["kind"], "user-profile");
    assert_eq!(mine["description"], "Rust with my Git tools");

    // show: resolved through `extends` + `with`, with the extra extension and its tier.
    let show = sb.vsp(&["show", "my-rust", "--json"]);
    assert_exit(&show, 0, "show");
    let shown = json(&show);
    assert_eq!(shown["kind"], "user-profile");
    assert_eq!(shown["extends"], "rust");
    let modules: Vec<&str> = shown["resolvedModules"]
        .as_array()
        .expect("modules")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        modules.contains(&"rust") && modules.contains(&"git-pro"),
        "{modules:?}"
    );
    assert_eq!(shown["extraExtensions"][0]["id"], EXTERNAL_EXT);
    assert_eq!(shown["extraExtensions"][0]["tier"], "user-unlocked");
    assert!(shown["extensions"]
        .as_array()
        .expect("extensions")
        .iter()
        .any(|e| e == EXTERNAL_EXT));

    // diff: overrides and the extra extension are in the plan.
    let diff = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "my-rust",
        "--json",
    ]);
    assert_exit(&diff, 0, "diff");
    let plan = json(&diff);
    assert_eq!(plan["target"]["preset"], "my-rust");
    let font = plan["settings_changes"]
        .as_array()
        .expect("settings changes")
        .iter()
        .find(|c| c["key"] == "editor.fontSize")
        .expect("font override planned");
    assert_eq!(font["desired"], 17);
    assert!(plan["extension_changes"]
        .as_array()
        .expect("extension changes")
        .iter()
        .any(|c| c["id"] == EXTERNAL_EXT));
    assert!(plan["warnings"][0]
        .as_str()
        .expect("warning")
        .contains("user-unlocked"));

    // `sync --dry-run` prints the same bytes as `diff`.
    let dry = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "my-rust",
        "--json",
    ]);
    assert_exit(&dry, 0, "sync --dry-run");
    assert_eq!(
        dry.stdout, diff.stdout,
        "diff and sync --dry-run must be identical"
    );

    // sync: settings are written and the external extension installs by exact ID.
    let sync = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "-p",
        "my-rust",
        "--json",
    ]);
    assert_exit(&sync, 0, "sync");
    let report = json(&sync);
    assert_eq!(report["kind"], "profile");
    assert_eq!(report["target"], "my-rust");
    assert!(report["installed"]
        .as_array()
        .expect("installed")
        .iter()
        .any(|e| e == EXTERNAL_EXT));
    assert!(report["warnings"][0]
        .as_str()
        .expect("warning")
        .contains("user-unlocked"));

    let settings = created_profile_settings(&sb);
    assert_eq!(settings["editor.fontSize"], 17);
    assert_eq!(settings["[rust]"]["editor.formatOnSave"], true);

    let log = fs::read_to_string(&code_log).expect("code log");
    assert!(
        log.contains(&format!("--install-extension {EXTERNAL_EXT}")),
        "external extension must install by exact ID: {log}"
    );
    // The profile name is one argument (`--profile=<name>`), never a separate token.
    assert!(log.contains("--profile=my-rust"), "{log}");
    assert!(!log.contains("--profile my-rust"), "{log}");

    // export: the bundle carries the overrides and the extra extension.
    let out_dir = sb.path("exports");
    let export = sb.vsp(&[
        "export",
        "my-rust",
        "--out",
        out_dir.to_str().expect("utf8"),
    ]);
    assert_exit(&export, 0, "export");
    let bundle: Value = serde_json::from_str(
        &fs::read_to_string(out_dir.join("my-rust.code-profile")).expect("bundle"),
    )
    .expect("bundle JSON");
    assert_eq!(bundle["settings"]["editor.fontSize"], 17);
    assert!(bundle["extensions"]["enabled"]
        .as_array()
        .expect("enabled")
        .iter()
        .any(|e| e == EXTERNAL_EXT));
}

#[test]
fn user_config_security_rejections_use_exit_4_and_5() {
    let sb = Sandbox::new();
    let data = sb.data_dir();

    let cases: Vec<(&str, String, i32, &str)> = vec![
        (
            "bare single-segment override key",
            r#"{"version":1,"profiles":{"p":{"overrides":{"chat":{"tools":{"autoApprove":true}}}}}}"#
                .to_string(),
            4,
            "literal VS Code setting IDs",
        ),
        (
            "bare claudeCode override key",
            r#"{"version":1,"profiles":{"p":{"overrides":{"claudeCode":{"claudeProcessWrapper":"/tmp/x"}}}}}"#
                .to_string(),
            4,
            "literal VS Code setting IDs",
        ),
        (
            "autonomy deny-list payload",
            r#"{"version":1,"profiles":{"p":{"overrides":{"chat.tools.autoApprove":true}}}}"#
                .to_string(),
            5,
            "deny-list",
        ),
        (
            "deny-list payload nested in a language block",
            r#"{"version":1,"profiles":{"p":{"overrides":{"[rust]":{"claudeCode.allowDangerouslySkipPermissions":"true"}}}}}"#
                .to_string(),
            5,
            "deny-list",
        ),
        (
            "wildcard trust.publishers",
            r#"{"version":1,"trust":{"publishers":["*"]}}"#.to_string(),
            4,
            "exact ID only",
        ),
        (
            "trustedPublishers in defaults",
            r#"{"version":1,"defaults":{"trustedPublishers":["acme"]}}"#.to_string(),
            4,
            "exact ID only",
        ),
        (
            "wildcard extension ID",
            r#"{"version":1,"profiles":{"p":{"extraExtensions":["acme.*"]}}}"#.to_string(),
            4,
            "exact extension ID",
        ),
        (
            "compromised catalog extension",
            format!(r#"{{"version":1,"profiles":{{"p":{{"extraExtensions":["{COMPROMISED_EXT}"]}}}}}}"#),
            5,
            "compromised",
        ),
        (
            "profile name collides with a catalog preset",
            r#"{"version":1,"profiles":{"rust":{}}}"#.to_string(),
            4,
            "collides",
        ),
    ];

    for (idx, (name, text, expected, needle)) in cases.iter().enumerate() {
        let cfg = sb.write_config_file(&format!("bad-{idx}.jsonc"), text);
        // Each consumer of the user configuration fails closed with the same exit code.
        for args in [
            vec!["--config", cfg.as_str(), "list"],
            vec!["--config", cfg.as_str(), "show", "rust"],
            vec![
                "--config",
                cfg.as_str(),
                "--vscode-data-dir",
                &data,
                "diff",
                "-p",
                "rust",
            ],
            vec![
                "--config",
                cfg.as_str(),
                "--vscode-data-dir",
                &data,
                "sync",
                "-p",
                "rust",
            ],
            vec!["--config", cfg.as_str(), "export", "rust", "--out", "out"],
        ] {
            let out = sb.vsp(&args);
            assert_exit(&out, *expected, &format!("{name}: vsp {}", args.join(" ")));
            assert!(
                stderr(&out).contains(needle),
                "{name}: stderr must mention '{needle}': {}",
                stderr(&out)
            );
        }

        // `vsp doctor` is the exception (Opus R2-M2): it records the rejected config as a
        // FAIL check and still runs every other check. This sandbox has no `code` CLI, so
        // the audit is also incomplete (exit 7).
        let doctor = sb.vsp(&[
            "--config",
            cfg.as_str(),
            "--vscode-data-dir",
            &data,
            "doctor",
        ]);
        assert_exit(&doctor, 7, &format!("{name}: vsp doctor"));
        let doctor_out = stdout(&doctor);
        assert!(
            doctor_out.contains("FAIL: user config") && doctor_out.contains(needle),
            "{name}: doctor must record the rejected config: {doctor_out}"
        );
        assert!(
            doctor_out.contains("PASS: catalog") && doctor_out.contains("Summary:"),
            "{name}: doctor must still run the other checks: {doctor_out}"
        );
    }
    assert!(
        !sb.path("cwd/out").exists(),
        "a rejected config must not write exports"
    );
    assert!(
        !sb.path("data/User/profiles").exists(),
        "a rejected config must not change the VS Code store"
    );

    // An explicit --config file that does not exist is a missing target (exit 2).
    let missing = sb.vsp(&["--config", "/nonexistent/vsp/config.jsonc", "list"]);
    assert_exit(&missing, 2, "missing --config");
}

#[test]
fn exact_id_external_extension_is_flagged_user_unlocked_in_doctor() {
    let sb = Sandbox::new();
    let cfg = sb.write_config_file("config.jsonc", &custom_profile_config());
    let data = sb.data_dir();
    // Doctor needs the `code` CLI for a complete audit (exit 7 without it).
    sb.install_code_stub();

    let doc = sb.vsp(&[
        "--config",
        &cfg,
        "--vscode-data-dir",
        &data,
        "doctor",
        "--json",
    ]);
    assert_exit(&doc, 0, "doctor");
    let report = json(&doc);
    assert_eq!(report["user_unlocked_extensions"][0], EXTERNAL_EXT);
    let checks = report["checks"].as_array().expect("checks");
    let flagged = checks
        .iter()
        .find(|c| {
            c["category"] == "user-config"
                && c["message"]
                    .as_str()
                    .is_some_and(|m| m.contains(EXTERNAL_EXT))
        })
        .expect("user-unlocked check");
    assert_eq!(flagged["severity"], "warn");
    assert!(flagged["message"]
        .as_str()
        .expect("message")
        .contains("user-unlocked (Tier 3)"));

    let text = sb.vsp(&["--config", &cfg, "--vscode-data-dir", &data, "doctor"]);
    assert_exit(&text, 0, "doctor text");
    assert!(
        stdout(&text).contains(&format!(
            "WARN: user-unlocked (Tier 3): profile 'my-rust' enables catalog-external extension '{EXTERNAL_EXT}'"
        )),
        "{}",
        stdout(&text)
    );

    // --strict treats the user-unlocked warning as a failure.
    let strict = sb.vsp(&[
        "--config",
        &cfg,
        "--vscode-data-dir",
        &data,
        "doctor",
        "--strict",
    ]);
    assert_exit(&strict, 1, "doctor --strict");

    // Without a user configuration, doctor reports a PASS and no user-unlocked list.
    let plain = sb.vsp(&["--vscode-data-dir", &data, "doctor", "--json"]);
    assert_exit(&plain, 0, "doctor without user config");
    let plain_report = json(&plain);
    assert!(plain_report.get("user_unlocked_extensions").is_none());
    assert!(plain_report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .any(|c| c["category"] == "user-config" && c["severity"] == "pass"));
}

#[test]
fn hostile_cwd_config_files_are_ignored_byte_for_byte() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    // A profile diff observes the installed extensions with the `code` CLI.
    sb.install_code_stub();
    let ws = sb.path("cwd");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");

    let run_all = |sb: &Sandbox| -> Vec<Output> {
        vec![
            sb.vsp(&["diff", ".", "--json"]),
            sb.vsp(&["diff", "."]),
            sb.vsp(&["--vscode-data-dir", &data, "diff", "-p", "rust", "--json"]),
            sb.vsp(&["--dry-run", "sync", ".", "--json"]),
            sb.vsp(&["sync", ".", "--json"]),
            sb.vsp(&["sync", "."]),
            sb.vsp(&["list", "--json"]),
        ]
    };
    let managed_files = |ws: &Path| -> Vec<Vec<u8>> {
        ["settings.json", "extensions.json", "tasks.json"]
            .iter()
            .map(|f| fs::read(ws.join(".vscode").join(f)).unwrap_or_default())
            .collect()
    };

    // First real sync creates `.vscode/`. The baseline is taken after it, so each later
    // run starts from the same converged state.
    assert_exit(&sb.vsp(&["sync", "."]), 0, "initial sync");
    let baseline = run_all(&sb);
    for (idx, out) in baseline.iter().enumerate() {
        assert_exit(out, 0, &format!("baseline run {idx}"));
    }
    let baseline_files = managed_files(&ws);

    // Hostile repo-local configuration: each file would be rejected (exit 4 or 5) or would
    // change the plan if vsp read it.
    fs::write(
        ws.join(".vsp.toml"),
        "version = 1\n[trust]\npublishers = [\"*\"]\n[defaults]\nux = \"retina\"\nai = \"off\"\n[profiles.rust.overrides]\n\"chat.tools.autoApprove\" = true\n",
    )
    .expect("write .vsp.toml");
    let hostile_jsonc = r#"{
  "version": 1,
  "trust": { "publishers": ["*"] },
  "defaults": { "ux": "retina", "ai": "off" },
  "profiles": { "rust": { "overrides": { "chat.tools.autoApprove": true },
                          "extraExtensions": ["evil.backdoor"] } }
}
"#;
    fs::write(ws.join(".vsp.jsonc"), hostile_jsonc).expect("write .vsp.jsonc");
    fs::write(ws.join("config.jsonc"), hostile_jsonc).expect("write config.jsonc");
    fs::create_dir_all(ws.join(".config/vsp")).expect("mkdir .config/vsp");
    fs::write(ws.join(".config/vsp/config.jsonc"), hostile_jsonc).expect("write nested config");

    let hostile = run_all(&sb);
    for (idx, (clean, dirty)) in baseline.iter().zip(&hostile).enumerate() {
        assert_eq!(
            clean.status.code(),
            dirty.status.code(),
            "run {idx}: exit code"
        );
        assert_eq!(
            clean.stdout, dirty.stdout,
            "run {idx}: stdout must be byte-identical"
        );
        assert_eq!(
            clean.stderr, dirty.stderr,
            "run {idx}: stderr must be byte-identical"
        );
    }
    assert_eq!(
        baseline_files,
        managed_files(&ws),
        "written files must be byte-identical"
    );
    let settings = String::from_utf8_lossy(&baseline_files[0]).into_owned();
    assert!(!settings.contains("autoApprove"));
}

#[test]
fn personal_defaults_apply_to_personal_targets_and_cli_flags_win() {
    let sb = Sandbox::new();
    sb.write_default_config(r#"{"version":1,"defaults":{"ux":"retina","ai":"off"}}"#);
    let data = sb.data_dir();
    // A profile diff observes the installed extensions with the `code` CLI.
    sb.install_code_stub();
    fs::write(
        sb.path("cwd/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");

    let plan = |args: &[&str]| -> Value {
        let mut full = vec!["--vscode-data-dir", data.as_str(), "diff", "--json"];
        full.extend_from_slice(args);
        let out = sb.vsp(&full);
        assert_exit(&out, 0, &format!("diff {args:?}"));
        json(&out)
    };

    // Defaults fill missing flags for a named profile.
    let by_default = plan(&["-p", "rust"]);
    assert_eq!(by_default["target"]["ux"], "retina");
    assert_eq!(by_default["target"]["ai"], "off");

    // Command-line flags have priority over the defaults.
    let by_flag = plan(&["-p", "rust", "--ux", "crisp", "--ai", "copilot"]);
    assert_eq!(by_flag["target"]["ux"], "crisp");
    assert_eq!(by_flag["target"]["ai"], "copilot");

    // A legacy alias keeps the UX preset that its name selects.
    let by_alias = plan(&["-p", "rust-profile-crisp"]);
    assert_eq!(by_alias["target"]["ux"], "crisp");

    // Workspace targets are not changed by personal defaults.
    let workspace = plan(&["."]);
    assert!(workspace["target"].get("ux").is_none(), "{workspace}");
    assert!(workspace["target"].get("ai").is_none(), "{workspace}");
}

#[test]
fn init_creates_config_refuses_clobber_and_applies() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    let default_path = sb.default_config_path();

    // --dry-run prints the starter and writes nothing.
    let dry = sb.vsp(&["--dry-run", "init"]);
    assert_exit(&dry, 0, "init --dry-run");
    assert!(stdout(&dry).contains("\"version\": 1"));
    assert!(!default_path.exists());

    // Default XDG location.
    let first = sb.vsp(&["init"]);
    assert_exit(&first, 0, "init");
    let written = fs::read_to_string(&default_path).expect("starter written");
    assert!(written.contains("// vsp personal configuration"));
    assert_eq!(
        fs::metadata(&default_path)
            .expect("meta")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // The starter is valid: the loader accepts it and the catalog presets still list.
    let list = sb.vsp(&["list"]);
    assert_exit(&list, 0, "list with starter config");
    assert!(stdout(&list).lines().any(|l| l == "rust"));

    // An existing config is never replaced without --force (exit 5).
    fs::write(&default_path, "// my edits\n{ \"version\": 1 }\n").expect("user edit");
    let second = sb.vsp(&["init"]);
    assert_exit(&second, 5, "init over existing config");
    assert_eq!(
        fs::read_to_string(&default_path).expect("read"),
        "// my edits\n{ \"version\": 1 }\n"
    );
    let dry_existing = sb.vsp(&["--dry-run", "init"]);
    assert_exit(&dry_existing, 5, "init --dry-run over existing config");

    let forced = sb.vsp(&["init", "--force"]);
    assert_exit(&forced, 0, "init --force");
    assert!(fs::read_to_string(&default_path)
        .expect("read")
        .contains("// vsp personal configuration"));

    // --config selects another file, and flags seed defaults and a custom profile.
    let custom = sb.path("custom/dir/my-config.jsonc");
    let custom_str = custom.to_str().expect("utf8");
    let seeded = sb.vsp(&[
        "--config", custom_str, "init", "-p", "my-rust", "--preset", "rust", "--with", "git-pro",
        "--ux", "retina", "--ai", "copilot",
    ]);
    assert_exit(&seeded, 0, "seeded init");
    let show = sb.vsp(&["--config", custom_str, "show", "my-rust", "--json"]);
    assert_exit(&show, 0, "show seeded profile");
    assert_eq!(json(&show)["extends"], "rust");
    let refuse = sb.vsp(&["--config", custom_str, "init"]);
    assert_exit(&refuse, 5, "init over existing --config file");

    // Invalid seeds are rejected before any write.
    let bad = sb.path("bad.jsonc");
    let bad_str = bad.to_str().expect("utf8");
    let unknown = sb.vsp(&[
        "--config",
        bad_str,
        "init",
        "-p",
        "mine",
        "--preset",
        "no-such-preset",
    ]);
    assert_exit(&unknown, 4, "init with unknown preset");
    assert!(!bad.exists(), "a rejected init must not write the config");
    let no_target = sb.vsp(&["--config", bad_str, "init", "--apply"]);
    assert_exit(&no_target, 4, "init --apply without a target");
    assert!(!bad.exists());

    // --apply writes the config and then converges the selected profile.
    sb.install_code_stub();
    let apply_cfg = sb.path("apply.jsonc");
    let apply_str = apply_cfg.to_str().expect("utf8");
    let applied = sb.vsp(&[
        "--config",
        apply_str,
        "--vscode-data-dir",
        &data,
        "--json",
        "init",
        "--apply",
        "-p",
        "my-rust",
        "--preset",
        "rust",
        "--with",
        "git-pro",
    ]);
    assert_exit(&applied, 0, "init --apply");
    let result = json(&applied);
    assert_eq!(result["created"], true);
    assert_eq!(result["applied"], true);
    assert_eq!(result["sync"]["target"], "my-rust");
    assert!(apply_cfg.is_file());
    let settings = created_profile_settings(&sb);
    assert!(settings.as_object().is_some_and(|m| !m.is_empty()));
}

#[test]
fn self_path_install_uninstall_and_non_symlink_refusal() {
    let sb = Sandbox::new();
    let canonical = fs::canonicalize(vsp_bin()).expect("canonical vsp");

    let path = sb.vsp(&["self", "path"]);
    assert_exit(&path, 0, "self path");
    assert_eq!(stdout(&path).trim_end(), canonical.to_str().expect("utf8"));

    let bin_dir = sb.path("local/bin");
    let bin_str = bin_dir.to_str().expect("utf8");

    // --dry-run changes nothing.
    let dry = sb.vsp(&["--dry-run", "self", "install", "--bin-dir", bin_str]);
    assert_exit(&dry, 0, "self install --dry-run");
    assert!(!bin_dir.exists());

    let install = sb.vsp(&["self", "install", "--bin-dir", bin_str, "--with-vspcli"]);
    assert_exit(&install, 0, "self install");
    assert_eq!(
        fs::read_link(bin_dir.join("vsp")).expect("vsp link"),
        canonical
    );
    assert_eq!(
        fs::read_link(bin_dir.join("vspcli")).expect("vspcli link"),
        canonical
    );

    // The links run: `vsp` normally, and `vspcli` in compatibility mode through argv[0].
    let via_vsp = sb
        .command(&bin_dir.join("vsp"))
        .arg("--version")
        .output()
        .expect("run link");
    assert_exit(&via_vsp, 0, "vsp link --version");
    assert_eq!(stdout(&via_vsp).trim_end(), "vsp 3.0.0");
    let via_vspcli = sb
        .command(&bin_dir.join("vspcli"))
        .arg("--version")
        .output()
        .expect("run vspcli link");
    assert_exit(&via_vspcli, 0, "vspcli link --version");
    assert!(
        stdout(&via_vspcli).starts_with("vspcli 3.0.0"),
        "{}",
        stdout(&via_vspcli)
    );
    let cli_list = sb
        .command(&bin_dir.join("vspcli"))
        .arg("--list")
        .output()
        .expect("list");
    assert_exit(&cli_list, 0, "vspcli link --list");
    assert!(stdout(&cli_list).lines().any(|l| l == "rust"));

    // Reinstall is idempotent. The legacy `--dir` spelling is still accepted.
    let again = sb.vsp(&["self", "install", "--dir", bin_str]);
    assert_exit(&again, 0, "self install --dir");
    assert!(stdout(&again).contains("unchanged"));

    let uninstall = sb.vsp(&["self", "uninstall", "--bin-dir", bin_str]);
    assert_exit(&uninstall, 0, "self uninstall");
    assert!(fs::symlink_metadata(bin_dir.join("vsp")).is_err());
    assert!(fs::symlink_metadata(bin_dir.join("vspcli")).is_err());
    let uninstall_again = sb.vsp(&["self", "uninstall", "--bin-dir", bin_str]);
    assert_exit(&uninstall_again, 0, "self uninstall when not installed");

    // A regular file is never replaced or deleted (exit 5).
    let occupied = sb.path("occupied/bin");
    fs::create_dir_all(&occupied).expect("mkdir occupied");
    fs::write(occupied.join("vsp"), b"user-owned file").expect("write regular file");
    let occupied_str = occupied.to_str().expect("utf8");
    let refuse_install = sb.vsp(&[
        "self",
        "install",
        "--bin-dir",
        occupied_str,
        "--with-vspcli",
    ]);
    assert_exit(&refuse_install, 5, "self install over a regular file");
    assert!(
        !occupied.join("vspcli").exists(),
        "a refused install must change nothing"
    );
    let refuse_uninstall = sb.vsp(&["self", "uninstall", "--bin-dir", occupied_str]);
    assert_exit(&refuse_uninstall, 5, "self uninstall of a regular file");
    assert_eq!(
        fs::read(occupied.join("vsp")).expect("read"),
        b"user-owned file"
    );

    // The default directory is ${XDG_BIN_HOME:-~/.local/bin}.
    let default_install = sb.vsp(&["self", "install"]);
    assert_exit(&default_install, 0, "self install default dir");
    assert_eq!(
        fs::read_link(sb.path("home/.local/bin/vsp")).expect("default link"),
        canonical
    );

    // update: version and guidance only.
    let update = sb.vsp(&["self", "update"]);
    assert_exit(&update, 0, "self update");
    let guidance = stdout(&update);
    assert!(guidance.starts_with("vsp 3.0.0"), "{guidance}");
    assert!(guidance.contains("brew upgrade"));
    assert!(guidance.contains("does not use the network"));
    let update_json = sb.vsp(&["self", "update", "--json"]);
    assert_exit(&update_json, 0, "self update --json");
    assert_eq!(json(&update_json)["current_version"], "3.0.0");
    assert_eq!(json(&update_json)["network_used"], false);
}

#[test]
fn completions_offer_live_presets_and_packs_without_restricting_parsing() {
    let sb = Sandbox::new();
    for shell in ["bash", "zsh", "fish"] {
        let out = sb.vsp(&["completions", shell]);
        assert_exit(&out, 0, &format!("completions {shell}"));
        let script = stdout(&out);
        for needle in ["rust", "git-pro", "rust-profile-crisp", "general-minimal"] {
            assert!(
                script.contains(needle),
                "{shell} completions must offer '{needle}'"
            );
        }
        assert!(script.contains("vsp"), "{shell}");
    }
    let bad = sb.vsp(&["completions", "tcsh"]);
    assert_exit(&bad, 4, "completions with an unsupported shell");

    // The live values are for completion only: an unknown preset is still a missing target
    // (exit 2), not a usage error (exit 4).
    let data = sb.data_dir();
    let unknown = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "diff",
        "--preset",
        "not-a-preset",
    ]);
    assert_exit(&unknown, 2, "diff with unknown preset");
}

#[test]
fn vspcli_compat_mode_translates_legacy_commands_and_keeps_legacy_exit_codes() {
    let sb = Sandbox::new();
    let ws = sb.path("ws");
    fs::create_dir_all(&ws).expect("mkdir ws");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let empty = sb.path("empty");
    fs::create_dir_all(&empty).expect("mkdir empty");
    let ws_str = ws.to_str().expect("utf8");
    let empty_str = empty.to_str().expect("utf8");

    // list / --list
    for args in [["list"], ["--list"]] {
        let out = sb.vspcli(&args);
        assert_exit(&out, 0, &format!("vspcli {args:?}"));
        assert!(stdout(&out).lines().any(|l| l == "rust"));
    }

    // detect: match (0), no match (3), missing path (2).
    let detect = sb.vspcli(&["detect", ws_str]);
    assert_exit(&detect, 0, "vspcli detect");
    assert!(
        stdout(&detect).split_whitespace().any(|t| t == "rust"),
        "{}",
        stdout(&detect)
    );
    let detect_json = sb.vspcli(&["detect", "--json", ws_str]);
    assert_exit(&detect_json, 0, "vspcli detect --json");
    assert!(json(&detect_json)["toolchains"]
        .as_array()
        .expect("toolchains")
        .iter()
        .any(|t| t == "rust"));
    assert_exit(
        &sb.vspcli(&["detect", empty_str]),
        3,
        "vspcli detect without a match",
    );
    assert_exit(
        &sb.vspcli(&["detect", "/nonexistent/workspace"]),
        2,
        "vspcli detect with a missing path",
    );

    // validate and doctor. Doctor needs the `code` CLI for a complete audit.
    assert_exit(&sb.vspcli(&["validate"]), 0, "vspcli validate");
    sb.install_code_stub();
    let doctor = sb.vspcli(&["doctor"]);
    assert_exit(&doctor, 0, "vspcli doctor");
    assert!(stdout(&doctor).contains("Summary:"));

    // --help and --version.
    let help = sb.vspcli(&["--help"]);
    assert_exit(&help, 0, "vspcli --help");
    assert!(stdout(&help).contains("vsp compatibility mode"));
    assert!(stdout(&help).contains("vspcli reconcile"));
    assert_exit(&sb.vspcli(&[]), 0, "vspcli without arguments");
    let version = sb.vspcli(&["--version"]);
    assert_exit(&version, 0, "vspcli --version");
    assert!(
        stdout(&version).starts_with("vspcli 3.0.0"),
        "{}",
        stdout(&version)
    );

    // Unknown commands and flags are usage errors (exit 4).
    for args in [
        vec!["--bogus"],
        vec!["frobnicate"],
        vec!["list", "--bogus"],
        vec!["doctor", "--bogus"],
        vec!["detect", "--bogus"],
        vec!["apply", "--bogus"],
        vec!["reconcile"],
        vec!["completions"],
        vec!["completions", "tcsh"],
        vec!["build"],
    ] {
        let out = sb.vspcli(&args);
        assert_exit(&out, 4, &format!("vspcli {args:?}"));
        assert!(stderr(&out).contains("vspcli:"), "{}", stderr(&out));
    }

    // completions name the legacy command and carry live names.
    let completions = sb.vspcli(&["completions", "bash"]);
    assert_exit(&completions, 0, "vspcli completions bash");
    assert!(stdout(&completions).contains("complete -F _vspcli_completions vspcli"));
    assert!(stdout(&completions).contains("git-pro"));

    // apply --dry-run and the deprecated --detect flag run the workspace planner.
    let apply = sb.vspcli(&["apply", "--dry-run", ws_str]);
    assert_exit(&apply, 0, "vspcli apply --dry-run");
    assert!(stdout(&apply).contains("vsp-plan/v1"));
    assert!(
        !ws.join(".vscode").exists(),
        "a dry run must not write the workspace"
    );
    let legacy_detect = sb.vspcli(&["--detect", "--dry-run", ws_str]);
    assert_exit(&legacy_detect, 0, "vspcli --detect --dry-run");
    assert!(
        stderr(&legacy_detect).contains("deprecated"),
        "{}",
        stderr(&legacy_detect)
    );

    // The hidden subcommand reaches the same code path without the environment variable.
    let hidden = sb.vsp(&["compat-vspcli", "--list"]);
    assert_exit(&hidden, 0, "vsp compat-vspcli --list");
    assert!(stdout(&hidden).lines().any(|l| l == "rust"));
    let hidden_bad = sb.vsp(&["compat-vspcli", "--bogus"]);
    assert_exit(&hidden_bad, 4, "vsp compat-vspcli --bogus");

    // Legacy doctor reported each failure as exit 1. `vsp doctor` reports an incomplete
    // observation as exit 7, and the compatibility mode maps it to 1.
    sb.install_failing_code_stub();
    let data = sb.data_dir();
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "doctor"]),
        7,
        "vsp doctor with a failing code CLI",
    );
    assert_exit(
        &sb.vspcli(&["doctor"]),
        1,
        "vspcli doctor with a failing code CLI",
    );
}

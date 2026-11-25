//! Round 3 regression tests for the Opus round-2 findings (process level).
//!
//! Each test names the finding that it covers (`R2-H1`, `R2-M1` to `R2-M3`, `R2-L1`,
//! `R2-L3` to `R2-L5`, `R2-L7`, `R2-L8`). The tests run the real `vsp` binary in a hermetic
//! environment: `HOME` and all XDG directories are temporary, and `PATH` holds only a stub
//! `code` CLI.
//!
//! Findings with a pure-function fix have unit tests next to the code instead: `R2-L1` and
//! `R2-L2` link rules (`self_ops.rs`), the blocked-publisher set (`model.rs`), the extra
//! extension filter (`user_config.rs`), and the pin-downgrade prompt (`vsix.rs`). The
//! workspace backup location is covered by `l6_merged_host_path_lint_and_workspace_backup`
//! in `r2_adversarial_regressions.rs`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

use vsp::exit_codes::ExitCode;
use vsp::plan::{compute_plan, PlanOptions};
use vsp::schema_loader::load_embedded;
use vsp::sync::{run_sync, SyncOptions};
use vsp::user_config::{apply_user_config, parse_user_config};

/// Extension that the catalog marks as compromised. Its publisher is in `blockedPublishers`.
const COMPROMISED_EXT: &str = "jscearcy.rust-doc-viewer";

/// A catalog-external ID from the blocked publisher of [`COMPROMISED_EXT`].
const BLOCKED_PUBLISHER_EXT: &str = "jscearcy.rust-doc-viewer-v2";

/// Catalog-external extension ID used as the Tier 3 `user-unlocked` sample.
const EXTERNAL_EXT: &str = "zz-external.sample-tool";

fn vsp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vsp"))
}

/// How the stub `code` CLI behaves.
#[derive(Default)]
struct CodeStub<'a> {
    /// Lines printed by `--list-extensions` (`id@version`).
    installed: &'a [&'a str],
    /// `--uninstall-extension` fails.
    fail_uninstall: bool,
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

    fn data(&self) -> String {
        self.path("data").to_str().expect("utf8").to_string()
    }

    fn storage_json(&self) -> String {
        fs::read_to_string(self.path("data/User/globalStorage/storage.json")).expect("storage.json")
    }

    fn code_log(&self) -> String {
        fs::read_to_string(self.path("code.log")).unwrap_or_default()
    }

    fn clear_code_log(&self) {
        let _ = fs::remove_file(self.path("code.log"));
    }

    fn default_config_path(&self) -> PathBuf {
        self.path("config/vsp/config.jsonc")
    }

    /// Writes the user configuration at the XDG default path with mode `0600`.
    fn write_default_config(&self, text: &str) {
        let path = self.default_config_path();
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir config");
        fs::write(&path, text).expect("write user config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod config");
    }

    /// Installs the stub `code` CLI. Each call replaces the earlier stub.
    fn install_code(&self, stub: &CodeStub<'_>) {
        fs::write(self.path("installed.txt"), stub.installed.join("\n") + "\n")
            .expect("write installed list");
        let uninstall = if stub.fail_uninstall {
            "exit 1"
        } else {
            "exit 0"
        };
        let script = format!(
            "#!/bin/sh\necho \"$*\" >> '{log}'\ncase \"$*\" in\n  *--list-extensions*) while IFS= read -r line; do [ -n \"$line\" ] && echo \"$line\"; done < '{list}'; exit 0 ;;\n  *--uninstall-extension*) {uninstall} ;;\nesac\nexit 0\n",
            log = self.path("code.log").display(),
            list = self.path("installed.txt").display(),
        );
        let stub_path = self.path("bin/code");
        fs::write(&stub_path, script).expect("write code stub");
        fs::set_permissions(&stub_path, fs::Permissions::from_mode(0o755)).expect("chmod stub");
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(vsp_bin());
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
        self.command().args(args).output().expect("run vsp")
    }

    /// Runs one legacy command line through the hidden `compat-vspcli` subcommand.
    fn vspcli(&self, args: &[&str]) -> Output {
        self.command()
            .env("VSCODE_USER_DATA_DIR", self.path("data"))
            .arg("compat-vspcli")
            .args(args)
            .output()
            .expect("run vsp compat-vspcli")
    }

    /// Returns the `settings.json` path of the single named profile in the store.
    fn profile_settings_path(&self) -> PathBuf {
        let mut found: Vec<PathBuf> = fs::read_dir(self.path("data/User/profiles"))
            .expect("profiles dir")
            .flatten()
            .map(|e| e.path().join("settings.json"))
            .filter(|p| p.is_file())
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one profile settings file");
        found.remove(0)
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

fn parse_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {}", stdout(out)))
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn extras_config(profile: &str, extras: &[&str]) -> String {
    let list: Vec<String> = extras.iter().map(|id| format!("\"{id}\"")).collect();
    format!(
        r#"{{"version":1,"profiles":{{"{profile}":{{"extends":"rust","extraExtensions":[{}]}}}}}}"#,
        list.join(",")
    )
}

/// Returns `true` when the `code` log has an install of exactly `id`.
fn log_installs(log: &str, id: &str) -> bool {
    log.lines().any(|line| {
        let mut words = line.split_whitespace();
        while let Some(word) = words.next() {
            if word == "--install-extension" && words.next() == Some(id) {
                return true;
            }
        }
        false
    })
}

/// R2-H1: an `extraExtensions` entry from a blocked publisher is refused with exit 5 in
/// every consumer, and the `code` CLI is never started.
#[test]
fn h1_extra_extension_from_blocked_publisher_is_refused() {
    let sb = Sandbox::new();
    sb.install_code(&CodeStub::default());
    let data = sb.data();
    sb.write_default_config(&extras_config("my-rust", &[BLOCKED_PUBLISHER_EXT]));

    for args in [
        vec!["--vscode-data-dir", data.as_str(), "sync", "-p", "my-rust"],
        vec![
            "--vscode-data-dir",
            data.as_str(),
            "--dry-run",
            "sync",
            "-p",
            "my-rust",
        ],
        vec!["--vscode-data-dir", data.as_str(), "diff", "-p", "my-rust"],
        vec!["list"],
        vec!["export", "my-rust", "--out", "out"],
    ] {
        let out = sb.vsp(&args);
        assert_exit(&out, 5, &format!("vsp {}", args.join(" ")));
        assert!(
            stderr(&out).contains("publisher 'jscearcy' is blocked"),
            "{}",
            stderr(&out)
        );
    }
    // The publisher-trust bypass is for unknown publishers. It never unblocks a publisher.
    let bypass = sb
        .command()
        .env("ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER", "1")
        .env("ARTAGON_VSCODE_ALLOW_UNLOCKED", "1")
        .args([
            "--vscode-data-dir",
            data.as_str(),
            "sync",
            "-p",
            "my-rust",
            "--allow-unlocked",
        ])
        .output()
        .expect("sync with bypass");
    assert_exit(&bypass, 5, "sync with the publisher-trust bypass");

    assert_eq!(sb.code_log(), "", "the code CLI must never be started");
    assert!(!sb.path("data/User/profiles").exists());
    assert!(!sb.path("cwd/out").exists());

    // The publisher of a `removed` extension with no active catalog extension is blocked.
    sb.write_default_config(&extras_config("my-rust", &["pmd.some-other-tool"]));
    let removed_publisher = sb.vsp(&["list"]);
    assert_exit(&removed_publisher, 5, "extra from a removed-only publisher");
    assert!(
        stderr(&removed_publisher).contains("publisher 'pmd' is blocked"),
        "{}",
        stderr(&removed_publisher)
    );

    // A publisher that the catalog still ships (it has active extensions) stays usable,
    // although one of its extensions has the `removed` status.
    sb.write_default_config(&extras_config("my-rust", &["vscjava.some-new-tool"]));
    assert_exit(&sb.vsp(&["list"]), 0, "extra from a shipped publisher");
    assert_eq!(sb.code_log(), "");
}

/// R2-H1: the planner and the sync engine check the extras again. A blocked publisher is
/// refused there also when the config-load check did not see it.
#[test]
fn h1_plan_and_sync_recheck_blocked_publishers() {
    let sb = Sandbox::new();
    let mut catalog = load_embedded().expect("embedded catalog");
    let config = parse_user_config(
        &extras_config("my-rust", &[EXTERNAL_EXT]),
        Path::new("config.jsonc"),
    )
    .expect("valid config");
    let overlay = apply_user_config(&mut catalog, &config).expect("config applies");

    // The catalog blocks the publisher after the overlay was built (a stale overlay).
    catalog.blocked_publishers.insert("zz-external".to_string());

    let plan_err = compute_plan(
        &catalog,
        &PlanOptions {
            profile: Some("my-rust".to_string()),
            vscode_data_dir: Some(sb.path("data")),
            user_overlay: overlay.clone(),
            ..Default::default()
        },
    )
    .expect_err("the planner must refuse a blocked publisher");
    assert_eq!(plan_err.exit_code(), ExitCode::SafetyRefusal, "{plan_err}");

    let sync_err = run_sync(
        &catalog,
        &SyncOptions {
            profile: Some("my-rust".to_string()),
            vscode_data_dir: Some(sb.path("data")),
            user_overlay: overlay,
            ..Default::default()
        },
    )
    .expect_err("the sync engine must refuse a blocked publisher");
    assert_eq!(sync_err.exit_code(), ExitCode::SafetyRefusal, "{sync_err}");
    assert!(
        !sb.path("data/User/profiles").exists(),
        "a refused sync changes nothing"
    );
}

/// R2-M1: an `extraExtensions` entry cannot override the AI mode. `defaults.ai: "off"`
/// drops catalog AI extensions, and `copilot` keeps only `github.copilot-chat`.
#[test]
fn m1_extra_extensions_respect_the_ai_mode() {
    const CLAUDE: &str = "anthropic.claude-code";
    const COPILOT: &str = "github.copilot-chat";

    let config = |ai: &str| {
        format!(
            r#"{{"version":1,"defaults":{{"ai":"{ai}"}},"profiles":{{"my-rust":{{"extends":"rust","extraExtensions":["{CLAUDE}","{COPILOT}","{EXTERNAL_EXT}"]}}}}}}"#
        )
    };
    let planned_installs = |sb: &Sandbox, data: &str| -> Vec<String> {
        let out = sb.vsp(&["--vscode-data-dir", data, "--json", "diff", "-p", "my-rust"]);
        assert_exit(&out, 0, "diff");
        parse_json(&out)["extension_changes"]
            .as_array()
            .expect("extension_changes")
            .iter()
            .filter(|c| c["action"] == "ensure_installed")
            .filter_map(|c| c["id"].as_str().map(ToString::to_string))
            .collect()
    };

    for (ai, claude_expected, copilot_expected) in [
        ("off", false, false),
        ("copilot", false, true),
        ("full", true, true),
    ] {
        let sb = Sandbox::new();
        sb.install_code(&CodeStub::default());
        let data = sb.data();
        sb.write_default_config(&config(ai));

        // The plan and the sync agree.
        let planned = planned_installs(&sb, &data);
        assert_eq!(
            planned.iter().any(|id| id == CLAUDE),
            claude_expected,
            "ai={ai}: plan {planned:?}"
        );
        assert_eq!(
            planned.iter().any(|id| id == COPILOT),
            copilot_expected,
            "ai={ai}: plan {planned:?}"
        );
        assert!(
            planned.iter().any(|id| id == EXTERNAL_EXT),
            "ai={ai}: a catalog-external extra is not an AI-group extension"
        );

        sb.clear_code_log();
        let sync = sb
            .command()
            .args(["--vscode-data-dir", data.as_str(), "sync", "-p", "my-rust"])
            .output()
            .expect("sync");
        assert_exit(&sync, 0, &format!("ai={ai}: sync"));
        let log = sb.code_log();
        assert_eq!(
            log_installs(&log, CLAUDE),
            claude_expected,
            "ai={ai}: {log}"
        );
        assert_eq!(
            log_installs(&log, COPILOT),
            copilot_expected,
            "ai={ai}: {log}"
        );
        assert!(log_installs(&log, EXTERNAL_EXT), "ai={ai}: {log}");
    }

    // The command-line flag has the same effect as the config default.
    let sb = Sandbox::new();
    sb.install_code(&CodeStub::default());
    let data = sb.data();
    sb.write_default_config(&config("full"));
    let off = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "-p",
        "my-rust",
        "--ai",
        "off",
    ]);
    assert_exit(&off, 0, "sync --ai off");
    assert!(!log_installs(&sb.code_log(), CLAUDE), "{}", sb.code_log());
    assert!(!log_installs(&sb.code_log(), COPILOT), "{}", sb.code_log());
}

/// R2-M2: a stale personal user config (it names an extension that the catalog now marks
/// compromised) does not block emergency remediation or bundle import. Each other sync
/// still fails closed.
#[test]
fn m2_stale_user_config_does_not_block_remediation_or_import() {
    let sb = Sandbox::new();
    let data = sb.data();
    let installed = format!("{COMPROMISED_EXT}@4.2.1");
    sb.install_code(&CodeStub {
        installed: &[installed.as_str()],
        ..Default::default()
    });

    // A bundle for the import case, made before the config goes stale.
    let export = sb.vsp(&["export", "rust", "--out", "bundles"]);
    assert_exit(&export, 0, "export");
    let bundle = sb.path("cwd/bundles/rust.code-profile");
    assert!(bundle.is_file());

    sb.write_default_config(&extras_config("my-rust", &[COMPROMISED_EXT]));
    let strict = sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust"]);
    assert_exit(&strict, 5, "an ordinary sync needs a valid config");
    assert_eq!(sb.code_log(), "");

    for args in [
        vec![
            "--vscode-data-dir",
            data.as_str(),
            "sync",
            "--all",
            "--compromised-only",
        ],
        vec![
            "--vscode-data-dir",
            data.as_str(),
            "sync",
            "-p",
            "rust",
            "--compromised-only",
        ],
    ] {
        sb.clear_code_log();
        let out = sb.vsp(&args);
        assert_exit(&out, 0, &format!("vsp {}", args.join(" ")));
        assert!(
            stderr(&out).contains("WARNING: the personal user config is ignored"),
            "{}",
            stderr(&out)
        );
        let log = sb.code_log();
        assert!(
            log.contains(&format!("--uninstall-extension {COMPROMISED_EXT}")),
            "the compromised extension must be removed: {log}"
        );
        assert!(!log.contains("--install-extension"), "{log}");
    }

    // Bundle import continues with a warning. Step 0 still removes the compromised
    // extension first.
    sb.clear_code_log();
    let import = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "--from",
        bundle.to_str().expect("utf8"),
        "-p",
        "imported",
    ]);
    assert_exit(&import, 0, "sync --from with a stale config");
    assert!(
        stderr(&import).contains("WARNING: the personal user config is ignored"),
        "{}",
        stderr(&import)
    );
    assert!(
        sb.code_log()
            .contains(&format!("--uninstall-extension {COMPROMISED_EXT}")),
        "{}",
        sb.code_log()
    );
}

/// R2-M2: `vsp doctor` records an unusable user config as a FAIL check and still runs the
/// other checks, so it still reports the compromised extension.
#[test]
fn m2_doctor_records_config_failure_and_continues() {
    let sb = Sandbox::new();
    let data = sb.data();
    let installed = format!("{COMPROMISED_EXT}@4.2.1");
    sb.install_code(&CodeStub {
        installed: &[installed.as_str()],
        ..Default::default()
    });
    sb.write_default_config(&extras_config("my-rust", &[COMPROMISED_EXT]));

    let doctor = sb.vsp(&["--vscode-data-dir", &data, "doctor", "--json"]);
    assert_exit(&doctor, 1, "doctor with a stale config");
    let report = parse_json(&doctor);
    let checks = report["checks"].as_array().expect("checks");
    let config_fail = checks
        .iter()
        .find(|c| c["category"] == "user-config")
        .expect("a user-config check");
    assert_eq!(config_fail["severity"], "fail");
    assert!(
        config_fail["message"]
            .as_str()
            .expect("message")
            .contains("cannot be used"),
        "{config_fail}"
    );
    assert!(
        checks.iter().any(|c| {
            c["severity"] == "fail"
                && c["category"] != "user-config"
                && c["message"]
                    .as_str()
                    .is_some_and(|m| m.contains(COMPROMISED_EXT))
        }),
        "doctor must still report the installed compromised extension: {report}"
    );
    assert!(checks.len() > 2, "the other checks still run: {report}");
}

/// R2-M3: legacy `reconcile` changes extensions only. It leaves `settings.json`,
/// `storage.json`, and the RTK wrappers as they are, and it never creates a profile.
#[test]
fn m3_compat_reconcile_leaves_settings_and_storage_untouched() {
    let sb = Sandbox::new();
    sb.install_code(&CodeStub::default());
    let data = sb.data();

    // A full sync creates the profile. The user then edits the settings file.
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust", "--no-rtk"]),
        0,
        "initial sync",
    );
    let settings = sb.profile_settings_path();
    let edited = "{\n  // edited by the user\n  \"my.userKey\": true\n}\n";
    fs::write(&settings, edited).expect("edit settings");
    let storage_before = sb.storage_json();
    let profile_dir = settings.parent().expect("profile dir").to_path_buf();
    let entries_before: Vec<_> = sorted_names(&profile_dir);
    let rtk_dir = sb.path("config/vsp/rtk");
    assert!(!rtk_dir.exists(), "--no-rtk installs no wrappers");

    sb.clear_code_log();
    let reconcile = sb.vspcli(&["reconcile", "--profile", "rust"]);
    assert_exit(&reconcile, 0, "vspcli reconcile --profile rust");
    assert_eq!(
        fs::read_to_string(&settings).expect("settings"),
        edited,
        "reconcile must not write settings.json"
    );
    assert_eq!(
        sb.storage_json(),
        storage_before,
        "storage.json is unchanged"
    );
    assert_eq!(
        sorted_names(&profile_dir),
        entries_before,
        "no backup or other file appears in the profile directory"
    );
    assert!(!rtk_dir.exists(), "reconcile installs no RTK wrappers");
    let log = sb.code_log();
    assert!(log.contains("--list-extensions"), "{log}");
    assert!(
        log.contains("--install-extension"),
        "reconcile still installs the missing extensions: {log}"
    );

    // The same translation through the environment variable.
    let via_env = sb
        .command()
        .env("VSP_COMPAT_VSPCLI", "1")
        .env("VSCODE_USER_DATA_DIR", sb.path("data"))
        .args(["reconcile", "--profile", "rust"])
        .output()
        .expect("reconcile through the environment variable");
    assert_exit(&via_env, 0, "VSP_COMPAT_VSPCLI=1 vsp reconcile");
    assert_eq!(fs::read_to_string(&settings).expect("settings"), edited);

    // A profile that does not exist is not created (exit 2).
    sb.clear_code_log();
    let missing = sb.vspcli(&["reconcile", "--profile", "go"]);
    assert_exit(&missing, 2, "reconcile of an unregistered profile");
    assert_eq!(sb.storage_json(), storage_before);
    assert_eq!(sb.code_log(), "", "no code call for a missing profile");

    // A full sync still writes the settings (the control case).
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust", "--no-rtk"]),
        0,
        "full sync",
    );
    assert_ne!(fs::read_to_string(&settings).expect("settings"), edited);

    // The engine mode is refused for a target whose sync is a settings write.
    fs::write(
        sb.path("cwd/Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    assert_exit(
        &sb.vsp(&["sync", ".", "--extensions-only"]),
        4,
        "workspace sync with --extensions-only",
    );
    assert!(!sb.path("cwd/.vscode").exists());
}

fn sorted_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// R2-L1: `vsp self install` refuses to replace a link of another tool, and
/// `vsp self uninstall` keeps it, unless `--force` is given.
#[test]
fn l1_self_install_and_uninstall_respect_foreign_links() {
    let sb = Sandbox::new();
    let bin = sb.path("local/bin");
    fs::create_dir_all(&bin).expect("mkdir bin");
    let foreign = sb.path("other-tool");
    fs::write(&foreign, "#!/bin/sh\n").expect("write foreign tool");
    symlink(&foreign, bin.join("vsp")).expect("foreign link");
    let bin_str = bin.to_str().expect("utf8");

    let refused = sb.vsp(&["self", "install", "--bin-dir", bin_str, "--with-vspcli"]);
    assert_exit(&refused, 5, "self install over a foreign link");
    assert!(stderr(&refused).contains("--force"), "{}", stderr(&refused));
    assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), foreign);
    assert!(fs::symlink_metadata(bin.join("vspcli")).is_err());

    let kept = sb.vsp(&["self", "uninstall", "--bin-dir", bin_str]);
    assert_exit(&kept, 0, "self uninstall of a foreign link");
    assert!(stdout(&kept).contains("kept"), "{}", stdout(&kept));
    assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), foreign);

    let forced = sb.vsp(&["self", "install", "--bin-dir", bin_str, "--force"]);
    assert_exit(&forced, 0, "self install --force");
    let own = fs::canonicalize(vsp_bin()).expect("canonical vsp");
    assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), own);

    // The link is now owned, so an uninstall without --force removes it.
    let removed = sb.vsp(&["self", "uninstall", "--bin-dir", bin_str]);
    assert_exit(&removed, 0, "self uninstall of an owned link");
    assert!(fs::symlink_metadata(bin.join("vsp")).is_err());

    symlink(&foreign, bin.join("vsp")).expect("foreign link again");
    let forced_removal = sb.vsp(&["self", "uninstall", "--bin-dir", bin_str, "--force"]);
    assert_exit(&forced_removal, 0, "self uninstall --force");
    assert!(fs::symlink_metadata(bin.join("vsp")).is_err());
    assert!(foreign.is_file(), "the link target is never deleted");
}

/// R2-L3: `vsp init` creates the config with an exclusive create (mode `0600`), keeps a
/// `0600` backup with `--force`, and refuses a symbolic link in the config path.
#[test]
fn l3_init_is_exclusive_and_force_keeps_a_backup() {
    let sb = Sandbox::new();
    let config = sb.default_config_path();
    let backup = sb.path("config/vsp/config.jsonc.bak");

    assert_exit(&sb.vsp(&["init"]), 0, "first init");
    assert_eq!(mode_of(&config), 0o600);
    let first = fs::read_to_string(&config).expect("config");
    assert!(!backup.exists(), "a new file needs no backup");

    let again = sb.vsp(&["init", "--ux", "crisp"]);
    assert_exit(&again, 5, "init over an existing config");
    assert_eq!(fs::read_to_string(&config).expect("config"), first);
    assert!(!backup.exists());

    let forced = sb.vsp(&["init", "--ux", "crisp", "--force"]);
    assert_exit(&forced, 0, "init --force");
    assert!(
        stdout(&forced).contains("Previous config saved as"),
        "{}",
        stdout(&forced)
    );
    assert_ne!(fs::read_to_string(&config).expect("config"), first);
    assert_eq!(fs::read_to_string(&backup).expect("backup"), first);
    assert_eq!(mode_of(&backup), 0o600);
    assert_eq!(mode_of(&config), 0o600);

    // A dangling symbolic link at the config path is never followed.
    let sb = Sandbox::new();
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).expect("mkdir outside");
    fs::create_dir_all(sb.path("config/vsp")).expect("mkdir config");
    symlink(outside.join("planted.jsonc"), sb.default_config_path()).expect("dangling link");
    for args in [vec!["init"], vec!["init", "--force"]] {
        assert_exit(
            &sb.vsp(&args),
            5,
            &format!("vsp {} through a link", args.join(" ")),
        );
    }
    assert!(!outside.join("planted.jsonc").exists());

    // A symbolic link for the `vsp` config directory is refused too.
    let sb = Sandbox::new();
    let outside = sb.path("outside");
    fs::create_dir_all(&outside).expect("mkdir outside");
    symlink(&outside, sb.path("config/vsp")).expect("linked config dir");
    for args in [vec!["init"], vec!["init", "--force"]] {
        let out = sb.vsp(&args);
        assert_exit(&out, 5, &format!("vsp {} in a linked dir", args.join(" ")));
        assert!(
            stderr(&out).contains("symlinked path component"),
            "{}",
            stderr(&out)
        );
    }
    assert_eq!(sorted_names(&outside), Vec::<String>::new());
}

/// R2-L4: a user config that its group or others can write is refused with exit 5.
#[test]
fn l4_group_or_world_writable_config_is_refused() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());
    sb.write_default_config(&extras_config("my-rust", &[EXTERNAL_EXT]));
    let config = sb.default_config_path();

    for mode in [0o600, 0o640, 0o644, 0o400] {
        fs::set_permissions(&config, fs::Permissions::from_mode(mode)).expect("chmod");
        assert_exit(&sb.vsp(&["list"]), 0, &format!("mode {mode:o} is accepted"));
    }
    for mode in [0o620, 0o602, 0o660, 0o666, 0o664] {
        fs::set_permissions(&config, fs::Permissions::from_mode(mode)).expect("chmod");
        for args in [
            vec!["list"],
            vec!["--vscode-data-dir", data.as_str(), "sync", "-p", "my-rust"],
        ] {
            let out = sb.vsp(&args);
            assert_exit(&out, 5, &format!("mode {mode:o}: vsp {}", args.join(" ")));
            assert!(
                stderr(&out).contains("lets group or others write it"),
                "{}",
                stderr(&out)
            );
        }
    }
    assert_eq!(sb.code_log(), "", "a refused config starts no code call");

    // The explicit --config path has the same rule.
    let explicit = sb.path("explicit.jsonc");
    fs::write(&explicit, r#"{"version":1}"#).expect("write explicit config");
    fs::set_permissions(&explicit, fs::Permissions::from_mode(0o666)).expect("chmod");
    let out = sb.vsp(&["--config", explicit.to_str().expect("utf8"), "list"]);
    assert_exit(&out, 5, "--config with mode 666");

    // `vsp doctor` records the refusal and continues.
    let doctor = sb.vsp(&["--vscode-data-dir", &data, "doctor"]);
    assert_exit(&doctor, 1, "doctor with a group-writable config");
    assert!(
        stdout(&doctor).contains("FAIL: user config") && stdout(&doctor).contains("Summary:"),
        "{}",
        stdout(&doctor)
    );
}

/// R2-L5: without an absolute `HOME` (and without an absolute `XDG_CONFIG_HOME`) there is
/// no default user config. Nothing is read from, or written to, the working directory.
#[test]
fn l5_relative_or_empty_home_never_uses_the_working_directory() {
    let sb = Sandbox::new();
    let planted = extras_config("planted", &[]);
    for dir in ["cwd/.config/vsp", "cwd/relhome/.config/vsp"] {
        fs::create_dir_all(sb.path(dir)).expect("mkdir planted dir");
        let file = sb.path(dir).join("config.jsonc");
        fs::write(&file, &planted).expect("plant config");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("chmod");
    }

    let run = |home: Option<&str>, args: &[&str]| -> Output {
        let mut cmd = sb.command();
        cmd.env_remove("XDG_CONFIG_HOME").env_remove("HOME");
        if let Some(value) = home {
            cmd.env("HOME", value);
        }
        cmd.args(args).output().expect("run vsp")
    };

    for home in [Some("."), Some("relhome"), Some(""), None] {
        let show = run(home, &["show", "planted"]);
        assert_exit(
            &show,
            2,
            &format!("HOME={home:?}: the planted profile must be unknown"),
        );
        assert_exit(
            &run(home, &["show", "rust"]),
            0,
            &format!("HOME={home:?}: built-in presets still work"),
        );
        let init = run(home, &["init"]);
        assert_exit(
            &init,
            4,
            &format!("HOME={home:?}: init has no default path"),
        );
        assert!(
            stderr(&init).contains("--config"),
            "HOME={home:?}: {}",
            stderr(&init)
        );
    }
    assert_eq!(
        fs::read_to_string(sb.path("cwd/.config/vsp/config.jsonc")).expect("planted"),
        planted,
        "the planted file is unchanged"
    );

    // An absolute HOME is the control case: the same file is then the default config.
    let home_cfg = sb.path("home/.config/vsp");
    fs::create_dir_all(&home_cfg).expect("mkdir home config");
    fs::write(home_cfg.join("config.jsonc"), &planted).expect("write home config");
    fs::set_permissions(
        home_cfg.join("config.jsonc"),
        fs::Permissions::from_mode(0o600),
    )
    .expect("chmod");
    let absolute_home = sb.path("home");
    let show = run(absolute_home.to_str(), &["show", "planted"]);
    assert_exit(&show, 0, "an absolute HOME finds the default config");
}

/// R2-L7: legacy `install-ext` validates the profile name, never creates a profile, applies
/// the running-editor guard, runs Step 0, and refuses a blocked publisher.
#[test]
fn l7_compat_install_ext_has_the_sync_gates() {
    const TARGET_EXT: &str = "editorconfig.editorconfig";
    let sb = Sandbox::new();
    sb.install_code(&CodeStub::default());
    let data = sb.data();
    let storage_before = sb.storage_json();

    // A profile that is not registered is not created.
    let missing = sb.vspcli(&["install-ext", "ghost", TARGET_EXT]);
    assert_exit(&missing, 2, "install-ext into an unregistered profile");
    let running = sb
        .command()
        .env("VSCODE_USER_DATA_DIR", sb.path("data"))
        .env("ARTAGON_VSCODE_TEST_RUNNING", "1")
        .args(["compat-vspcli", "install-ext", "ghost", TARGET_EXT])
        .output()
        .expect("install-ext while VS Code runs");
    assert_exit(&running, 1, "install-ext with a running editor");
    assert!(
        stderr(&running).contains("VS Code appears to be running"),
        "{}",
        stderr(&running)
    );
    // An unsafe profile name is refused before any other step.
    let unsafe_name = sb.vspcli(&["install-ext", "a/../b", TARGET_EXT]);
    assert_exit(&unsafe_name, 1, "install-ext with an unsafe profile name");
    assert!(
        stderr(&unsafe_name).contains("unsafe VS Code profile name"),
        "{}",
        stderr(&unsafe_name)
    );
    assert_eq!(sb.code_log(), "", "no code call before the gates pass");
    assert_eq!(sb.storage_json(), storage_before);

    // Register the profile with a full sync.
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust", "--no-rtk"]),
        0,
        "initial sync",
    );

    // Step 0 runs before the install.
    let installed = format!("{COMPROMISED_EXT}@4.2.1");
    sb.install_code(&CodeStub {
        installed: &[installed.as_str()],
        ..Default::default()
    });
    sb.clear_code_log();
    let ok = sb.vspcli(&["install-ext", "rust", TARGET_EXT]);
    assert_exit(&ok, 0, "install-ext into a registered profile");
    let log = sb.code_log();
    let uninstall_at = log
        .lines()
        .position(|l| l.contains(&format!("--uninstall-extension {COMPROMISED_EXT}")))
        .unwrap_or_else(|| panic!("Step 0 must uninstall the compromised extension: {log}"));
    let install_at = log
        .lines()
        .position(|l| l.contains(&format!("--install-extension {TARGET_EXT}")))
        .unwrap_or_else(|| panic!("the extension must be installed: {log}"));
    assert!(uninstall_at < install_at, "Step 0 comes first: {log}");

    // A failed Step 0 removal stops the command before the install.
    sb.install_code(&CodeStub {
        installed: &[installed.as_str()],
        fail_uninstall: true,
    });
    sb.clear_code_log();
    let step0_failed = sb.vspcli(&["install-ext", "rust", TARGET_EXT]);
    assert_exit(&step0_failed, 1, "install-ext with a failed Step 0");
    assert!(
        !sb.code_log().contains("--install-extension"),
        "{}",
        sb.code_log()
    );

    // A blocked publisher is refused, also with the publisher-trust bypass.
    sb.install_code(&CodeStub::default());
    sb.clear_code_log();
    let blocked = sb
        .command()
        .env("VSCODE_USER_DATA_DIR", sb.path("data"))
        .env("ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER", "1")
        .args([
            "compat-vspcli",
            "install-ext",
            "rust",
            BLOCKED_PUBLISHER_EXT,
        ])
        .output()
        .expect("install-ext of a blocked publisher");
    assert_exit(&blocked, 1, "install-ext of a blocked publisher");
    assert!(
        stderr(&blocked).contains("publisher 'jscearcy' is blocked"),
        "{}",
        stderr(&blocked)
    );
    assert_eq!(sb.code_log(), "");
}

/// R2-L8: `VSP_COMPAT_VSPCLI=1` still selects the legacy mode for a process named `vsp`.
/// A usage error in that case names the variable, so the cause is clear.
#[test]
fn l8_env_forced_compat_mode_still_works_and_explains_usage_errors() {
    let sb = Sandbox::new();
    let compat = |args: &[&str]| -> Output {
        sb.command()
            .env("VSP_COMPAT_VSPCLI", "1")
            .args(args)
            .output()
            .expect("run vsp in compat mode")
    };

    let list = compat(&["list"]);
    assert_exit(&list, 0, "VSP_COMPAT_VSPCLI=1 vsp list");
    assert!(stdout(&list).lines().any(|l| l == "rust"));
    assert!(!stderr(&list).contains("VSP_COMPAT_VSPCLI"));

    // A native command line is a legacy usage error (exit 4), with the hint.
    let native = compat(&["sync", "-p", "rust"]);
    assert_exit(&native, 4, "VSP_COMPAT_VSPCLI=1 vsp sync -p rust");
    assert!(
        stderr(&native).contains("VSP_COMPAT_VSPCLI=1 is set"),
        "{}",
        stderr(&native)
    );

    // Other values do not select the legacy mode.
    let not_forced = sb
        .command()
        .env("VSP_COMPAT_VSPCLI", "true")
        .args(["list", "presets"])
        .output()
        .expect("run vsp");
    assert_exit(&not_forced, 0, "VSP_COMPAT_VSPCLI=true is not compat mode");

    // The explicit subcommand gives no hint: the variable is not the cause.
    let explicit = sb.vspcli(&["frobnicate"]);
    assert_exit(&explicit, 4, "vsp compat-vspcli frobnicate");
    assert!(!stderr(&explicit).contains("VSP_COMPAT_VSPCLI"));
}

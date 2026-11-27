//! Round 2 adversarial remediation regression tests (process level).
//!
//! Each test names the review finding that it covers (Opus `H*`/`M*`/`L*`, Astra `A*`,
//! Fable `N2`). The tests run the real `vsp` binary in a hermetic environment: `HOME` and all
//! XDG directories are temporary, and `PATH` holds only a stub `code` CLI.
//!
//! Findings with a pure-function fix have unit tests next to the code instead:
//! `L2`, `L3`, `H1` index rules (`vsix.rs`), `M3` (`cst_merge.rs`), `M4`, `L5` (`policy.rs`),
//! `L7`, `A18` (`storage.rs`), `L10` (`legacy_loader.rs`), `A03`, `A04`, `A05`
//! (`dev_ops.rs`), `A12` (`detect.rs`), and `A21` path rule (`provenance.rs`).

#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;

use serde_json::{json, Value};
use tempfile::TempDir;

use vsp::resolver::sha256_hex;

const COMPROMISED_EXT: &str = "jscearcy.rust-doc-viewer";
const EXTERNAL_EXT: &str = "zz-external.sample-tool";

fn vsp_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_vsp"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root")
        .to_path_buf()
}

/// How the stub `code` CLI behaves.
#[derive(Default)]
struct CodeStub<'a> {
    /// Lines printed by `--list-extensions` (`id@version`).
    installed: &'a [String],
    /// `--list-extensions` fails.
    fail_list: bool,
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

    fn code_log(&self) -> String {
        fs::read_to_string(self.path("code.log")).unwrap_or_default()
    }

    fn clear_code_log(&self) {
        let _ = fs::remove_file(self.path("code.log"));
    }

    /// Installs the stub `code` CLI. Each call replaces the earlier stub.
    fn install_code(&self, stub: &CodeStub<'_>) {
        fs::write(self.path("installed.txt"), stub.installed.join("\n") + "\n")
            .expect("write installed list");
        let list = if stub.fail_list {
            "echo 'list failed' >&2; exit 3".to_string()
        } else {
            format!(
                "while IFS= read -r line; do [ -n \"$line\" ] && echo \"$line\"; done < '{}'; exit 0",
                self.path("installed.txt").display()
            )
        };
        let uninstall = if stub.fail_uninstall {
            "exit 1"
        } else {
            "exit 0"
        };
        let script = format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  *--list-extensions*) {list} ;;\n  *--uninstall-extension*) {uninstall} ;;\nesac\nexit 0\n",
            self.path("code.log").display()
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
            .env("ARTAGON_VSCODE_TEST_RUNNING", "0");
        cmd
    }

    /// Runs `vsp` with an explicit empty pin manifest (no network, no lock tiers).
    fn vsp(&self, args: &[&str]) -> Output {
        self.command()
            .env("ARTAGON_VSCODE_PINS_FILE", self.path("pins.json"))
            .args(args)
            .output()
            .expect("run vsp")
    }

    /// Runs `vsp` against the shipped (embedded) `vsp.lock.json`.
    fn vsp_shipped_lock(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run vsp")
    }

    /// Returns the provenance record of `profile` from the XDG state directory.
    fn provenance(&self, profile: &str) -> Value {
        let dir = self.path("state/vsp/profiles");
        for entry in fs::read_dir(&dir).expect("provenance dir").flatten() {
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(val) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if val["profile"] == profile {
                return val;
            }
        }
        panic!(
            "no provenance record for profile '{profile}' in {}",
            dir.display()
        );
    }

    /// Returns the parsed `settings.json` of the single named profile in the store.
    fn profile_settings(&self) -> Value {
        let mut found: Vec<PathBuf> = fs::read_dir(self.path("data/User/profiles"))
            .expect("profiles dir")
            .flatten()
            .map(|e| e.path().join("settings.json"))
            .filter(|p| p.is_file())
            .collect();
        assert_eq!(found.len(), 1, "expected exactly one profile settings file");
        let text = fs::read_to_string(found.remove(0)).expect("read settings");
        vsp::legacy_loader::parse_jsonc_value(&text, "settings").expect("valid JSONC")
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

/// Returns the extension IDs of preset `name` (`vsp show <name> --json`).
fn preset_extensions(sb: &Sandbox, name: &str) -> Vec<String> {
    let out = sb.vsp(&["show", name, "--json"]);
    assert_exit(&out, 0, "show");
    parse_json(&out)["extensions"]
        .as_array()
        .expect("extensions")
        .iter()
        .filter_map(|v| v.as_str().map(ToString::to_string))
        .collect()
}

fn installed_lines(ids: &[String]) -> Vec<String> {
    ids.iter().map(|id| format!("{id}@1.0.0")).collect()
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir copy target");
    for entry in fs::read_dir(src).expect("read copy source").flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else if entry.file_name() != ".DS_Store" {
            fs::copy(&from, &to).expect("copy file");
        }
    }
}

/// Copies the repository v2.2 catalog (the files that the loader requires) to `dst`.
fn copy_v2_catalog(dst: &Path) {
    let root = repo_root();
    fs::create_dir_all(dst.join("catalog")).expect("mkdir catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), dst.join(rel)).unwrap_or_else(|e| panic!("copy {rel}: {e}"));
    }
    copy_dir(&root.join("packs"), &dst.join("packs"));
}

fn read_lock(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read lock")).expect("lock JSON")
}

/// Starts a mock Marketplace: `POST` returns one stable universal version whose VSIX asset
/// is served by `GET` on the same server. Returns `(api_url, vsix_bytes)`.
fn start_mock_marketplace() -> (String, Vec<u8>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock marketplace");
    let addr = listener.local_addr().expect("addr");
    let vsix_bytes = b"mock-vsix-archive-bytes".to_vec();
    let served = vsix_bytes.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            // Read the full request (headers and `Content-Length` body) before the reply.
            // A reply that closes the socket while the client still writes its body makes
            // the client see a disconnect.
            let mut raw: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let content_length = text[..head_end]
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if raw.len() >= head_end + 4 + content_length {
                        break;
                    }
                }
            }
            let request = String::from_utf8_lossy(&raw).into_owned();
            let (content_type, body): (&str, Vec<u8>) = if request.starts_with("POST") {
                let payload = json!({"results": [{"extensions": [{
                    "publisher": {"publisherName": "mock"},
                    "extensionName": "mock",
                    "versions": [{
                        "version": "9.8.7",
                        "properties": [
                            {"key": "Microsoft.VisualStudio.Code.Engine", "value": "^1.80.0"}
                        ],
                        "files": [{
                            "assetType": "Microsoft.VisualStudio.Services.VSIXPackage",
                            "source": format!("http://{addr}/vsix")
                        }]
                    }]
                }]}]});
                ("application/json", payload.to_string().into_bytes())
            } else {
                ("application/octet-stream", served.clone())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    (format!("http://{addr}/query"), vsix_bytes)
}

/// Opus H1 / Astra A08: against the shipped lock, a Tier 1 `pending_refresh` extension is
/// never installed unverified (exit 5). `--allow-unlocked` installs it with a warning and a
/// provenance record. Opus M1: a later sync without the flag keeps that record.
#[test]
fn h1_m1_shipped_lock_fails_closed_and_unlocked_bypass_is_sticky() {
    let sb = Sandbox::new();
    let data = sb.data();
    let desired = preset_extensions(&sb, "rust");

    // Stage an isolated v2 catalog copy with a synthetic pending_refresh Tier 1 extension so this
    // regression stays deterministic even after all 103 production lock entries are verified.
    let fixture_cat = sb.path("pending-lock-cat");
    fs::create_dir_all(fixture_cat.join("catalog")).expect("mkdir catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(repo_root().join(rel), fixture_cat.join(rel)).expect("copy v2 catalog file");
    }
    fn copy_dir_rec(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).expect("mkdir dst");
        for entry in fs::read_dir(src).expect("read dir") {
            let entry = entry.expect("entry");
            let from = entry.path();
            let to = dst.join(entry.file_name());
            if entry.file_type().expect("ft").is_dir() {
                copy_dir_rec(&from, &to);
            } else {
                fs::copy(&from, &to).expect("copy file");
            }
        }
    }
    copy_dir_rec(&repo_root().join("packs"), &fixture_cat.join("packs"));

    let pending = "anthropic.claude-code".to_string();
    assert!(
        desired.iter().any(|id| id == &pending),
        "rust preset must include {pending}"
    );
    let mut lock = read_lock(&fixture_cat.join("vsp.lock.json"));
    lock["extensions"][&pending] = serde_json::json!({
        "tier": 1,
        "universal": false,
        "status": "pending_refresh",
        "version": null,
        "targets": {}
    });
    fs::write(
        fixture_cat.join("vsp.lock.json"),
        serde_json::to_string_pretty(&lock).expect("serialize lock"),
    )
    .expect("write mutated lock");
    let cat_arg = fixture_cat.to_str().expect("utf8").to_string();

    // Pre-register `rust` so `code --list-extensions --profile=rust` observes `others`
    // (FABLE-01 treats unregistered profiles as empty installed sets).
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}]}"#,
    )
    .expect("register rust profile");

    // Every other extension is already installed, so only the pending one must resolve.
    let others: Vec<String> = desired
        .iter()
        .filter(|id| **id != pending)
        .cloned()
        .collect();
    sb.install_code(&CodeStub {
        installed: &installed_lines(&others),
        ..Default::default()
    });

    let strict = sb.vsp_shipped_lock(&[
        "--catalog",
        &cat_arg,
        "--vscode-data-dir",
        &data,
        "sync",
        "-p",
        "rust",
    ]);
    assert_exit(&strict, 5, "sync against a pending Tier 1 lock");
    assert!(
        stderr(&strict).contains("Tier 1 hash lock"),
        "{}",
        stderr(&strict)
    );
    assert!(stderr(&strict).contains(&pending), "{}", stderr(&strict));
    assert!(
        !sb.code_log().contains("--install-extension"),
        "a pending Tier 1 extension must not be installed: {}",
        sb.code_log()
    );
    // OPUS-02 / CARBON-02: pre-write VSIX safety refusal performs ZERO writes (no settings or provenance).
    assert!(
        !sb.path("state/vsp/profiles").exists(),
        "pre-write exit 5 must not write provenance"
    );
    assert!(
        !sb.path("data/User/profiles/abcd1234/settings.json")
            .exists(),
        "pre-write exit 5 must not write settings.json"
    );

    // Explicit bypass: installs by ID, warns, and records the bypass.
    sb.clear_code_log();
    let bypass = sb.vsp_shipped_lock(&[
        "--catalog",
        &cat_arg,
        "--vscode-data-dir",
        &data,
        "--json",
        "sync",
        "-p",
        "rust",
        "--allow-unlocked",
    ]);
    assert_exit(&bypass, 0, "sync --allow-unlocked");
    assert!(stderr(&bypass).contains("WARNING"), "{}", stderr(&bypass));
    assert_eq!(parse_json(&bypass)["unlockedBypass"], true);
    assert!(sb
        .code_log()
        .contains(&format!("--install-extension {pending}")));
    assert_eq!(sb.provenance("rust")["unlockedBypass"], true);

    // M1: everything is installed now. A sync without the flag verifies nothing, so the
    // record must stay, and doctor must keep the warning.
    sb.install_code(&CodeStub {
        installed: &installed_lines(&desired),
        ..Default::default()
    });
    let later = sb.vsp_shipped_lock(&[
        "--catalog",
        &cat_arg,
        "--vscode-data-dir",
        &data,
        "--json",
        "sync",
        "-p",
        "rust",
    ]);
    assert_exit(&later, 0, "later sync without --allow-unlocked");
    assert_eq!(
        parse_json(&later)["unlockedBypass"],
        false,
        "this run did not bypass"
    );
    assert_eq!(
        sb.provenance("rust")["unlockedBypass"],
        true,
        "the provenance record is sticky"
    );
    let doctor = sb.vsp_shipped_lock(&["--vscode-data-dir", &data, "doctor"]);
    assert!(
        stdout(&doctor).contains("was last synced with --allow-unlocked"),
        "{}",
        stdout(&doctor)
    );
}

/// Astra A16: a verified platform-only Tier 1 lock record (per-platform targets, no
/// universal digest) passes validation, and a broken one is still rejected.
#[test]
fn a16_platform_only_tier1_lock_records_validate() {
    let sb = Sandbox::new();
    let fixture = sb.path("catalog-v2");
    copy_v2_catalog(&fixture);
    let fixture_str = fixture.to_str().expect("utf8");
    let lock_path = fixture.join("vsp.lock.json");
    let mut lock = read_lock(&lock_path);

    let native_id = lock["extensions"]
        .as_object()
        .expect("extensions")
        .iter()
        .find(|(_, e)| e["tier"] == 1 && e["universal"] == false)
        .map(|(id, _)| id.clone())
        .expect("the shipped lock has a platform-specific Tier 1 extension");
    let sha = "ab".repeat(32);
    lock["extensions"][&native_id] = json!({
        "tier": 1, "universal": false, "status": "verified", "version": "2.1.0",
        "verifiedAt": "2026-11-24T00:00:00Z",
        "targets": {
            "darwin-arm64": {"sha256": sha, "vsixUrl": "https://marketplace.visualstudio.com/a"},
            "linux-x64": {"sha256": sha, "vsixUrl": "https://x.gallerycdn.vsassets.io/b"}
        }
    });
    fs::write(&lock_path, lock.to_string()).expect("write lock");
    let ok = sb.vsp_shipped_lock(&["--catalog", fixture_str, "dev", "validate"]);
    assert_exit(&ok, 0, "validate a platform-only Tier 1 record");

    // A platform-only record without targets, or with a bad target digest, is rejected.
    lock["extensions"][&native_id]["targets"] = json!({});
    fs::write(&lock_path, lock.to_string()).expect("write lock");
    let empty = sb.vsp_shipped_lock(&["--catalog", fixture_str, "dev", "validate"]);
    assert_exit(&empty, 4, "platform-only record without targets");

    lock["extensions"][&native_id]["targets"] = json!({"darwin-arm64": {"sha256": "nothex", "vsixUrl": "https://marketplace.visualstudio.com/a"}});
    fs::write(&lock_path, lock.to_string()).expect("write lock");
    let bad = sb.vsp_shipped_lock(&["--catalog", fixture_str, "dev", "validate"]);
    assert_exit(&bad, 4, "platform-only record with an invalid digest");
}

/// Opus H2: an unsafe profile name (from a bundle `.name`, from `-p`, or from
/// `storage.json`) is refused with exit 5 before `code` is spawned or anything is written.
#[test]
fn h2_unsafe_profile_names_never_reach_code() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());
    let storage = sb.path("data/User/globalStorage/storage.json");
    let storage_before = fs::read_to_string(&storage).expect("storage");

    let bundle = sb.path("evil.code-profile");
    fs::write(
        &bundle,
        r#"{"name":"--install-extension=attackerpub.backdoor","settings":{},"extensions":{"enabled":[]}}"#,
    )
    .expect("write bundle");
    let bundle_str = bundle.to_str().expect("utf8");

    for args in [
        vec!["--vscode-data-dir", &data, "sync", "--from", bundle_str],
        vec![
            "--vscode-data-dir",
            &data,
            "--dry-run",
            "sync",
            "--from",
            bundle_str,
        ],
        vec![
            "--vscode-data-dir",
            &data,
            "sync",
            "--profile=--install-extension=x.y",
            "--preset",
            "rust",
        ],
        vec![
            "--vscode-data-dir",
            &data,
            "diff",
            "--profile=../escape",
            "--preset",
            "rust",
        ],
        vec![
            "--vscode-data-dir",
            &data,
            "sync",
            "--profile=a/b",
            "--preset",
            "rust",
        ],
        vec![
            "--vscode-data-dir",
            &data,
            "open",
            "--profile=-x",
            "--dry-run",
        ],
    ] {
        let out = sb.vsp(&args);
        assert_exit(&out, 5, &format!("vsp {}", args.join(" ")));
        assert!(
            stderr(&out).contains("unsafe VS Code profile name"),
            "{}",
            stderr(&out)
        );
    }
    assert_eq!(
        sb.code_log(),
        "",
        "code must never be spawned for an unsafe name"
    );
    assert_eq!(
        fs::read_to_string(&storage).expect("storage"),
        storage_before
    );
    assert!(!sb.path("data/User/profiles").exists());

    // A hostile name in storage.json stops `--all` with exit 5, and `code` never gets it.
    fs::write(
        &storage,
        r#"{"userDataProfiles":[{"name":"--install-extension=evil.ext","location":"abcd1234"}]}"#,
    )
    .expect("write hostile storage");
    let sweep = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "--all",
        "--compromised-only",
    ]);
    assert_exit(&sweep, 5, "sweep with a hostile storage.json name");
    assert!(!sb.code_log().contains("evil.ext"), "{}", sb.code_log());
}

/// Opus M2: a publisher outside the allowlist is refused; the environment bypass warns,
/// is recorded in provenance, and is reported by doctor.
#[test]
fn m2_publisher_bypass_is_loud_and_recorded() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());
    let bundle = sb.path("ext.code-profile");
    fs::write(
        &bundle,
        r#"{"name":"imported","settings":{"editor.tabSize":2},"extensions":{"enabled":["unlisted-publisher.tool"]}}"#,
    )
    .expect("write bundle");
    let bundle_str = bundle.to_str().expect("utf8");

    let refused = sb.vsp(&["--vscode-data-dir", &data, "sync", "--from", bundle_str]);
    assert_exit(&refused, 5, "publisher outside the allowlist");
    assert!(
        stderr(&refused).contains("allowlist"),
        "{}",
        stderr(&refused)
    );
    assert!(!sb.code_log().contains("--install-extension"));

    let bypass = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .env("ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER", "1")
        .args(["--vscode-data-dir", &data, "sync", "--from", bundle_str])
        .output()
        .expect("run bypass");
    assert_exit(&bypass, 0, "publisher bypass");
    assert!(
        stderr(&bypass).contains("WARNING: ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1"),
        "{}",
        stderr(&bypass)
    );
    assert_eq!(sb.provenance("imported")["publisherTrustBypass"], true);

    let doctor = sb.vsp(&["--vscode-data-dir", &data, "doctor"]);
    assert!(
        stdout(&doctor).contains("publisher allowlist bypassed"),
        "{}",
        stdout(&doctor)
    );
}

/// Opus M5 + L8: a failed Step 0 uninstall stops the sync with exit 5 before any install,
/// `storage.json` change, or settings write. The `--all` sweep keeps exit 5.
#[test]
fn m5_l8_step0_failure_aborts_with_exit_5() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub {
        installed: &[format!("{COMPROMISED_EXT}@4.2.1")],
        fail_uninstall: true,
        ..Default::default()
    });
    let storage = sb.path("data/User/globalStorage/storage.json");
    // Register `rust` in storage.json so `sync` observes its pre-existing installed extensions (FABLE-01).
    fs::write(
        &storage,
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}]}"#,
    )
    .expect("register rust profile");
    let storage_before = fs::read_to_string(&storage).expect("storage");

    let sync = sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust"]);
    assert_exit(&sync, 5, "sync with a failing Step 0");
    assert!(stderr(&sync).contains("Step 0 failed"), "{}", stderr(&sync));
    let log = sb.code_log();
    assert!(
        log.contains(&format!("--uninstall-extension {COMPROMISED_EXT}")),
        "{log}"
    );
    assert!(
        !log.contains("--install-extension"),
        "no install after a failed Step 0: {log}"
    );
    assert_eq!(
        fs::read_to_string(&storage).expect("storage"),
        storage_before
    );
    assert!(!sb.path("data/User/profiles").exists(), "no settings write");

    let single = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "-p",
        "rust",
        "--compromised-only",
    ]);
    assert_exit(&single, 5, "--compromised-only with a failing Step 0");
    let sweep = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "sync",
        "--all",
        "--compromised-only",
    ]);
    assert_exit(&sweep, 5, "--all --compromised-only keeps exit 5");
}

/// Opus M6: doctor reports exit 7 when the `code` CLI is missing or the VS Code store
/// cannot be resolved.
#[test]
fn m6_doctor_is_incomplete_without_code_or_store() {
    let sb = Sandbox::new();
    let data = sb.data();

    let no_code = sb.vsp(&["--vscode-data-dir", &data, "doctor", "--json"]);
    assert_exit(&no_code, 7, "doctor without the code CLI");
    let report = parse_json(&no_code);
    assert_eq!(report["incomplete_observation"], true);

    sb.install_code(&CodeStub::default());
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "doctor"]),
        0,
        "doctor with code",
    );

    let bad_store = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .env("VSCODE_USER_DIR", sb.path("data/NotTheUserDir"))
        .args(["doctor"])
        .output()
        .expect("doctor with an unrepresentable store");
    assert_exit(&bad_store, 7, "doctor with an unresolved store");
    assert!(
        stdout(&bad_store).contains("audit incomplete"),
        "{}",
        stdout(&bad_store)
    );
}

/// Opus M7: `--ai off` uninstalls the installed catalog AI-group extensions, and the plan
/// shows the removal.
#[test]
fn m7_ai_off_uninstalls_installed_ai_extensions() {
    let sb = Sandbox::new();
    let data = sb.data();
    let ai_ids = [
        "anthropic.claude-code",
        "github.copilot-chat",
        "openai.chatgpt",
    ];
    let mut installed: Vec<String> = ai_ids.iter().map(|id| format!("{id}@1.0.0")).collect();
    installed.push("editorconfig.editorconfig@0.18.2".to_string());
    sb.install_code(&CodeStub {
        installed: &installed,
        ..Default::default()
    });
    // Register `rust` so `sync` queries its already-installed extensions (FABLE-01).
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"abcd1234"}]}"#,
    )
    .expect("register rust profile");

    let diff = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--json",
        "diff",
        "-p",
        "rust",
        "--ai",
        "off",
    ]);
    assert_exit(&diff, 0, "diff --ai off");
    let changes = parse_json(&diff)["extension_changes"].clone();
    for id in ai_ids {
        assert!(
            changes
                .as_array()
                .expect("changes")
                .iter()
                .any(|c| c["id"] == id && c["action"] == "uninstall"),
            "plan must uninstall {id}: {changes}"
        );
    }

    let sync = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--json",
        "sync",
        "-p",
        "rust",
        "--ai",
        "off",
    ]);
    assert_exit(&sync, 0, "sync --ai off");
    let log = sb.code_log();
    for id in ai_ids {
        assert!(
            log.contains(&format!("--uninstall-extension {id}")),
            "{id}: {log}"
        );
        assert!(
            !log.contains(&format!("--install-extension {id}")),
            "{id}: {log}"
        );
    }
    assert!(
        !log.contains("--uninstall-extension editorconfig.editorconfig"),
        "{log}"
    );
    assert_eq!(sb.profile_settings()["chat.disableAIFeatures"], true);
}

/// Opus L6: the host-path lint runs on the MERGED workspace settings (managed keys), and a
/// changed workspace settings file gets a `0600` backup.
#[test]
fn l6_merged_host_path_lint_and_workspace_backup() {
    let sb = Sandbox::new();
    let ws = sb.path("cwd");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let settings = ws.join(".vscode/settings.json");

    let plan = sb.vsp(&["--json", "diff", "."]);
    assert_exit(&plan, 0, "diff");
    let object_key = parse_json(&plan)["settings_changes"]
        .as_array()
        .expect("changes")
        .iter()
        .find(|c| c["desired"].is_object())
        .map(|c| c["key"].as_str().expect("key").to_string())
        .expect("the rust workspace has an object-valued managed setting");

    // The existing file has a host path INSIDE a managed object. The desired value alone is
    // clean; only the merged value has the leak.
    fs::create_dir_all(ws.join(".vscode")).expect("mkdir .vscode");
    let leaky = format!(
        "{{\n  // mine\n  \"my.userKey\": true,\n  {}: {{ \"my.tool.path\": \"/Users/someone/bin/tool\" }}\n}}\n",
        Value::String(object_key.clone())
    );
    fs::write(&settings, &leaky).expect("write leaky settings");
    let refused = sb.vsp(&["sync", "."]);
    assert_exit(&refused, 5, "merged host-path leak");
    assert!(
        stderr(&refused).contains("host-path leak"),
        "{}",
        stderr(&refused)
    );
    assert_eq!(
        fs::read_to_string(&settings).expect("settings"),
        leaky,
        "file unchanged"
    );

    // A clean existing file is merged, and the earlier content is kept in a 0600 backup.
    // The backup is under the private XDG state directory. The team `.vscode/` directory
    // (usually tracked by git) never gets a `*.bak.*` file.
    let clean = "{\n  // mine\n  \"my.userKey\": true\n}\n";
    fs::write(&settings, clean).expect("write clean settings");
    assert_exit(&sb.vsp(&["sync", "."]), 0, "sync");

    let vscode_entries = || -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(ws.join(".vscode"))
            .expect("ls .vscode")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        vscode_entries(),
        vec!["extensions.json", "settings.json", "tasks.json"],
        ".vscode/ holds the managed files only"
    );

    let state_backups = || -> Vec<PathBuf> {
        let root = sb.path("state/vsp/backups/workspaces");
        let mut found = Vec::new();
        for ws_dir in fs::read_dir(&root).expect("backup root").flatten() {
            assert_eq!(
                fs::metadata(ws_dir.path())
                    .expect("dir meta")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700,
                "the per-workspace backup directory is private"
            );
            for entry in fs::read_dir(ws_dir.path()).expect("ws backups").flatten() {
                found.push(entry.path());
            }
        }
        found
    };
    let backups = state_backups();
    assert_eq!(backups.len(), 1, "one backup for one changed write");
    assert!(backups[0]
        .file_name()
        .expect("name")
        .to_string_lossy()
        .starts_with("settings.json.bak."));
    assert_eq!(fs::read_to_string(&backups[0]).expect("backup"), clean);
    assert_eq!(
        fs::metadata(&backups[0])
            .expect("meta")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // An unchanged second sync makes no new backup.
    assert_exit(&sb.vsp(&["sync", "."]), 0, "second sync");
    assert_eq!(state_backups().len(), 1);

    // Five more changed writes make six backups. Only the newest five are kept.
    for round in 0..5 {
        fs::write(&settings, format!("{{\n  \"my.userKey\": {round}\n}}\n"))
            .expect("rewrite settings");
        assert_exit(&sb.vsp(&["sync", "."]), 0, "sync after a user edit");
    }
    assert_eq!(state_backups().len(), 5, "bounded to the newest five");
    assert_eq!(
        vscode_entries(),
        vec!["extensions.json", "settings.json", "tasks.json"]
    );
}

/// Astra A02: a profile diff observes the installed extensions. Missing, compromised, and
/// unobservable states are all reported.
#[test]
fn a02_profile_diff_reports_extension_drift() {
    let sb = Sandbox::new();
    let data = sb.data();
    let desired = preset_extensions(&sb, "rust");
    let check = [
        "--vscode-data-dir",
        data.as_str(),
        "--json",
        "diff",
        "-p",
        "rust",
        "--check",
    ];

    // Converge settings and extensions first.
    sb.install_code(&CodeStub {
        installed: &installed_lines(&desired),
        ..Default::default()
    });
    assert_exit(
        &sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "rust"]),
        0,
        "sync",
    );
    let clean = sb.vsp(&check);
    assert_exit(&clean, 0, "diff --check on a converged profile");
    assert_eq!(parse_json(&clean)["has_drift"], false);
    assert_eq!(parse_json(&clean)["observation_complete"], true);

    // Remove one required extension: settings still match, so only extensions drift.
    let removed = desired.last().expect("an extension").clone();
    let partial: Vec<String> = desired
        .iter()
        .filter(|id| **id != removed)
        .cloned()
        .collect();
    sb.install_code(&CodeStub {
        installed: &installed_lines(&partial),
        ..Default::default()
    });
    let drift = sb.vsp(&check);
    assert_exit(&drift, 3, "diff --check with a missing extension");
    let plan = parse_json(&drift);
    assert_eq!(
        plan["settings_changes"].as_array().expect("settings").len(),
        0
    );
    assert_eq!(plan["extension_changes"][0]["id"], removed.as_str());
    assert_eq!(plan["extension_changes"][0]["action"], "ensure_installed");

    // A compromised extension is drift too.
    let mut with_bad = installed_lines(&desired);
    with_bad.push(format!("{COMPROMISED_EXT}@4.2.1"));
    sb.install_code(&CodeStub {
        installed: &with_bad,
        ..Default::default()
    });
    let compromised = sb.vsp(&check);
    assert_exit(&compromised, 3, "diff --check with a compromised extension");
    assert_eq!(
        parse_json(&compromised)["extension_changes"][0],
        json!({"id": COMPROMISED_EXT, "action": "uninstall_compromised"})
    );

    // A failed observation is exit 7, never "clean" and never "drift".
    sb.install_code(&CodeStub {
        fail_list: true,
        ..Default::default()
    });
    let failed = sb.vsp(&check);
    assert_exit(&failed, 7, "diff --check when code --list-extensions fails");
    assert_eq!(parse_json(&failed)["observation_complete"], false);
    let dry = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
    ]);
    assert_exit(&dry, 7, "sync --dry-run when code --list-extensions fails");
}

/// Astra A09: malformed target JSONC is an incomplete observation (exit 7), not drift.
#[test]
fn a09_malformed_target_jsonc_is_incomplete_observation() {
    let sb = Sandbox::new();
    let ws = sb.path("cwd");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    fs::create_dir_all(ws.join(".vscode")).expect("mkdir .vscode");

    // No file at all: a complete observation with drift.
    assert_exit(
        &sb.vsp(&["diff", ".", "--check"]),
        3,
        "diff --check without target files",
    );

    for (file, content) in [
        ("settings.json", "{ \"a\": "),
        ("settings.json", "[1, 2, 3]"),
        ("extensions.json", "{ not json"),
    ] {
        let path = ws.join(".vscode").join(file);
        fs::write(&path, content).expect("write malformed file");
        let out = sb.vsp(&["--json", "diff", ".", "--check"]);
        assert_exit(&out, 7, &format!("diff --check with malformed {file}"));
        let plan = parse_json(&out);
        assert_eq!(plan["observation_complete"], false);
        assert_eq!(plan["has_drift"], false, "no drift is classified");
        assert_eq!(
            plan["settings_changes"].as_array().expect("settings").len(),
            0
        );
        assert!(plan["warnings"][0]
            .as_str()
            .expect("warning")
            .contains("observation incomplete"));
        let dry = sb.vsp(&["--dry-run", "sync", "."]);
        assert_exit(&dry, 7, &format!("sync --dry-run with malformed {file}"));
        fs::remove_file(&path).expect("remove malformed file");
    }

    // The Default user profile target follows the same rule.
    fs::write(sb.path("data/User/settings.json"), "{ broken").expect("write user settings");
    let data = sb.data();
    let user = sb.vsp(&["--vscode-data-dir", &data, "diff", "--user", "--check"]);
    assert_exit(&user, 7, "diff --user with malformed settings.json");
}

/// Astra A06: `sync --dry-run` applies the same selector and bundle validation as the
/// mutating `sync`.
#[test]
fn a06_dry_run_uses_the_same_validation_as_sync() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());

    for args in [
        vec!["--dry-run", "sync", "--all", "-p", "rust"],
        vec!["sync", "--all", "-p", "rust"],
        vec!["--dry-run", "sync", "--user", "-p", "rust"],
        vec!["sync", "--user", "-p", "rust"],
        vec!["diff", "--user", "-p", "rust"],
        vec!["--dry-run", "sync", "--all", "--user"],
        vec!["--dry-run", "sync", ".", "--user"],
        vec!["diff", ".", "--user"],
    ] {
        let mut full = vec!["--vscode-data-dir", data.as_str()];
        full.extend(args.iter().copied());
        assert_exit(&sb.vsp(&full), 4, &format!("vsp {}", args.join(" ")));
    }

    let write_bundle = |name: &str, text: &str| -> String {
        let path = sb.path(name);
        fs::write(&path, text).expect("write bundle");
        path.to_str().expect("utf8").to_string()
    };
    let invalid = write_bundle("invalid.code-profile", "{ not json");
    let n1 = write_bundle(
        "n1.code-profile",
        r#"{"name":"p","settings":{"chat":{"tools":{"autoApprove":true}}}}"#,
    );
    let terminal = write_bundle(
        "terminal.code-profile",
        r#"{"name":"p","settings":{"terminal.integrated.profiles.osx":{"x":{"path":"/tmp/evil"}}}}"#,
    );
    let bad_ext = write_bundle(
        "badext.code-profile",
        r#"{"name":"p","settings":{},"extensions":{"enabled":["not an id"]}}"#,
    );
    let good = write_bundle(
        "good.code-profile",
        r#"{"name":"imported","settings":{"editor.tabSize":2},"extensions":{"enabled":["editorconfig.editorconfig"]}}"#,
    );

    for (bundle, expected) in [(&invalid, 1), (&n1, 5), (&terminal, 5), (&bad_ext, 5)] {
        for dry in [true, false] {
            let mut args = vec!["--vscode-data-dir", data.as_str()];
            if dry {
                args.push("--dry-run");
            }
            args.extend(["sync", "--from", bundle.as_str()]);
            assert_exit(&sb.vsp(&args), expected, &format!("vsp {}", args.join(" ")));
        }
    }
    assert!(
        !sb.path("data/User/profiles").exists(),
        "rejected bundles write nothing"
    );

    let preview = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "--json",
        "sync",
        "--from",
        &good,
    ]);
    assert_exit(&preview, 0, "dry-run of a valid bundle");
    let payload = parse_json(&preview);
    assert_eq!(payload["bundle"]["profileName"], "imported");
    assert_eq!(
        payload["bundle"]["installIds"][0],
        "editorconfig.editorconfig"
    );
    assert!(
        !sb.path("data/User/profiles").exists(),
        "a dry run writes nothing"
    );
}

/// Astra A07: `open` refuses an unrepresentable `VSCODE_USER_DIR` before any sync or
/// launch. Fable N2: `open --sync` applies a custom user profile like `vsp sync`.
#[test]
fn a07_n2_open_validates_store_first_and_threads_user_overlay() {
    let sb = Sandbox::new();
    sb.install_code(&CodeStub::default());
    let ws = sb.path("ws");
    fs::create_dir_all(&ws).expect("mkdir ws");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("write Cargo.toml");
    let ws_str = ws.to_str().expect("utf8");

    for args in [
        vec!["open", "-p", "rust", "--dry-run"],
        vec!["open", "-p", "rust", ws_str, "--sync"],
        vec!["open", ws_str, "--sync"],
    ] {
        let out = sb
            .command()
            .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
            .env("VSCODE_USER_DIR", sb.path("data/NotTheUserDir"))
            .args(&args)
            .output()
            .expect("open with an unrepresentable store");
        assert_exit(&out, 4, &format!("vsp {}", args.join(" ")));
    }
    assert!(
        !ws.join(".vscode").exists(),
        "no workspace sync before the store check"
    );
    assert_eq!(sb.code_log(), "", "no launch and no sync");

    // N2: a custom user profile with an override of a managed UX key and an exact-ID
    // external extension.
    let config = sb.path("config/vsp/config.jsonc");
    fs::create_dir_all(config.parent().expect("parent")).expect("mkdir config");
    fs::write(
        &config,
        format!(
            r#"{{"version":1,"defaults":{{"ai":"off"}},"profiles":{{"my-rust":{{"extends":"rust",
               "overrides":{{"editor.fontSize":19}},"extraExtensions":["{EXTERNAL_EXT}"]}}}}}}"#
        ),
    )
    .expect("write user config");
    let data = sb.data();
    let open = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "open",
        "-p",
        "my-rust",
        "--sync",
    ]);
    assert_exit(&open, 0, "open --sync with a user profile");
    let log = sb.code_log();
    assert!(
        log.contains(&format!("--install-extension {EXTERNAL_EXT}")),
        "{log}"
    );
    assert!(
        log.lines()
            .last()
            .expect("launch line")
            .ends_with("--profile=my-rust"),
        "{log}"
    );
    let settings = sb.profile_settings();
    assert_eq!(settings["editor.fontSize"], 19, "last-layer override");
    assert_eq!(
        settings["chat.disableAIFeatures"], true,
        "defaults.ai = off"
    );
}

/// Astra A13: `dev dist` refuses a reused directory with a stale bundle. Astra A20: a
/// partial export keeps the checksums of earlier bundles in the same directory.
#[test]
fn a13_a20_dist_and_export_inventories_are_consistent() {
    let sb = Sandbox::new();
    let dist = sb.path("dist");
    let dist_str = dist.to_str().expect("utf8");

    assert_exit(
        &sb.vsp(&["dev", "dist", "--out", dist_str]),
        0,
        "first dist",
    );
    let sums_first = fs::read(dist.join("SHA256SUMS")).expect("sums");
    assert_exit(
        &sb.vsp(&["dev", "dist", "--out", dist_str]),
        0,
        "dist into the same directory",
    );
    assert_eq!(fs::read(dist.join("SHA256SUMS")).expect("sums"), sums_first);
    let bundle_count = fs::read_dir(&dist)
        .expect("ls dist")
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".code-profile"))
        .count();
    assert_eq!(bundle_count, 37);
    assert_eq!(String::from_utf8_lossy(&sums_first).lines().count(), 37);

    fs::write(dist.join("removed-preset.code-profile"), "{}").expect("write stale bundle");
    let stale = sb.vsp(&["dev", "dist", "--out", dist_str]);
    assert_exit(&stale, 5, "dist with a stale bundle");
    assert!(
        stderr(&stale).contains("removed-preset.code-profile"),
        "{}",
        stderr(&stale)
    );
    assert!(
        dist.join("removed-preset.code-profile").is_file(),
        "vsp deletes no foreign file"
    );
    assert_eq!(
        fs::read(dist.join("SHA256SUMS")).expect("sums"),
        sums_first,
        "nothing rewritten"
    );

    // A20: two disjoint exports into one directory.
    let exports = sb.path("exports");
    let exports_str = exports.to_str().expect("utf8");
    assert_exit(
        &sb.vsp(&["export", "rust", "--out", exports_str]),
        0,
        "export rust",
    );
    assert_exit(
        &sb.vsp(&["export", "general", "--out", exports_str]),
        0,
        "export general",
    );
    let sums = fs::read_to_string(exports.join("SHA256SUMS")).expect("sums");
    let entries: Vec<(&str, &str)> = sums
        .lines()
        .map(|l| l.split_once("  ").expect("sum line"))
        .collect();
    assert_eq!(
        entries.iter().map(|(_, name)| *name).collect::<Vec<_>>(),
        vec!["general.code-profile", "rust.code-profile"]
    );
    for (digest, name) in entries {
        assert_eq!(
            sha256_hex(&fs::read(exports.join(name)).expect("bundle")),
            digest,
            "{name}"
        );
    }
}

/// Astra A15: an implicit toolchain preset and a custom alias from `presets.jsonc` resolve
/// in `show`, `diff`, `sync --dry-run`, and `export`.
#[test]
fn a15_implicit_presets_and_custom_aliases_resolve_in_the_cli() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());

    // Implicit toolchain preset from the embedded catalog: `general + biome`.
    let diff = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--json",
        "diff",
        "--user",
        "--preset",
        "biome",
    ]);
    assert_exit(&diff, 0, "diff --user --preset biome");
    let plan = parse_json(&diff);
    assert_eq!(plan["target"]["preset"], "biome");
    assert!(plan["resolved_modules"]
        .as_array()
        .expect("modules")
        .iter()
        .any(|m| m == "biome"));
    let profile = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--json",
        "--dry-run",
        "sync",
        "-p",
        "biome",
    ]);
    assert_exit(&profile, 0, "sync --dry-run -p biome");
    let exports = sb.path("exports");
    let exported = sb.vsp(&["export", "biome", "--out", exports.to_str().expect("utf8")]);
    assert_exit(&exported, 0, "export biome");
    assert!(exports.join("biome.code-profile").is_file());
    // The implicit preset is not listed and an unknown name is still a missing target.
    assert!(!stdout(&sb.vsp(&["list"])).lines().any(|l| l == "biome"));
    let unknown = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "diff",
        "--user",
        "--preset",
        "nope",
    ]);
    assert_exit(&unknown, 2, "unknown preset");

    // Custom alias in an external presets.jsonc.
    let fixture = sb.path("catalog-v2");
    copy_v2_catalog(&fixture);
    let presets_path = fixture.join("presets.jsonc");
    let presets = fs::read_to_string(&presets_path).expect("presets");
    assert!(presets.contains("\"legacyAliases\": {"), "fixture marker");
    fs::write(
        &presets_path,
        presets.replacen(
            "\"legacyAliases\": {",
            "\"legacyAliases\": {\n    \"team-rust-hidpi\": { \"preset\": \"rust\", \"ux\": \"retina\" },",
            1,
        ),
    )
    .expect("write presets");
    let fixture_str = fixture.to_str().expect("utf8");

    let alias = sb.vsp(&[
        "--catalog",
        fixture_str,
        "--vscode-data-dir",
        &data,
        "--json",
        "diff",
        "--user",
        "--preset",
        "team-rust-hidpi",
    ]);
    assert_exit(&alias, 0, "diff with a custom alias");
    let alias_plan = parse_json(&alias);
    assert_eq!(alias_plan["target"]["preset"], "rust");
    assert_eq!(alias_plan["target"]["ux"], "retina");

    let show = sb.vsp(&[
        "--catalog",
        fixture_str,
        "--json",
        "show",
        "team-rust-hidpi",
    ]);
    assert_exit(&show, 0, "show a custom alias");
    assert_eq!(parse_json(&show)["name"], "rust");
    let alias_profile = sb.vsp(&[
        "--catalog",
        fixture_str,
        "--vscode-data-dir",
        &data,
        "--json",
        "diff",
        "-p",
        "team-rust-hidpi",
    ]);
    assert_exit(&alias_profile, 0, "diff -p with a custom alias");
    assert_eq!(parse_json(&alias_profile)["target"]["profile"], "rust");
}

/// Astra A21: a symlinked directory beneath the XDG config root is refused before any RTK
/// directory creation, chmod, or file write.
#[test]
fn a21_rtk_install_rejects_symlinked_ancestors() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.install_code(&CodeStub::default());
    fs::create_dir_all(sb.path("data/User")).expect("mkdir User");

    let outside = sb.path("outside");
    fs::create_dir_all(&outside).expect("mkdir outside");
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).expect("chmod outside");
    std::os::unix::fs::symlink(&outside, sb.path("config/vsp")).expect("symlink vsp dir");

    let args = [
        "--vscode-data-dir",
        data.as_str(),
        "sync",
        "--user",
        "--with",
        "rtk-terminal",
    ];
    let refused = sb.vsp(&args);
    assert_exit(&refused, 5, "RTK install through a symlinked ancestor");
    assert_eq!(
        fs::read_dir(&outside).expect("ls outside").count(),
        0,
        "nothing written outside"
    );
    assert_eq!(
        fs::metadata(&outside).expect("meta").permissions().mode() & 0o777,
        0o700,
        "the outside directory mode is unchanged"
    );
    assert!(
        !sb.path("data/User/settings.json").exists(),
        "no settings write"
    );

    // With a real directory the same command succeeds and installs the wrappers.
    fs::remove_file(sb.path("config/vsp")).expect("remove link");
    assert_exit(&sb.vsp(&args), 0, "RTK install into a real directory");
    assert!(sb.path("config/vsp/rtk/rtk-init.zsh").is_file());
    assert!(sb.path("config/vsp/rtk/zsh/.zshrc").is_file());

    // A linked `rtk/zsh` directory is refused too.
    fs::remove_dir_all(sb.path("config/vsp/rtk/zsh")).expect("remove zsh dir");
    std::os::unix::fs::symlink(&outside, sb.path("config/vsp/rtk/zsh")).expect("symlink zsh dir");
    assert_exit(
        &sb.vsp(&args),
        5,
        "RTK install through a symlinked zsh directory",
    );
    assert_eq!(fs::read_dir(&outside).expect("ls outside").count(), 0);
}

/// Astra A01 + A10: `dev pins --refresh` updates `vsp.lock.json` for a v2.2 catalog and
/// keeps tier and policy metadata; a legacy manifest keeps `pinRequired`.
#[test]
fn a01_a10_pins_refresh_targets_the_lockfile_and_keeps_policy() {
    let sb = Sandbox::new();
    let (api_url, vsix_bytes) = start_mock_marketplace();
    let expected_sha = sha256_hex(&vsix_bytes);

    // A01: v2.2 catalog. `pinRequired` is set to true to prove that the policy is kept.
    let fixture = sb.path("catalog-v2");
    copy_v2_catalog(&fixture);
    let lock_path = fixture.join("vsp.lock.json");
    let mut before = read_lock(&lock_path);
    before["pinRequired"] = json!(true);
    fs::write(&lock_path, before.to_string()).expect("write lock");

    let refresh = sb
        .command()
        .env("VSP_MARKETPLACE_API_URL", &api_url)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .args([
            "--catalog",
            fixture.to_str().expect("utf8"),
            "--json",
            "dev",
            "pins",
            "--refresh",
        ])
        .output()
        .expect("dev pins --refresh");
    assert_exit(&refresh, 0, "refresh a v2.2 lockfile");
    let reported = PathBuf::from(parse_json(&refresh)["path"].as_str().expect("path"));
    assert_eq!(
        fs::canonicalize(reported).expect("reported path"),
        fs::canonicalize(&lock_path).expect("lock path"),
        "the refresh must write vsp.lock.json"
    );
    assert!(
        !fixture.join("catalog/extension-pins.json").exists(),
        "the unused legacy manifest must not be written"
    );

    let after = read_lock(&lock_path);
    assert_eq!(after["pinRequired"], true, "policy kept");
    assert_eq!(after["platforms"], before["platforms"]);
    let mut refreshed_t1 = 0;
    let mut refreshed_t2 = 0;
    for (id, old) in before["extensions"].as_object().expect("extensions") {
        let new = &after["extensions"][id];
        assert_eq!(new["tier"], old["tier"], "{id}: tier kept");
        if old["tier"] == 1 && old["universal"] == true {
            refreshed_t1 += 1;
            assert_eq!(new["status"], "verified", "{id}");
            assert_eq!(new["version"], "9.8.7", "{id}");
            assert_eq!(new["sha256"], expected_sha.as_str(), "{id}");
            assert_eq!(
                new["targets"]["universal"]["sha256"],
                expected_sha.as_str(),
                "{id}"
            );
            assert_eq!(new["universal"], true, "{id}");
        } else if old["tier"] == 2 {
            // OPUS-01 / CARBON-01: `dev pins --refresh` now also version-locks Tier 2 entries.
            refreshed_t2 += 1;
            assert_eq!(new["status"], "verified", "{id}");
            assert_eq!(new["version"], "9.8.7", "{id}");
            assert!(new["verifiedAt"].is_string(), "{id}");
        } else {
            assert_eq!(new, old, "{id}: entry outside the refresh set is unchanged");
        }
    }
    assert!(refreshed_t1 > 0, "the lock has Tier 1 universal entries");
    assert!(refreshed_t2 > 0, "the lock has Tier 2 entries");

    // A10: a legacy manifest with pinRequired=true keeps that policy.
    let legacy_pins = sb.path("legacy-pins.json");
    fs::write(
        &legacy_pins,
        r#"{"version":1,"pinRequired":true,"pins":{"editorconfig.editorconfig":{"sha256":"","version":"0.1.0","vsix_url":""}}}"#,
    )
    .expect("write legacy pins");
    let legacy = sb
        .command()
        .env("VSP_MARKETPLACE_API_URL", &api_url)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .env("ARTAGON_VSCODE_PINS_FILE", &legacy_pins)
        .args([
            "--catalog",
            repo_root().to_str().expect("utf8"),
            "dev",
            "pins",
            "--refresh",
        ])
        .output()
        .expect("legacy refresh");
    assert_exit(&legacy, 0, "refresh a legacy manifest");
    let manifest: Value =
        serde_json::from_str(&fs::read_to_string(&legacy_pins).expect("pins")).expect("pins JSON");
    assert_eq!(
        manifest["pinRequired"], true,
        "pinRequired is not reset to false"
    );
    assert_eq!(
        manifest["pins"]["editorconfig.editorconfig"]["sha256"],
        expected_sha.as_str()
    );
}

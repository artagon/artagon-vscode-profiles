//! Wave 3A black-box CLI integration tests covering CLI ergonomics, `RunContext`,
//! universal zero-write `--dry-run`, `BrokenPipe`, strict `--json` & structured JSON errors,
//! pre-load `diff` selector validation, `PlanExtras` preview parity & `sync --dry-run` exit 5,
//! dedicated `vsp use <PROFILE> [PATH]`, native `--extensions-only`, `-v/--verbose`,
//! hidden stub `dev` verbs, `--help` headings/examples, selector hints, human-readable
//! `diff`/`list`/`show`, `vsp init` reserved-name guard, scoped user config, and
//! `doctor --strict` clean install (`CARBON-03`).

#![cfg(unix)]

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;

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

    fn write_default_config(&self, text: &str) {
        let path = self.default_config_path();
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir config");
        fs::write(&path, text).expect("write default config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod 600");
    }

    fn write_config_file(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir parent");
        }
        fs::write(&path, text).expect("write config file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod 600");
        path
    }

    fn install_code_stub(&self, script: &str) {
        let stub = self.path("bin/code");
        fs::write(&stub, script).expect("write code stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod stub");
    }

    fn install_empty_pins(&self) -> PathBuf {
        let pins = self.path("pins.json");
        fs::write(&pins, "{\"version\":1,\"pins\":{}}\n").expect("write empty pins");
        pins
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(vsp_bin());
        cmd.current_dir(self.path("cwd"))
            .env_clear()
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("config"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.path("bin").display()),
            )
            .env("ARTAGON_VSCODE_TEST_RUNNING", "0");
        cmd
    }

    fn vsp(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("run vsp")
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_exit(out: &Output, expected: i32, ctx: &str) {
    assert_eq!(
        out.status.code(),
        Some(expected),
        "{ctx}: expected exit {expected}\nstdout={}\nstderr={}",
        stdout(out),
        stderr(out)
    );
}

fn parse_stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}):\n{}", stdout(out)))
}

fn parse_stderr_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stderr)
        .unwrap_or_else(|e| panic!("stderr is not JSON ({e}):\n{}", stderr(out)))
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir dst");
    for entry in fs::read_dir(src).expect("read src").flatten() {
        let p = entry.path();
        let t = dst.join(entry.file_name());
        if p.is_dir() {
            copy_tree(&p, &t);
        } else if entry.file_name() != ".DS_Store" {
            fs::copy(&p, &t).expect("copy file");
        }
    }
}

#[test]
fn universal_dry_run_performs_zero_writes_across_export_dist_dev_and_self() {
    let sb = Sandbox::new();

    // 1. `export --dry-run` (text and --json): zero dir creations, prints planned paths.
    let exp_dir = sb.path("no-write-exports");
    let exp_str = exp_dir.to_str().expect("utf8");
    let exp_text = sb.vsp(&["--dry-run", "export", "rust", "--out", exp_str]);
    assert_exit(&exp_text, 0, "export --dry-run text");
    assert!(
        !exp_dir.exists(),
        "export --dry-run must not create output dir"
    );
    assert!(stdout(&exp_text).contains("DRY-RUN:"));
    assert!(stdout(&exp_text).contains("rust.code-profile"));
    assert!(stdout(&exp_text).contains("SHA256SUMS"));

    let exp_json = sb.vsp(&["--dry-run", "--json", "export", "rust", "--out", exp_str]);
    assert_exit(&exp_json, 0, "export --dry-run json");
    assert!(!exp_dir.exists());
    let exp_val = parse_stdout_json(&exp_json);
    assert_eq!(exp_val["dry_run"], true);
    let planned = exp_val["planned_paths"].as_array().expect("planned_paths");
    assert_eq!(planned.len(), 2);

    // 2. `dev dist --dry-run` (text and --json): zero dir creations, 38 planned paths.
    let dist_dir = sb.path("no-write-dist");
    let dist_str = dist_dir.to_str().expect("utf8");
    let dist_json = sb.vsp(&["--dry-run", "--json", "dev", "dist", "--out", dist_str]);
    assert_exit(&dist_json, 0, "dev dist --dry-run json");
    assert!(
        !dist_dir.exists(),
        "dev dist --dry-run must not create output dir"
    );
    let dist_val = parse_stdout_json(&dist_json);
    assert_eq!(dist_val["dry_run"], true);
    assert_eq!(
        dist_val["planned_paths"]
            .as_array()
            .expect("planned_paths")
            .len(),
        38
    );

    // 3. `dev pins --check --strict` / `--release-gate`: passes exit 0 on the refreshed
    //    shipped lockfile (0 pending Tier 1 / Tier 2), and fails exit 1 on a synthetic catalog
    //    with pending entries!
    let pins_ok = sb.vsp(&["dev", "pins", "--check", "--json"]);
    assert_exit(&pins_ok, 0, "dev pins --check");
    assert_eq!(parse_stdout_json(&pins_ok)["strictReleaseGate"], false);

    for gate_flag in ["--strict", "--release-gate"] {
        let strict_ok = sb.vsp(&["dev", "pins", "--check", gate_flag, "--json"]);
        assert_exit(
            &strict_ok,
            0,
            &format!("shipped lock dev pins --check {gate_flag}"),
        );
        let strict_val = parse_stdout_json(&strict_ok);
        assert_eq!(strict_val["strictReleaseGate"], true);
        assert_eq!(strict_val["status"], "ok");
        assert_eq!(strict_val["pendingTier1"], 0);
        assert_eq!(strict_val["pendingTier2"], 0);
    }

    let pending_gate_repo = sb.path("pending-gate-repo");
    let root_for_gate = repo_root();
    fs::create_dir_all(pending_gate_repo.join("catalog")).expect("mkdir gate catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root_for_gate.join(rel), pending_gate_repo.join(rel)).expect("copy gate file");
    }
    copy_tree(
        &root_for_gate.join("packs"),
        &pending_gate_repo.join("packs"),
    );
    let mut gate_lock: Value = serde_json::from_str(
        &fs::read_to_string(pending_gate_repo.join("vsp.lock.json")).expect("read gate lock"),
    )
    .expect("parse gate lock");
    gate_lock["extensions"]["anthropic.claude-code"] = json!({
        "tier": 1,
        "universal": false,
        "status": "pending_refresh",
        "version": null,
        "targets": {}
    });
    fs::write(
        pending_gate_repo.join("vsp.lock.json"),
        serde_json::to_string_pretty(&gate_lock).expect("ser gate lock"),
    )
    .expect("write gate lock");
    let pending_gate_str = pending_gate_repo.to_str().expect("utf8");
    for gate_flag in ["--strict", "--release-gate"] {
        let strict_fail = sb.vsp(&[
            "--catalog",
            pending_gate_str,
            "dev",
            "pins",
            "--check",
            gate_flag,
            "--json",
        ]);
        assert_exit(
            &strict_fail,
            1,
            &format!("synthetic pending dev pins --check {gate_flag}"),
        );
        let strict_val = parse_stdout_json(&strict_fail);
        assert_eq!(strict_val["strictReleaseGate"], true);
        assert_eq!(strict_val["status"], "failed");
    }

    // 3b. `dev pins --refresh --dry-run`: zero writes to lockfile and zero VSIX cache writes!
    let pins_repo = sb.path("pins-repo");
    let root_for_pins = repo_root();
    fs::create_dir_all(pins_repo.join("catalog")).expect("mkdir pins catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root_for_pins.join(rel), pins_repo.join(rel)).expect("copy catalog file");
    }
    copy_tree(&root_for_pins.join("packs"), &pins_repo.join("packs"));
    let lock_path = pins_repo.join("vsp.lock.json");
    let lock_bytes_before = fs::read(&lock_path).expect("read lock before");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock pins server");
    listener
        .set_nonblocking(true)
        .expect("set nonblocking mock server");
    let addr = listener.local_addr().expect("addr");
    let stop = Arc::new(AtomicBool::new(false));
    let stop_srv = Arc::clone(&stop);
    let server = thread::spawn(move || {
        while !stop_srv.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let n = stream.read(&mut buf).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        if let Some(head_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header_str = String::from_utf8_lossy(&raw[..head_end]);
                            let cl = header_str
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
                        let payload = json!({
                            "results": [{
                                "extensions": [{
                                    "versions": [
                                        {
                                            "version": "1.2.3",
                                            "properties": [],
                                            "files": [{
                                                "assetType": "Microsoft.VisualStudio.Services.VSIXPackage",
                                                "source": format!("http://{addr}/mock.vsix")
                                            }]
                                        },
                                        {
                                            "version": "1.2.3",
                                            "targetPlatform": "darwin-arm64",
                                            "properties": [],
                                            "files": [{
                                                "assetType": "Microsoft.VisualStudio.Services.VSIXPackage",
                                                "source": format!("http://{addr}/mock.vsix")
                                            }]
                                        },
                                        {
                                            "version": "1.2.3",
                                            "targetPlatform": "linux-x64",
                                            "properties": [],
                                            "files": [{
                                                "assetType": "Microsoft.VisualStudio.Services.VSIXPackage",
                                                "source": format!("http://{addr}/mock.vsix")
                                            }]
                                        }
                                    ]
                                }]
                            }]
                        });
                        ("application/json", payload.to_string().into_bytes())
                    } else {
                        ("application/octet-stream", b"mock-vsix-payload".to_vec())
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(&body);
                }
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    });

    let pins_dry = sb
        .command()
        .env("VSP_MARKETPLACE_API_URL", format!("http://{addr}/query"))
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .args([
            "--catalog",
            pins_repo.to_str().expect("utf8"),
            "--dry-run",
            "--json",
            "dev",
            "pins",
            "--refresh",
        ])
        .output()
        .expect("dev pins --refresh --dry-run");
    stop.store(true, Ordering::Relaxed);
    let _ = server.join();
    assert_exit(&pins_dry, 0, "dev pins --refresh --dry-run");
    let pins_dry_val = parse_stdout_json(&pins_dry);
    assert_eq!(pins_dry_val["dryRun"], true);
    assert_eq!(
        fs::read(&lock_path).expect("read lock after"),
        lock_bytes_before,
        "dev pins --refresh --dry-run must not modify vsp.lock.json"
    );
    assert!(
        !sb.path("cache/vsp/vsix").exists(),
        "dev pins --refresh --dry-run must not write VSIX cache"
    );

    // 4. `dev docs --write --dry-run`: zero writes on drifted README.md.
    let root = repo_root();
    let docs_repo = sb.path("docs-repo");
    fs::create_dir_all(docs_repo.join("docs")).expect("mkdir docs");
    fs::create_dir_all(docs_repo.join("catalog")).expect("mkdir catalog");
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
        fs::copy(root.join(rel), docs_repo.join(rel)).expect("copy repo file");
    }
    copy_tree(&root.join("packs"), &docs_repo.join("packs"));
    let readme_path = docs_repo.join("README.md");
    let drifted = fs::read_to_string(&readme_path)
        .expect("read README")
        .replace(
            "<!-- BEGIN GENERATED-MATRIX -->",
            "<!-- BEGIN GENERATED-MATRIX -->\nSTALE_DRIFT_LINE",
        );
    fs::write(&readme_path, &drifted).expect("write drifted README");

    let docs_dry = sb.vsp(&[
        "--catalog",
        docs_repo.to_str().expect("utf8"),
        "--dry-run",
        "--json",
        "dev",
        "docs",
        "--write",
    ]);
    assert_exit(&docs_dry, 0, "dev docs --write --dry-run");
    let docs_val = parse_stdout_json(&docs_dry);
    assert_eq!(docs_val["dryRun"], true);
    assert!(docs_val["updated"]
        .as_array()
        .expect("updated")
        .iter()
        .any(|u| u == "README.md"));
    assert_eq!(
        fs::read_to_string(&readme_path).expect("re-read README"),
        drifted,
        "dev docs --write --dry-run must not mutate README.md"
    );

    // 5. `dev hooks --dry-run`: zero `git config core.hooksPath` mutation.
    let git_repo = sb.path("git-repo");
    fs::create_dir_all(git_repo.join("scripts/git-hooks")).expect("mkdir hooks");
    let init = Command::new("git")
        .args(["-C", git_repo.to_str().expect("utf8"), "init"])
        .output()
        .expect("git init");
    assert!(init.status.success());
    let hook = git_repo.join("scripts/git-hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 0\n").expect("write hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod hook");

    let hooks_dry = sb.vsp(&[
        "--catalog",
        git_repo.to_str().expect("utf8"),
        "--dry-run",
        "--json",
        "dev",
        "hooks",
    ]);
    assert_exit(&hooks_dry, 0, "dev hooks --dry-run");
    let hooks_val = parse_stdout_json(&hooks_dry);
    assert_eq!(hooks_val["dryRun"], true);
    let git_cfg = Command::new("git")
        .args([
            "-C",
            git_repo.to_str().expect("utf8"),
            "config",
            "--get",
            "core.hooksPath",
        ])
        .output()
        .expect("git config --get");
    assert!(
        !git_cfg.status.success(),
        "dev hooks --dry-run must not write core.hooksPath"
    );

    // 6. `self install --dry-run` & `self uninstall --dry-run`: zero symlink writes.
    let bin_dir = sb.path("self-bin");
    let bin_str = bin_dir.to_str().expect("utf8");
    let self_dry = sb.vsp(&["--dry-run", "self", "install", "--bin-dir", bin_str]);
    assert_exit(&self_dry, 0, "self install --dry-run");
    assert!(!bin_dir.exists());

    let self_real = sb.vsp(&["self", "install", "--bin-dir", bin_str]);
    assert_exit(&self_real, 0, "self install");
    assert!(bin_dir.join("vsp").is_symlink());

    let self_un_dry = sb.vsp(&["--dry-run", "self", "uninstall", "--bin-dir", bin_str]);
    assert_exit(&self_un_dry, 0, "self uninstall --dry-run");
    assert!(
        bin_dir.join("vsp").is_symlink(),
        "self uninstall --dry-run must not remove link"
    );
}

#[test]
fn broken_pipe_exits_0_with_empty_stderr_never_101() {
    let sb = Sandbox::new();

    // Test both large (`completions bash`) and JSON (`--json list packs`) outputs by closing
    // the read end of stdout immediately after spawning/reading 1 byte.
    for args in [
        vec!["completions", "bash"],
        vec!["--json", "list", "packs"],
        vec!["--json", "list", "presets"],
    ] {
        let mut child = sb
            .command()
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn vsp");

        // Read 1 byte (or drop immediately) and close stdout to trigger EPIPE in child.
        if let Some(mut out_pipe) = child.stdout.take() {
            let mut byte = [0u8; 1];
            let _ = out_pipe.read(&mut byte);
            drop(out_pipe);
        }

        let status = child.wait().expect("wait child");
        let mut err_buf = String::new();
        if let Some(mut err_pipe) = child.stderr.take() {
            let _ = err_pipe.read_to_string(&mut err_buf);
        }

        assert_eq!(
            status.code(),
            Some(0),
            "BrokenPipe on `vsp {}` must exit 0 (never 101), stderr={err_buf}",
            args.join(" ")
        );
        assert!(
            err_buf.is_empty(),
            "BrokenPipe on `vsp {}` must leave stderr empty, got: {err_buf}",
            args.join(" ")
        );
    }
}

#[test]
fn strict_json_consistency_and_structured_json_errors() {
    let sb = Sandbox::new();

    // 1. `vsp completions <shell> --json` rejected with exit 4 + JSON error on stderr + empty stdout.
    for shell in ["bash", "zsh", "fish"] {
        let out = sb.vsp(&["completions", shell, "--json"]);
        assert_exit(&out, 4, &format!("completions {shell} --json"));
        assert!(out.stdout.is_empty(), "stdout must be empty on error");
        let err = parse_stderr_json(&out);
        assert_eq!(err["error"]["code"], 4);
        assert_eq!(err["error"]["kind"], "usage_or_config");
        assert!(err["error"]["message"]
            .as_str()
            .expect("msg")
            .contains("--json is not supported with completions"));
    }

    // 2. `vsp dev test --json` emits valid JSON object, never plain text.
    let dev_test = sb.vsp(&["dev", "test", "--json"]);
    assert_exit(&dev_test, 0, "dev test --json");
    let dt_json = parse_stdout_json(&dev_test);
    assert_eq!(dt_json["rust_suite"], "cargo test --workspace");
    assert_eq!(dt_json["bats_suite"], "bash scripts/tests/run.sh");

    // 3. Structured JSON errors for target_missing (2), usage_or_config (4), safety_refusal (5),
    //    and clap parse error (4).
    let missing = sb.vsp(&["--json", "show", "nonexistent-item-xyz"]);
    assert_exit(&missing, 2, "show missing --json");
    assert!(missing.stdout.is_empty());
    let missing_err = parse_stderr_json(&missing);
    assert_eq!(missing_err["error"]["code"], 2);
    assert_eq!(missing_err["error"]["kind"], "target_missing");

    let clap_bad = sb.vsp(&["--json", "--definitely-invalid-flag"]);
    assert_exit(&clap_bad, 4, "unknown flag --json");
    assert!(clap_bad.stdout.is_empty());
    let clap_err = parse_stderr_json(&clap_bad);
    assert_eq!(clap_err["error"]["code"], 4);
    assert_eq!(clap_err["error"]["kind"], "usage_or_config");

    sb.write_default_config("{\"version\":1}\n");
    let clobber = sb.vsp(&["--json", "init"]);
    assert_exit(&clobber, 5, "init clobber --json");
    assert!(clobber.stdout.is_empty());
    let clobber_err = parse_stderr_json(&clobber);
    assert_eq!(clobber_err["error"]["code"], 5);
    assert_eq!(clobber_err["error"]["kind"], "safety_refusal");
}

#[test]
fn diff_validates_selectors_before_loading_config_or_catalog() {
    let sb = Sandbox::new();

    // Missing selector with missing --config must exit 4 (not 2!).
    let empty = sb.vsp(&["--config", "/nonexistent/missing.jsonc", "diff"]);
    assert_exit(&empty, 4, "diff with missing selector and missing config");
    assert!(
        stderr(&empty).contains("selector"),
        "expected selector hint, got: {}",
        stderr(&empty)
    );

    // Missing selector with missing --catalog must also exit 4 (not 2!).
    let empty_cat = sb.vsp(&["--catalog", "/nonexistent/catalog", "diff"]);
    assert_exit(
        &empty_cat,
        4,
        "diff with missing selector and missing catalog",
    );

    // Conflicting selectors with missing --config must exit 4 (not 2!).
    let conflict = sb.vsp(&[
        "--config",
        "/nonexistent/missing.jsonc",
        "diff",
        "--user",
        "-p",
        "rust",
    ]);
    assert_exit(
        &conflict,
        4,
        "diff with conflicting selectors and missing config",
    );

    // Conflicting --ai and --no-ai with missing --config must exit 4.
    let bad_ai = sb.vsp(&[
        "--config",
        "/nonexistent/missing.jsonc",
        "diff",
        "-p",
        "rust",
        "--ai",
        "full",
        "--no-ai",
    ]);
    assert_exit(
        &bad_ai,
        4,
        "diff with conflicting ai flags and missing config",
    );
}

#[test]
fn plan_extras_diff_and_sync_dry_run_parity_and_would_refuse_exit_5() {
    let sb = Sandbox::new();
    let data = sb.data_dir();

    // Pre-register `rust` profile with retired + unmanaged installed extensions.
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"rust1111"}]}"#,
    )
    .expect("seed storage");
    sb.install_code_stub(concat!(
        "#!/bin/sh\n",
        "printf '%s\\n' 'panicbit.cargo@0.1.0' 'ms-python.python@2026.1.0'\n",
        "exit 0\n",
    ));

    // Stage a temp catalog with one synthetic pending Tier 1 pin so refusal prediction is
    // tested deterministically even when the shipped lockfile has 0 pending entries.
    let pending_cat = sb.path("pending-plan-cat");
    let root = repo_root();
    fs::create_dir_all(pending_cat.join("catalog")).expect("mkdir catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), pending_cat.join(rel)).expect("copy cat file");
    }
    copy_tree(&root.join("packs"), &pending_cat.join("packs"));
    let mut pending_lock: Value = serde_json::from_str(
        &fs::read_to_string(pending_cat.join("vsp.lock.json")).expect("read lock"),
    )
    .expect("parse lock");
    pending_lock["extensions"]["anthropic.claude-code"] = json!({
        "tier": 1,
        "universal": false,
        "status": "pending_refresh",
        "version": null,
        "targets": {}
    });
    fs::write(
        pending_cat.join("vsp.lock.json"),
        serde_json::to_string_pretty(&pending_lock).expect("ser lock"),
    )
    .expect("write lock");
    let pending_cat_str = pending_cat.to_str().expect("utf8");

    // 0. Shipped embedded lockfile (0 pending Tier 1): both `diff` and `sync --dry-run` exit 0!
    let shipped_dry = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
        "--json",
    ]);
    assert_exit(&shipped_dry, 0, "shipped lock sync --dry-run -p rust");
    assert!(parse_stdout_json(&shipped_dry)
        .get("would_refuse")
        .is_none());

    // 1. Without `--allow-unlocked` against a pending Tier 1 pin:
    //    `diff` exits 0, `sync --dry-run` prints identical plan AND exits 5!
    let diff_strict = sb.vsp(&[
        "--catalog",
        pending_cat_str,
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "rust",
        "--json",
    ]);
    assert_exit(&diff_strict, 0, "diff -p rust strict");
    let sync_dry_strict = sb.vsp(&[
        "--catalog",
        pending_cat_str,
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
        "--json",
    ]);
    assert_exit(
        &sync_dry_strict,
        5,
        "sync --dry-run -p rust strict must exit 5",
    );
    assert_eq!(
        diff_strict.stdout, sync_dry_strict.stdout,
        "diff and sync --dry-run must emit identical plan JSON"
    );
    let plan_strict = parse_stdout_json(&sync_dry_strict);
    assert!(
        plan_strict["would_refuse"]
            .as_array()
            .is_some_and(|arr| !arr.is_empty()),
        "would_refuse must be populated: {plan_strict}"
    );

    // 2. With `--allow-unlocked`: BOTH `diff` and `sync --dry-run` exit 0 with identical output.
    let diff_unlocked = sb.vsp(&[
        "--catalog",
        pending_cat_str,
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "rust",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(&diff_unlocked, 0, "diff -p rust --allow-unlocked");
    let sync_dry_unlocked = sb.vsp(&[
        "--catalog",
        pending_cat_str,
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(
        &sync_dry_unlocked,
        0,
        "sync --dry-run -p rust --allow-unlocked",
    );
    assert_eq!(diff_unlocked.stdout, sync_dry_unlocked.stdout);
    assert!(parse_stdout_json(&sync_dry_unlocked)
        .get("would_refuse")
        .is_none());

    // 3. `--prune` (retired) and `--prune-mode unmanaged` work identically on `diff` and `sync --dry-run`.
    let diff_prune = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "rust",
        "--prune",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(&diff_prune, 0, "diff --prune");
    let sync_prune = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
        "--prune",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(&sync_prune, 0, "sync --dry-run --prune");
    assert_eq!(diff_prune.stdout, sync_prune.stdout);
    let prune_exts = parse_stdout_json(&diff_prune)["extension_changes"]
        .as_array()
        .expect("extension_changes")
        .clone();
    assert!(prune_exts
        .iter()
        .any(|c| c["id"] == "panicbit.cargo" && c["action"] == "uninstall"));
    assert!(!prune_exts
        .iter()
        .any(|c| c["id"] == "ms-python.python" && c["action"] == "uninstall"));

    let diff_unmanaged = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "rust",
        "--prune-mode",
        "unmanaged",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(&diff_unmanaged, 0, "diff --prune-mode unmanaged");
    let sync_unmanaged = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "sync",
        "-p",
        "rust",
        "--prune-mode",
        "unmanaged",
        "--allow-unlocked",
        "--json",
    ]);
    assert_exit(&sync_unmanaged, 0, "sync --dry-run --prune-mode unmanaged");
    assert_eq!(diff_unmanaged.stdout, sync_unmanaged.stdout);
    let unm_exts = parse_stdout_json(&diff_unmanaged)["extension_changes"]
        .as_array()
        .expect("extension_changes")
        .clone();
    assert!(unm_exts
        .iter()
        .any(|c| c["id"] == "ms-python.python" && c["action"] == "uninstall"));
}

#[test]
fn dedicated_use_verb_wins_over_same_named_directory_and_appears_in_completions() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    fs::create_dir_all(sb.path("cwd/shell")).expect("mkdir cwd/shell");
    fs::create_dir_all(sb.path("cwd/my-app")).expect("mkdir cwd/my-app");

    // 1. `vsp use shell` inside cwd containing `./shell` opens profile `shell`, NOT path `./shell`.
    let use_shell = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "--json",
        "use",
        "shell",
    ]);
    assert_exit(&use_shell, 0, "vsp use shell");
    let shell_val = parse_stdout_json(&use_shell);
    assert_eq!(shell_val["profile"], "shell");
    assert!(shell_val["path"].is_null());
    let cmd_arr = shell_val["command"].as_array().expect("command");
    assert!(cmd_arr.iter().any(|a| a == "--profile=shell"));
    assert!(!cmd_arr.iter().any(|a| a == "--" || a == "shell"));

    // 2. `vsp use rust ./my-app` opens profile `rust` with workspace `./my-app`.
    let use_both = sb.vsp(&[
        "--vscode-data-dir",
        &data,
        "--dry-run",
        "--json",
        "use",
        "rust",
        "my-app",
    ]);
    assert_exit(&use_both, 0, "vsp use rust my-app");
    let both_val = parse_stdout_json(&use_both);
    assert_eq!(both_val["profile"], "rust");
    assert_eq!(both_val["path"], "my-app");

    // 3. `vsp open rust` (nonexistent `./rust`) suggests `vsp use rust`.
    let open_hint = sb.vsp(&["--vscode-data-dir", &data, "--dry-run", "open", "rust"]);
    assert_exit(&open_hint, 2, "open rust hint");
    assert!(
        stderr(&open_hint).contains("vsp use rust"),
        "expected vsp use hint: {}",
        stderr(&open_hint)
    );

    // 4. Shell completions include `use` subcommand.
    for shell in ["bash", "zsh", "fish"] {
        let comp = sb.vsp(&["completions", shell]);
        assert_exit(&comp, 0, &format!("completions {shell}"));
        assert!(
            stdout(&comp).contains("use"),
            "{shell} completions must include `use`"
        );
    }
}

#[test]
fn native_sync_extensions_only_is_exposed_and_leaves_settings_untouched() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    let pins = sb.install_empty_pins();
    let code_log = sb.path("code.log");
    sb.install_code_stub(&format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n",
        code_log.display()
    ));

    // Exposed in `--help` (no longer hidden).
    let help = sb.vsp(&["sync", "--help"]);
    assert_exit(&help, 0, "sync --help");
    assert!(
        stdout(&help).contains("--extensions-only"),
        "sync --help must expose --extensions-only"
    );

    // Initial full sync creates the profile.
    let init = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", &pins)
        .args(["--vscode-data-dir", &data, "sync", "-p", "rust", "--no-rtk"])
        .output()
        .expect("initial sync");
    assert_exit(&init, 0, "initial sync");

    let settings_path = fs::read_dir(sb.path("data/User/profiles"))
        .expect("profiles")
        .flatten()
        .map(|e| e.path().join("settings.json"))
        .find(|p| p.is_file())
        .expect("profile settings");
    let custom = "{\n  \"user.customOnly\": true\n}\n";
    fs::write(&settings_path, custom).expect("edit settings");

    // Dry-run preview with --extensions-only clears settings_changes.
    let dry = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", &pins)
        .args([
            "--vscode-data-dir",
            &data,
            "--dry-run",
            "--json",
            "sync",
            "-p",
            "rust",
            "--extensions-only",
        ])
        .output()
        .expect("dry extensions-only");
    assert_exit(&dry, 0, "sync --dry-run --extensions-only");
    let dry_val = parse_stdout_json(&dry);
    assert!(dry_val["settings_changes"]
        .as_array()
        .expect("settings_changes")
        .is_empty());
    assert!(!dry_val["extension_changes"]
        .as_array()
        .expect("extension_changes")
        .is_empty());

    // Mutating native `vsp sync -p rust --extensions-only` leaves settings.json untouched.
    let ext_only = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", &pins)
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "rust",
            "--extensions-only",
        ])
        .output()
        .expect("sync --extensions-only");
    assert_exit(&ext_only, 0, "sync --extensions-only");
    assert_eq!(fs::read_to_string(&settings_path).expect("read"), custom);

    // Refused on workspace / --user even under --dry-run (exit 4).
    fs::write(sb.path("cwd/Cargo.toml"), "[package]\nname=\"demo\"\n").expect("cargo");
    assert_exit(
        &sb.vsp(&["--dry-run", "sync", ".", "--extensions-only"]),
        4,
        "dry-run workspace --extensions-only",
    );
    assert_exit(
        &sb.vsp(&["--dry-run", "sync", "--user", "--extensions-only"]),
        4,
        "dry-run user --extensions-only",
    );
}

#[test]
fn verbose_logging_hidden_dev_verbs_help_headings_and_selector_hints() {
    let sb = Sandbox::new();
    let data = sb.data_dir();

    // 1. `-v/--verbose` emits `[vsp] ...` diagnostics to stderr without polluting JSON stdout.
    let verbose = sb.vsp(&["-v", "--json", "list", "presets"]);
    assert_exit(&verbose, 0, "-v --json list presets");
    assert!(parse_stdout_json(&verbose).is_array());
    let err = stderr(&verbose);
    assert!(err.contains("[vsp] catalog: embedded"), "{err}");
    assert!(err.contains("[vsp] config:"), "{err}");
    assert!(err.contains("[vsp] target:"), "{err}");
    assert!(err.contains("[vsp] done: exit=0 elapsed="), "{err}");

    // 2. Stubbed `dev test` and `dev release` hidden from `dev --help`, and `--marketplace-only`
    //    hidden from `dev compat --help`, while `dev test --json` still runs.
    let dev_help = sb.vsp(&["dev", "--help"]);
    assert_exit(&dev_help, 0, "dev --help");
    let dh = stdout(&dev_help);
    assert!(dh.contains("validate") && dh.contains("dist") && dh.contains("pins"));
    assert!(!dh.contains("\n  test"), "dev test must be hidden:\n{dh}");
    assert!(
        !dh.contains("\n  release"),
        "dev release must be hidden:\n{dh}"
    );
    let compat_help = sb.vsp(&["dev", "compat", "--help"]);
    assert_exit(&compat_help, 0, "dev compat --help");
    assert!(!stdout(&compat_help).contains("--marketplace-only"));

    // 3. Help headings & copyable examples on sync, diff, open, use, doctor.
    for sub in ["sync", "diff", "open", "use", "doctor"] {
        let help = sb.vsp(&[sub, "--help"]);
        assert_exit(&help, 0, &format!("{sub} --help"));
        let text = stdout(&help);
        assert!(
            text.contains("Examples:"),
            "{sub} --help missing Examples section:\n{text}"
        );
        assert!(
            text.contains("Output & Runtime") && text.contains("Advanced / Overrides"),
            "{sub} --help missing global help headings:\n{text}"
        );
    }
    let sync_help = stdout(&sb.vsp(&["sync", "--help"]));
    assert!(sync_help.contains("Target Selectors"));
    assert!(sync_help.contains("Composition"));
    assert!(sync_help.contains("Safety & Supply Chain"));
    assert!(sync_help.contains("-p my-rust --preset rust"));
    assert!(sync_help.contains("Exit codes:"));

    let diff_help = stdout(&sb.vsp(&["diff", "--help"]));
    assert!(diff_help.contains("3  ") && diff_help.contains("7  "));

    // 4. Actionable selector hints.
    let sync_pos_profile = sb.vsp(&["sync", "rust"]);
    assert_exit(&sync_pos_profile, 2, "vsp sync rust");
    assert!(
        stderr(&sync_pos_profile).contains("vsp sync -p rust"),
        "{}",
        stderr(&sync_pos_profile)
    );

    let diff_pos_profile = sb.vsp(&["diff", "rust"]);
    assert_exit(&diff_pos_profile, 2, "vsp diff rust");
    assert!(
        stderr(&diff_pos_profile).contains("vsp diff -p rust"),
        "{}",
        stderr(&diff_pos_profile)
    );

    let sync_unknown_prof = sb.vsp(&["--vscode-data-dir", &data, "sync", "-p", "custom-missing"]);
    assert_exit(&sync_unknown_prof, 2, "vsp sync -p custom-missing");
    assert!(
        stderr(&sync_unknown_prof).contains("vsp sync -p custom-missing --preset"),
        "{}",
        stderr(&sync_unknown_prof)
    );

    let diff_unknown_prof = sb.vsp(&["--vscode-data-dir", &data, "diff", "-p", "custom-missing"]);
    assert_exit(&diff_unknown_prof, 2, "vsp diff -p custom-missing");
    assert!(
        stderr(&diff_unknown_prof).contains("vsp diff -p custom-missing --preset"),
        "{}",
        stderr(&diff_unknown_prof)
    );
}

#[test]
fn human_readable_diff_list_and_show_ux() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    sb.write_default_config(
        r#"{"version":1,"profiles":{"my-rust":{"description":"My custom Rust","extends":"rust"}}}"#,
    );

    // 1. `print_plan`: lowercase verbs, `- current` / `+ desired`, `[lock_status]`,
    //    `Would refuse`, summary line at the bottom, zero ANSI escape codes when piped.
    let ws = sb.path("ws");
    fs::create_dir_all(ws.join(".vscode")).expect("mkdir .vscode");
    fs::write(ws.join("Cargo.toml"), "[package]\nname=\"demo\"\n").expect("cargo");
    fs::write(
        ws.join(".vscode/settings.json"),
        "{\"editor.tabSize\": 8}\n",
    )
    .expect("settings");

    let diff_ws = sb.vsp(&["diff", ws.to_str().expect("utf8")]);
    assert_exit(&diff_ws, 0, "diff workspace text");
    let ws_out = stdout(&diff_ws);
    assert!(ws_out.contains("target=workspace"), "{ws_out}");
    assert!(
        ws_out.contains("setting update: editor.tabSize"),
        "{ws_out}"
    );
    assert!(ws_out.contains("- editor.tabSize: 8"), "{ws_out}");
    assert!(ws_out.contains("+ editor.tabSize: 2"), "{ws_out}");
    assert!(
        !ws_out.contains("\x1b["),
        "piped output must have no ANSI color"
    );
    assert!(
        ws_out
            .trim_end()
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("Summary:")),
        "Summary line must be at the bottom:\n{ws_out}"
    );

    let diff_prof = sb.vsp(&["--vscode-data-dir", &data, "diff", "-p", "rust"]);
    assert_exit(&diff_prof, 0, "diff profile text");
    let prof_out = stdout(&diff_prof);
    assert!(prof_out.contains("target=profile"), "{prof_out}");
    assert!(
        prof_out.contains("extension ensure_installed:"),
        "{prof_out}"
    );
    assert!(prof_out.contains("[verified]"), "{prof_out}");
    assert!(prof_out.contains("[version]"), "{prof_out}");
    assert!(
        prof_out
            .trim_end()
            .lines()
            .last()
            .is_some_and(|l| l.starts_with("Summary:")),
        "Summary line must be at the bottom:\n{prof_out}"
    );

    // Verify `Would refuse (` and `[pending]` rendering against a synthetic pending Tier 1 catalog.
    let pending_ux_cat = sb.path("pending-ux-cat");
    let root = repo_root();
    fs::create_dir_all(pending_ux_cat.join("catalog")).expect("mkdir ux cat");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), pending_ux_cat.join(rel)).expect("copy ux file");
    }
    copy_tree(&root.join("packs"), &pending_ux_cat.join("packs"));
    let mut ux_lock: Value = serde_json::from_str(
        &fs::read_to_string(pending_ux_cat.join("vsp.lock.json")).expect("read ux lock"),
    )
    .expect("parse ux lock");
    ux_lock["extensions"]["anthropic.claude-code"] = json!({
        "tier": 1,
        "universal": false,
        "status": "pending_refresh",
        "version": null,
        "targets": {}
    });
    fs::write(
        pending_ux_cat.join("vsp.lock.json"),
        serde_json::to_string_pretty(&ux_lock).expect("ser ux lock"),
    )
    .expect("write ux lock");
    let diff_prof_pending = sb.vsp(&[
        "--catalog",
        pending_ux_cat.to_str().expect("utf8"),
        "--vscode-data-dir",
        &data,
        "diff",
        "-p",
        "rust",
    ]);
    assert_exit(&diff_prof_pending, 0, "diff profile pending text");
    let pending_prof_out = stdout(&diff_prof_pending);
    assert!(pending_prof_out.contains("[pending]"), "{pending_prof_out}");
    assert!(
        pending_prof_out.contains("Would refuse ("),
        "{pending_prof_out}"
    );

    // 2. `vsp list`: bare names when piped, aligned badges/descriptions with `-l/--long`.
    let list_bare = sb.vsp(&["list"]);
    assert_exit(&list_bare, 0, "list piped");
    assert!(stdout(&list_bare)
        .lines()
        .all(|l| !l.contains(' ') && !l.contains('\t')));
    assert!(stdout(&list_bare).lines().any(|l| l == "my-rust"));

    let list_long = sb.vsp(&["list", "-l"]);
    assert_exit(&list_long, 0, "list -l");
    let long_out = stdout(&list_long);
    assert!(long_out.contains("[preset]"), "{long_out}");
    assert!(long_out.contains("[user]"), "{long_out}");
    assert!(long_out.contains("My custom Rust"), "{long_out}");

    let packs_bare = sb.vsp(&["list", "packs"]);
    assert_exit(&packs_bare, 0, "list packs piped");
    assert!(stdout(&packs_bare)
        .lines()
        .all(|l| !l.contains(' ') && !l.contains('\t')));
    assert!(stdout(&packs_bare).lines().any(|l| l == "rust"));

    let packs_long = sb.vsp(&["list", "packs", "--long"]);
    assert_exit(&packs_long, 0, "list packs --long");
    assert!(stdout(&packs_long).contains("[toolchain]"));
    assert!(stdout(&packs_long).contains("[capability]"));

    // 3. `vsp show --pack` / `pack:<id>` / `--preset` / `preset:<name>` & pack extensions in text.
    for args in [
        vec!["show", "--pack", "rust"],
        vec!["show", "pack:rust"],
        vec!["show", "--pack", "shell"],
        vec!["show", "pack:shell"],
    ] {
        let out = sb.vsp(&args);
        assert_exit(&out, 0, &format!("vsp {}", args.join(" ")));
        let txt = stdout(&out);
        assert!(txt.starts_with("Pack: "), "expected Pack output:\n{txt}");
        assert!(
            txt.contains("Workspace extensions (") || txt.contains("Profile extensions ("),
            "pack text mode must display extension IDs:\n{txt}"
        );
    }
    assert!(stdout(&sb.vsp(&["show", "--pack", "rust"])).contains("rust-lang.rust-analyzer"));
    assert!(stdout(&sb.vsp(&["show", "--pack", "shell"])).contains("timonwong.shellcheck"));

    let show_preset = sb.vsp(&["show", "--preset", "rust"]);
    assert_exit(&show_preset, 0, "show --preset rust");
    assert!(stdout(&show_preset).starts_with("Preset: rust"));

    let show_prefix_preset = sb.vsp(&["show", "preset:biome"]);
    assert_exit(&show_prefix_preset, 0, "show preset:biome");
    assert!(stdout(&show_prefix_preset).starts_with("Preset: biome"));

    let show_conflict = sb.vsp(&["show", "--pack", "preset:rust"]);
    assert_exit(&show_conflict, 4, "show --pack preset:rust conflict");
}

#[test]
fn init_rejects_composition_flags_on_reserved_preset_without_apply_but_preserves_apply() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    let pins = sb.install_empty_pins();
    sb.install_code_stub("#!/bin/sh\nexit 0\n");

    let cfg_path = sb.path("init-guard.jsonc");
    let cfg_str = cfg_path.to_str().expect("utf8");

    for args in [
        vec![
            "--config", cfg_str, "init", "-p", "rust", "--with", "git-pro",
        ],
        vec![
            "--config", cfg_str, "init", "-p", "rust", "--preset", "general",
        ],
        vec![
            "--config",
            cfg_str,
            "init",
            "-p",
            "rust",
            "--without",
            "git-pro",
        ],
        vec![
            "--config",
            cfg_str,
            "init",
            "-p",
            "rust-profile-crisp",
            "--with",
            "git-pro",
        ],
    ] {
        let out = sb.vsp(&args);
        assert_exit(&out, 4, &format!("vsp {}", args.join(" ")));
        assert!(
            stderr(&out).contains("custom -p"),
            "expected custom -p guidance: {}",
            stderr(&out)
        );
        assert!(!cfg_path.exists(), "rejected init must not write config");
    }

    // With `--apply`, targeting built-in preset `rust` with `--with git-pro` is preserved!
    let applied = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", &pins)
        .args([
            "--config",
            cfg_str,
            "--vscode-data-dir",
            &data,
            "init",
            "--apply",
            "-p",
            "rust",
            "--with",
            "git-pro",
            "--no-rtk",
        ])
        .output()
        .expect("init --apply");
    assert_exit(&applied, 0, "init --apply -p rust --with git-pro");
    assert!(cfg_path.is_file());
}

#[test]
fn scoped_user_config_and_clean_install_doctor_strict() {
    let sb = Sandbox::new();
    let data = sb.data_dir();
    sb.install_code_stub("#!/bin/sh\nexit 0\n");

    // 1. Clean install (`CARBON-03`): `vsp sync --user` followed by `vsp doctor --strict` exits 0!
    let sync_user = sb.vsp(&["--vscode-data-dir", &data, "sync", "--user"]);
    assert_exit(&sync_user, 0, "vsp sync --user");
    let doc_clean = sb.vsp(&["--vscode-data-dir", &data, "doctor", "--strict"]);
    assert_exit(&doc_clean, 0, "vsp doctor --strict on clean install");

    // 2. When user config explicitly enables `defaults.rtkTerminal: true` and wrappers are absent,
    //    `doctor --strict` exits 1 with an RTK warning!
    sb.write_default_config(r#"{"version":1,"defaults":{"rtkTerminal":true}}"#);
    let doc_rtk_warn = sb.vsp(&["--vscode-data-dir", &data, "doctor", "--strict"]);
    assert_exit(
        &doc_rtk_warn,
        1,
        "doctor --strict with rtkTerminal=true and no wrappers",
    );
    assert!(stdout(&doc_rtk_warn).contains("WARN:"));
    assert!(stdout(&doc_rtk_warn).contains("RTK"));

    // 3. Scoped config (`SOL-06`): break default `~/.config/vsp/config.jsonc` with invalid JSONC.
    sb.write_default_config("{ malformed jsonc !!!");
    let ws = sb.path("ws");
    fs::create_dir_all(&ws).expect("mkdir ws");
    fs::write(ws.join("Cargo.toml"), "[package]\nname=\"demo\"\n").expect("cargo");
    let ws_str = ws.to_str().expect("utf8");

    // Catalog-only & dev commands skip default config cleanly (exit 0).
    for args in [
        vec!["list", "packs"],
        vec!["list", "ux"],
        vec!["dev", "validate"],
        vec!["dev", "pins", "--check"],
    ] {
        let out = sb.vsp(&args);
        assert_exit(&out, 0, &format!("skip default on vsp {}", args.join(" ")));
    }

    // Built-in `show` and workspace `diff --workspace . --preset <builtin>` succeed via fallback.
    assert_exit(
        &sb.vsp(&["show", "rust"]),
        0,
        "show builtin rust with broken default",
    );
    assert_exit(
        &sb.vsp(&["show", "--pack", "rust"]),
        0,
        "show --pack rust with broken default",
    );
    assert_exit(
        &sb.vsp(&["diff", "--workspace", ws_str, "--preset", "rust"]),
        0,
        "workspace diff --preset rust with broken default",
    );

    // Custom profile lookup MUST fail closed (exit 4, not 2!).
    assert_exit(
        &sb.vsp(&["show", "my-custom-profile"]),
        4,
        "show custom profile with broken default must fail closed",
    );
    assert_exit(
        &sb.vsp(&[
            "diff",
            "--workspace",
            ws_str,
            "--preset",
            "my-custom-profile",
        ]),
        4,
        "workspace diff custom preset with broken default must fail closed",
    );

    // Explicit `--config PATH` ALWAYS fails closed across every command!
    let bad_explicit = sb.write_config_file("bad-explicit.jsonc", "{ broken");
    let bad_str = bad_explicit.to_str().expect("utf8");
    for args in [
        vec!["--config", bad_str, "list", "packs"],
        vec!["--config", bad_str, "list", "ux"],
        vec!["--config", bad_str, "show", "rust"],
        vec!["--config", bad_str, "show", "--pack", "rust"],
        vec!["--config", bad_str, "dev", "validate"],
        vec![
            "--config",
            bad_str,
            "diff",
            "--workspace",
            ws_str,
            "--preset",
            "rust",
        ],
    ] {
        assert_exit(
            &sb.vsp(&args),
            4,
            &format!("explicit bad --config on vsp {}", args.join(" ")),
        );
    }

    // `doctor` still reports malformed default user config as FAIL while running other checks.
    let doc_broken = sb.vsp(&["--vscode-data-dir", &data, "doctor"]);
    assert_exit(&doc_broken, 1, "doctor with malformed default config");
    assert!(stdout(&doc_broken).contains("FAIL: user config"));
    assert!(stdout(&doc_broken).contains("PASS: catalog"));
}

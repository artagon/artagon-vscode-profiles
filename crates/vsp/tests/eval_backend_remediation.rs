//! Wave 2A Backend Remediation regression suite covering:
//! - FABLE-01: unregistered profile sync skips `code --list-extensions` (realistic VS Code stub)
//! - OPUS-02 / CARBON-02 / ASTRA-08: pre-write VSIX refusal exits 5 with zero mutations;
//!   mid-apply external `code` failures exit 6 (`PartialApply`) with applied-vs-failed summary
//! - OPUS-04 / CARBON-01: selective `--allow-unlocked` still verifies pinned SHA-256 digests
//! - OPUS-01 / CARBON-01: Tier 2 `pending_refresh` warns, records provenance, and fails `doctor --strict`
//! - OPUS-03: `ARTAGON_VSCODE_PINS_FILE` override warns, sticks in provenance, and fails `doctor --strict`
//! - OPUS-05 / OPUS-07 / ASTRA-10: grouped multi-line refusal diagnostics & audience-aware guidance
//! - OPUS-11 / FABLE-12 / CARBON-08: unified fail-closed lockfile parsing & pending counts in `doctor`
//! - CARBON-03: `doctor --strict` passes out of the box without optional RTK wrappers, warns when enabled
//! - FABLE-07 / FABLE-08 / OPUS-09: subprocess timeout (exit 7), stderr capture, batched uninstall,
//!   smart re-list missing-only install fallback, and parallelized `doctor` scan
//! - CARBON-05: declarative `retireOnSwitch` and `policy.defaultProfileExpectations`
//! - CARBON-09 / SOL-01 / SOL-08: `dev pins`/`docs`/`hooks` `dry_run`, per-platform Tier 1 + Tier 2
//!   refresh, XDG cache warming, `strict_release_gate`, and `Packs` matrix format

#![cfg(unix)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::Instant;

use serde_json::{json, Value};
use tempfile::TempDir;

use vsp::dev_ops::{
    run_dev_docs_with_options, run_dev_hooks_with_options, run_dev_pins_with_options,
    DevDocsOptions, DevHooksOptions, DevPinsOptions,
};
use vsp::doctor::{run_doctor, run_doctor_with_options, CheckSeverity};
use vsp::exit_codes::ExitCode;
use vsp::jsonc::parse_jsonc_value;
use vsp::provenance::{now_rfc3339_utc, ProfileProvenance};
use vsp::resolver::sha256_hex;
use vsp::schema_loader::{
    load_embedded, load_from_dir, DefaultProfileExpectation, LockEntry, LockEntryStatus,
};
use vsp::user_config::parse_user_config;

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

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir dst");
    for entry in fs::read_dir(src).expect("read src").flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else if entry.file_name() != ".DS_Store" {
            fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn copy_v2_catalog(dst: &Path) {
    let root = repo_root();
    fs::create_dir_all(dst.join("catalog")).expect("mkdir catalog");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), dst.join(rel)).expect("copy manifest");
    }
    copy_dir(&root.join("packs"), &dst.join("packs"));
}

struct Sandbox {
    root: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let root = TempDir::new().expect("sandbox");
        for dir in [
            "home",
            "config",
            "state",
            "cache",
            "bin",
            "cwd",
            "data/User/globalStorage",
        ] {
            fs::create_dir_all(root.path().join(dir)).expect("mkdir");
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
        .expect("seed empty pins");
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

    fn write_code_stub(&self, script: &str) {
        let stub = self.path("bin/code");
        fs::write(&stub, script).expect("write code stub");
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("chmod code stub");
    }

    fn write_clean_default_settings(&self) {
        fs::write(
            self.path("data/User/settings.json"),
            r#"{
  "telemetry.telemetryLevel": "off",
  "security.workspace.trust.untrustedFiles": "prompt",
  "extensions.autoUpdate": false
}
"#,
        )
        .expect("write clean Default settings");
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

    fn provenance(&self, profile: &str) -> Option<Value> {
        let dir = self.path("state/vsp/profiles");
        if !dir.is_dir() {
            return None;
        }
        for entry in fs::read_dir(&dir).ok()?.flatten() {
            let text = fs::read_to_string(entry.path()).ok()?;
            let val: Value = serde_json::from_str(&text).ok()?;
            if val["profile"] == profile {
                return Some(val);
            }
        }
        None
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// FABLE-01: Realistic `code` stub returning exit 1 `Profile '<name>' not found.` when the
/// profile is absent from `storage.json`. Fresh profile sync AND fresh bundle import must
/// succeed without calling `code --list-extensions` before registration.
#[test]
fn fable01_unregistered_profile_sync_succeeds_with_realistic_code_stub() {
    let sb = Sandbox::new();
    let data = sb.data();
    let storage = sb.path("data/User/globalStorage/storage.json");
    let log = sb.path("code.log");

    // Realistic VS Code stub: exits 1 "Profile '<name>' not found." if --list-extensions
    // targets a profile not listed in storage.json!
    let realistic_stub = format!(
        r#"#!/bin/sh
echo "$*" >> '{log}'
profile=""
is_list=0
for arg in "$@"; do
  case "$arg" in
    --profile=*) profile="${{arg#--profile=}}" ;;
    --list-extensions) is_list=1 ;;
  esac
done
if [ "$is_list" -eq 1 ] && [ -n "$profile" ]; then
  if ! /usr/bin/grep -q "\"name\":\"$profile\"" '{storage}' 2>/dev/null && ! /usr/bin/grep -q "\"name\": \"$profile\"" '{storage}' 2>/dev/null; then
    echo "Profile '$profile' not found." >&2
    exit 1
  fi
fi
exit 0
"#,
        log = log.display(),
        storage = storage.display(),
    );
    sb.write_code_stub(&realistic_stub);

    let out = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args(["--vscode-data-dir", &data, "sync", "-p", "rust", "--no-rtk"])
        .output()
        .expect("run sync");
    assert_eq!(
        out.status.code(),
        Some(0),
        "fresh sync on unregistered profile failed:\nstdout={}\nstderr={}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        !sb.code_log().contains("--list-extensions"),
        "sync must NOT invoke --list-extensions for an unregistered profile: {}",
        sb.code_log()
    );
    assert!(
        sb.code_log().contains("--install-extension"),
        "sync must install missing extensions after registering profile: {}",
        sb.code_log()
    );

    // Bundle import into an unregistered profile must also succeed without pre-registration list.
    sb.clear_code_log();
    let bundle = sb.path("fresh.code-profile");
    fs::write(
        &bundle,
        r#"{"name":"fresh-bundle","settings":{"editor.tabSize":2},"extensions":{"enabled":["editorconfig.editorconfig"]}}"#,
    )
    .expect("write bundle");
    let imp = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "--from",
            bundle.to_str().expect("utf8"),
        ])
        .output()
        .expect("run bundle import");
    assert_eq!(
        imp.status.code(),
        Some(0),
        "fresh bundle import failed:\nstderr={}",
        stderr(&imp)
    );
    assert!(!sb.code_log().contains("--list-extensions"));
}

/// OPUS-02 / CARBON-02 / ASTRA-08 / OPUS-05 / OPUS-07 / ASTRA-10:
/// - Pre-write VSIX safety refusal returns exit 5 with ZERO mutations to storage.json,
///   extensions, settings.json, RTK wrappers, or provenance, and emits grouped multi-line
///   audience-aware guidance (embedded vs external `--catalog <DIR>`).
/// - Mid-apply `code` failure after mutations began returns exit 6 (`PartialApply`) with
///   explicit applied-vs-failed summary preserving child exit code and stderr.
#[test]
fn opus02_pre_write_vsix_refusal_zero_mutations_and_mid_apply_exit_6() {
    let sb = Sandbox::new();
    let data = sb.data();
    let storage = sb.path("data/User/globalStorage/storage.json");
    let storage_before = fs::read_to_string(&storage).expect("storage");
    sb.write_code_stub(&format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n",
        sb.path("code.log").display()
    ));

    // 1. Embedded catalog refusal -> exit 5, ZERO mutations, end-user guidance!
    let embedded_refusal = sb
        .command()
        .args(["--vscode-data-dir", &data, "sync", "-p", "rust"])
        .output()
        .expect("embedded sync");
    assert_eq!(embedded_refusal.status.code(), Some(5));
    assert_eq!(
        fs::read_to_string(&storage).expect("storage after"),
        storage_before,
        "storage.json must be untouched on pre-write exit 5"
    );
    assert!(!sb.path("data/User/profiles").exists());
    assert!(!sb.path("state/vsp/profiles").exists());
    assert!(!sb.path("config/vsp/rtk").exists());
    assert_eq!(
        sb.code_log(),
        "",
        "zero code invocations on pre-write exit 5"
    );

    let emb_err = stderr(&embedded_refusal);
    assert!(
        emb_err.contains("Tier 1 hash lock pending_refresh"),
        "{emb_err}"
    );
    assert!(emb_err.contains("    - anthropic.claude-code"), "{emb_err}");
    assert!(
        emb_err
            .contains("This release ships unverified Tier 1 lock entries; pass `--allow-unlocked`"),
        "embedded catalog must emit end-user remedy: {emb_err}"
    );
    assert!(
        !emb_err.contains("dev pins --refresh"),
        "embedded catalog must not tell end users to run dev pins --refresh: {emb_err}"
    );

    // 2. External `--catalog <DIR>` refusal -> exit 5, ZERO mutations, maintainer guidance!
    let ext_cat = sb.path("ext-catalog");
    copy_v2_catalog(&ext_cat);
    let ext_cat_str = ext_cat.to_str().expect("utf8");
    let ext_refusal = sb
        .command()
        .args([
            "--catalog",
            ext_cat_str,
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "rust",
        ])
        .output()
        .expect("external catalog sync");
    assert_eq!(ext_refusal.status.code(), Some(5));
    assert_eq!(
        fs::read_to_string(&storage).expect("storage after ext"),
        storage_before
    );
    let ext_err = stderr(&ext_refusal);
    assert!(
        ext_err.contains("dev pins --refresh") && ext_err.contains("retry with `--catalog"),
        "external catalog must emit maintainer remedy: {ext_err}"
    );

    // 3. Mid-apply external `code --install-extension` failure -> exit 6 (PartialApply) with
    //    explicit applied-vs-failed summary and child exit code + stderr preserved!
    sb.write_code_stub(&format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$*\" in\n  *--install-extension*) echo 'marketplace quota exceeded' >&2; exit 42 ;;\nesac\nexit 0\n",
        sb.path("code.log").display()
    ));
    let partial = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "general-minimal",
            "--no-rtk",
        ])
        .output()
        .expect("mid-apply fail");
    assert_eq!(
        partial.status.code(),
        Some(6),
        "mid-apply code failure must exit 6 (PartialApply):\nstderr={}",
        stderr(&partial)
    );
    let perr = stderr(&partial);
    assert!(perr.contains("partial apply"), "{perr}");
    assert!(perr.contains("Applied:"), "{perr}");
    assert!(perr.contains("Failed:"), "{perr}");
    assert!(
        perr.contains("exit=42: marketplace quota exceeded"),
        "must preserve child exit code and trimmed stderr: {perr}"
    );
}

/// OPUS-04 / CARBON-01: `--allow-unlocked` is selective — it STILL verifies SHA-256 for
/// verified Tier 1 pins (failing closed on mismatch) while relaxing only `pending_refresh`
/// / missing-target entries.
#[test]
fn opus04_selective_allow_unlocked_verifies_pinned_sha256_and_relaxes_pending_only() {
    let sb = Sandbox::new();
    let fixture_cat = sb.path("cat");
    copy_v2_catalog(&fixture_cat);

    let vsix_file = sb.path("editorconfig.vsix");
    let vsix_bytes = b"verified-vsix-payload-123";
    fs::write(&vsix_file, vsix_bytes).expect("write vsix");
    let good_sha = sha256_hex(vsix_bytes);
    let bad_sha = "f".repeat(64);
    let data = sb.data();
    sb.write_code_stub(&format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nexit 0\n",
        sb.path("code.log").display()
    ));

    // 1. Even with `allow_unlocked = true`, a tampered verified pin SHA-256 MUST fail closed
    //    (exit 5, zero mutations)!
    let bad_pins = sb.path("bad-pins.json");
    fs::write(
        &bad_pins,
        json!({
            "version": 1,
            "pinRequired": false,
            "pins": {
                "editorconfig.editorconfig": {
                    "version": "0.18.2",
                    "sha256": bad_sha,
                    "vsix_url": format!("file://{}", vsix_file.display()),
                    "verified_at": "2026-11-25T00:00:00Z"
                }
            }
        })
        .to_string(),
    )
    .expect("write bad pins");
    let tampered = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", &bad_pins)
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "general-minimal",
            "--allow-unlocked",
            "--no-rtk",
        ])
        .output()
        .expect("tampered sync");
    assert_eq!(
        tampered.status.code(),
        Some(5),
        "tampered verified Tier 1 digest must STILL exit 5 under --allow-unlocked:\nstderr={}",
        stderr(&tampered)
    );
    assert!(
        stderr(&tampered).contains("sha256 mismatch"),
        "{}",
        stderr(&tampered)
    );
    assert!(!sb.path("data/User/profiles").exists());

    // 2. Against v2.2 `vsp.lock.json`: with a verified Tier 1 entry (`editorconfig.editorconfig`)
    //    backed by the XDG VSIX cache and remaining Tier 1 entries marked `pending_refresh`,
    //    `--allow-unlocked` STILL verifies the pinned SHA-256 on disk and passes the verified
    //    `.vsix` path to `code`, relaxing ONLY the pending entries!
    let lock_path = fixture_cat.join("vsp.lock.json");
    let mut lock: Value =
        serde_json::from_str(&fs::read_to_string(&lock_path).expect("read lock")).expect("json");
    let real_url = "https://EditorConfig.gallerycdn.vsassets.io/extensions/editorconfig/editorconfig/0.18.2/1/Microsoft.VisualStudio.Services.VSIXPackage";
    for (id, entry) in lock["extensions"].as_object_mut().expect("extensions") {
        if entry["tier"] == 1 {
            if id == "editorconfig.editorconfig" {
                *entry = json!({
                    "tier": 1,
                    "universal": true,
                    "status": "verified",
                    "version": "0.18.2",
                    "sha256": good_sha,
                    "vsixUrl": real_url,
                    "verifiedAt": "2026-11-25T00:00:00Z",
                    "targets": {
                        "universal": {
                            "sha256": good_sha,
                            "vsixUrl": real_url
                        }
                    }
                });
            } else {
                let is_univ = entry["universal"].as_bool().unwrap_or(true);
                *entry = json!({
                    "tier": 1,
                    "universal": is_univ,
                    "status": "pending_refresh",
                    "targets": {}
                });
            }
        }
    }
    fs::write(&lock_path, lock.to_string()).expect("write selective lock");
    let cache_dir = sb.path("cache/vsp/vsix");
    fs::create_dir_all(&cache_dir).expect("mkdir cache");
    let cached_vsix = cache_dir.join(format!("editorconfig.editorconfig-{good_sha}.vsix"));
    fs::write(&cached_vsix, vsix_bytes).expect("seed cached vsix");

    let ok = sb
        .command()
        .args([
            "--catalog",
            fixture_cat.to_str().expect("utf8"),
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "general-minimal",
            "--allow-unlocked",
            "--no-rtk",
        ])
        .output()
        .expect("selective allow-unlocked sync");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "selective --allow-unlocked failed:\nstderr={}",
        stderr(&ok)
    );
    assert!(
        sb.code_log().contains(&cached_vsix.display().to_string()),
        "verified Tier 1 pin must install from verified VSIX path: {}",
        sb.code_log()
    );
    assert!(
        stderr(&ok).contains("--allow-unlocked relaxed"),
        "expected selective bypass warning on stderr: {}",
        stderr(&ok)
    );
    let prov = sb.provenance("general-minimal").expect("provenance");
    assert_eq!(prov["unlockedBypass"], true);
}

/// OPUS-01 / CARBON-01: Tier 2 `pending_refresh` / `version: null` emits a stderr WARN,
/// records `tier2Unpinned` in provenance, and triggers `WARN` in `doctor` (failing `--strict`).
#[test]
fn opus01_tier2_unpinned_warns_records_provenance_and_fails_doctor_strict() {
    let sb = Sandbox::new();
    let fixture_cat = sb.path("cat");
    copy_v2_catalog(&fixture_cat);

    let vsix_bytes = b"valid-tier1-vsix";
    let sha = sha256_hex(vsix_bytes);
    let cache_dir = sb.path("cache/vsp/vsix");
    fs::create_dir_all(&cache_dir).expect("mkdir cache");

    // Verify ALL Tier 1 entries with valid Marketplace URLs + pre-warmed XDG cache files so
    // `sync -p rust` succeeds cleanly WITHOUT `--allow-unlocked` and WITHOUT network access,
    // while leaving Tier 2 entries (`rust-lang.rust-analyzer`, `vadimcn.vscode-lldb`, etc.) as
    // `pending_refresh`!
    let lock_path = fixture_cat.join("vsp.lock.json");
    let mut lock: Value =
        serde_json::from_str(&fs::read_to_string(&lock_path).expect("read lock")).expect("json");
    let all_platforms = [
        "darwin-arm64",
        "darwin-x64",
        "linux-x64",
        "linux-arm64",
        "win32-x64",
    ];
    for (id, entry) in lock["extensions"].as_object_mut().expect("extensions") {
        if entry["tier"] == 1 {
            let (pub_name, ext_name) = id.split_once('.').unwrap_or(("test", "ext"));
            let url = format!(
                "https://{pub_name}.gallerycdn.vsassets.io/extensions/{pub_name}/{ext_name}/1.0.0/1/Microsoft.VisualStudio.Services.VSIXPackage"
            );
            fs::write(cache_dir.join(format!("{id}-{sha}.vsix")), vsix_bytes)
                .expect("seed tier1 cache");
            let is_univ = entry["universal"].as_bool().unwrap_or(true);
            if is_univ {
                *entry = json!({
                    "tier": 1,
                    "universal": true,
                    "status": "verified",
                    "version": "1.0.0",
                    "sha256": sha,
                    "vsixUrl": url,
                    "verifiedAt": "2026-11-25T00:00:00Z",
                    "targets": {
                        "universal": {
                            "sha256": sha,
                            "vsixUrl": url
                        }
                    }
                });
            } else {
                let mut targets = serde_json::Map::new();
                for plat in all_platforms {
                    targets.insert(plat.to_string(), json!({ "sha256": sha, "vsixUrl": url }));
                }
                *entry = json!({
                    "tier": 1,
                    "universal": false,
                    "status": "verified",
                    "version": "1.0.0",
                    "verifiedAt": "2026-11-25T00:00:00Z",
                    "targets": targets
                });
            }
        }
    }
    fs::write(&lock_path, lock.to_string()).expect("write lock");

    let data = sb.data();
    sb.write_clean_default_settings();
    sb.write_code_stub("#!/bin/sh\nexit 0\n");

    let sync = sb
        .command()
        .args([
            "--catalog",
            fixture_cat.to_str().expect("utf8"),
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "rust",
            "--no-rtk",
        ])
        .output()
        .expect("sync");
    assert_eq!(sync.status.code(), Some(0), "stderr={}", stderr(&sync));
    assert!(
        stderr(&sync).contains("WARN") && stderr(&sync).contains("Tier 2"),
        "expected Tier 2 unpinned stderr WARN: {}",
        stderr(&sync)
    );

    let prov = sb.provenance("rust").expect("provenance");
    let t2_list = prov["tier2Unpinned"].as_array().expect("tier2Unpinned");
    assert!(
        !t2_list.is_empty(),
        "provenance must record unpinned Tier 2 IDs: {prov}"
    );
    assert!(
        t2_list.iter().any(|v| v == "rust-lang.rust-analyzer"),
        "expected rust-lang.rust-analyzer in tier2Unpinned: {t2_list:?}"
    );

    let doc_strict = sb
        .command()
        .args([
            "--catalog",
            fixture_cat.to_str().expect("utf8"),
            "--vscode-data-dir",
            &data,
            "doctor",
            "--strict",
        ])
        .output()
        .expect("doctor --strict");
    assert_eq!(
        doc_strict.status.code(),
        Some(1),
        "doctor --strict must fail when provenance records tier2_unpinned:\nstdout={}",
        stdout(&doc_strict)
    );
    assert!(
        stdout(&doc_strict).contains("unpinned Tier 2"),
        "{}",
        stdout(&doc_strict)
    );
}

/// OPUS-03: `ARTAGON_VSCODE_PINS_FILE` emits a stderr WARNING, sets sticky
/// `ProfileProvenance.pinsFileOverride = true`, and triggers `WARN` (`FAIL` under `--strict`)
/// in `doctor` both when active in env AND when recorded in provenance.
#[test]
fn opus03_pins_file_override_warns_sticks_in_provenance_and_fails_doctor_strict() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.write_clean_default_settings();
    sb.write_code_stub("#!/bin/sh\nexit 0\n");

    // 1. Active env override during doctor --strict -> WARN (exit 1).
    let doc_env = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args(["--vscode-data-dir", &data, "doctor", "--strict"])
        .output()
        .expect("doctor with override env");
    assert_eq!(
        doc_env.status.code(),
        Some(1),
        "stdout={}",
        stdout(&doc_env)
    );
    assert!(
        stdout(&doc_env).contains("ARTAGON_VSCODE_PINS_FILE"),
        "{}",
        stdout(&doc_env)
    );

    // 2. Sync with ARTAGON_VSCODE_PINS_FILE -> stderr WARNING + provenance.pinsFileOverride = true.
    let sync = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "general-minimal",
            "--no-rtk",
        ])
        .output()
        .expect("sync with override");
    assert_eq!(sync.status.code(), Some(0), "stderr={}", stderr(&sync));
    assert!(
        stderr(&sync).contains("WARNING: ARTAGON_VSCODE_PINS_FILE="),
        "{}",
        stderr(&sync)
    );
    let prov = sb.provenance("general-minimal").expect("provenance");
    assert_eq!(prov["pinsFileOverride"], true);

    // 3. Doctor WITHOUT env var STILL warns & fails --strict due to sticky provenance!
    let doc_sticky = sb
        .command()
        .args(["--vscode-data-dir", &data, "doctor", "--strict"])
        .output()
        .expect("doctor sticky");
    assert_eq!(
        doc_sticky.status.code(),
        Some(1),
        "doctor --strict must warn on sticky pinsFileOverride:\nstdout={}",
        stdout(&doc_sticky)
    );
    assert!(
        stdout(&doc_sticky).contains("ARTAGON_VSCODE_PINS_FILE override"),
        "{}",
        stdout(&doc_sticky)
    );
}

/// CARBON-03 + OPUS-11 / FABLE-12 / CARBON-08:
/// - Clean out-of-the-box `vsp doctor --strict` PASSES without XDG RTK wrappers and reports
///   `pending_tier1_pins` and `pending_tier2_pins` as informational PASS counts.
/// - Missing RTK wrappers WARN (and fail `--strict`) when `user_config.defaults.rtk_terminal == true`
///   OR when any synced profile provenance includes `rtk-terminal`.
/// - Malformed `vsp.lock.json` fails closed as `FAIL` in `doctor`.
#[test]
fn carbon03_opus11_doctor_strict_rtk_gating_and_fail_closed_lockfile() {
    let sb = Sandbox::new();
    let data = sb.data();
    sb.write_clean_default_settings();
    sb.write_code_stub("#!/bin/sh\nexit 0\n");

    // 1. Clean out-of-the-box install (no RTK wrappers) -> `doctor --strict` exits 0!
    let clean = sb
        .command()
        .args(["--vscode-data-dir", &data, "doctor", "--strict", "--json"])
        .output()
        .expect("clean doctor --strict");
    assert_eq!(
        clean.status.code(),
        Some(0),
        "out-of-the-box doctor --strict must exit 0 without RTK wrappers:\nstdout={}",
        stdout(&clean)
    );
    let report: Value = serde_json::from_slice(&clean.stdout).expect("json");
    assert_eq!(report["warns"], 0);
    assert_eq!(report["fails"], 0);
    assert!(
        report["pending_tier1_pins"].as_u64().unwrap_or(0) > 0,
        "must report informational pending_tier1_pins: {report}"
    );
    assert!(
        report["pending_tier2_pins"].as_u64().unwrap_or(0) > 0,
        "must report informational pending_tier2_pins: {report}"
    );
    assert!(
        report["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .any(|c| {
                c["category"] == "rtk"
                    && c["severity"] == "pass"
                    && c["message"] == "optional rtk-terminal wrappers not enabled"
            }),
        "expected optional RTK PASS: {report}"
    );

    // 2. `user_config.defaults.rtkTerminal: true` without RTK wrappers -> WARN (exit 1 under --strict).
    let cfg_path = sb.path("rtk-config.jsonc");
    fs::write(
        &cfg_path,
        r#"{ "version": 1, "defaults": { "rtkTerminal": true } }"#,
    )
    .expect("write cfg");
    fs::set_permissions(&cfg_path, fs::Permissions::from_mode(0o600)).expect("chmod cfg");
    let rtk_cfg_doc = sb
        .command()
        .args([
            "--config",
            cfg_path.to_str().expect("utf8"),
            "--vscode-data-dir",
            &data,
            "doctor",
            "--strict",
        ])
        .output()
        .expect("doctor with rtk config");
    assert_eq!(rtk_cfg_doc.status.code(), Some(1));
    assert!(
        stdout(&rtk_cfg_doc).contains("WARN: XDG RTK wrappers not yet installed"),
        "{}",
        stdout(&rtk_cfg_doc)
    );

    // 3. Direct library check via `run_doctor_with_options` with parsed UserConfig
    //    (must never report "optional rtk-terminal wrappers not enabled" when enabled in config).
    let parsed_cfg = parse_user_config(
        r#"{ "version": 1, "defaults": { "rtkTerminal": true } }"#,
        &cfg_path,
    )
    .expect("parse cfg");
    let embedded = load_embedded().expect("embedded");
    let lib_rep =
        run_doctor_with_options(&embedded, Some(&sb.path("data")), true, Some(&parsed_cfg));
    assert!(
        lib_rep.checks.iter().any(|c| {
            c.category == "rtk" && c.message != "optional rtk-terminal wrappers not enabled"
        }),
        "run_doctor_with_options must treat RTK as enabled when defaults.rtk_terminal == true: {:?}",
        lib_rep.checks
    );

    // 4. Synced profile provenance with `rtk-terminal` (and wrappers removed) -> WARN (exit 1).
    let user_dir = sb.path("data/User");
    let prov = ProfileProvenance {
        version: 1,
        profile: "default".to_string(),
        user_dir: user_dir.display().to_string(),
        preset: None,
        resolved_modules: vec!["core-editor".to_string(), "rtk-terminal".to_string()],
        managed_settings_keys: Vec::new(),
        managed_extensions: Vec::new(),
        ux: None,
        ai_mode: None,
        unlocked_bypass: false,
        publisher_trust_bypass: false,
        pins_file_override: false,
        tier2_unpinned: Vec::new(),
        last_sync_at: now_rfc3339_utc(),
    };
    let state_override = sb.path("state");
    // Save provenance manually at the hash path used inside sandbox XDG_STATE_HOME.
    let key = format!("{}|default", user_dir.display());
    let hash = sha256_hex(key.as_bytes());
    let prov_dir = state_override.join("vsp/profiles");
    fs::create_dir_all(&prov_dir).expect("mkdir prov");
    fs::write(
        prov_dir.join(format!("{hash}.json")),
        serde_json::to_string(&prov).expect("ser prov"),
    )
    .expect("write prov");

    let rtk_prov_doc = sb
        .command()
        .args(["--vscode-data-dir", &data, "doctor", "--strict"])
        .output()
        .expect("doctor with rtk provenance");
    assert_eq!(rtk_prov_doc.status.code(), Some(1));
    assert!(
        stdout(&rtk_prov_doc).contains("WARN: XDG RTK wrappers not yet installed"),
        "{}",
        stdout(&rtk_prov_doc)
    );

    // 5. Malformed `vsp.lock.json` on disk -> FAIL in `doctor` (never silently returns empty).
    let bad_dir = sb.path("bad-lock-dir");
    fs::create_dir_all(&bad_dir).expect("mkdir bad");
    fs::write(bad_dir.join("vsp.lock.json"), "{ not valid json").expect("write bad lock");
    let mut bad_cat = load_embedded().expect("embedded");
    bad_cat.root = bad_dir;
    let bad_rep = run_doctor(&bad_cat, Some(&sb.path("data")), false);
    assert!(
        bad_rep
            .checks
            .iter()
            .any(|c| c.category == "lockfile" && c.severity == CheckSeverity::Fail),
        "malformed vsp.lock.json must produce FAIL in doctor: {:?}",
        bad_rep.checks
    );
}

/// FABLE-07 / FABLE-08 / OPUS-09:
/// - Batched `--uninstall-extension` sends multiple uninstalls in one `code` invocation.
/// - Failed batched install emits fallback notice, re-lists installed extensions, and retries
///   ONLY still-missing extensions serially.
/// - Bounded subprocess timeout (`VSP_CODE_TIMEOUT_SECS=1`) kills hung `code` and exits 7.
/// - Parallelized `doctor` scans 6 registered profiles in deterministic order.
#[test]
fn fable07_fable08_opus09_batch_uninstall_smart_retry_timeout_and_parallel_doctor() {
    let sb = Sandbox::new();
    let data = sb.data();
    let log = sb.path("code.log");

    // 1. Batched `--uninstall-extension`: 3 AI extensions removed in ONE `code` invocation!
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"rust","location":"aaaa1111"}]}"#,
    )
    .expect("register rust");
    sb.write_code_stub(&format!(
        r#"#!/bin/sh
echo "$*" >> '{log}'
case "$*" in
  *--list-extensions*)
    echo 'anthropic.claude-code@1.0.0'
    echo 'github.copilot-chat@1.0.0'
    echo 'openai.chatgpt@1.0.0'
    exit 0
    ;;
esac
exit 0
"#,
        log = log.display()
    ));
    let ai_off = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "rust",
            "--ai",
            "off",
            "--no-rtk",
        ])
        .output()
        .expect("ai off sync");
    assert_eq!(ai_off.status.code(), Some(0), "stderr={}", stderr(&ai_off));
    let uninstall_lines: Vec<String> = sb
        .code_log()
        .lines()
        .filter(|l| l.contains("--uninstall-extension"))
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        uninstall_lines.len(),
        1,
        "expected ONE batched --uninstall-extension call, got: {uninstall_lines:#?}"
    );
    assert!(uninstall_lines[0].contains("anthropic.claude-code"));
    assert!(uninstall_lines[0].contains("github.copilot-chat"));
    assert!(uninstall_lines[0].contains("openai.chatgpt"));

    // 2. Failed batched install -> re-list + retry ONLY still-missing extensions!
    sb.clear_code_log();
    let state_file = sb.path("partial-installed.txt");
    fs::write(&state_file, "").expect("init state");
    let smart_stub = format!(
        r#"#!/bin/sh
echo "$*" >> '{log}'
case "$*" in
  *--list-extensions*)
    /bin/cat '{state}'
    exit 0
    ;;
  *--install-extension*--install-extension*)
    # Simulate partial batched success: editorconfig succeeded before batch aborted.
    echo 'editorconfig.editorconfig@0.18.2' >> '{state}'
    echo 'partial batch failure' >&2
    exit 1
    ;;
  *--install-extension*)
    exit 0
    ;;
esac
exit 0
"#,
        log = log.display(),
        state = state_file.display(),
    );
    sb.write_code_stub(&smart_stub);
    let bundle = sb.path("multi.code-profile");
    fs::write(
        &bundle,
        r#"{"name":"rust","settings":{},"extensions":{"enabled":["editorconfig.editorconfig","tamasfe.even-better-toml"]}}"#,
    )
    .expect("write multi bundle");
    let smart_out = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "--from",
            bundle.to_str().expect("utf8"),
        ])
        .output()
        .expect("smart retry");
    assert_eq!(
        smart_out.status.code(),
        Some(0),
        "stderr={}",
        stderr(&smart_out)
    );
    assert!(
        stderr(&smart_out)
            .contains("re-listing installed extensions and retrying missing ones serially"),
        "expected fallback notice on stderr: {}",
        stderr(&smart_out)
    );
    let serial_install_lines: Vec<String> = sb
        .code_log()
        .lines()
        .filter(|l| l.matches("--install-extension").count() == 1)
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        serial_install_lines.len(),
        1,
        "only the still-missing extension should be retried serially: {serial_install_lines:#?}"
    );
    assert!(serial_install_lines[0].contains("tamasfe.even-better-toml"));
    assert!(!serial_install_lines[0].contains("editorconfig.editorconfig"));

    // 3. Hung `code` stub timed out cleanly with VSP_CODE_TIMEOUT_SECS=1 -> exit 7!
    sb.write_code_stub("#!/bin/sh\n/bin/sleep 30\n");
    let start = Instant::now();
    let timed_out = sb
        .command()
        .env("VSP_CODE_TIMEOUT_SECS", "1")
        .args(["--vscode-data-dir", &data, "doctor"])
        .output()
        .expect("timeout doctor");
    assert_eq!(
        timed_out.status.code(),
        Some(7),
        "hung code must time out with exit 7:\nstdout={}\nstderr={}",
        stdout(&timed_out),
        stderr(&timed_out)
    );
    assert!(
        stdout(&timed_out).contains("timed out"),
        "{}",
        stdout(&timed_out)
    );
    assert!(
        start.elapsed().as_secs() < 6,
        "timeout took too long: {:?}",
        start.elapsed()
    );

    // 4. Parallelized `doctor` across 6 registered profiles preserves deterministic order.
    sb.write_clean_default_settings();
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[
            {"name":"p1","location":"11111111"},
            {"name":"p2","location":"22222222"},
            {"name":"p3","location":"33333333"},
            {"name":"p4","location":"44444444"},
            {"name":"p5","location":"55555555"},
            {"name":"p6","location":"66666666"}
        ]}"#,
    )
    .expect("write 6 profiles");
    sb.write_code_stub(
        r#"#!/bin/sh
case "$*" in
  *--profile=p4*) echo 'jscearcy.rust-doc-viewer@4.2.1' ;;
  *--profile=p2*) echo 'editorconfig.editorconfig@0.17.0' ;;
esac
exit 0
"#,
    );
    let par_doc = sb
        .command()
        .args(["--vscode-data-dir", &data, "doctor", "--json"])
        .output()
        .expect("parallel doctor");
    assert_eq!(par_doc.status.code(), Some(1));
    let par_json: Value = serde_json::from_slice(&par_doc.stdout).expect("json");
    let msgs: Vec<&str> = par_json["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter_map(|c| c["message"].as_str())
        .collect();
    let drift_idx = msgs
        .iter()
        .position(|m| m.contains("profile 'p2'") && m.contains("version drift"))
        .expect("p2 drift");
    let comp_idx = msgs
        .iter()
        .position(|m| m.contains("profile 'p4'") && m.contains("compromised"))
        .expect("p4 compromised");
    assert!(
        drift_idx < comp_idx,
        "parallel doctor must emit findings in deterministic sorted profile order"
    );
}

/// CARBON-05: `sync.rs` reads `catalog.retire_on_switch` (settings + extensions) and
/// `doctor.rs` / `sync --user` read `catalog.policy.default_profile_expectations`.
#[test]
fn carbon05_retire_on_switch_and_policy_default_profile_expectations() {
    let sb = Sandbox::new();
    let data = sb.data();

    // 1. Workspace switch from `ts-eslint-prettier` to `biome` retires `retireOnSwitch.settings`!
    let ws = sb.path("ws-switch");
    fs::create_dir_all(ws.join(".vscode")).expect("mkdir .vscode");
    fs::write(
        ws.join(".vscode/settings.json"),
        r#"{
  "eslint.workingDirectories": [{ "mode": "auto" }],
  "eslint.validate": ["typescript"],
  "user.keepMe": true
}
"#,
    )
    .expect("seed ws settings");
    let ws_out = sb
        .command()
        .args([
            "sync",
            ws.to_str().expect("utf8"),
            "--preset",
            "astro",
            "--with",
            "biome",
            "--without",
            "ts-eslint-prettier",
        ])
        .output()
        .expect("switch to biome");
    assert_eq!(ws_out.status.code(), Some(0), "stderr={}", stderr(&ws_out));
    let merged_ws = parse_jsonc_value(
        &fs::read_to_string(ws.join(".vscode/settings.json")).expect("read ws"),
        "ws",
    )
    .expect("jsonc");
    assert!(
        merged_ws.get("eslint.workingDirectories").is_none(),
        "retireOnSwitch must delete eslint.workingDirectories: {merged_ws}"
    );
    assert!(
        merged_ws.get("eslint.validate").is_none(),
        "retireOnSwitch must delete eslint.validate: {merged_ws}"
    );
    assert_eq!(merged_ws["user.keepMe"], true);

    // 2. Profile switch to `ai-copilot` retires rival `retireOnSwitch.extensions`!
    fs::write(
        sb.path("data/User/globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"ai-copilot","location":"cccc3333"}]}"#,
    )
    .expect("register ai-copilot");
    sb.write_code_stub(&format!(
        r#"#!/bin/sh
echo "$*" >> '{}'
case "$*" in
  *--list-extensions*)
    echo 'anthropic.claude-code@1.0.0'
    echo 'openai.chatgpt@1.0.0'
    exit 0
    ;;
esac
exit 0
"#,
        sb.path("code.log").display()
    ));
    let copilot_sync = sb
        .command()
        .env("ARTAGON_VSCODE_PINS_FILE", sb.path("pins.json"))
        .args([
            "--vscode-data-dir",
            &data,
            "sync",
            "-p",
            "ai-copilot",
            "--no-rtk",
        ])
        .output()
        .expect("ai-copilot sync");
    assert_eq!(
        copilot_sync.status.code(),
        Some(0),
        "stderr={}",
        stderr(&copilot_sync)
    );
    assert!(
        sb.code_log()
            .contains("--uninstall-extension anthropic.claude-code"),
        "retireOnSwitch must uninstall rival anthropic.claude-code: {}",
        sb.code_log()
    );
    assert!(
        sb.code_log()
            .contains("--uninstall-extension openai.chatgpt"),
        "retireOnSwitch must uninstall rival openai.chatgpt: {}",
        sb.code_log()
    );

    // 3. Extra `catalog.policy.default_profile_expectations` audited by `run_doctor`!
    sb.write_clean_default_settings();
    let mut catalog = load_embedded().expect("embedded");
    if let Some(pol) = catalog.policy.as_mut() {
        pol.default_profile_expectations.insert(
            "custom.auditPolicyKey".to_string(),
            DefaultProfileExpectation {
                expected: json!(true),
                value_type: "boolean".to_string(),
                reason: "custom org security floor".to_string(),
            },
        );
    }
    let rep = run_doctor(&catalog, Some(&sb.path("data")), true);
    assert!(
        rep.checks.iter().any(|c| {
            c.category == "default-profile"
                && c.severity == CheckSeverity::Warn
                && c.message.contains("custom.auditPolicyKey=MISSING")
        }),
        "doctor must enforce catalog.policy.default_profile_expectations: {:?}",
        rep.checks
    );
}

/// CARBON-09 / OPUS-01 / CARBON-01 / SOL-01 / SOL-08:
/// - `dev pins --refresh` refreshes BOTH per-platform Tier 1 (`universal: false`) AND Tier 2
///   version locks, warms the XDG VSIX cache, and honors `dry_run = true` with ZERO writes.
/// - `dev pins --check` supports `strict_release_gate = true` rejecting `pending_refresh`.
/// - `dev docs --write --dry-run` and `dev hooks --dry-run` perform ZERO writes / `git config` calls.
#[test]
fn dev_ops_platform_tier1_tier2_refresh_cache_warm_strict_gate_and_dry_run() {
    let sb = Sandbox::new();
    let fixture = sb.path("cat-refresh");
    copy_v2_catalog(&fixture);

    // Shrink lockfile to 3 representative entries (Tier 1 universal, Tier 1 native, Tier 2)
    // so mock refresh is fast and deterministic.
    let lock_path = fixture.join("vsp.lock.json");
    let initial_lock = json!({
        "version": 2,
        "pinRequired": false,
        "platforms": ["darwin-arm64", "linux-x64", "universal"],
        "extensions": {
            "editorconfig.editorconfig": {
                "tier": 1,
                "universal": true,
                "status": "pending_refresh",
                "targets": {}
            },
            "anthropic.claude-code": {
                "tier": 1,
                "universal": false,
                "status": "pending_refresh",
                "targets": {}
            },
            "aaron-bond.better-comments": {
                "tier": 2,
                "universal": true,
                "status": "pending_refresh",
                "version": null
            }
        }
    });
    fs::write(
        &lock_path,
        serde_json::to_string_pretty(&initial_lock).expect("ser"),
    )
    .expect("write small lock");

    let univ_bytes = b"universal-vsix-bytes".to_vec();
    let arm_bytes = b"darwin-arm64-vsix-bytes".to_vec();
    let linux_bytes = b"linux-x64-vsix-bytes".to_vec();
    let univ_sha = sha256_hex(&univ_bytes);
    let arm_sha = sha256_hex(&arm_bytes);
    let linux_sha = sha256_hex(&linux_bytes);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let u_serve = univ_bytes.clone();
    let a_serve = arm_bytes.clone();
    let l_serve = linux_bytes.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
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
            let req = String::from_utf8_lossy(&raw).into_owned();
            let (ct, body) = if req.starts_with("POST") {
                let payload = if req.contains("anthropic.claude-code") {
                    json!({"results":[{"extensions":[{"versions":[
                        {
                            "version": "2.5.0",
                            "targetPlatform": "darwin-arm64",
                            "properties": [],
                            "files": [{"assetType":"Microsoft.VisualStudio.Services.VSIXPackage","source":format!("http://{addr}/arm.vsix")}]
                        },
                        {
                            "version": "2.5.0",
                            "targetPlatform": "linux-x64",
                            "properties": [],
                            "files": [{"assetType":"Microsoft.VisualStudio.Services.VSIXPackage","source":format!("http://{addr}/linux.vsix")}]
                        }
                    ]}]}]})
                } else if req.contains("aaron-bond.better-comments") {
                    json!({"results":[{"extensions":[{"versions":[
                        {"version": "3.20.0", "properties": [], "files": []}
                    ]}]}]})
                } else {
                    json!({"results":[{"extensions":[{"versions":[
                        {
                            "version": "0.18.2",
                            "properties": [],
                            "files": [{"assetType":"Microsoft.VisualStudio.Services.VSIXPackage","source":format!("http://{addr}/univ.vsix")}]
                        }
                    ]}]}]})
                };
                ("application/json", payload.to_string().into_bytes())
            } else if req.starts_with("GET /arm.vsix") {
                ("application/octet-stream", a_serve.clone())
            } else if req.starts_with("GET /linux.vsix") {
                ("application/octet-stream", l_serve.clone())
            } else {
                ("application/octet-stream", u_serve.clone())
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    // 1. `strict_release_gate = true` rejects pending_refresh Tier 1 and Tier 2 entries!
    let mut small_cat = load_from_dir(&repo_root()).expect("load repo");
    small_cat.root = fixture.clone();
    let gate_fail = run_dev_pins_with_options(
        &small_cat,
        DevPinsOptions {
            check: true,
            refresh: false,
            strict_release_gate: true,
            dry_run: false,
            json: false,
            quiet: true,
        },
    )
    .expect("strict gate check");
    assert_eq!(
        gate_fail,
        ExitCode::Operational,
        "strict_release_gate must reject pending_refresh Tier 1/2 entries"
    );

    // 2. Refresh via CLI (isolated env with VSP_MARKETPLACE_API_URL + XDG_CACHE_HOME)
    //    verifies Tier 1 universal, Tier 1 native targets, Tier 2 version, AND warms XDG cache!
    let full_fixture = sb.path("full-cat");
    copy_v2_catalog(&full_fixture);
    let full_lock_path = full_fixture.join("vsp.lock.json");
    let before_bytes = fs::read(&full_lock_path).expect("read full lock");

    let cli_refresh = sb
        .command()
        .env("VSP_MARKETPLACE_API_URL", format!("http://{addr}/query"))
        .env("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL", "1")
        .args([
            "--catalog",
            full_fixture.to_str().expect("utf8"),
            "--json",
            "dev",
            "pins",
            "--refresh",
        ])
        .output()
        .expect("full cli refresh");
    assert_eq!(
        cli_refresh.status.code(),
        Some(0),
        "cli refresh failed:\nstdout={}\nstderr={}",
        stdout(&cli_refresh),
        stderr(&cli_refresh)
    );
    let after_lock: Value =
        serde_json::from_str(&fs::read_to_string(&full_lock_path).expect("read after"))
            .expect("json");
    assert_ne!(
        fs::read(&full_lock_path).expect("after bytes"),
        before_bytes
    );

    // Tier 1 universal refreshed:
    let ec = &after_lock["extensions"]["editorconfig.editorconfig"];
    assert_eq!(ec["status"], "verified");
    assert_eq!(ec["version"], "0.18.2");
    assert_eq!(ec["sha256"], univ_sha.as_str());

    // Tier 1 platform-specific (`universal: false`) refreshed with per-platform targets & NO flat sha256/vsixUrl!
    let claude = &after_lock["extensions"]["anthropic.claude-code"];
    assert_eq!(claude["status"], "verified");
    assert_eq!(claude["version"], "2.5.0");
    assert!(
        claude.get("sha256").is_none(),
        "universal:false must have no flat sha256"
    );
    assert!(
        claude.get("vsixUrl").is_none(),
        "universal:false must have no flat vsixUrl"
    );
    assert_eq!(
        claude["targets"]["darwin-arm64"]["sha256"],
        arm_sha.as_str()
    );
    assert_eq!(claude["targets"]["linux-x64"]["sha256"], linux_sha.as_str());

    // Tier 2 refreshed with version lock!
    let better_comments = &after_lock["extensions"]["aaron-bond.better-comments"];
    assert_eq!(better_comments["status"], "verified");
    assert_eq!(better_comments["version"], "3.20.0");

    // XDG VSIX cache MUST have been warmed instead of discarded to io::sink()!
    assert!(
        sb.path(&format!(
            "cache/vsp/vsix/editorconfig.editorconfig-{univ_sha}.vsix"
        ))
        .is_file(),
        "universal VSIX must be cached in XDG cache during refresh"
    );
    assert!(
        sb.path(&format!(
            "cache/vsp/vsix/anthropic.claude-code-{arm_sha}.vsix"
        ))
        .is_file(),
        "darwin-arm64 VSIX must be cached in XDG cache during refresh"
    );
    assert!(
        sb.path(&format!(
            "cache/vsp/vsix/anthropic.claude-code-{linux_sha}.vsix"
        ))
        .is_file(),
        "linux-x64 VSIX must be cached in XDG cache during refresh"
    );

    // 3. `strict_release_gate = true` passes when all Tier 1/2 entries are verified!
    let mut verified_cat = load_from_dir(&repo_root()).expect("load");
    let mut clean_lock = verified_cat.lockfile.clone().expect("lockfile");
    clean_lock.extensions = BTreeMap::from([(
        "aaron-bond.better-comments".to_string(),
        LockEntry {
            tier: 2,
            universal: Some(true),
            status: Some(LockEntryStatus::Verified),
            version: Some("3.20.0".to_string()),
            sha256: None,
            vsix_url: None,
            verified_at: Some("2026-11-25T00:00:00Z".to_string()),
            targets: BTreeMap::new(),
        },
    )]);
    verified_cat.lockfile = Some(clean_lock);
    verified_cat.root = PathBuf::from("<embedded>");
    let gate_ok = run_dev_pins_with_options(
        &verified_cat,
        DevPinsOptions {
            check: true,
            refresh: false,
            strict_release_gate: true,
            dry_run: false,
            json: false,
            quiet: true,
        },
    )
    .expect("strict gate pass");
    assert_eq!(gate_ok, ExitCode::Ok);

    // 4. `run_dev_docs_with_options` + `run_dev_hooks_with_options` with `dry_run = true` perform
    //    ZERO file writes and ZERO `git config` calls!
    let docs_dir = sb.path("docs-dry");
    fs::create_dir_all(docs_dir.join("docs")).expect("mkdir docs");
    fs::copy(
        repo_root().join("docs/rust.md"),
        docs_dir.join("docs/rust.md"),
    )
    .expect("copy rust");
    fs::copy(repo_root().join("README.md"), docs_dir.join("README.md")).expect("copy readme");
    for f in ["AGENTS.md", "CLAUDE.md", "CODEX.md", "GEMINI.md"] {
        fs::copy(repo_root().join(f), docs_dir.join(f)).expect("copy agent doc");
    }
    let readme_orig = fs::read_to_string(docs_dir.join("README.md")).expect("readme");
    let mut docs_cat = load_from_dir(&repo_root()).expect("cat");
    docs_cat.root = docs_dir.clone();
    let docs_dry = run_dev_docs_with_options(
        &docs_cat,
        DevDocsOptions {
            check: false,
            write: true,
            dry_run: true,
            json: false,
            quiet: true,
        },
    )
    .expect("docs dry-run");
    assert_eq!(docs_dry, ExitCode::Ok);
    assert_eq!(
        fs::read_to_string(docs_dir.join("README.md")).expect("readme after"),
        readme_orig
    );

    // Hooks dry-run works even WITHOUT a git repository (zero `git config` calls!).
    let hooks_repo = sb.path("hooks-dry");
    let hooks_dir = hooks_repo.join("scripts/git-hooks");
    fs::create_dir_all(&hooks_dir).expect("mkdir hooks");
    let pre_commit = hooks_dir.join("pre-commit");
    fs::write(&pre_commit, "#!/bin/sh\nexit 0\n").expect("write hook");
    fs::set_permissions(&pre_commit, fs::Permissions::from_mode(0o755)).expect("chmod hook");
    let hooks_dry = run_dev_hooks_with_options(
        &hooks_repo,
        DevHooksOptions {
            dry_run: true,
            json: false,
            quiet: true,
        },
    )
    .expect("hooks dry-run");
    assert_eq!(hooks_dry, ExitCode::Ok);
    assert!(
        !hooks_repo.join(".git").exists(),
        "dry-run hooks must not require or touch git config"
    );
}

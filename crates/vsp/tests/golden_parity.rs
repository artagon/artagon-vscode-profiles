//! End-to-end golden parity, determinism, N1 security, DAG resolution, and CLI exit code
//! integration tests for `vsp`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};
use tempfile::TempDir;

use vsp::legacy_loader::{load_legacy_catalog, parse_jsonc_value};
use vsp::model::{
    AiMode, ExtensionStatus, ModuleDefinition, ModuleExtensions, ModuleKind, ModuleManifest,
    ModuleSettingsRefs, UxPreset, LEGACY_BUNDLE_ALIASES,
};
use vsp::policy::{check_autonomy_denylist, is_valid_marketplace_vsix_url, validate_repository};
use vsp::resolver::{
    build_dist, compile_canonical_profile_settings, compile_code_profile_bundle,
    compile_profile_extensions, compile_settings, compile_tasks, compile_workspace_extensions,
    merge_json_value, resolve_modules, resolve_profile_recipe, sha256_hex,
    to_canonical_json_string, ResolveRequest, RtkTaskMode, SettingsScope,
};
use vsp::schema_loader::{load_embedded, load_from_dir};

fn repo_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("crates/vsp lives two levels below repo root")
        .to_path_buf()
}

fn load_golden_oracle() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v2_1_golden_sha256.json");
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read golden oracle {}: {e}", path.display()));
    serde_json::from_str(&raw).expect("parse v2_1_golden_sha256.json")
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
            .join(format!("golden-parity-sandbox-{}", std::process::id()));
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

#[test]
fn golden_parity_all_15_profile_settings_byte_identical() {
    let root = repo_root();
    let oracle = load_golden_oracle();
    let expected = oracle["merged_settings"]
        .as_object()
        .expect("oracle merged_settings");
    assert_eq!(
        expected.len(),
        15,
        "oracle must contain exactly 15 profiles"
    );

    let legacy = load_legacy_catalog(&root).expect("load legacy catalog");
    let embedded = load_embedded().expect("load embedded v2.2 catalog");
    assert_eq!(legacy.profiles.len(), 15);
    assert_eq!(embedded.profiles.len(), 15);

    for (profile_name, exp_sha_val) in expected {
        let exp_sha = exp_sha_val.as_str().expect("sha256 str");

        let leg_compiled = compile_canonical_profile_settings(&legacy, profile_name)
            .unwrap_or_else(|e| panic!("compile legacy settings for {profile_name}: {e}"));
        let leg_bytes = to_canonical_json_string(&leg_compiled).expect("serialize legacy");

        let emb_compiled = compile_canonical_profile_settings(&embedded, profile_name)
            .unwrap_or_else(|e| panic!("compile embedded settings for {profile_name}: {e}"));
        let emb_bytes = to_canonical_json_string(&emb_compiled).expect("serialize embedded");

        assert_eq!(
            emb_bytes, leg_bytes,
            "embedded vs legacy canonical settings mismatch for '{profile_name}'"
        );
        assert_eq!(
            sha256_hex(emb_bytes.as_bytes()),
            exp_sha,
            "v2.1 golden SHA-256 mismatch for _merged/{profile_name}.json"
        );
    }
}

#[test]
fn golden_parity_all_15_profile_extensions_byte_identical() {
    let root = repo_root();
    let oracle = load_golden_oracle();
    let expected = oracle["profile_extensions"]
        .as_object()
        .expect("oracle profile_extensions");
    assert_eq!(expected.len(), 15);

    let legacy = load_legacy_catalog(&root).expect("load legacy catalog");
    let embedded = load_embedded().expect("load embedded v2.2 catalog");
    assert_eq!(legacy.profiles.len(), 15);
    assert_eq!(embedded.profiles.len(), 15);

    for (profile_name, exp_sha_val) in expected {
        let exp_sha = exp_sha_val.as_str().expect("sha256 str");

        let leg_resolved = resolve_profile_recipe(
            &legacy,
            profile_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )
        .unwrap_or_else(|e| panic!("resolve legacy {profile_name}: {e}"));
        let leg_exts = compile_profile_extensions(&legacy, &leg_resolved)
            .unwrap_or_else(|e| panic!("compile legacy extensions for {profile_name}: {e}"));
        let leg_str = to_canonical_json_string(&leg_exts).expect("serialize legacy extensions");

        let emb_resolved = resolve_profile_recipe(
            &embedded,
            profile_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )
        .unwrap_or_else(|e| panic!("resolve embedded {profile_name}: {e}"));
        let emb_exts = compile_profile_extensions(&embedded, &emb_resolved)
            .unwrap_or_else(|e| panic!("compile embedded extensions for {profile_name}: {e}"));
        let emb_str = to_canonical_json_string(&emb_exts).expect("serialize embedded extensions");

        assert_eq!(
            emb_str, leg_str,
            "embedded vs legacy extensions mismatch for profiles/{profile_name}/extensions.json"
        );
        assert_eq!(
            sha256_hex(emb_str.as_bytes()),
            exp_sha,
            "v2.1 golden SHA-256 mismatch for profiles/{profile_name}/extensions.json"
        );
    }
}

#[test]
fn golden_parity_all_15_exported_bundles_and_sha256sums() {
    let root = repo_root();
    let oracle = load_golden_oracle();
    let expected_bundles = oracle["canonical_bundles"]
        .as_object()
        .expect("oracle canonical_bundles");
    assert_eq!(expected_bundles.len(), 15);

    let legacy = load_legacy_catalog(&root).expect("load legacy catalog");
    let embedded = load_embedded().expect("load embedded v2.2 catalog");

    let mut lines_15 = Vec::new();
    for (fname, exp_sha_val) in expected_bundles {
        let exp_sha = exp_sha_val.as_str().expect("sha256 str");
        let profile_name = fname.strip_suffix(".code-profile").expect("bundle suffix");

        let leg_bundle = compile_code_profile_bundle(&legacy, profile_name)
            .unwrap_or_else(|e| panic!("legacy bundle {profile_name}: {e}"));
        let leg_str = to_canonical_json_string(&leg_bundle).expect("serialize legacy bundle");

        let emb_bundle = compile_code_profile_bundle(&embedded, profile_name)
            .unwrap_or_else(|e| panic!("embedded bundle {profile_name}: {e}"));
        let emb_str = to_canonical_json_string(&emb_bundle).expect("serialize embedded bundle");

        assert_eq!(
            emb_str, leg_str,
            "embedded vs legacy bundle mismatch for exports/{fname}"
        );
        let actual_sha = sha256_hex(emb_str.as_bytes());
        assert_eq!(
            actual_sha, exp_sha,
            "v2.1 golden SHA-256 mismatch for {fname}"
        );
        lines_15.push(format!("{actual_sha}  {fname}\n"));
    }

    let sums_15_joined: String = lines_15.concat();
    assert_eq!(
        sha256_hex(sums_15_joined.as_bytes()),
        oracle["exports_sha256sums_15_sha256"]
            .as_str()
            .expect("exports_sha256sums_15_sha256"),
        "15-entry exports/SHA256SUMS digest drifted from v2.1 golden oracle"
    );
}

#[test]
fn dist_22_legacy_aliases_and_copilot_only_security_invariant() {
    let oracle = load_golden_oracle();
    let oracle_aliases = oracle["legacy_aliases"]
        .as_object()
        .expect("oracle legacy_aliases");
    let oracle_dist_sums = oracle["dist_sha256sums"]
        .as_object()
        .expect("oracle dist_sha256sums");
    assert_eq!(oracle_aliases.len(), 22);
    assert_eq!(oracle_dist_sums.len(), 37);

    let catalog = load_embedded().expect("load embedded v2.2 catalog");
    let tmp = TempDir::new().expect("tempdir");

    let report = build_dist(&catalog, tmp.path()).expect("build_dist");
    assert_eq!(report.canonical_bundles.len(), 15);
    assert_eq!(report.alias_bundles.len(), 22);
    assert_eq!(report.checksums.len(), 37);

    for &(alias_file, canonical_file) in LEGACY_BUNDLE_ALIASES {
        assert_eq!(
            oracle_aliases.get(alias_file).and_then(Value::as_str),
            Some(canonical_file),
            "alias {alias_file} target mismatch vs v2.1 golden oracle"
        );
        let alias_path = tmp.path().join(alias_file);
        let canonical_path = tmp.path().join(canonical_file);
        assert!(alias_path.is_file(), "missing materialized {alias_file}");
        assert!(
            !alias_path.is_symlink(),
            "dist alias {alias_file} must be a materialized regular file, not a symlink"
        );
        assert_eq!(
            fs::read(&alias_path).expect("read alias"),
            fs::read(&canonical_path).expect("read canonical"),
            "alias {alias_file} must match {canonical_file} byte-for-byte"
        );
    }

    // Verify exact 37-entry SHA256SUMS matches v2.1 golden oracle!
    let dist_sums_bytes = fs::read(&report.sha256sums_path).expect("read dist SHA256SUMS");
    assert_eq!(
        sha256_hex(&dist_sums_bytes),
        oracle["dist_sha256sums_sha256"]
            .as_str()
            .expect("dist_sha256sums_sha256"),
        "37-entry dist SHA256SUMS drifted from v2.1 golden oracle"
    );
    for (digest, fname) in &report.checksums {
        assert_eq!(
            oracle_dist_sums.get(fname).and_then(Value::as_str),
            Some(digest.as_str()),
            "dist SHA-256 mismatch for {fname}"
        );
    }

    // Security invariant: ai-profile-{crisp,retina}.code-profile == ai-copilot.code-profile
    // with ZERO non-Copilot AI IDs.
    let forbidden_ai_ids = [
        "google.google-antigravity",
        "anthropic.claude-code",
        "openai.chatgpt",
        "google.geminicodeassist",
        "github.copilot",
    ];
    let copilot_bytes = fs::read(tmp.path().join("ai-copilot.code-profile")).expect("copilot");
    for alias in [
        "ai-profile-crisp.code-profile",
        "ai-profile-retina.code-profile",
    ] {
        let bytes = fs::read(tmp.path().join(alias)).expect("read ai-profile alias");
        assert_eq!(
            bytes, copilot_bytes,
            "{alias} must equal ai-copilot.code-profile"
        );
        let parsed: Value = serde_json::from_slice(&bytes).expect("parse bundle");
        let enabled: Vec<&str> = parsed["extensions"]["enabled"]
            .as_array()
            .expect("enabled array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            enabled.contains(&"github.copilot-chat"),
            "{alias} must include github.copilot-chat"
        );
        for bad in forbidden_ai_ids {
            assert!(
                !enabled.iter().any(|id| id.eq_ignore_ascii_case(bad)),
                "{alias} leaked non-Copilot AI extension '{bad}'"
            );
        }
    }

    // Verify SHA256SUMS is strictly LC_ALL=C sorted by filename.
    let filenames: Vec<&str> = report.checksums.iter().map(|(_, f)| f.as_str()).collect();
    let mut sorted = filenames.clone();
    sorted.sort_unstable();
    assert_eq!(filenames, sorted, "SHA256SUMS must be LC_ALL=C sorted");
}

#[test]
fn dev_dist_is_100_percent_byte_deterministic_across_two_runs() {
    let root = repo_root();
    let dir1 = TempDir::new().expect("dir1");
    let dir2 = TempDir::new().expect("dir2");

    for dir in [&dir1, &dir2] {
        let out = vsp_cmd()
            .args([
                "--catalog",
                root.to_str().expect("utf8"),
                "dev",
                "dist",
                "--out",
                dir.path().to_str().expect("utf8"),
            ])
            .output()
            .expect("run vsp dev dist");
        assert!(
            out.status.success(),
            "vsp dev dist failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let sums1 = fs::read(dir1.path().join("SHA256SUMS")).expect("sums1");
    let sums2 = fs::read(dir2.path().join("SHA256SUMS")).expect("sums2");
    assert_eq!(
        sums1, sums2,
        "SHA256SUMS must be byte-identical across runs"
    );

    let entries1: Vec<String> = fs::read_dir(dir1.path())
        .expect("ls dir1")
        .map(|e| e.expect("e").file_name().to_string_lossy().to_string())
        .collect();
    let entries2: Vec<String> = fs::read_dir(dir2.path())
        .expect("ls dir2")
        .map(|e| e.expect("e").file_name().to_string_lossy().to_string())
        .collect();
    let set1: BTreeSet<_> = entries1.into_iter().collect();
    let set2: BTreeSet<_> = entries2.into_iter().collect();
    assert_eq!(set1, set2);
    assert_eq!(
        set1.len(),
        38,
        "15 canonical + 22 aliases + SHA256SUMS = 38 files"
    );

    for fname in set1 {
        assert_eq!(
            fs::read(dir1.path().join(&fname)).expect("f1"),
            fs::read(dir2.path().join(&fname)).expect("f2"),
            "file {fname} differed between two dist runs"
        );
    }
}

#[test]
fn n1_security_autonomy_denylist_rejects_all_5_payloads() {
    let payloads = [
        (
            "n1_flat_autoapprove",
            json!({"chat.tools.autoApprove": true}),
        ),
        (
            "n1_nested_autoapprove",
            json!({"chat": {"tools": {"autoApprove": true}}}),
        ),
        (
            "n1_hybrid_nested_autoapprove",
            json!({"chat": {"tools.terminal.autoApprove": true}}),
        ),
        (
            "n1_nested_exec_override",
            json!({"claudeCode": {"claudeProcessWrapper": "/tmp/x"}}),
        ),
        (
            "n1_string_bool_skip_perms",
            json!({"claudeCode.allowDangerouslySkipPermissions": "true"}),
        ),
        (
            "n1_uppercase_exec_override",
            json!({"CLAUDECODE": {"CLAUDEPROCESSWRAPPER": "/tmp/y"}}),
        ),
        (
            "n1_nested_string_bool_skip_perms",
            json!({"claudeCode": {"allowDangerouslySkipPermissions": "TRUE"}}),
        ),
        (
            "n1_gemini_yolo_string_bool",
            json!({"geminicodeassist": {"agentYoloMode": "true"}}),
        ),
        (
            "n1_bypass_string_value",
            json!({"custom": ["--dangerously-skip-permissions"]}),
        ),
    ];

    for (label, doc) in payloads {
        let hits = check_autonomy_denylist(&doc, label);
        assert!(
            !hits.is_empty(),
            "expected autonomy deny-list violation for {label}, got none"
        );
    }

    // Legitimate settings must NOT trigger false positives.
    let benign = json!({
        "chat.tools.autoExpandFailures": true,
        "chat.tools.terminal.outputReflow": true,
        "claudeCode.allowDangerouslySkipPermissions": "false",
        "claudeCode.hideOnboarding": true,
        "geminicodeassist.agentYoloMode": false
    });
    let benign_hits = check_autonomy_denylist(&benign, "benign");
    assert!(
        benign_hits.is_empty(),
        "unexpected false positive violations: {benign_hits:?}"
    );
}

#[test]
fn repo_dev_validate_passes_cleanly() {
    let root = repo_root();
    let legacy = load_legacy_catalog(&root).expect("load legacy catalog");
    let legacy_report = validate_repository(&legacy);
    assert!(
        legacy_report.is_ok(),
        "expected 0 validation violations on legacy catalog, got: {:#?}",
        legacy_report.violations
    );
    assert!(legacy_report.checked_files >= 90);

    let v2_dir = load_from_dir(&root).expect("load v2.2 catalog from dir");
    let v2_report = validate_repository(&v2_dir);
    assert!(
        v2_report.is_ok(),
        "expected 0 validation violations on v2.2 catalog dir, got: {:#?}",
        v2_report.violations
    );

    let embedded = load_embedded().expect("load embedded v2.2 catalog");
    let emb_report = validate_repository(&embedded);
    assert!(
        emb_report.is_ok(),
        "expected 0 validation violations on embedded v2.2 catalog, got: {:#?}",
        emb_report.violations
    );
    assert!(emb_report.checked_files >= 40);

    // Validate via CLI `vsp --catalog <root> dev validate`.
    let out = vsp_cmd()
        .args(["--catalog", root.to_str().expect("utf8"), "dev", "validate"])
        .output()
        .expect("run vsp --catalog dev validate");
    assert_eq!(
        out.status.code(),
        Some(0),
        "vsp --catalog dev validate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Validate via CLI `vsp dev validate` from a completely empty temp cwd (embedded snapshot!).
    let empty_cwd = TempDir::new().expect("empty cwd");
    let out_emb = vsp_cmd()
        .current_dir(empty_cwd.path())
        .env_remove("VSPCLI_ROOT")
        .args(["dev", "validate"])
        .output()
        .expect("run embedded vsp dev validate");
    assert_eq!(
        out_emb.status.code(),
        Some(0),
        "embedded vsp dev validate failed from empty cwd: {}",
        String::from_utf8_lossy(&out_emb.stderr)
    );
}

#[test]
fn cli_embedded_default_works_from_empty_cwd_ignores_untrusted_cwd_and_never_falls_back() {
    let oracle = load_golden_oracle();

    // 1. Version is bumped to 3.0.0.
    let ver = vsp_cmd().arg("--version").output().expect("run --version");
    assert_eq!(ver.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&ver.stdout).contains("3.0.0"),
        "expected version 3.0.0, got: {}",
        String::from_utf8_lossy(&ver.stdout)
    );

    // 2. From an empty temp cwd with no --catalog and no VSPCLI_ROOT:
    //    `vsp dev dist` succeeds twice and emits byte-identical bundles matching the v2.1 oracle.
    let empty_cwd = TempDir::new().expect("empty cwd");
    let d1 = TempDir::new().expect("d1");
    let d2 = TempDir::new().expect("d2");
    for d in [&d1, &d2] {
        let out = vsp_cmd()
            .current_dir(empty_cwd.path())
            .env_remove("VSPCLI_ROOT")
            .args(["dev", "dist", "--out", d.path().to_str().expect("utf8")])
            .output()
            .expect("embedded vsp dev dist");
        assert_eq!(
            out.status.code(),
            Some(0),
            "embedded dev dist failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let sums1 = fs::read(d1.path().join("SHA256SUMS")).expect("sums1");
    let sums2 = fs::read(d2.path().join("SHA256SUMS")).expect("sums2");
    assert_eq!(sums1, sums2);
    assert_eq!(
        sha256_hex(&sums1),
        oracle["dist_sha256sums_sha256"]
            .as_str()
            .expect("oracle sums")
    );

    // 3. Untrusted poisoned cwd MUST NEVER be auto-loaded when --catalog/VSPCLI_ROOT are omitted.
    let poisoned_cwd = TempDir::new().expect("poisoned cwd");
    fs::write(poisoned_cwd.path().join("presets.jsonc"), "{ INVALID JSONC").expect("write");
    fs::create_dir_all(poisoned_cwd.path().join("catalog")).expect("mkdir");
    fs::write(
        poisoned_cwd.path().join("catalog/extensions.json"),
        "{ BROKEN",
    )
    .expect("write");
    let list_emb = vsp_cmd()
        .current_dir(poisoned_cwd.path())
        .env_remove("VSPCLI_ROOT")
        .args(["list", "presets"])
        .output()
        .expect("list presets in poisoned cwd");
    assert_eq!(list_emb.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&list_emb.stdout).lines().count(),
        15
    );

    // 4. Explicit --catalog / VSPCLI_ROOT MUST NEVER fall back to embedded defaults!
    let missing_dir = empty_cwd.path().join("does-not-exist");
    let miss_out = vsp_cmd()
        .args(["--catalog", missing_dir.to_str().expect("utf8"), "list"])
        .output()
        .expect("missing catalog");
    assert_eq!(miss_out.status.code(), Some(2));

    let empty_cat = TempDir::new().expect("empty cat");
    let empty_flag_out = vsp_cmd()
        .args([
            "--catalog",
            empty_cat.path().to_str().expect("utf8"),
            "list",
        ])
        .output()
        .expect("empty --catalog");
    assert_eq!(empty_flag_out.status.code(), Some(4));

    let empty_env_out = vsp_cmd()
        .env("VSPCLI_ROOT", empty_cat.path())
        .args(["list"])
        .output()
        .expect("empty VSPCLI_ROOT");
    assert_eq!(empty_env_out.status.code(), Some(4));
}

#[test]
fn marketplace_url_validator_rejects_spoofed_hosts() {
    assert!(is_valid_marketplace_vsix_url(
        "https://Catppuccin.gallerycdn.vsassets.io/extensions/catppuccin/catppuccin-vsc/3.19.0/1/Microsoft.VisualStudio.Services.VSIXPackage"
    ));
    assert!(is_valid_marketplace_vsix_url(
        "https://marketplace.visualstudio.com/_apis/public/gallery/publishers/a/vsextensions/b/1.0.0/vspackage"
    ));
    assert!(!is_valid_marketplace_vsix_url(
        "http://marketplace.visualstudio.com/pkg"
    ));
    assert!(!is_valid_marketplace_vsix_url(
        "https://marketplace.visualstudio.com@evil.example/pkg"
    ));
    assert!(!is_valid_marketplace_vsix_url(
        "https://evil.example/marketplace.visualstudio.com"
    ));
    assert!(!is_valid_marketplace_vsix_url(
        "https://marketplace.visualstudio.com?evil=1"
    ));
}

#[test]
fn dag_resolver_handles_cycles_conflicts_default_with_and_union_merge() {
    let root = repo_root();
    let mut catalog = load_legacy_catalog(&root).expect("load catalog");

    // Conditional defaultWith: astro + --with biome suppresses ts-eslint-prettier cleanly.
    let astro_biome = resolve_profile_recipe(
        &catalog,
        "astro",
        &["biome".to_string()],
        &[],
        Some(UxPreset::Default),
        None,
    )
    .expect("astro + biome should resolve cleanly");
    assert!(astro_biome.contains(&"biome".to_string()));
    assert!(!astro_biome.contains(&"ts-eslint-prettier".to_string()));

    // Conflict: biome + ts-eslint-prettier must fail with exit 4.
    let conflict = resolve_modules(
        &catalog,
        &ResolveRequest {
            roots: vec!["biome".to_string(), "ts-eslint-prettier".to_string()],
            ..Default::default()
        },
    )
    .expect_err("conflict should fail");
    assert_eq!(conflict.exit_code().as_i32(), 4);

    // Contradictory --with and --without must fail with exit 4.
    let contra = resolve_modules(
        &catalog,
        &ResolveRequest {
            roots: vec!["core-editor".to_string()],
            cli_with: vec!["rust".to_string()],
            cli_without: vec!["rust".to_string()],
            ..Default::default()
        },
    )
    .expect_err("contradiction should fail");
    assert_eq!(contra.exit_code().as_i32(), 4);

    // --ai copilot switches ai-agents -> ai-copilot cleanly.
    let rust_copilot = resolve_profile_recipe(
        &catalog,
        "rust",
        &[],
        &[],
        Some(UxPreset::Default),
        Some(AiMode::Copilot),
    )
    .expect("rust --ai copilot");
    assert!(rust_copilot.contains(&"ai-copilot".to_string()));
    assert!(!rust_copilot.contains(&"ai-agents".to_string()));

    // Inject synthetic cycle: cyc-a -> cyc-b -> cyc-a.
    for (id, req) in [("cyc-a", "cyc-b"), ("cyc-b", "cyc-a")] {
        catalog.modules.insert(
            id.to_string(),
            ModuleDefinition {
                manifest: ModuleManifest {
                    version: 1,
                    id: id.to_string(),
                    kind: ModuleKind::Capability,
                    description: String::new(),
                    requires: vec![req.to_string()],
                    default_with: Vec::new(),
                    conflicts: Vec::new(),
                    extensions: ModuleExtensions::default(),
                    settings: ModuleSettingsRefs::default(),
                    tasks: None,
                    detect: None,
                },
                dir: root.clone(),
                workspace_settings: None,
                profile_settings: None,
                tasks_doc: None,
            },
        );
    }
    let cycle_err = resolve_modules(
        &catalog,
        &ResolveRequest {
            roots: vec!["cyc-a".to_string()],
            ..Default::default()
        },
    )
    .expect_err("cycle must fail");
    assert_eq!(cycle_err.exit_code().as_i32(), 4);

    // Set-union merge vs overwrite merge.
    let merged_union = merge_json_value(
        "shellcheck.exclude",
        json!(["SC1090", "SC1091"]),
        json!(["SC1091", "SC2034"]),
    );
    assert_eq!(merged_union, json!(["SC1090", "SC1091", "SC2034"]));

    let merged_other = merge_json_value("editor.rulers", json!([80, 120]), json!([100]));
    assert_eq!(merged_other, json!([100]));
}

#[test]
fn cli_exit_codes_help_unknown_flag_missing_path_detect_and_diff_check() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");

    // --help exits 0.
    let help = vsp_cmd().arg("--help").output().expect("run --help");
    assert_eq!(help.status.code(), Some(0));

    // --version exits 0.
    let ver = vsp_cmd().arg("--version").output().expect("run --version");
    assert_eq!(ver.status.code(), Some(0));

    // Unknown flag exits 4 (NOT clap's default 2!).
    let bad_flag = vsp_cmd()
        .arg("--definitely-unknown-flag")
        .output()
        .expect("run unknown flag");
    assert_eq!(bad_flag.status.code(), Some(4));

    // diff without explicit selector exits 4.
    let no_sel = vsp_cmd()
        .args(["--catalog", catalog_arg, "diff"])
        .output()
        .expect("run diff without selector");
    assert_eq!(no_sel.status.code(), Some(4));

    // sync without explicit selector exits 4.
    let sync_no_sel = vsp_cmd()
        .args(["--catalog", catalog_arg, "sync"])
        .output()
        .expect("run sync without selector");
    assert_eq!(sync_no_sel.status.code(), Some(4));

    // detect on missing path exits 2.
    let missing = TempDir::new().expect("tmp");
    let missing_path = missing.path().join("does-not-exist");
    let det_missing = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "detect",
            missing_path.to_str().expect("utf8"),
        ])
        .output()
        .expect("run detect missing");
    assert_eq!(det_missing.status.code(), Some(2));

    // diff on missing path exits 2.
    let diff_missing = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            missing_path.to_str().expect("utf8"),
        ])
        .output()
        .expect("run diff missing");
    assert_eq!(diff_missing.status.code(), Some(2));

    // detect on empty temp dir exits 3 and leaves dir untouched (no .vscode/).
    let empty_ws = TempDir::new().expect("empty ws");
    let det_empty = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "detect",
            empty_ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("run detect empty");
    assert_eq!(det_empty.status.code(), Some(3));
    assert!(!empty_ws.path().join(".vscode").exists());

    // detect on Rust workspace with --explain exits 0.
    let rust_ws = TempDir::new().expect("rust ws");
    fs::write(
        rust_ws.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\n",
    )
    .expect("write");
    let det_rust = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "detect",
            "--explain",
            rust_ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("run detect rust");
    assert_eq!(det_rust.status.code(), Some(0));
    let det_stdout = String::from_utf8_lossy(&det_rust.stdout);
    assert!(det_stdout.contains("rust") && det_stdout.contains("Cargo.toml"));
    assert!(!rust_ws.path().join(".vscode").exists());

    // diff --check on drifted Rust workspace exits 3 and emits vsp-plan/v1 JSON.
    let diff_drifted = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            "--check",
            "--json",
            rust_ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("run diff --check");
    assert_eq!(diff_drifted.status.code(), Some(3));
    let plan_val: Value =
        serde_json::from_slice(&diff_drifted.stdout).expect("parse vsp-plan/v1 JSON");
    assert_eq!(plan_val["schema"], "vsp-plan/v1");
    assert_eq!(plan_val["has_drift"], true);

    // sync --dry-run produces the exact same plan as diff and exits 0.
    let sync_dry = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--dry-run",
            "--json",
            "sync",
            rust_ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("run sync --dry-run");
    assert_eq!(sync_dry.status.code(), Some(0));
    assert_eq!(sync_dry.stdout, diff_drifted.stdout);

    // Populate clean workspace files and verify diff --check exits 0!
    let catalog = load_legacy_catalog(&root).expect("load catalog");
    let mods = resolve_profile_recipe(&catalog, "rust", &[], &[], Some(UxPreset::Default), None)
        .expect("resolve rust");
    let ws_settings =
        compile_settings(&catalog, SettingsScope::Workspace, &mods, None).expect("ws settings");
    let ws_exts = compile_workspace_extensions(&catalog, &mods).expect("ws exts");
    let ws_tasks = compile_tasks(&catalog, RtkTaskMode::Plain, &mods)
        .expect("ws tasks")
        .expect("rust has tasks");
    fs::create_dir_all(rust_ws.path().join(".vscode")).expect("mkdir .vscode");
    fs::write(
        rust_ws.path().join(".vscode/settings.json"),
        to_canonical_json_string(&ws_settings).expect("ser settings"),
    )
    .expect("write settings");
    fs::write(
        rust_ws.path().join(".vscode/extensions.json"),
        to_canonical_json_string(&ws_exts).expect("ser exts"),
    )
    .expect("write exts");
    fs::write(
        rust_ws.path().join(".vscode/tasks.json"),
        to_canonical_json_string(&ws_tasks).expect("ser tasks"),
    )
    .expect("write tasks");

    let diff_clean = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            "--check",
            "--json",
            rust_ws.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("run clean diff --check");
    assert_eq!(
        diff_clean.status.code(),
        Some(0),
        "expected clean diff to exit 0, stdout={}",
        String::from_utf8_lossy(&diff_clean.stdout)
    );
    let clean_val: Value = serde_json::from_slice(&diff_clean.stdout).expect("parse clean plan");
    assert_eq!(clean_val["has_drift"], false);
}

#[cfg(unix)]
#[test]
fn doctor_exits_7_on_failing_code_stub_and_enforces_strict_autoupdate() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let stub_bin = TempDir::new().expect("stub bin");
    let fake_data = TempDir::new().expect("fake vscode data");
    let fake_xdg = TempDir::new().expect("fake xdg");

    // Create valid Default profile User/settings.json and XDG RTK files.
    let user_dir = fake_data.path().join("User");
    fs::create_dir_all(&user_dir).expect("mkdir User");
    fs::write(
        user_dir.join("settings.json"),
        r#"{
  "telemetry.telemetryLevel": "off",
  "security.workspace.trust.untrustedFiles": "prompt",
  "extensions.autoUpdate": false
}
"#,
    )
    .expect("write user settings");

    let rtk_dir = fake_xdg.path().join("artagon-vscode/rtk");
    fs::create_dir_all(&rtk_dir).expect("mkdir rtk");
    for f in ["rtk-init.bash", "rtk-init.fish", "rtk-init.zsh"] {
        fs::write(rtk_dir.join(f), "# stub\n").expect("write rtk");
    }

    // 1. Failing `code` stub -> doctor MUST exit 7 (IncompleteObservation)!
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'simulated IPC error' >&2\nexit 1\n",
    );
    let out_fail = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run doctor with failing code");
    assert_eq!(
        out_fail.status.code(),
        Some(7),
        "expected exit 7 on failing code --list-extensions, stdout={} stderr={}",
        String::from_utf8_lossy(&out_fail.stdout),
        String::from_utf8_lossy(&out_fail.stderr)
    );
    assert!(String::from_utf8_lossy(&out_fail.stdout).contains("audit incomplete"));

    // 2. Clean `code` stub + boolean `false` autoUpdate -> doctor --strict exits 0!
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'editorconfig.editorconfig@0.18.2'\nexit 0\n",
    );
    let out_clean = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
            "--strict",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run clean doctor --strict");
    assert_eq!(
        out_clean.status.code(),
        Some(0),
        "expected exit 0 on clean doctor --strict, stdout={}",
        String::from_utf8_lossy(&out_clean.stdout)
    );

    // 3. String "false" for extensions.autoUpdate -> WARN in normal mode, FAIL (exit 1) under --strict!
    fs::write(
        user_dir.join("settings.json"),
        r#"{
  "telemetry.telemetryLevel": "off",
  "security.workspace.trust.untrustedFiles": "prompt",
  "extensions.autoUpdate": "false"
}
"#,
    )
    .expect("write string autoUpdate");
    let out_warn = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run warn doctor");
    assert_eq!(out_warn.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out_warn.stdout).contains("WARN:"));

    let out_strict = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
            "--strict",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run strict doctor");
    assert_eq!(out_strict.status.code(), Some(1));

    // 4. Compromised extension reported by `code` -> FAIL (exit 1)!
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'jscearcy.rust-doc-viewer@4.2.1'\nexit 0\n",
    );
    let out_comp = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run compromised doctor");
    assert_eq!(out_comp.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_comp.stdout).contains("SECURITY: compromised"));
}

#[test]
fn cli_list_show_completions_export_and_self_path() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");

    // list presets
    let list_out = vsp_cmd()
        .args(["--catalog", catalog_arg, "list", "presets"])
        .output()
        .expect("list presets");
    assert_eq!(list_out.status.code(), Some(0));
    let lines: Vec<&str> = std::str::from_utf8(&list_out.stdout)
        .expect("utf8")
        .lines()
        .collect();
    assert_eq!(lines.len(), 15);

    // show legacy alias ai-profile-crisp -> resolves to ai-copilot
    let show_out = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--json",
            "show",
            "ai-profile-crisp",
        ])
        .output()
        .expect("show legacy");
    assert_eq!(show_out.status.code(), Some(0));
    let show_val: Value = serde_json::from_slice(&show_out.stdout).expect("parse show");
    assert_eq!(show_val["name"], "ai-copilot");

    // show unknown -> exit 2
    let show_missing = vsp_cmd()
        .args(["--catalog", catalog_arg, "show", "not-a-real-pack"])
        .output()
        .expect("show missing");
    assert_eq!(show_missing.status.code(), Some(2));

    // completions for bash, zsh, fish
    for shell in ["bash", "zsh", "fish"] {
        let comp = vsp_cmd()
            .args(["completions", shell])
            .output()
            .expect("completions");
        assert_eq!(comp.status.code(), Some(0));
        assert!(!comp.stdout.is_empty());
    }

    // self path
    let self_path = vsp_cmd()
        .args(["self", "path"])
        .output()
        .expect("self path");
    assert_eq!(self_path.status.code(), Some(0));
    assert!(!self_path.stdout.is_empty());

    // export single profile + SHA256SUMS
    let exp_dir = TempDir::new().expect("export dir");
    let exp = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "export",
            "rust",
            "--out",
            exp_dir.path().to_str().expect("utf8"),
        ])
        .output()
        .expect("export rust");
    assert_eq!(exp.status.code(), Some(0));
    assert!(exp_dir.path().join("rust.code-profile").is_file());
    assert!(exp_dir.path().join("SHA256SUMS").is_file());
}

#[test]
fn strict_jsonc_parser_handles_comments_and_rejects_empty_or_multi_doc() {
    let parsed = parse_jsonc_value("// comment\n{ /* block */ \"a\": 1, }\n", "ok.jsonc")
        .expect("valid jsonc");
    assert_eq!(parsed["a"], 1);

    assert!(parse_jsonc_value("// only comments\n", "empty.jsonc").is_err());
    assert!(parse_jsonc_value("{}{}", "multi.jsonc").is_err());

    // Verify compromised extension is tracked in catalog/extensions.json.
    let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
    let comp = catalog
        .extension_record("jscearcy.rust-doc-viewer")
        .expect("compromised record");
    assert_eq!(comp.status, ExtensionStatus::Compromised);
}

#[cfg(unix)]
#[test]
fn doctor_pin_drift_and_multi_profile_partial_failure_handling() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let stub_bin = TempDir::new().expect("stub bin");
    let fake_data = TempDir::new().expect("fake vscode data");
    let fake_xdg = TempDir::new().expect("fake xdg");

    let user_dir = fake_data.path().join("User");
    fs::create_dir_all(user_dir.join("globalStorage")).expect("mkdir globalStorage");
    fs::write(
        user_dir.join("settings.json"),
        r#"{
  "telemetry.telemetryLevel": "off",
  "security.workspace.trust.untrustedFiles": "prompt",
  "extensions.autoUpdate": false
}
"#,
    )
    .expect("write settings");
    fs::write(
        user_dir.join("globalStorage/storage.json"),
        r#"{"userDataProfiles":[{"name":"broken-prof","location":"abcd1234"},{"name":"rust","location":"beef5678"}]}"#,
    )
    .expect("write storage");

    let rtk_dir = fake_xdg.path().join("artagon-vscode/rtk");
    fs::create_dir_all(&rtk_dir).expect("mkdir rtk");
    for f in ["rtk-init.bash", "rtk-init.fish", "rtk-init.zsh"] {
        fs::write(rtk_dir.join(f), "# stub\n").expect("write rtk");
    }

    // 1. Pin drift WARN: installed editorconfig@0.17.0 vs pinned 0.18.2.
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\necho 'editorconfig.editorconfig@0.17.0'\nexit 0\n",
    );
    let out_drift = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
            "--strict",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run pin drift doctor");
    assert_eq!(out_drift.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out_drift.stdout).contains("version drift"));

    // 2. One profile fails `code --list-extensions`, another has compromised ext:
    //    doctor MUST continue scanning remaining profiles AND exit 7!
    write_executable_stub(
        stub_bin.path(),
        "code",
        "#!/bin/sh\ncase \"$*\" in\n  *\"--profile=broken-prof\"*) echo 'broken profile' >&2; exit 2 ;;\n  *\"--profile=rust\"*) echo 'jscearcy.rust-doc-viewer@4.2.1'; exit 0 ;;\n  *) echo 'editorconfig.editorconfig@0.18.2'; exit 0 ;;\nesac\n",
    );
    let out_partial = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "doctor",
        ])
        .env("PATH", stub_bin.path())
        .env("XDG_CONFIG_HOME", fake_xdg.path())
        .output()
        .expect("run partial doctor");
    assert_eq!(out_partial.status.code(), Some(7));
    let stdout = String::from_utf8_lossy(&out_partial.stdout);
    assert!(stdout.contains("audit incomplete"));
    assert!(stdout.contains("SECURITY: compromised extension 'jscearcy.rust-doc-viewer'"));
}

#[test]
fn diff_supports_user_profile_ux_and_ai_flags_and_rejects_contradictions() {
    let root = repo_root();
    let catalog_arg = root.to_str().expect("utf8");
    let fake_data = TempDir::new().expect("fake data");

    // --user with --ux crisp and --no-ai emits valid vsp-plan/v1 JSON with chat.disableAIFeatures=true.
    let user_plan = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "--json",
            "diff",
            "--user",
            "--ux",
            "crisp",
            "--no-ai",
        ])
        .output()
        .expect("diff --user");
    assert_eq!(user_plan.status.code(), Some(0));
    let val: Value = serde_json::from_slice(&user_plan.stdout).expect("parse user plan");
    assert_eq!(val["schema"], "vsp-plan/v1");
    assert_eq!(val["target"]["kind"], "user");
    assert_eq!(val["target"]["ux"], "crisp");
    assert_eq!(val["target"]["ai"], "off");
    let changes = val["settings_changes"]
        .as_array()
        .expect("settings_changes");
    assert!(changes
        .iter()
        .any(|c| c["key"] == "chat.disableAIFeatures" && c["desired"] == true));
    assert!(changes
        .iter()
        .any(|c| c["key"] == "workbench.colorTheme" && c["desired"] == "Tokyo Night"));

    // Contradictory --ai full --no-ai MUST exit 4.
    let contra = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "diff",
            "--user",
            "--ai",
            "full",
            "--no-ai",
        ])
        .output()
        .expect("diff contra");
    assert_eq!(contra.status.code(), Some(4));

    // Legacy profile name -p ai-profile-retina resolves to ai-copilot + ux retina.
    let legacy_plan = vsp_cmd()
        .args([
            "--catalog",
            catalog_arg,
            "--vscode-data-dir",
            fake_data.path().to_str().expect("utf8"),
            "--json",
            "diff",
            "-p",
            "ai-profile-retina",
        ])
        .output()
        .expect("diff legacy");
    assert_eq!(legacy_plan.status.code(), Some(0));
    let lval: Value = serde_json::from_slice(&legacy_plan.stdout).expect("parse legacy plan");
    assert_eq!(lval["target"]["profile"], "ai-copilot");
    assert_eq!(lval["target"]["ux"], "retina");
    let ext_changes = lval["extension_changes"].as_array().expect("ext changes");
    assert!(ext_changes.iter().any(|e| e["id"] == "github.copilot-chat"));
    assert!(!ext_changes
        .iter()
        .any(|e| e["id"] == "anthropic.claude-code"));
}

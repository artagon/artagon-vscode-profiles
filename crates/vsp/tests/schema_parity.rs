//! Dual-layout schema parity, embedded snapshot isolation, 4-tier lockfile,
//! dynamic publisher derivation, implicit toolchain preset, and strict
//! validation integration tests for v2.2 consolidated schema (`schema_loader`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tempfile::TempDir;

use vsp::legacy_loader::load_legacy_catalog;
use vsp::model::{ExtensionStatus, PinsLoadState, UxPreset, LEGACY_BUNDLE_ALIASES};
use vsp::policy::validate_repository;
use vsp::resolver::{
    build_dist, compile_canonical_profile_settings, compile_code_profile_bundle,
    compile_profile_extensions, compile_settings, compile_tasks, compile_workspace_extensions,
    resolve_profile_recipe, sha256_hex, to_canonical_json_string, RtkTaskMode, SettingsScope,
};
use vsp::schema_loader::{
    embedded_rtk_scripts, embedded_snapshot_files, load_embedded, load_embedded_schema,
    load_from_dir, load_from_snapshot, load_schema_from_dir, resolve_preset_or_toolchain,
    tier0_blocked_ids, LockEntryStatus,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/vsp lives two levels below repo root")
        .to_path_buf()
}

fn clone_embedded_map() -> BTreeMap<String, Vec<u8>> {
    embedded_snapshot_files()
        .iter()
        .map(|&(k, v)| (k.to_string(), v.to_vec()))
        .collect()
}

#[test]
fn parity_all_15_profiles_settings_extensions_and_bundles_byte_identical() {
    let root = repo_root();
    let legacy = load_legacy_catalog(&root).expect("load legacy catalog");
    let from_dir = load_from_dir(&root).expect("load v2.2 schema from dir");
    let embedded = load_embedded().expect("load embedded v2.2 snapshot");

    assert_eq!(legacy.profiles.len(), 15);
    assert_eq!(from_dir.profiles.len(), 15);
    assert_eq!(embedded.profiles.len(), 15);

    let legacy_names: Vec<&String> = legacy.profiles.keys().collect();
    let dir_names: Vec<&String> = from_dir.profiles.keys().collect();
    let emb_names: Vec<&String> = embedded.profiles.keys().collect();
    assert_eq!(legacy_names, dir_names);
    assert_eq!(legacy_names, emb_names);

    for profile_name in legacy.profiles.keys() {
        // 1. Canonical UX-agnostic profile settings byte-for-byte parity.
        let leg_settings = to_canonical_json_string(
            &compile_canonical_profile_settings(&legacy, profile_name).expect("legacy settings"),
        )
        .expect("ser legacy settings");
        let dir_settings = to_canonical_json_string(
            &compile_canonical_profile_settings(&from_dir, profile_name).expect("dir settings"),
        )
        .expect("ser dir settings");
        let emb_settings = to_canonical_json_string(
            &compile_canonical_profile_settings(&embedded, profile_name).expect("emb settings"),
        )
        .expect("ser emb settings");

        assert_eq!(
            dir_settings, leg_settings,
            "load_from_dir settings mismatch for profile '{profile_name}'"
        );
        assert_eq!(
            emb_settings, leg_settings,
            "load_embedded settings mismatch for profile '{profile_name}'"
        );

        // 2. Profile extensions byte-for-byte parity (and resolved module order parity).
        let leg_mods = resolve_profile_recipe(
            &legacy,
            profile_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )
        .expect("legacy resolve");
        let dir_mods = resolve_profile_recipe(
            &from_dir,
            profile_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )
        .expect("dir resolve");
        let emb_mods = resolve_profile_recipe(
            &embedded,
            profile_name,
            &[],
            &[],
            Some(UxPreset::Default),
            None,
        )
        .expect("emb resolve");
        assert_eq!(dir_mods, leg_mods, "resolved modules mismatch for '{profile_name}'");
        assert_eq!(emb_mods, leg_mods, "embedded resolved modules mismatch for '{profile_name}'");

        let leg_exts = to_canonical_json_string(
            &compile_profile_extensions(&legacy, &leg_mods).expect("legacy exts"),
        )
        .expect("ser legacy exts");
        let dir_exts = to_canonical_json_string(
            &compile_profile_extensions(&from_dir, &dir_mods).expect("dir exts"),
        )
        .expect("ser dir exts");
        let emb_exts = to_canonical_json_string(
            &compile_profile_extensions(&embedded, &emb_mods).expect("emb exts"),
        )
        .expect("ser emb exts");
        assert_eq!(dir_exts, leg_exts, "dir extensions mismatch for '{profile_name}'");
        assert_eq!(emb_exts, leg_exts, "embedded extensions mismatch for '{profile_name}'");

        // 3. Workspace settings, workspace extensions, and tasks parity.
        let leg_ws = to_canonical_json_string(
            &compile_settings(&legacy, SettingsScope::Workspace, &leg_mods, None)
                .expect("leg ws"),
        )
        .expect("ser leg ws");
        let dir_ws = to_canonical_json_string(
            &compile_settings(&from_dir, SettingsScope::Workspace, &dir_mods, None)
                .expect("dir ws"),
        )
        .expect("ser dir ws");
        let emb_ws = to_canonical_json_string(
            &compile_settings(&embedded, SettingsScope::Workspace, &emb_mods, None)
                .expect("emb ws"),
        )
        .expect("ser emb ws");
        assert_eq!(dir_ws, leg_ws, "dir workspace settings mismatch for '{profile_name}'");
        assert_eq!(emb_ws, leg_ws, "emb workspace settings mismatch for '{profile_name}'");

        let leg_wext = compile_workspace_extensions(&legacy, &leg_mods).expect("leg wext");
        let dir_wext = compile_workspace_extensions(&from_dir, &dir_mods).expect("dir wext");
        let emb_wext = compile_workspace_extensions(&embedded, &emb_mods).expect("emb wext");
        assert_eq!(dir_wext, leg_wext, "dir workspace extensions mismatch for '{profile_name}'");
        assert_eq!(emb_wext, leg_wext, "emb workspace extensions mismatch for '{profile_name}'");

        let leg_tasks = compile_tasks(&legacy, RtkTaskMode::Plain, &leg_mods).expect("leg tasks");
        let dir_tasks = compile_tasks(&from_dir, RtkTaskMode::Plain, &dir_mods).expect("dir tasks");
        let emb_tasks = compile_tasks(&embedded, RtkTaskMode::Plain, &emb_mods).expect("emb tasks");
        assert_eq!(dir_tasks, leg_tasks, "dir tasks mismatch for '{profile_name}'");
        assert_eq!(emb_tasks, leg_tasks, "emb tasks mismatch for '{profile_name}'");

        // 4. Exported `.code-profile` bundle byte-for-byte parity.
        let leg_bundle = to_canonical_json_string(
            &compile_code_profile_bundle(&legacy, profile_name).expect("leg bundle"),
        )
        .expect("ser leg bundle");
        let dir_bundle = to_canonical_json_string(
            &compile_code_profile_bundle(&from_dir, profile_name).expect("dir bundle"),
        )
        .expect("ser dir bundle");
        let emb_bundle = to_canonical_json_string(
            &compile_code_profile_bundle(&embedded, profile_name).expect("emb bundle"),
        )
        .expect("ser emb bundle");
        assert_eq!(dir_bundle, leg_bundle, "dir bundle mismatch for '{profile_name}'");
        assert_eq!(emb_bundle, leg_bundle, "emb bundle mismatch for '{profile_name}'");
    }

    // Decision D5 & shell == general SHA-256 verification.
    let ai_settings =
        compile_canonical_profile_settings(&embedded, "ai").expect("ai canonical settings");
    assert_eq!(
        ai_settings.get("chat.editor.fontSize"),
        Some(&serde_json::json!(14)),
        "Decision D5: ai must preserve chat.editor.fontSize=14"
    );
    let general_json = to_canonical_json_string(
        &compile_canonical_profile_settings(&embedded, "general").expect("general"),
    )
    .expect("ser general");
    let shell_json = to_canonical_json_string(
        &compile_canonical_profile_settings(&embedded, "shell").expect("shell"),
    )
    .expect("ser shell");
    assert_eq!(
        sha256_hex(general_json.as_bytes()),
        sha256_hex(shell_json.as_bytes()),
        "shell preset must produce identical SHA-256 to general"
    );
}

#[test]
fn parity_37_entry_dev_dist_sha256sums_and_22_legacy_aliases_identical() {
    let root = repo_root();
    let legacy = load_legacy_catalog(&root).expect("legacy");
    let from_dir = load_from_dir(&root).expect("from_dir");
    let embedded = load_embedded().expect("embedded");

    let d_leg = TempDir::new().expect("d_leg");
    let d_dir = TempDir::new().expect("d_dir");
    let d_emb = TempDir::new().expect("d_emb");

    let r_leg = build_dist(&legacy, d_leg.path()).expect("dist legacy");
    let r_dir = build_dist(&from_dir, d_dir.path()).expect("dist dir");
    let r_emb = build_dist(&embedded, d_emb.path()).expect("dist emb");

    assert_eq!(r_leg.checksums.len(), 37);
    assert_eq!(r_dir.checksums.len(), 37);
    assert_eq!(r_emb.checksums.len(), 37);

    let sums_leg = fs::read(&r_leg.sha256sums_path).expect("read sums leg");
    let sums_dir = fs::read(&r_dir.sha256sums_path).expect("read sums dir");
    let sums_emb = fs::read(&r_emb.sha256sums_path).expect("read sums emb");

    assert_eq!(sums_dir, sums_leg, "load_from_dir SHA256SUMS drifted from legacy_loader");
    assert_eq!(sums_emb, sums_leg, "load_embedded SHA256SUMS drifted from legacy_loader");

    // Anchor embedded + dir SHA256SUMS against the snapshotted v2.1 golden oracle fixture!
    let oracle_raw = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v2_1_golden_sha256.json"),
    )
    .expect("read v2_1_golden_sha256.json");
    let oracle: Value = serde_json::from_str(&oracle_raw).expect("parse golden oracle");
    assert_eq!(
        sha256_hex(&sums_emb),
        oracle["dist_sha256sums_sha256"]
            .as_str()
            .expect("dist_sha256sums_sha256"),
        "embedded v2.2 SHA256SUMS drifted from v2.1 golden oracle"
    );

    // Verify all 22 legacy aliases in presets.jsonc match LEGACY_BUNDLE_ALIASES
    // and ai-profile-{crisp,retina} map strictly to ai-copilot with ZERO non-Copilot AI IDs.
    let schema = load_embedded_schema().expect("embedded schema");
    assert_eq!(schema.legacy_aliases.len(), 22);
    for &(alias_file, canonical_file) in LEGACY_BUNDLE_ALIASES {
        let alias_key = alias_file.strip_suffix(".code-profile").expect("suffix");
        let canonical_name = canonical_file.strip_suffix(".code-profile").expect("suffix");
        let entry = schema
            .legacy_aliases
            .get(alias_key)
            .unwrap_or_else(|| panic!("missing legacy alias '{alias_key}' in presets.jsonc"));
        assert_eq!(entry.preset, canonical_name);
    }

    let forbidden_ai = [
        "google.google-antigravity",
        "anthropic.claude-code",
        "openai.chatgpt",
        "google.geminicodeassist",
        "github.copilot",
    ];
    for copilot_alias in ["ai-profile-crisp", "ai-profile-retina"] {
        let resolved =
            resolve_preset_or_toolchain(&schema, copilot_alias).expect("resolve copilot alias");
        assert_eq!(resolved.recipe.name, "ai-copilot");
        let bundle = compile_code_profile_bundle(&schema.catalog, &resolved.recipe.name)
            .expect("compile ai-copilot bundle");
        assert!(bundle
            .extensions
            .enabled
            .iter()
            .any(|id| id.eq_ignore_ascii_case("github.copilot-chat")));
        for bad in forbidden_ai {
            assert!(
                !bundle
                    .extensions
                    .enabled
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(bad)),
                "{copilot_alias} leaked forbidden AI ID '{bad}'"
            );
        }
    }
}

#[test]
fn derived_publishers_match_catalog_publishers_json_100_percent() {
    let root = repo_root();
    let legacy = load_legacy_catalog(&root).expect("legacy");
    let dir_schema = load_schema_from_dir(&root).expect("dir schema");
    let emb_schema = load_embedded_schema().expect("emb schema");

    assert_eq!(dir_schema.blocked_publishers, vec!["jscearcy".to_string()]);
    assert_eq!(
        dir_schema.catalog.publishers.publishers,
        legacy.publishers.publishers,
        "derived publishers from catalog/extensions.jsonc must match catalog/publishers.json 100%"
    );
    assert_eq!(
        emb_schema.catalog.publishers.publishers,
        legacy.publishers.publishers
    );
    assert_eq!(emb_schema.catalog.publishers.publishers.len(), 82);
}

#[test]
fn four_tier_lockfile_covers_all_103_active_and_preserves_8_universal_pins_verbatim() {
    let root = repo_root();
    let legacy = load_legacy_catalog(&root).expect("legacy");
    let schema = load_embedded_schema().expect("embedded schema");

    let active_ids: BTreeSet<String> = schema
        .catalog
        .extensions
        .extensions
        .iter()
        .filter(|(_, r)| r.status == ExtensionStatus::Active)
        .map(|(k, _)| k.clone())
        .collect();
    assert_eq!(active_ids.len(), 103, "expected exactly 103 active catalog extensions");
    assert_eq!(schema.lockfile.extensions.len(), 103);

    let mut t1 = 0usize;
    let mut t1_verified = 0usize;
    let mut t1_pending = 0usize;
    let mut t2 = 0usize;
    let mut t3 = 0usize;

    for id in &active_ids {
        let entry = schema
            .lockfile
            .extensions
            .get(id)
            .unwrap_or_else(|| panic!("active extension '{id}' missing from vsp.lock.json"));
        match entry.tier {
            1 => {
                t1 += 1;
                match entry.status {
                    Some(LockEntryStatus::Verified) => t1_verified += 1,
                    Some(LockEntryStatus::PendingRefresh) => t1_pending += 1,
                    None => panic!("Tier 1 entry '{id}' must specify status"),
                }
            }
            2 => t2 += 1,
            3 => t3 += 1,
            other => panic!("unexpected lock tier {other} for '{id}'"),
        }
    }

    assert_eq!(t1, 28, "Tier 1 must cover exact 28 hash-locked IDs");
    assert_eq!(t1_verified, 8, "all 8 existing universal pins must be verified");
    assert_eq!(t1_pending, 20, "remaining 20 Tier 1 IDs must be pending_refresh");
    assert!(t2 <= 51, "Tier 2 count ({t2}) must be <= 51");
    assert!(t3 >= 24, "Tier 3 count ({t3}) must be >= 24");
    assert_eq!(t1 + t2 + t3, 103);

    // Tier 0 (compromised + removed) stay in catalog/extensions.jsonc, NOT in vsp.lock.json.
    let tier0 = tier0_blocked_ids(&schema);
    assert!(tier0.contains(&"jscearcy.rust-doc-viewer".to_string()));
    assert_eq!(tier0.len(), 7, "1 compromised + 6 removed = 7 Tier 0 IDs");
    for blocked in &tier0 {
        assert!(!schema.lockfile.extensions.contains_key(blocked));
    }

    // All 8 existing universal SHA-256 VSIX pins must match catalog/extension-pins.json verbatim!
    let PinsLoadState::Loaded(legacy_pins) = &legacy.pins else {
        panic!("legacy pins should be loaded");
    };
    let PinsLoadState::Loaded(new_pins) = &schema.catalog.pins else {
        panic!("v2.2 mapped pins should be loaded");
    };
    assert_eq!(legacy_pins.pins.len(), 8);
    assert_eq!(new_pins.pins.len(), 8);
    for (id, old_pin) in &legacy_pins.pins {
        let new_pin = new_pins
            .pins
            .get(id)
            .unwrap_or_else(|| panic!("missing mapped pin '{id}'"));
        assert_eq!(new_pin.version, old_pin.version, "version mismatch for {id}");
        assert_eq!(new_pin.sha256, old_pin.sha256, "sha256 mismatch for {id}");
        assert_eq!(new_pin.vsix_url, old_pin.vsix_url, "vsix_url mismatch for {id}");
        assert_eq!(
            new_pin.verified_at, old_pin.verified_at,
            "verified_at mismatch for {id}"
        );

        let lock_entry = schema.lockfile.extensions.get(id).expect("lock entry");
        assert_eq!(lock_entry.universal, Some(true));
        assert_eq!(lock_entry.status, Some(LockEntryStatus::Verified));
        assert_eq!(lock_entry.version.as_deref(), Some(old_pin.version.as_str()));
        assert_eq!(lock_entry.sha256.as_deref(), Some(old_pin.sha256.as_str()));
        assert_eq!(
            lock_entry.vsix_url.as_deref(),
            Some(old_pin.vsix_url.as_str())
        );
        assert_eq!(lock_entry.verified_at, old_pin.verified_at);
    }
}

#[test]
fn v2_catalog_passes_policy_validate_repository_and_has_no_default_pack_file() {
    let root = repo_root();
    assert!(
        !root.join("packs/default.jsonc").exists(),
        "packs/default.jsonc must NOT exist (virtual default UX)"
    );
    assert!(
        !root.join("packs/default").exists(),
        "packs/default directory must NOT exist"
    );

    let catalog = load_from_dir(&root).expect("load_from_dir");
    // Virtual default UX resolves cleanly to empty `{}` profile settings.
    let default_ux = catalog.modules.get("default").expect("virtual default UX");
    assert_eq!(
        default_ux.profile_settings.as_ref().map(serde_json::Map::len),
        Some(0)
    );

    // Full repository validator passes with 0 violations on v2.2 catalog!
    let report = validate_repository(&catalog);
    assert!(
        report.is_ok(),
        "validate_repository failed on v2.2 catalog: {:#?}",
        report.violations
    );

    // Declarative retireOnSwitch metadata loaded for mutually exclusive toolchains.
    let schema = load_schema_from_dir(&root).expect("load_schema_from_dir");
    let biome_retire = schema
        .retire_on_switch
        .get("biome")
        .expect("biome retireOnSwitch");
    assert!(biome_retire
        .extensions
        .contains(&"dbaeumer.vscode-eslint".to_string()));
    assert!(biome_retire
        .settings
        .contains(&"eslint.validate".to_string()));

    let eslint_retire = schema
        .retire_on_switch
        .get("ts-eslint-prettier")
        .expect("ts-eslint-prettier retireOnSwitch");
    assert!(eslint_retire
        .extensions
        .contains(&"biomejs.biome".to_string()));
}

#[test]
fn embedded_snapshot_loads_cleanly_from_empty_temp_cwd_and_includes_7_rtk_scripts() {
    let root = repo_root();

    // Verify all 7 RTK scripts are embedded verbatim matching modules/capabilities/rtk-terminal/scripts/.
    let scripts = embedded_rtk_scripts();
    assert_eq!(scripts.len(), 7, "expected exact 7 embedded RTK scripts");
    for (rel_path, embedded_bytes) in &scripts {
        let suffix = rel_path
            .strip_prefix("packs/rtk-terminal/scripts/")
            .expect("rtk script prefix");
        let legacy_file = root
            .join("modules/capabilities/rtk-terminal/scripts")
            .join(suffix);
        let legacy_bytes = fs::read(&legacy_file)
            .unwrap_or_else(|e| panic!("read legacy {}: {e}", legacy_file.display()));
        assert_eq!(
            *embedded_bytes,
            legacy_bytes.as_slice(),
            "embedded RTK script '{rel_path}' differs from legacy copy"
        );
    }

    // Verify embedded snapshot loads cleanly even when CWD is a completely empty temp directory.
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    let original_cwd = std::env::current_dir().expect("get cwd");
    let _guard = CwdGuard(original_cwd);
    let empty_dir = TempDir::new().expect("empty cwd");
    std::env::set_current_dir(empty_dir.path()).expect("chdir empty");
    assert_eq!(
        fs::read_dir(".").expect("read empty cwd").count(),
        0,
        "temp cwd must contain zero files"
    );

    let schema = load_embedded_schema().expect("embedded snapshot must load from empty temp cwd");
    let catalog = load_embedded().expect("load_embedded must succeed from empty temp cwd");
    drop(_guard);

    assert_eq!(schema.catalog.profiles.len(), 15);
    assert_eq!(catalog.profiles.len(), 15);
    assert_eq!(schema.catalog.modules.len(), 32); // 31 packs + virtual default UX
    assert_eq!(schema.pack_scripts["rtk-terminal"].len(), 7);
}

#[test]
fn implicit_toolchain_presets_resolve_as_general_plus_toolchain() {
    let schema = load_embedded_schema().expect("embedded schema");

    // `biome` and `python-ty` are toolchain packs not explicitly listed in presets.jsonc.
    assert!(!schema.catalog.profiles.contains_key("biome"));
    let biome_preset =
        resolve_preset_or_toolchain(&schema, "biome").expect("implicit biome preset");
    assert!(biome_preset.is_implicit_toolchain);
    assert_eq!(biome_preset.recipe.name, "biome");
    assert!(biome_preset.recipe.use_defaults);
    assert_eq!(biome_preset.recipe.modules, vec!["biome".to_string()]);

    let py_ty_preset =
        resolve_preset_or_toolchain(&schema, "python-ty").expect("implicit python-ty preset");
    assert!(py_ty_preset.is_implicit_toolchain);
    assert_eq!(py_ty_preset.recipe.modules, vec!["python-ty".to_string()]);

    // Non-toolchain capability packs (like `dx-pro`) do NOT become implicit presets.
    assert!(resolve_preset_or_toolchain(&schema, "dx-pro").is_none());
}

#[test]
fn strict_validator_rejects_incomplete_bad_version_duplicate_id_and_escape_without_fallback() {
    // 1. Missing external dir -> TargetMissing (exit 2), never falls back to embedded!
    let tmp = TempDir::new().expect("tmp");
    let missing = tmp.path().join("nonexistent-catalog");
    let err = load_from_dir(&missing).expect_err("missing dir must fail");
    assert_eq!(err.exit_code().as_i32(), 2);

    // 2. Empty external dir -> UsageOrConfig (exit 4), never falls back to embedded!
    let empty = TempDir::new().expect("empty");
    let err_empty = load_from_dir(empty.path()).expect_err("empty dir must fail");
    assert_eq!(err_empty.exit_code().as_i32(), 4);

    // 3. Incomplete snapshot (missing vsp.lock.json) -> UsageOrConfig (exit 4).
    let mut missing_lock = clone_embedded_map();
    missing_lock.remove("vsp.lock.json");
    let err_lock =
        load_from_snapshot(PathBuf::from("<test>"), &missing_lock).expect_err("missing lockfile");
    assert_eq!(err_lock.exit_code().as_i32(), 4);

    // 4. Unknown schema version -> UsageOrConfig (exit 4).
    let mut bad_ver = clone_embedded_map();
    let mut pol: Value = serde_json::from_str(
        &std::str::from_utf8(&bad_ver["catalog/policy.jsonc"])
            .expect("utf8")
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .expect("parse policy");
    pol["version"] = serde_json::json!(99);
    bad_ver.insert(
        "catalog/policy.jsonc".to_string(),
        serde_json::to_vec(&pol).expect("ser"),
    );
    let err_ver =
        load_from_snapshot(PathBuf::from("<test>"), &bad_ver).expect_err("bad schema version");
    assert_eq!(err_ver.exit_code().as_i32(), 4);

    // 5. Duplicate pack ID (`packs/rust.jsonc` + `packs/rust/rust.jsonc`) -> UsageOrConfig (exit 4).
    let mut dup = clone_embedded_map();
    let rust_bytes = dup["packs/rust.jsonc"].clone();
    dup.insert("packs/rust/rust.jsonc".to_string(), rust_bytes);
    let err_dup = load_from_snapshot(PathBuf::from("<test>"), &dup).expect_err("duplicate pack id");
    assert_eq!(err_dup.exit_code().as_i32(), 4);

    // 6. Out-of-root snapshot path -> SafetyRefusal (exit 5).
    let mut escape = clone_embedded_map();
    escape.insert("../outside.jsonc".to_string(), b"{}".to_vec());
    let err_esc =
        load_from_snapshot(PathBuf::from("<test>"), &escape).expect_err("out-of-root path");
    assert_eq!(err_esc.exit_code().as_i32(), 5);

    // 7. Dummy `packs/default.jsonc` rejected -> UsageOrConfig (exit 4).
    let mut dummy_default = clone_embedded_map();
    dummy_default.insert(
        "packs/default.jsonc".to_string(),
        br#"{"version":2,"id":"default","kind":"ux"}"#.to_vec(),
    );
    let err_def =
        load_from_snapshot(PathBuf::from("<test>"), &dummy_default).expect_err("dummy default");
    assert_eq!(err_def.exit_code().as_i32(), 4);

    // 8. Missing active extension from vsp.lock.json -> UsageOrConfig (exit 4).
    let mut missing_entry = clone_embedded_map();
    let mut lock_val: Value =
        serde_json::from_slice(&missing_entry["vsp.lock.json"]).expect("parse lock");
    lock_val["extensions"]
        .as_object_mut()
        .expect("exts obj")
        .remove("rust-lang.rust-analyzer");
    missing_entry.insert(
        "vsp.lock.json".to_string(),
        serde_json::to_vec(&lock_val).expect("ser lock"),
    );
    let err_missing_lock_ext =
        load_from_snapshot(PathBuf::from("<test>"), &missing_entry).expect_err("missing lock ext");
    assert_eq!(err_missing_lock_ext.exit_code().as_i32(), 4);

    // 9. Spoofed non-Marketplace VSIX URL in verified Tier 1 pin -> SafetyRefusal (exit 5).
    let mut spoofed_url = clone_embedded_map();
    let mut lock_spoof: Value =
        serde_json::from_slice(&spoofed_url["vsp.lock.json"]).expect("parse lock");
    lock_spoof["extensions"]["enkia.tokyo-night"]["vsixUrl"] =
        serde_json::json!("https://evil.example/enkia.tokyo-night.vsix");
    spoofed_url.insert(
        "vsp.lock.json".to_string(),
        serde_json::to_vec(&lock_spoof).expect("ser lock"),
    );
    let err_spoof =
        load_from_snapshot(PathBuf::from("<test>"), &spoofed_url).expect_err("spoofed vsix url");
    assert_eq!(err_spoof.exit_code().as_i32(), 5);

    // 10. Security invariant: redirecting ai-profile-crisp away from ai-copilot -> SafetyRefusal (exit 5).
    let mut bad_alias = clone_embedded_map();
    let presets_txt = std::str::from_utf8(&bad_alias["presets.jsonc"])
        .expect("utf8")
        .replace(
            r#""ai-profile-crisp": { "preset": "ai-copilot", "ux": "crisp" }"#,
            r#""ai-profile-crisp": { "preset": "ai", "ux": "crisp" }"#,
        );
    bad_alias.insert("presets.jsonc".to_string(), presets_txt.into_bytes());
    let err_alias =
        load_from_snapshot(PathBuf::from("<test>"), &bad_alias).expect_err("bad ai-profile alias");
    assert_eq!(err_alias.exit_code().as_i32(), 5);

    // 11. Preset `extends` cycle -> UsageOrConfig (exit 4).
    let mut cyclic = clone_embedded_map();
    let cyc_presets = serde_json::json!({
        "version": 2,
        "presets": {
            "a": { "extends": "b", "packs": [] },
            "b": { "extends": "a", "packs": [] }
        },
        "legacyAliases": {}
    });
    cyclic.insert(
        "presets.jsonc".to_string(),
        serde_json::to_vec(&cyc_presets).expect("ser presets"),
    );
    let err_cyc =
        load_from_snapshot(PathBuf::from("<test>"), &cyclic).expect_err("cyclic presets");
    assert_eq!(err_cyc.exit_code().as_i32(), 4);

    // 12. Active extension from blocked publisher -> SafetyRefusal (exit 5).
    let mut blocked_active = clone_embedded_map();
    let exts_txt = std::str::from_utf8(&blocked_active["catalog/extensions.jsonc"])
        .expect("utf8")
        .replacen(r#""jscearcy""#, r#""rust-lang""#, 1);
    blocked_active.insert(
        "catalog/extensions.jsonc".to_string(),
        exts_txt.into_bytes(),
    );
    let err_blocked =
        load_from_snapshot(PathBuf::from("<test>"), &blocked_active).expect_err("blocked active");
    assert_eq!(err_blocked.exit_code().as_i32(), 5);
}

#[cfg(unix)]
#[test]
fn load_from_dir_rejects_symlink_escaping_catalog_root() {
    use std::os::unix::fs::symlink;

    let root = repo_root();
    let tmp = TempDir::new().expect("tmp");
    let cat_root = tmp.path().join("cat");
    let outside = tmp.path().join("outside-secret.jsonc");
    fs::write(&outside, r#"{"version":2,"id":"escape","kind":"capability"}"#).expect("write");

    fs::create_dir_all(cat_root.join("catalog")).expect("mkdir catalog");
    fs::create_dir_all(cat_root.join("packs")).expect("mkdir packs");
    for rel in [
        "catalog/extensions.jsonc",
        "catalog/policy.jsonc",
        "presets.jsonc",
        "vsp.lock.json",
    ] {
        fs::copy(root.join(rel), cat_root.join(rel)).expect("copy core");
    }
    symlink(&outside, cat_root.join("packs/escape.jsonc")).expect("symlink");

    let err = load_from_dir(&cat_root).expect_err("symlink escape must be rejected");
    assert_eq!(err.exit_code().as_i32(), 5);
}

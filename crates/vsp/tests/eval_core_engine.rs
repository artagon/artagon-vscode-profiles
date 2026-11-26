//! Wave 1 core engine, schema retention, policy wiring, dry-run export/dist,
//! bounded workspace detection, indexed CST merge, provenance backward-compatibility,
//! and structured error-kind regression tests.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use tempfile::TempDir;

use vsp::cst_merge::{merge_jsonc_object_str, CstMergeOptions};
use vsp::detect::{detect_workspace, MAX_DETECT_READ_BYTES};
use vsp::exit_codes::{ExitCode, VspError};
use vsp::jsonc::{
    parse_jsonc_bytes, parse_jsonc_settings_object, parse_jsonc_value, read_strict_json_value,
};
use vsp::legacy_loader::{self, load_legacy_catalog};
use vsp::model::{PinsLoadState, UxPreset};
use vsp::policy::{
    check_autonomy_denylist_with_policy, check_default_profile_expectations,
    matches_autonomy_pattern, validate_repository,
};
use vsp::provenance::ProfileProvenance;
use vsp::resolver::{
    build_dist, build_dist_with_options, export_profiles, export_profiles_with_options,
    BundleAdditions,
};
use vsp::schema_loader::{
    load_embedded, load_embedded_schema, load_from_dir, AutonomyDenyListPolicy,
    DefaultProfileExpectation,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/vsp lives two levels below repo root")
        .to_path_buf()
}

#[test]
fn neutral_jsonc_module_and_legacy_reexports_behave_identically() {
    let raw = r#"// Leading comment
{
  "$schema": "https://example.invalid/schema",
  "@extends": "./base.jsonc",
  /* Block comment */
  "editor.fontSize": 14,
}
"#;
    let v1 = parse_jsonc_value(raw, "direct").expect("parse direct");
    let v2 = legacy_loader::parse_jsonc_value(raw, "reexport").expect("parse reexport");
    assert_eq!(v1, v2);

    let typed: Value = parse_jsonc_bytes(raw.as_bytes(), "bytes").expect("parse bytes");
    assert_eq!(typed, v1);

    let tmp = TempDir::new().expect("tmp");
    let jsonc_path = tmp.path().join("settings.jsonc");
    fs::write(&jsonc_path, raw).expect("write jsonc");
    let obj1 = parse_jsonc_settings_object(&jsonc_path).expect("settings object");
    let obj2 = legacy_loader::parse_jsonc_settings_object(&jsonc_path).expect("reexport obj");
    assert_eq!(obj1, obj2);
    assert!(!obj1.contains_key("$schema"));
    assert!(!obj1.contains_key("@extends"));
    assert_eq!(obj1.get("editor.fontSize"), Some(&json!(14)));

    let strict_path = tmp.path().join("strict.json");
    fs::write(&strict_path, r#"{"ok": true}"#).expect("write strict");
    let s1 = read_strict_json_value(&strict_path, false).expect("strict direct");
    let s2 = legacy_loader::read_strict_json_value(&strict_path, false).expect("strict reexport");
    assert_eq!(s1, s2);
}

#[test]
fn workspace_catalog_retains_loaded_schema_policy_retire_and_lockfile() {
    let schema = load_embedded_schema().expect("embedded schema");
    let embedded = load_embedded().expect("embedded catalog");
    let from_dir = load_from_dir(&repo_root()).expect("dir catalog");

    for (label, catalog) in [("embedded", &embedded), ("from_dir", &from_dir)] {
        assert!(catalog.is_v2_layout(), "{label} must be v2 layout");

        let policy = catalog
            .policy
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: catalog.policy must be retained"));
        assert_eq!(policy.vscode_version, schema.policy.vscode_version);
        assert_eq!(
            policy.autonomy_deny_list, schema.policy.autonomy_deny_list,
            "{label}: autonomyDenyList mismatch"
        );
        // Verify catalog/policy.jsonc alignment with policy.rs floor.
        let perm_modes = policy
            .autonomy_deny_list
            .deny_values
            .get("claudeCode.initialPermissionMode")
            .expect("claudeCode.initialPermissionMode in denyValues");
        assert!(perm_modes.iter().any(|m| m == "bypassPermissions"));
        assert!(perm_modes.iter().any(|m| m == "acceptEdits"));
        for prefix in [
            "terminal.integrated.profiles",
            "terminal.integrated.env",
            "terminal.integrated.automationProfile",
            "terminal.integrated.defaultProfile",
        ] {
            assert!(
                policy
                    .autonomy_deny_list
                    .presence_key_prefixes
                    .iter()
                    .any(|p| p == prefix),
                "{label}: missing presenceKeyPrefixes entry '{prefix}'"
            );
        }

        // Verify retireOnSwitch metadata is retained on WorkspaceCatalog.
        assert_eq!(
            catalog.retire_on_switch, schema.retire_on_switch,
            "{label}: retire_on_switch mismatch"
        );
        assert!(catalog.retire_on_switch.contains_key("biome"));
        assert!(catalog.retire_on_switch.contains_key("ts-eslint-prettier"));

        // Verify full 4-tier LockfileManifest is retained on WorkspaceCatalog.
        let lockfile = catalog
            .lockfile
            .as_ref()
            .unwrap_or_else(|| panic!("{label}: catalog.lockfile must be retained"));
        assert_eq!(lockfile, &schema.lockfile);
        assert_eq!(lockfile.extensions.len(), 103);

        // Verify groupOrder is retained on WorkspaceCatalog.
        assert_eq!(catalog.group_order, schema.group_order);
    }

    // Legacy catalog retains None/empty for v2-only fields and reports legacy layout.
    let legacy = load_legacy_catalog(&repo_root()).expect("legacy catalog");
    assert!(!legacy.is_v2_layout());
    assert!(legacy.policy.is_none());
    assert!(legacy.lockfile.is_none());
    assert!(legacy.retire_on_switch.is_empty());
}

#[test]
fn presets_legacy_aliases_are_single_source_of_truth_for_v2_catalog() {
    let mut v2 = load_embedded().expect("embedded");
    assert_eq!(
        v2.resolve_alias("ai-profile-crisp"),
        Some(("ai-copilot".to_string(), UxPreset::Crisp))
    );
    assert_eq!(v2.bundle_alias_pairs().len(), 22);

    // Removing an alias from v2 legacy_aliases must NOT silently fall back to the legacy table.
    v2.legacy_aliases.remove("rust-profile-crisp");
    assert_eq!(v2.resolve_alias("rust-profile-crisp"), None);
    assert_eq!(v2.bundle_alias_pairs().len(), 21);
    assert!(!v2
        .bundle_alias_pairs()
        .iter()
        .any(|(a, _)| a == "rust-profile-crisp.code-profile"));

    // Adding a custom alias in v2 presets.jsonc works in both resolve_alias and bundle_alias_pairs.
    v2.legacy_aliases.insert(
        "custom-rust-crisp".to_string(),
        ("rust".to_string(), UxPreset::Crisp),
    );
    assert_eq!(
        v2.resolve_alias("custom-rust-crisp"),
        Some(("rust".to_string(), UxPreset::Crisp))
    );
    assert!(v2
        .bundle_alias_pairs()
        .iter()
        .any(|(a, c)| { a == "custom-rust-crisp.code-profile" && c == "rust.code-profile" }));

    // Legacy layout still uses the built-in fallback table.
    let legacy = load_legacy_catalog(&repo_root()).expect("legacy");
    assert!(legacy.legacy_aliases.is_empty());
    assert_eq!(
        legacy.resolve_alias("rust-profile-crisp"),
        Some(("rust".to_string(), UxPreset::Crisp))
    );
    assert_eq!(legacy.bundle_alias_pairs().len(), 22);
}

#[test]
fn export_and_dist_prevalidate_names_and_support_zero_write_dry_run() {
    let catalog = load_embedded().expect("embedded");
    let tmp = TempDir::new().expect("tmp");

    // 1. Unknown profile name MUST fail BEFORE creating out_dir.
    let invalid_out = tmp.path().join("must-not-be-created");
    let err = export_profiles(
        &catalog,
        &["nonexistent-profile".to_string()],
        &invalid_out,
        false,
    )
    .expect_err("unknown profile must fail");
    assert_eq!(err.exit_code(), ExitCode::TargetMissing);
    assert!(
        !invalid_out.exists(),
        "export_profiles must not create out_dir when a profile name is invalid"
    );

    // Mixed valid + invalid profile list must also fail with zero directory creation.
    let mixed_out = tmp.path().join("mixed-invalid-out");
    let err = export_profiles_with_options(
        &catalog,
        &["rust".to_string(), "definitely-missing".to_string()],
        &mixed_out,
        false,
        &BTreeMap::new(),
        false,
    )
    .expect_err("mixed invalid list must fail");
    assert_eq!(err.exit_code(), ExitCode::TargetMissing);
    assert!(
        !mixed_out.exists(),
        "mixed invalid export must not create out_dir"
    );

    // 2. export_profiles_with_options(..., dry_run = true) writes ZERO files/dirs.
    let dry_export_dir = tmp.path().join("dry-export");
    let mut additions = BTreeMap::new();
    additions.insert(
        "rust".to_string(),
        BundleAdditions {
            settings: Map::new(),
            extensions: vec!["example.extra".to_string()],
        },
    );
    let dry_export = export_profiles_with_options(
        &catalog,
        &["rust".to_string()],
        &dry_export_dir,
        false,
        &additions,
        true,
    )
    .expect("dry export");
    assert!(
        !dry_export_dir.exists(),
        "export dry_run=true must not create out_dir"
    );
    assert_eq!(dry_export.canonical_bundles, vec!["rust.code-profile"]);
    assert!(dry_export.alias_bundles.is_empty());
    assert_eq!(dry_export.checksums.len(), 1);
    assert_eq!(
        dry_export.planned_paths(),
        vec![
            dry_export_dir.join("rust.code-profile"),
            dry_export_dir.join("SHA256SUMS"),
        ]
    );

    // 3. build_dist_with_options(..., dry_run = true) writes ZERO files/dirs and matches real dist.
    let dry_dist_dir = tmp.path().join("dry-dist");
    let dry_dist = build_dist_with_options(&catalog, &dry_dist_dir, true).expect("dry dist");
    assert!(
        !dry_dist_dir.exists(),
        "build_dist dry_run=true must not create out_dir"
    );
    assert_eq!(dry_dist.canonical_bundles.len(), 15);
    assert_eq!(dry_dist.alias_bundles.len(), 22);
    assert_eq!(dry_dist.checksums.len(), 37);
    assert_eq!(dry_dist.planned_paths().len(), 38);

    let real_dist_dir = tmp.path().join("real-dist");
    let real_dist = build_dist(&catalog, &real_dist_dir).expect("real dist");
    assert_eq!(
        dry_dist.checksums, real_dist.checksums,
        "dry_run dist checksums must match real build_dist byte-for-byte"
    );

    // 4. Stale bundle in dist dir is still refused under dry_run=true without modifying files.
    let stale_dir = tmp.path().join("stale-dist");
    fs::create_dir_all(&stale_dir).expect("mkdir stale");
    fs::write(stale_dir.join("obsolete.code-profile"), "{}\n").expect("write stale");
    let err = build_dist_with_options(&catalog, &stale_dir, true)
        .expect_err("stale bundle must fail closed even in dry_run");
    assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
    assert!(
        !stale_dir.join("SHA256SUMS").exists(),
        "failed dry_run must not write SHA256SUMS"
    );
}

#[test]
fn declarative_policy_drives_validation_without_weakening_safety_floor() {
    // Pattern matcher unit coverage.
    assert!(matches_autonomy_pattern(
        "^chat\\.tools\\..*autoApprove$",
        "chat.tools.terminal.autoApprove"
    ));
    assert!(matches_autonomy_pattern(
        "^chat\\.tools\\..*autoApprove$",
        "[rust].chat.tools.terminal.autoApprove"
    ));
    assert!(!matches_autonomy_pattern(
        "^chat\\.tools\\..*autoApprove$",
        "chat.tools.terminal.timeout"
    ));

    // Empty policy MUST NOT weaken the built-in floor.
    let empty_policy = AutonomyDenyListPolicy::default();
    for payload in [
        json!({"chat.tools.terminal.autoApprove": true}),
        json!({"claudeCode.initialPermissionMode": "acceptEdits"}),
        json!({"claudeCode.initialPermissionMode": "bypassPermissions"}),
        json!({"terminal.integrated.profiles.osx": {}}),
        json!({"claudeCode.claudeProcessWrapper": "/tmp/evil"}),
        json!({"some.arg": "--dangerously-skip-permissions"}),
    ] {
        assert!(
            !check_autonomy_denylist_with_policy(&payload, "floor", Some(&empty_policy)).is_empty(),
            "built-in floor must remain enforced even with empty policy: {payload}"
        );
    }

    // Declarative policy rules actively add enforcement on top of the floor.
    let custom_policy = AutonomyDenyListPolicy {
        patterns: vec!["^acme\\.agent\\..*autoRun$".to_string()],
        presence_keys: vec!["acme.shellWrapper".to_string()],
        presence_key_prefixes: vec!["acme.execProfiles".to_string()],
        deny_true_keys: vec!["acme.yoloEnabled".to_string()],
        deny_values: BTreeMap::from([("acme.mode".to_string(), vec!["unrestricted".to_string()])]),
        deny_value_substrings: vec!["--no-sandbox-override".to_string()],
    };
    for payload in [
        json!({"acme.agent.tasks.autoRun": true}),
        json!({"acme": {"shellWrapper": "/bin/sh"}}),
        json!({"acme.execProfiles.linux": {"path": "/bin/sh"}}),
        json!({"acme.yoloEnabled": "TRUE"}),
        json!({"acme.mode": "UNRESTRICTED"}),
        json!({"acme.args": ["--no-sandbox-override"]}),
    ] {
        assert!(
            !check_autonomy_denylist_with_policy(&payload, "custom", Some(&custom_policy))
                .is_empty(),
            "custom declarative policy rule must trigger on: {payload}"
        );
    }
    assert!(check_autonomy_denylist_with_policy(
        &json!({"acme.mode": "safe", "acme.yoloEnabled": false}),
        "custom",
        Some(&custom_policy)
    )
    .is_empty());

    // Default profile expectations helper enforces both floor and custom policy entries.
    let mut catalog = load_embedded().expect("embedded");
    let mut pol = catalog.policy.clone().expect("policy");
    pol.default_profile_expectations.insert(
        "update.mode".to_string(),
        DefaultProfileExpectation {
            expected: json!("manual"),
            value_type: "string".to_string(),
            reason: "Require manual updates".to_string(),
        },
    );
    let mut default_settings = Map::new();
    default_settings.insert("telemetry.telemetryLevel".to_string(), json!("off"));
    default_settings.insert(
        "security.workspace.trust.untrustedFiles".to_string(),
        json!("prompt"),
    );
    default_settings.insert("extensions.autoUpdate".to_string(), json!(false));
    let hits = check_default_profile_expectations(&default_settings, Some(&pol), "default");
    assert!(
        hits.iter().any(|v| v.message.contains("update.mode")),
        "custom defaultProfileExpectations entry must be checked: {hits:?}"
    );

    // Mutating catalog.policy.forbidden_keys drives validate_repository directly.
    catalog
        .policy
        .as_mut()
        .expect("policy")
        .forbidden_keys
        .push("workbench.sideBar.location".to_string());
    let report = validate_repository(&catalog);
    assert!(
        report
            .violations
            .iter()
            .any(|v| v.message.contains("workbench.sideBar.location")),
        "catalog.policy.forbidden_keys must drive validate_repository"
    );
}

#[test]
fn v2_validation_diagnostics_use_v2_layout_labels_not_legacy_paths() {
    let mut catalog = load_embedded().expect("embedded");
    // Break UX symmetry, a pack conflict, and pins state in the v2.2 catalog.
    catalog
        .modules
        .get_mut("crisp")
        .expect("crisp")
        .profile_settings
        .as_mut()
        .expect("prof")
        .insert("workbench.productIconTheme".to_string(), json!("wrong"));
    catalog
        .modules
        .get_mut("rust")
        .expect("rust")
        .manifest
        .requires
        .push("nonexistent-pack".to_string());
    catalog.pins = PinsLoadState::Missing(PathBuf::from("vsp.lock.json"));

    let report = validate_repository(&catalog);
    assert!(!report.is_ok());

    for v in &report.violations {
        assert!(
            !v.source.contains("module.json") && !v.source.contains("modules/ux"),
            "v2.2 diagnostic leaked legacy path in source: {v:?}"
        );
        assert!(
            !v.message.contains("modules/ux") && !v.message.contains("extension-pins.json"),
            "v2.2 diagnostic leaked legacy path in message: {v:?}"
        );
    }
    assert!(report
        .violations
        .iter()
        .any(|v| v.source.contains("packs/crisp.jsonc")));
    assert!(report
        .violations
        .iter()
        .any(|v| v.source.contains("packs/rust.jsonc")));
    assert!(report
        .violations
        .iter()
        .any(|v| v.message.contains("vsp.lock.json is missing")));
}

#[test]
fn collision_checker_detects_scalar_type_and_object_prefix_parity() {
    let mut v2 = load_embedded().expect("embedded");
    let mut legacy = load_legacy_catalog(&repo_root()).expect("legacy");

    for cat in [&mut v2, &mut legacy] {
        // 1. Scalar value mismatch across jointly selectable siblings (`git-pro` vs `dx-pro`).
        cat.modules
            .get_mut("git-pro")
            .expect("git-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("eval.scalar".to_string(), json!(1));
        cat.modules
            .get_mut("dx-pro")
            .expect("dx-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("eval.scalar".to_string(), json!("1"));

        // 2. Object-vs-leaf collision in reverse order (`git-pro` object vs `dx-pro` leaf).
        cat.modules
            .get_mut("git-pro")
            .expect("git-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("eval.nested".to_string(), json!({"a": true}));
        cat.modules
            .get_mut("dx-pro")
            .expect("dx-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("eval.nested".to_string(), json!("leaf"));

        // 3. Union array keys (`shellcheck.exclude`) must NOT be reported as sibling collisions.
        cat.modules
            .get_mut("git-pro")
            .expect("git-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("shellcheck.exclude".to_string(), json!(["SC1000"]));
        cat.modules
            .get_mut("dx-pro")
            .expect("dx-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("shellcheck.exclude".to_string(), json!(["SC2000"]));
    }

    let r_v2 = validate_repository(&v2);
    let r_leg = validate_repository(&legacy);

    for (label, report) in [("v2", &r_v2), ("legacy", &r_leg)] {
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.message.contains("sibling collision (pr)")
                    && v.message.contains("eval.scalar")),
            "{label}: missing scalar collision: {:?}",
            report.violations
        );
        assert!(
            report.violations.iter().any(|v| {
                v.message.contains("object-vs-leaf collision (pr)")
                    && v.message.contains("eval.nested")
            }),
            "{label}: missing object-vs-leaf collision: {:?}",
            report.violations
        );
        assert!(
            !report
                .violations
                .iter()
                .any(|v| v.message.contains("shellcheck.exclude")),
            "{label}: union array key should not trigger collision"
        );
    }
}

#[test]
fn detect_caps_file_reads_at_one_mib_and_shares_walk_across_rules() {
    assert_eq!(MAX_DETECT_READ_BYTES, 1024 * 1024);

    let catalog = load_embedded().expect("embedded");
    let tmp = TempDir::new().expect("tmp");
    let ws = tmp.path();

    // Create a Spring-Boot-shaped Java project where `@SpringBootApplication` only appears
    // AFTER the 1 MiB read cap — must NOT trigger `java-spring`, while `java-maven` still matches.
    fs::write(ws.join("pom.xml"), "<project/>").expect("write pom");
    fs::create_dir_all(ws.join("src/main/java")).expect("mkdir java");
    let mut big_past_cap = vec![b' '; (MAX_DETECT_READ_BYTES as usize) + 64];
    big_past_cap.extend_from_slice(b"\n@SpringBootApplication\n");
    fs::write(ws.join("src/main/java/HugeApplication.java"), &big_past_cap)
        .expect("write huge java");

    let res_past = detect_workspace(&catalog, ws).expect("detect past cap");
    assert!(res_past.toolchains.contains(&"java-maven".to_string()));
    assert!(
        !res_past.toolchains.contains(&"java-spring".to_string()),
        "pattern past 1 MiB cap must not be read"
    );

    // Place `@SpringBootApplication` within the first 1 MiB of a >1 MiB file -> MUST match!
    let mut big_within_cap = b"@SpringBootApplication\n".to_vec();
    big_within_cap.resize((MAX_DETECT_READ_BYTES as usize) + 1024, b' ');
    fs::write(
        ws.join("src/main/java/HugeApplication.java"),
        &big_within_cap,
    )
    .expect("rewrite huge java");

    // Also include a .bats file and Cargo.toml so multiple rules share the cached workspace walk.
    fs::write(ws.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("write cargo");
    fs::create_dir_all(ws.join("tests")).expect("mkdir tests");
    fs::write(ws.join("tests/first.bats"), "@test \"a\" { true; }\n").expect("write bats");

    let res_within = detect_workspace(&catalog, ws).expect("detect within cap");
    assert!(res_within.toolchains.contains(&"rust".to_string()));
    assert!(res_within.toolchains.contains(&"java-maven".to_string()));
    assert!(res_within.toolchains.contains(&"java-spring".to_string()));
    assert!(res_within.toolchains.contains(&"shell".to_string()));
}

#[test]
fn cst_merge_indexed_handles_large_objects_duplicates_comments_and_deletions() {
    let mut input = String::from("// Banner comment\n{\n");
    for i in 0..150 {
        input.push_str(&format!(
            "  // user comment {i}\n  \"user.key.{i:03}\": {i},\n"
        ));
    }
    input.push_str(
        r#"  // First duplicate of managed key
  "managed.dup": "first",
  "managed.dup": "second",
  /* Third effective duplicate */
  "managed.dup": "third",
  "escaped.\"key\"": "old",
  "[rust]": {
    // keep nested user comment
    "editor.tabSize": 2,
    "rust-analyzer.check.command": "check",
    "rust-analyzer.check.command": "clippy"
  },
  "shellcheck.exclude": [
    "SC1090" // keep array comment
  ],
  "retired.one": true,
  "retired.one": false
}
"#,
    );

    let desired_val = json!({
        "managed.dup": "managed-final",
        "escaped.\"key\"": "new\\value",
        "[rust]": {
            "rust-analyzer.check.command": "clippy-strict",
            "editor.formatOnSave": true
        },
        "shellcheck.exclude": ["SC1090", "SC2034"],
        "brand.new.key": 42
    });
    let desired = desired_val.as_object().expect("obj");
    let opts = CstMergeOptions {
        delete_keys: vec!["retired.one".to_string()],
        ..Default::default()
    };

    let merged = merge_jsonc_object_str(&input, desired, &opts).expect("indexed cst merge");
    assert!(merged.contains("// Banner comment"));
    assert!(merged.contains("// user comment 0"));
    assert!(merged.contains("// user comment 149"));
    assert!(merged.contains("// keep nested user comment"));
    assert!(merged.contains("// keep array comment"));
    assert_eq!(merged.matches("\"managed.dup\"").count(), 1);
    assert_eq!(merged.matches("\"rust-analyzer.check.command\"").count(), 1);
    assert!(!merged.contains("retired.one"));

    let parsed = parse_jsonc_value(&merged, "merged").expect("valid jsonc");
    assert_eq!(parsed["managed.dup"], "managed-final");
    assert_eq!(parsed["escaped.\"key\""], "new\\value");
    assert_eq!(parsed["[rust]"]["editor.tabSize"], 2);
    assert_eq!(
        parsed["[rust]"]["rust-analyzer.check.command"],
        "clippy-strict"
    );
    assert_eq!(parsed["[rust]"]["editor.formatOnSave"], true);
    assert_eq!(parsed["shellcheck.exclude"], json!(["SC1090", "SC2034"]));
    assert_eq!(parsed["brand.new.key"], 42);
    assert_eq!(parsed["user.key.000"], 0);
    assert_eq!(parsed["user.key.149"], 149);
}

#[test]
fn profile_provenance_new_fields_are_backward_compatible() {
    // Legacy JSON lacking `pinsFileOverride` and `tier2Unpinned` must deserialize cleanly.
    let legacy_json = r#"{
  "version": 1,
  "profile": "rust",
  "userDir": "/tmp/vscode/User",
  "preset": "rust",
  "resolvedModules": ["core-editor", "rust"],
  "managedSettingsKeys": ["editor.fontSize"],
  "managedExtensions": ["rust-lang.rust-analyzer"],
  "unlockedBypass": false,
  "lastSyncAt": "2026-01-01T00:00:00Z"
}"#;
    let parsed: ProfileProvenance =
        serde_json::from_str(legacy_json).expect("deserialize legacy provenance");
    assert!(!parsed.pins_file_override);
    assert!(parsed.tier2_unpinned.is_empty());

    // Default values are omitted on serialization.
    let reserialized = serde_json::to_value(&parsed).expect("serialize default");
    assert!(reserialized.get("pinsFileOverride").is_none());
    assert!(reserialized.get("tier2Unpinned").is_none());

    // Non-default values round-trip as camelCase.
    let mut updated = parsed;
    updated.pins_file_override = true;
    updated.tier2_unpinned = vec!["foo.bar".to_string(), "baz.qux".to_string()];
    let encoded = serde_json::to_string(&updated).expect("serialize updated");
    assert!(encoded.contains("\"pinsFileOverride\":true"));
    assert!(encoded.contains("\"tier2Unpinned\":[\"foo.bar\",\"baz.qux\"]"));

    let decoded: ProfileProvenance = serde_json::from_str(&encoded).expect("round-trip");
    assert!(decoded.pins_file_override);
    assert_eq!(
        decoded.tier2_unpinned,
        vec!["foo.bar".to_string(), "baz.qux".to_string()]
    );
}

#[test]
fn vsp_error_kind_returns_stable_snake_case_for_all_variants() {
    let cases = [
        (
            VspError::Operational("op".into()),
            ExitCode::Operational,
            "operational",
        ),
        (
            VspError::TargetMissing("tm".into()),
            ExitCode::TargetMissing,
            "target_missing",
        ),
        (
            VspError::NegativeCheck("nc".into()),
            ExitCode::NegativeCheck,
            "negative_check",
        ),
        (
            VspError::UsageOrConfig("uc".into()),
            ExitCode::UsageOrConfig,
            "usage_or_config",
        ),
        (
            VspError::SafetyRefusal("sr".into()),
            ExitCode::SafetyRefusal,
            "safety_refusal",
        ),
        (
            VspError::RetryableLock("rl".into()),
            ExitCode::RetryableLock,
            "retryable_lock",
        ),
        (
            VspError::IncompleteObservation("io".into()),
            ExitCode::IncompleteObservation,
            "incomplete_observation",
        ),
    ];

    for (err, expected_code, expected_kind) in cases {
        assert_eq!(err.exit_code(), expected_code);
        assert_eq!(err.kind(), expected_kind);
    }
}

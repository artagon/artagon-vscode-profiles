//! Security policy checks and full repository integrity validator (`vsp dev validate`).
//!
//! Includes the nested-safe autonomy/exec-override deny-list defeating all 5 N1 payloads,
//! strict Marketplace VSIX URL & pin validation, setting scope enforcement, recursive
//! scalar-leaf & object-vs-leaf collision checking, and UX symmetry verification.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::legacy_loader::{parse_jsonc_settings_object, read_strict_json_value};
use crate::model::{
    ExtensionStatus, ModuleKind, PinsLoadState, WorkspaceCatalog, COMPILER_UNION_KEYS,
    EXPECTED_UX_KEYS, GROUP_ORDER,
};
use crate::resolver::{resolve_modules, resolve_profile_recipe, sha256_hex, ResolveRequest};

/// Execution-override keys denied anywhere in configuration (matched case-insensitively
/// against both leaf keys and joined dot-paths).
pub const EXEC_OVERRIDE_KEYS: &[&str] = &[
    "claudeCode.claudeProcessWrapper",
    "claudeCode.environmentVariables",
    "antigravity.serverArgs",
    "antigravity.releaseBaseUrl",
    "antigravity.caCertificates",
    "chatgpt.cliExecutable",
];

/// Individual policy or validation violation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyViolation {
    /// File or logical source where the violation was found.
    pub source: String,
    /// Human-readable description of the violation.
    pub message: String,
}

/// Aggregated report returned by [`validate_repository`].
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    /// Total JSON/JSONC/bundle files checked.
    pub checked_files: usize,
    /// List of all detected violations (empty when valid).
    pub violations: Vec<PolicyViolation>,
}

impl ValidationReport {
    /// Returns `true` when zero violations were found.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }
}

/// Normalizes typed JSON booleans and case-insensitive `"true"`/`"false"` strings.
#[must_use]
pub fn normalize_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::String(s) => {
            if s.eq_ignore_ascii_case("true") {
                Some(true)
            } else if s.eq_ignore_ascii_case("false") {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Checks whether a leaf key or its joined dotted path matches `target` case-insensitively.
#[must_use]
pub fn matches_key_or_path(leaf_key: &str, joined_path: &str, target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    let kl = leaf_key.to_ascii_lowercase();
    let pl = joined_path.to_ascii_lowercase();
    kl == t || pl == t || pl.ends_with(&format!(".{t}"))
}

/// Checks whether a key or dotted path matches `(^|\.)chat\.tools\..*autoapprov` case-insensitively.
#[must_use]
pub fn matches_chat_tools_autoapprove(text: &str) -> bool {
    let lc = text.to_ascii_lowercase();
    if let Some(tail) = lc.strip_prefix("chat.tools.") {
        if tail.contains("autoapprov") {
            return true;
        }
    }
    let mut rest = lc.as_str();
    while let Some(idx) = rest.find(".chat.tools.") {
        let tail = &rest[idx + ".chat.tools.".len()..];
        if tail.contains("autoapprov") {
            return true;
        }
        rest = tail;
    }
    false
}

/// Recursively inspects a parsed JSON document against the autonomy and execution-override
/// deny-list.
///
/// Defeats all 5 N1 nested/string/case-variation payloads:
/// 1. `{"chat.tools.autoApprove": true}`
/// 2. `{"chat": {"tools": {"autoApprove": true}}}`
/// 3. `{"chat": {"tools.terminal.autoApprove": true}}`
/// 4. `{"claudeCode": {"claudeProcessWrapper": "/tmp/x"}}`
/// 5. `{"claudeCode.allowDangerouslySkipPermissions": "true"}`
#[must_use]
pub fn check_autonomy_denylist(value: &Value, label: &str) -> Vec<PolicyViolation> {
    let mut violations = Vec::new();
    let mut path = Vec::new();
    inspect_autonomy_node(value, &mut path, label, &mut violations);
    violations
}

fn inspect_autonomy_node(
    value: &Value,
    path: &mut Vec<String>,
    label: &str,
    violations: &mut Vec<PolicyViolation>,
) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                path.push(key.clone());
                let joined = path.join(".");
                let norm_b = normalize_bool(val);

                if matches_chat_tools_autoapprove(key) || matches_chat_tools_autoapprove(&joined) {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy key: {joined}"),
                    });
                } else if matches_key_or_path(
                    key,
                    &joined,
                    "claudeCode.allowDangerouslySkipPermissions",
                ) && norm_b == Some(true)
                {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy setting: {joined}=true"),
                    });
                } else if matches_key_or_path(key, &joined, "claudeCode.initialPermissionMode")
                    && val
                        .as_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("bypasspermissions"))
                {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy setting: {joined}=bypassPermissions"),
                    });
                } else if matches_key_or_path(key, &joined, "geminicodeassist.agentYoloMode")
                    && norm_b == Some(true)
                {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy setting: {joined}=true"),
                    });
                } else if EXEC_OVERRIDE_KEYS
                    .iter()
                    .any(|target| matches_key_or_path(key, &joined, target))
                {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden exec-override key: {joined}"),
                    });
                }

                inspect_autonomy_node(val, path, label, violations);
                path.pop();
            }
        }
        Value::Array(items) => {
            for item in items {
                inspect_autonomy_node(item, path, label, violations);
            }
        }
        Value::String(s) => {
            let lc = s.to_ascii_lowercase();
            if lc.contains("bypasspermissions") || lc.contains("--dangerously-skip-permissions") {
                violations.push(PolicyViolation {
                    source: label.to_string(),
                    message: format!("forbidden bypassPermissions value at: {}", path.join(".")),
                });
            }
        }
        _ => {}
    }
}

/// Validates that `digest` is a lowercase 64-character hexadecimal SHA-256 string.
#[must_use]
pub fn is_valid_sha256_hex(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Validates that a VSIX URL uses `https://` and an official Visual Studio Marketplace host
/// with no userinfo (`@`) or authority spoofing (`?`, `#`).
#[must_use]
pub fn is_valid_marketplace_vsix_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = match rest.find('/') {
        Some(idx) => &rest[..idx],
        None => rest,
    };
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains('?')
        || authority.contains('#')
        || authority.chars().any(char::is_whitespace)
    {
        return false;
    }

    let (host, port_opt) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(port) = port_opt {
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }

    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2
        || labels.iter().any(|lbl| {
            lbl.is_empty()
                || !lbl
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return false;
    }

    let host_lc = host.to_ascii_lowercase();
    if host_lc == "marketplace.visualstudio.com" {
        return true;
    }
    for suffix in [".gallery.vsassets.io", ".gallerycdn.vsassets.io"] {
        if let Some(prefix) = host_lc.strip_suffix(suffix) {
            if !prefix.is_empty() && !prefix.starts_with('.') && !prefix.ends_with('.') {
                return true;
            }
        }
    }
    false
}

/// Runs the complete repository & security validation suite backing `vsp dev validate`.
///
/// Validates:
/// 1. Single-document strict JSON/JSONC syntax across `catalog/`, `modules/`, `profiles/`,
///    `_merged/`, and `exports/` (when present).
/// 2. Module manifests, sibling file references, symmetric `conflicts[]`, and DAG acyclicity.
/// 3. `catalog/extensions.json` schema, active-module parity, `GROUP_ORDER` coverage, and
///    exact `LC_ALL=C` sorted `catalog/publishers.json` parity.
/// 4. Fail-closed `catalog/extension-pins.json` presence, SHA-256 format, and Marketplace URLs.
/// 5. Workspace & profile setting fragments against autonomy deny-list, workspace lints, and
///    `catalog/setting-scopes.json`.
/// 6. Recursive scalar-leaf & object-vs-leaf prefix collision detection across jointly
///    selectable modules within the same scope.
/// 7. Exact 15-key UX symmetry (`crisp`, `retina`, `default`, disjoint from `core-editor`).
/// 8. Profile recipes, `_merged/*.json`, `exports/*.code-profile`, `exports/SHA256SUMS`,
///    and forbidden security-bypass paths.
#[must_use]
pub fn validate_repository(catalog: &WorkspaceCatalog) -> ValidationReport {
    let mut violations = Vec::new();
    let mut checked_files = 0usize;

    // 1. Strict JSON / JSONC syntax pass.
    for dir_name in ["catalog", "modules", "profiles", "_merged", "exports"] {
        let dir = catalog.root.join(dir_name);
        if dir.is_dir() {
            validate_tree_syntax(&catalog.root, &dir, &mut checked_files, &mut violations);
        }
    }

    // 2. Module sibling files, references, symmetric conflicts, and DAG acyclicity.
    for (mid, def) in &catalog.modules {
        let manifest_label = def.dir.join("module.json").display().to_string();
        for (field, rel) in [
            ("settings.workspace", &def.manifest.settings.workspace),
            ("settings.profile", &def.manifest.settings.profile),
            ("tasks", &def.manifest.tasks),
        ] {
            if let Some(rel_path) = rel {
                if !rel_path.is_empty() && !def.dir.join(rel_path).is_file() {
                    violations.push(PolicyViolation {
                        source: manifest_label.clone(),
                        message: format!("references missing {field} file '{rel_path}'"),
                    });
                }
            }
        }

        for r in def
            .manifest
            .requires
            .iter()
            .chain(def.manifest.default_with.iter())
            .chain(def.manifest.conflicts.iter())
        {
            if !catalog.modules.contains_key(r) {
                violations.push(PolicyViolation {
                    source: manifest_label.clone(),
                    message: format!("unknown module reference '{r}'"),
                });
            }
        }

        for conf in &def.manifest.conflicts {
            if let Some(other) = catalog.modules.get(conf) {
                if !other.manifest.conflicts.iter().any(|c| c == mid) {
                    violations.push(PolicyViolation {
                        source: manifest_label.clone(),
                        message: format!(
                            "asymmetric conflict ('{mid}' conflicts with '{conf}', but '{conf}' does not declare '{mid}')"
                        ),
                    });
                }
            }
        }

        if let Err(err) = resolve_modules(
            catalog,
            &ResolveRequest {
                roots: vec![mid.clone()],
                ..Default::default()
            },
        ) {
            violations.push(PolicyViolation {
                source: manifest_label,
                message: format!("module '{mid}' DAG resolution failed: {err}"),
            });
        }
    }

    // 3. Catalog integrity, active-module parity, publisher allowlist parity, and GROUP_ORDER.
    validate_catalog_and_publishers(catalog, &mut violations);

    // 4. Fail-closed extension-pins.json validation.
    validate_pins(catalog, &mut violations);

    // 5. Settings fragment validation (autonomy deny-list, workspace lints, setting-scopes).
    let ux_keys = catalog.ux_managed_keys();
    for def in catalog.modules.values() {
        if let Some(rel) = &def.manifest.settings.workspace {
            if let Some(ws_map) = &def.workspace_settings {
                let src = def.dir.join(rel).display().to_string();
                validate_settings_fragment(
                    catalog,
                    &src,
                    ws_map,
                    true,
                    &ux_keys,
                    &mut violations,
                );
            }
        }
        if let Some(rel) = &def.manifest.settings.profile {
            if let Some(prof_map) = &def.profile_settings {
                let src = def.dir.join(rel).display().to_string();
                validate_settings_fragment(
                    catalog,
                    &src,
                    prof_map,
                    false,
                    &ux_keys,
                    &mut violations,
                );
            }
        }
    }

    // 6. Recursive scalar-leaf & object-vs-leaf prefix collision check.
    validate_module_collisions(catalog, &mut violations);

    // 7. UX symmetry check (crisp, retina, default, core-editor disjointness).
    validate_ux_symmetry(catalog, &mut violations);

    // 8. Profile recipes, tracked _merged/*.json, exports/*.code-profile, SHA256SUMS,
    //    and forbidden bypass paths.
    validate_profiles_and_artifacts(catalog, &ux_keys, &mut violations);

    ValidationReport {
        checked_files,
        violations,
    }
}

fn validate_tree_syntax(
    root: &Path,
    dir: &Path,
    checked: &mut usize,
    violations: &mut Vec<PolicyViolation>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if path.is_symlink() {
            if !path.exists() {
                violations.push(PolicyViolation {
                    source: path.display().to_string(),
                    message: "broken symlink".to_string(),
                });
            }
            continue;
        }
        if path.is_dir() {
            validate_tree_syntax(root, &path, checked, violations);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".jsonc") {
            *checked += 1;
            if let Err(err) = parse_jsonc_settings_object(&path) {
                violations.push(PolicyViolation {
                    source: path.display().to_string(),
                    message: err.to_string(),
                });
            }
        } else if name.ends_with(".json") || name.ends_with(".code-profile") {
            *checked += 1;
            let expect_array =
                name == "extensions.json" && path != root.join("catalog/extensions.json");
            if let Err(err) = read_strict_json_value(&path, expect_array) {
                violations.push(PolicyViolation {
                    source: path.display().to_string(),
                    message: err.to_string(),
                });
            }
        }
    }
}

fn validate_catalog_and_publishers(
    catalog: &WorkspaceCatalog,
    violations: &mut Vec<PolicyViolation>,
) {
    let ext_src = "catalog/extensions.json";
    for (k, rec) in &catalog.extensions.extensions {
        if k != &k.to_ascii_lowercase() {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("non-lowercase catalog key: {k}"),
            });
        }
        let prefix = k.split('.').next().unwrap_or("");
        if rec.publisher != prefix {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!(
                    "publisher prefix mismatch for {k}: publisher={} expected={prefix}",
                    rec.publisher
                ),
            });
        }
        if !rec.status.is_valid() {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("invalid status for {k}: {}", rec.status.as_str()),
            });
        }
        if rec.status == ExtensionStatus::Superseded {
            let valid_replacement = rec
                .replaced_by
                .as_deref()
                .filter(|r| !r.is_empty())
                .and_then(|r| catalog.extensions.extensions.get(r))
                .is_some_and(|target| target.status == ExtensionStatus::Active);
            if !valid_replacement {
                violations.push(PolicyViolation {
                    source: ext_src.to_string(),
                    message: format!("superseded {k} missing active replacedBy"),
                });
            }
        }
        if rec.status == ExtensionStatus::Active
            && !GROUP_ORDER
                .iter()
                .any(|g| g.eq_ignore_ascii_case(&rec.group))
        {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("catalog group '{}' is missing from GROUP_ORDER", rec.group),
            });
        }
    }

    let mut active_mod_ids: BTreeSet<String> = BTreeSet::new();
    let mut unwanted_mod_ids: BTreeSet<String> = BTreeSet::new();
    for def in catalog.modules.values() {
        for id in def
            .manifest
            .extensions
            .workspace
            .iter()
            .chain(def.manifest.extensions.profile.iter())
        {
            active_mod_ids.insert(id.to_ascii_lowercase());
        }
        for id in &def.manifest.extensions.unwanted {
            unwanted_mod_ids.insert(id.to_ascii_lowercase());
        }
    }

    for id in &active_mod_ids {
        let is_active = catalog
            .extensions
            .extensions
            .get(id)
            .is_some_and(|rec| rec.status == ExtensionStatus::Active);
        if !is_active {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("module extension not active in catalog: {id}"),
            });
        }
    }
    for (k, rec) in &catalog.extensions.extensions {
        if rec.status == ExtensionStatus::Active && !active_mod_ids.contains(k) {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("orphan active catalog entry unreferenced by any module: {k}"),
            });
        }
    }
    for id in &unwanted_mod_ids {
        if !catalog.extensions.extensions.contains_key(id) {
            violations.push(PolicyViolation {
                source: ext_src.to_string(),
                message: format!("module unwanted extension missing from catalog: {id}"),
            });
        }
    }

    // Exact LC_ALL=C sorted unique lowercase active module publishers.
    let expected_pubs: Vec<String> = active_mod_ids
        .iter()
        .map(|id| id.split('.').next().unwrap_or("").to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let actual_pubs = &catalog.publishers.publishers;
    let mut normalized_actual: Vec<String> = actual_pubs
        .iter()
        .map(|p| p.to_ascii_lowercase())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    normalized_actual.sort();
    if actual_pubs != &normalized_actual {
        violations.push(PolicyViolation {
            source: "catalog/publishers.json".to_string(),
            message: "catalog/publishers.json is not strictly lowercase, unique, and LC_ALL=C sorted".to_string(),
        });
    }
    if normalized_actual != expected_pubs {
        violations.push(PolicyViolation {
            source: "catalog/publishers.json".to_string(),
            message: "catalog/publishers.json drifted from active module publishers".to_string(),
        });
    }
}

fn validate_pins(catalog: &WorkspaceCatalog, violations: &mut Vec<PolicyViolation>) {
    match &catalog.pins {
        PinsLoadState::Missing(path) => {
            violations.push(PolicyViolation {
                source: path.display().to_string(),
                message: "catalog/extension-pins.json is missing or unreadable".to_string(),
            });
        }
        PinsLoadState::Invalid { path, error } => {
            violations.push(PolicyViolation {
                source: path.display().to_string(),
                message: format!("catalog/extension-pins.json parse failed: {error}"),
            });
        }
        PinsLoadState::Loaded(manifest) => {
            let src = "catalog/extension-pins.json";
            for (k, pin) in &manifest.pins {
                if k != &k.to_ascii_lowercase() {
                    violations.push(PolicyViolation {
                        source: src.to_string(),
                        message: format!("non-lowercase pin key: {k}"),
                    });
                }
                if !is_valid_sha256_hex(&pin.sha256) {
                    violations.push(PolicyViolation {
                        source: src.to_string(),
                        message: format!("invalid sha256 for pin {k}"),
                    });
                }
                if !is_valid_marketplace_vsix_url(&pin.vsix_url) {
                    violations.push(PolicyViolation {
                        source: src.to_string(),
                        message: format!(
                            "invalid or non-Marketplace vsix_url for pin {k}: {}",
                            pin.vsix_url
                        ),
                    });
                }
                if !pin.version.is_empty() && semver::Version::parse(&pin.version).is_err() {
                    violations.push(PolicyViolation {
                        source: src.to_string(),
                        message: format!(
                            "invalid semver version for pin {k}: {}",
                            pin.version
                        ),
                    });
                }
            }
        }
    }
    if let Some(scopes) = &catalog.setting_scopes {
        if !scopes.vscode_version.is_empty()
            && semver::Version::parse(&scopes.vscode_version).is_err()
        {
            violations.push(PolicyViolation {
                source: "catalog/setting-scopes.json".to_string(),
                message: format!(
                    "invalid semver vscodeVersion: {}",
                    scopes.vscode_version
                ),
            });
        }
    }
}

fn validate_settings_fragment(
    catalog: &WorkspaceCatalog,
    source: &str,
    map: &Map<String, Value>,
    is_workspace: bool,
    ux_keys: &[String],
    violations: &mut Vec<PolicyViolation>,
) {
    let val = Value::Object(map.clone());
    violations.extend(check_autonomy_denylist(&val, source));

    if is_workspace {
        if map.contains_key("editor.defaultFormatter") {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: "top-level editor.defaultFormatter forbidden in workspace settings (scope inside [lang])".to_string(),
            });
        }
        for k in map.keys() {
            if ux_keys.iter().any(|u| u == k) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("personal UX key forbidden in workspace settings: {k}"),
                });
            }
        }
        if let Some(Value::Object(we)) = map.get("files.watcherExclude") {
            for k in we.keys() {
                if !k.ends_with("/**") {
                    violations.push(PolicyViolation {
                        source: source.to_string(),
                        message: format!("files.watcherExclude missing trailing /** : {k}"),
                    });
                }
            }
        }
        let serialized = val.to_string();
        for bad in [
            "/opt/homebrew/",
            "/usr/local/opt/",
            "/Users/",
            "${command:jenv.javaHome}",
        ] {
            if serialized.contains(bad) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("hardcoded host path or broken jenv command found ({bad})"),
                });
            }
        }
    }

    let Some(scopes) = &catalog.setting_scopes else {
        return;
    };

    for (key, value) in map {
        if scopes.forbidden_keys.iter().any(|fk| fk == key) {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!("forbidden/deprecated VS Code {} key: {key}", scopes.vscode_version),
            });
        }
        let Some(spec) = scopes.settings.get(key) else {
            continue;
        };

        if is_workspace
            && matches!(spec.scope.as_str(), "machine" | "profile" | "application")
        {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!(
                    "key {key} has scope={} and is forbidden in settings.workspace.jsonc",
                    spec.scope
                ),
            });
        } else if spec.scope == "application" && spec.allow_in_compiled_profile == Some(false) {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!(
                    "key {key} is application-scoped and disallowed in compiled modules"
                ),
            });
        }

        let actual_type = json_type_name(value);
        if spec.value_type == "integer" {
            if !is_json_integer(value) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("key {key} expected integer, got {actual_type}"),
                });
            }
        } else if !(spec.allow_boolean && value.is_boolean()) && actual_type != spec.value_type {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!("key {key} expected {}, got {actual_type}", spec.value_type),
            });
        }

        if let (Some(allowed), Some(s)) = (&spec.allowed_enum, value.as_str()) {
            if !allowed.iter().any(|a| a == s) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("key {key} value \"{s}\" not in allowed enum {allowed:?}"),
                });
            }
        }

        if spec.nested_value_type.as_deref() == Some("integer") {
            if let Value::Object(obj) = value {
                for (sub_k, sub_v) in obj {
                    if !is_json_integer(sub_v) {
                        violations.push(PolicyViolation {
                            source: source.to_string(),
                            message: format!("key {key}.{sub_k} must be integer"),
                        });
                    }
                }
            }
        }
    }
}

fn validate_module_collisions(
    catalog: &WorkspaceCatalog,
    violations: &mut Vec<PolicyViolation>,
) {
    // Precompute transitive requires[] ancestors for each module.
    let mut ancestors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in catalog.modules.keys() {
        let mut seen = BTreeSet::new();
        let mut stack = vec![id.clone()];
        while let Some(cur) = stack.pop() {
            if let Some(def) = catalog.modules.get(&cur) {
                for req in &def.manifest.requires {
                    if seen.insert(req.clone()) {
                        stack.push(req.clone());
                    }
                }
            }
        }
        ancestors.insert(id.clone(), seen);
    }

    let mods: Vec<_> = catalog.modules.values().collect();
    for i in 0..mods.len() {
        for j in (i + 1)..mods.len() {
            let a = mods[i];
            let b = mods[j];

            if a.manifest.kind == ModuleKind::Ux && b.manifest.kind == ModuleKind::Ux {
                continue;
            }
            if a.manifest.conflicts.iter().any(|c| c == &b.manifest.id)
                || b.manifest.conflicts.iter().any(|c| c == &a.manifest.id)
            {
                continue;
            }
            if ancestors
                .get(&a.manifest.id)
                .is_some_and(|set| set.contains(&b.manifest.id))
                || ancestors
                    .get(&b.manifest.id)
                    .is_some_and(|set| set.contains(&a.manifest.id))
            {
                continue;
            }

            for (scope_label, map_a, map_b) in [
                ("ws", &a.workspace_settings, &b.workspace_settings),
                ("pr", &a.profile_settings, &b.profile_settings),
            ] {
                let (Some(ma), Some(mb)) = (map_a, map_b) else {
                    continue;
                };
                let la = collect_scalar_leaves(ma);
                let lb = collect_scalar_leaves(mb);
                let oa = collect_object_paths(ma);
                let ob = collect_object_paths(mb);

                for (path_a, val_a) in &la {
                    if let Some(val_b) = lb.get(path_a) {
                        if val_a != val_b || json_type_name(val_a) != json_type_name(val_b) {
                            violations.push(PolicyViolation {
                                source: format!("{}/{}", a.manifest.id, b.manifest.id),
                                message: format!(
                                    "sibling collision ({scope_label}) between {} and {} at {}: {} vs {}",
                                    a.manifest.id,
                                    b.manifest.id,
                                    path_a.join("."),
                                    val_a,
                                    val_b
                                ),
                            });
                        }
                    }
                    if ob.contains(path_a) {
                        violations.push(PolicyViolation {
                            source: format!("{}/{}", a.manifest.id, b.manifest.id),
                            message: format!(
                                "object-vs-leaf collision ({scope_label}) between {} (leaf) and {} (object) at {}",
                                a.manifest.id,
                                b.manifest.id,
                                path_a.join(".")
                            ),
                        });
                    }
                }
                for path_b in lb.keys() {
                    if oa.contains(path_b) {
                        violations.push(PolicyViolation {
                            source: format!("{}/{}", a.manifest.id, b.manifest.id),
                            message: format!(
                                "object-vs-leaf collision ({scope_label}) between {} (object) and {} (leaf) at {}",
                                a.manifest.id,
                                b.manifest.id,
                                path_b.join(".")
                            ),
                        });
                    }
                }
            }
        }
    }
}

fn validate_ux_symmetry(catalog: &WorkspaceCatalog, violations: &mut Vec<PolicyViolation>) {
    let (Some(crisp), Some(retina), Some(default_ux), Some(core)) = (
        catalog.modules.get("crisp").and_then(|m| m.profile_settings.as_ref()),
        catalog.modules.get("retina").and_then(|m| m.profile_settings.as_ref()),
        catalog.modules.get("default").and_then(|m| m.profile_settings.as_ref()),
        catalog.modules.get("core-editor").and_then(|m| m.profile_settings.as_ref()),
    ) else {
        violations.push(PolicyViolation {
            source: "modules/ux".to_string(),
            message: "missing required UX or core-editor settings fragments (crisp, retina, default, core-editor)".to_string(),
        });
        return;
    };

    let crisp_keys: Vec<&str> = crisp.keys().map(String::as_str).collect();
    let retina_keys: Vec<&str> = retina.keys().map(String::as_str).collect();
    if crisp_keys != retina_keys {
        violations.push(PolicyViolation {
            source: "modules/ux".to_string(),
            message: "crisp and retina UX presets have different key sets".to_string(),
        });
    }
    if crisp_keys != EXPECTED_UX_KEYS {
        violations.push(PolicyViolation {
            source: "modules/ux".to_string(),
            message: "UX presets do not match the exact 15 symmetric keys".to_string(),
        });
    }
    if !default_ux.is_empty() {
        violations.push(PolicyViolation {
            source: "modules/ux/default/settings.profile.jsonc".to_string(),
            message: "modules/ux/default/settings.profile.jsonc must be empty {}".to_string(),
        });
    }
    if retina.get("workbench.iconTheme").and_then(Value::as_str) != Some("catppuccin-mocha") {
        violations.push(PolicyViolation {
            source: "modules/ux/retina/settings.profile.jsonc".to_string(),
            message: "retina workbench.iconTheme must be 'catppuccin-mocha'".to_string(),
        });
    }
    if retina.get("workbench.productIconTheme").and_then(Value::as_str) != Some("fluent-icons") {
        violations.push(PolicyViolation {
            source: "modules/ux/retina/settings.profile.jsonc".to_string(),
            message: "retina workbench.productIconTheme must be 'fluent-icons'".to_string(),
        });
    }
    if crisp.get("workbench.productIconTheme").and_then(Value::as_str)
        != Some("material-product-icons")
    {
        violations.push(PolicyViolation {
            source: "modules/ux/crisp/settings.profile.jsonc".to_string(),
            message: "crisp workbench.productIconTheme must be 'material-product-icons'".to_string(),
        });
    }
    for k in &crisp_keys {
        if core.contains_key(*k) {
            violations.push(PolicyViolation {
                source: "modules/ux".to_string(),
                message: format!("key '{k}' appears in both UX preset and core-editor"),
            });
        }
    }
}

fn validate_profiles_and_artifacts(
    catalog: &WorkspaceCatalog,
    ux_keys: &[String],
    violations: &mut Vec<PolicyViolation>,
) {
    for (name, recipe) in &catalog.profiles {
        let src = format!("profiles/{name}/profile.json");
        if let Ok(val) = serde_json::to_value(recipe) {
            violations.extend(check_autonomy_denylist(&val, &src));
        }
        if let Err(err) = resolve_profile_recipe(catalog, name, &[], &[], None, None) {
            violations.push(PolicyViolation {
                source: src,
                message: format!("profile '{name}' recipe failed resolution: {err}"),
            });
        }
    }

    // Forbidden security-bypass paths.
    for bad in ["agents", ".agents/settings.local.json"] {
        if catalog.root.join(bad).exists() {
            violations.push(PolicyViolation {
                source: bad.to_string(),
                message: format!("forbidden security-bypass file exists in repo: {bad}"),
            });
        }
    }

    // Tracked _merged/*.json invariants (if present).
    let merged_dir = catalog.root.join("_merged");
    if merged_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&merged_dir) {
            let mut files: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json") && p.is_file())
                .collect();
            files.sort();
            for path in files {
                let src = path.display().to_string();
                if let Ok(val) = read_strict_json_value(&path, false) {
                    violations.extend(check_autonomy_denylist(&val, &src));
                    check_baseline_invariants(&val, &src, false, ux_keys, violations);
                }
            }
        }
    }

    // Tracked exports/*.code-profile and exports/SHA256SUMS (if present).
    let exports_dir = catalog.root.join("exports");
    if exports_dir.is_dir() {
        let mut regular_bundles = BTreeSet::new();
        if let Ok(entries) = fs::read_dir(&exports_dir) {
            let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
            files.sort();
            for path in files {
                if path.is_symlink() || !path.is_file() {
                    continue;
                }
                if path.extension().is_some_and(|e| e == "code-profile") {
                    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        regular_bundles.insert(fname.to_string());
                    }
                    let src = path.display().to_string();
                    if let Ok(val) = read_strict_json_value(&path, false) {
                        if let Some(settings) = val.get("settings") {
                            violations.extend(check_autonomy_denylist(settings, &src));
                            check_baseline_invariants(settings, &src, true, ux_keys, violations);
                        }
                    }
                }
            }
        }

        let sums_path = exports_dir.join("SHA256SUMS");
        if sums_path.is_file() {
            if let Ok(content) = fs::read_to_string(&sums_path) {
                let mut listed = BTreeSet::new();
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let mut parts = trimmed.split_whitespace();
                    let (Some(exp_hash), Some(fname)) = (parts.next(), parts.next()) else {
                        violations.push(PolicyViolation {
                            source: "exports/SHA256SUMS".to_string(),
                            message: format!("malformed checksum line: {trimmed}"),
                        });
                        continue;
                    };
                    listed.insert(fname.to_string());
                    let fpath = exports_dir.join(fname);
                    match fs::read(&fpath) {
                        Ok(bytes) => {
                            let act = sha256_hex(&bytes);
                            if act != exp_hash {
                                violations.push(PolicyViolation {
                                    source: "exports/SHA256SUMS".to_string(),
                                    message: format!(
                                        "mismatch for {fname} (expected {exp_hash}, got {act})"
                                    ),
                                });
                            }
                        }
                        Err(_) => {
                            violations.push(PolicyViolation {
                                source: "exports/SHA256SUMS".to_string(),
                                message: format!("references missing file: {fname}"),
                            });
                        }
                    }
                }
                for bname in regular_bundles {
                    if !listed.contains(&bname) {
                        violations.push(PolicyViolation {
                            source: "exports/SHA256SUMS".to_string(),
                            message: format!("exports/{bname} is missing from exports/SHA256SUMS"),
                        });
                    }
                }
            }
        }
    }
}

fn check_baseline_invariants(
    settings: &Value,
    source: &str,
    check_no_ux_leak: bool,
    ux_keys: &[String],
    violations: &mut Vec<PolicyViolation>,
) {
    let Some(obj) = settings.as_object() else {
        return;
    };
    if obj.get("workbench.sideBar.location").and_then(Value::as_str) != Some("left") {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "workbench.sideBar.location != left".to_string(),
        });
    }
    if obj
        .get("security.workspace.trust.untrustedFiles")
        .and_then(Value::as_str)
        != Some("prompt")
    {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "security.workspace.trust.untrustedFiles != prompt".to_string(),
        });
    }
    if obj.get("telemetry.telemetryLevel").and_then(Value::as_str) != Some("off") {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "telemetry.telemetryLevel != off".to_string(),
        });
    }
    if obj.get("editor.hover.enabled").and_then(Value::as_str) != Some("on") {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "editor.hover.enabled != \"on\"".to_string(),
        });
    }
    if obj.contains_key("$schema") || obj.contains_key("@extends") {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "contains forbidden $schema or @extends metadata key".to_string(),
        });
    }
    if check_no_ux_leak {
        for k in obj.keys() {
            if ux_keys.iter().any(|u| u == k) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("exported bundle leaks UX key: {k}"),
                });
            }
        }
    }
}

fn collect_scalar_leaves(map: &Map<String, Value>) -> BTreeMap<Vec<String>, Value> {
    fn walk(val: &Value, prefix: &mut Vec<String>, out: &mut BTreeMap<Vec<String>, Value>) {
        match val {
            Value::Object(obj) => {
                for (k, v) in obj {
                    prefix.push(k.clone());
                    walk(v, prefix, out);
                    prefix.pop();
                }
            }
            Value::Array(_) => {
                if prefix.len() == 1 && COMPILER_UNION_KEYS.contains(&prefix[0].as_str()) {
                    return;
                }
                out.insert(prefix.clone(), val.clone());
            }
            _ => {
                out.insert(prefix.clone(), val.clone());
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut prefix = Vec::new();
    walk(&Value::Object(map.clone()), &mut prefix, &mut out);
    out
}

fn collect_object_paths(map: &Map<String, Value>) -> BTreeSet<Vec<String>> {
    fn walk(val: &Value, prefix: &mut Vec<String>, out: &mut BTreeSet<Vec<String>>) {
        if let Value::Object(obj) = val {
            if !prefix.is_empty() {
                out.insert(prefix.clone());
            }
            for (k, v) in obj {
                prefix.push(k.clone());
                walk(v, prefix, out);
                prefix.pop();
            }
        }
    }
    let mut out = BTreeSet::new();
    let mut prefix = Vec::new();
    walk(&Value::Object(map.clone()), &mut prefix, &mut out);
    out
}

fn json_type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn is_json_integer(val: &Value) -> bool {
    match val {
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                true
            } else if let Some(f) = n.as_f64() {
                f.is_finite() && f.fract() == 0.0
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_loader::load_legacy_catalog;
    use serde_json::json;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn missing_pins_file_fails_closed_in_validate_repository() {
        let mut catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        catalog.pins = PinsLoadState::Missing(PathBuf::from("/nonexistent/extension-pins.json"));
        let report = validate_repository(&catalog);
        assert!(!report.is_ok());
        assert!(report
            .violations
            .iter()
            .any(|v| v.message.contains("extension-pins.json is missing")));
    }

    #[test]
    fn collision_checker_catches_leaf_and_object_prefix_conflicts() {
        let mut catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        // Mutate two jointly selectable non-ancestor capabilities (`git-pro` and `dx-pro`).
        catalog
            .modules
            .get_mut("git-pro")
            .expect("git-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("test.conflict".to_string(), json!("alpha"));
        catalog
            .modules
            .get_mut("dx-pro")
            .expect("dx-pro")
            .profile_settings
            .as_mut()
            .expect("prof")
            .insert("test.conflict".to_string(), json!({ "nested": true }));

        let report = validate_repository(&catalog);
        assert!(report
            .violations
            .iter()
            .any(|v| v.message.contains("object-vs-leaf collision") && v.message.contains("test.conflict")));
    }

    #[test]
    fn normalize_bool_and_sha256_helpers() {
        assert_eq!(normalize_bool(&json!(true)), Some(true));
        assert_eq!(normalize_bool(&json!("TRUE")), Some(true));
        assert_eq!(normalize_bool(&json!("false")), Some(false));
        assert_eq!(normalize_bool(&json!("yes")), None);
        assert!(is_valid_sha256_hex(&"a".repeat(64)));
        assert!(!is_valid_sha256_hex(&"A".repeat(64)));
        assert!(!is_valid_sha256_hex("short"));
    }
}

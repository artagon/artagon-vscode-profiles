//! Maintainer operations backing `vsp dev pins`, `vsp dev compat`, `vsp dev docs`, and
//! `vsp dev hooks`.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{ExtensionPinsManifest, PinsLoadState, UxPreset, WorkspaceCatalog};
use crate::policy::{is_valid_marketplace_vsix_url, is_valid_sha256_hex};
use crate::provenance::{now_rfc3339_utc, write_file_atomic};
use crate::resolver::{
    compile_profile_extensions, resolve_profile_recipe, to_canonical_json_string,
};
use crate::vscode::find_code_binary;
use crate::vsix::{download_and_hash_vsix_url, query_marketplace_universal_pin};

/// Default VS Code Marketplace extension query endpoint.
pub const DEFAULT_MARKETPLACE_API_URL: &str =
    "https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery";

const RUST_POLICY_BEGIN: &str = "<!-- BEGIN RUST-DOCS-POLICY -->";
const RUST_POLICY_END: &str = "<!-- END RUST-DOCS-POLICY -->";
const README_MATRIX_BEGIN: &str = "<!-- BEGIN GENERATED-MATRIX -->";
const README_MATRIX_END: &str = "<!-- END GENERATED-MATRIX -->";

const AGENT_DOC_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CODEX.md", "GEMINI.md"];

const DEFAULT_PIN_IDS: &[&str] = &[
    "catppuccin.catppuccin-vsc",
    "catppuccin.catppuccin-vsc-icons",
    "editorconfig.editorconfig",
    "enkia.tokyo-night",
    "miguelsolorio.fluent-icons",
    "pkief.material-icon-theme",
    "pkief.material-product-icons",
    "tamasfe.even-better-toml",
];

fn marketplace_api_url() -> String {
    env::var("VSP_MARKETPLACE_API_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MARKETPLACE_API_URL.to_string())
}

/// Validates (`--check`) or refreshes (`--refresh`) `catalog/extension-pins.json`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if both `--check` and `--refresh` are set,
/// [`VspError::Operational`] on pin format violations, or [`VspError::IncompleteObservation`]
/// (exit `7`) if the Marketplace API fails during `--refresh`.
pub fn run_dev_pins(
    catalog: &WorkspaceCatalog,
    check: bool,
    refresh: bool,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    if check && refresh {
        return Err(VspError::UsageOrConfig(
            "vsp dev pins: cannot combine --check and --refresh".to_string(),
        ));
    }

    if refresh {
        let env_pins = env::var("ARTAGON_VSCODE_PINS_FILE")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let pins_path = match env_pins {
            Some(p) => p,
            None => {
                if catalog.root == Path::new(crate::schema_loader::EMBEDDED_ROOT_LABEL) {
                    return Err(VspError::UsageOrConfig(
                        "vsp dev pins --refresh requires --catalog <REPO_DIR> when using embedded defaults"
                            .to_string(),
                    ));
                }
                catalog.root.join("catalog/extension-pins.json")
            }
        };

        let mut target_ids: BTreeSet<String> = DEFAULT_PIN_IDS
            .iter()
            .map(|id| (*id).to_string())
            .collect();
        if let PinsLoadState::Loaded(existing) = &catalog.pins {
            for k in existing.pins.keys() {
                target_ids.insert(k.to_ascii_lowercase());
            }
        }

        let api_url = marketplace_api_url();
        let now = now_rfc3339_utc();
        let mut pins = BTreeMap::new();
        for ext_id in &target_ids {
            let mut pin = query_marketplace_universal_pin(ext_id, &api_url)?;
            let digest = download_and_hash_vsix_url(&pin.vsix_url, ext_id)?;
            pin.sha256 = digest;
            pin.verified_at = Some(now.clone());
            if !quiet && !json {
                println!("pinned {}@{} ({})", ext_id, pin.version, pin.sha256);
            }
            pins.insert(ext_id.clone(), pin);
        }

        let manifest = ExtensionPinsManifest {
            version: 1,
            note: Some(
                "Install-time verified SHA-256 VSIX pins for universal core/UX extensions. Refresh via vsp dev pins --refresh."
                    .to_string(),
            ),
            pin_required: false,
            pins,
        };
        let content = to_canonical_json_string(&manifest)?;
        write_file_atomic(&pins_path, content.as_bytes(), 0o644, true)?;

        if json {
            let payload = serde_json::json!({
                "status": "refreshed",
                "path": pins_path.display().to_string(),
                "count": manifest.pins.len(),
            });
            print!("{}", to_canonical_json_string(&payload)?);
        } else if !quiet {
            println!("Wrote {}", pins_path.display());
        }
        return Ok(ExitCode::Ok);
    }

    // Default or `--check`: validate existing `catalog/extension-pins.json`.
    match &catalog.pins {
        PinsLoadState::Loaded(manifest) => {
            let mut errors = Vec::new();
            for (k, pin) in &manifest.pins {
                if k != &k.to_ascii_lowercase() {
                    errors.push(format!("non-lowercase pin key: {k}"));
                }
                if !is_valid_sha256_hex(&pin.sha256) {
                    errors.push(format!("invalid sha256 for pin {k}"));
                }
                if !is_valid_marketplace_vsix_url(&pin.vsix_url) {
                    errors.push(format!("invalid vsix_url for pin {k}: {}", pin.vsix_url));
                }
                if pin.version.is_empty() || semver::Version::parse(&pin.version).is_err() {
                    errors.push(format!("invalid semver version for pin {k}: {}", pin.version));
                }
            }

            if json {
                let payload = serde_json::json!({
                    "status": if errors.is_empty() { "ok" } else { "failed" },
                    "count": manifest.pins.len(),
                    "pinRequired": manifest.pin_required,
                    "errors": errors,
                });
                print!("{}", to_canonical_json_string(&payload)?);
            } else if errors.is_empty() {
                if !quiet {
                    println!(
                        "catalog/extension-pins.json: {} valid pinned extension(s)",
                        manifest.pins.len()
                    );
                }
            } else {
                for e in &errors {
                    eprintln!("FAILED: {e}");
                }
            }

            if errors.is_empty() {
                Ok(ExitCode::Ok)
            } else {
                Ok(ExitCode::Operational)
            }
        }
        PinsLoadState::Missing(path) => Err(VspError::Operational(format!(
            "missing extension pins manifest: {}",
            path.display()
        ))),
        PinsLoadState::Invalid { path, error } => Err(VspError::Operational(format!(
            "invalid extension pins manifest {}: {error}",
            path.display()
        ))),
    }
}

/// Options for `vsp dev compat`.
#[derive(Debug, Clone, Default)]
pub struct CompatOptions {
    /// Audit all canonical profiles in the catalog.
    pub all: bool,
    /// Optional newline-separated or JSON array extensions file.
    pub extensions_file: Option<PathBuf>,
    /// Override detected VS Code semver (e.g. `1.147.0`).
    pub vscode_version: Option<String>,
    /// Specific profile names to audit.
    pub profiles: Vec<String>,
}

/// Single audited extension entry emitted by `vsp dev compat --json`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatEntry {
    /// Extension identifier (`publisher.name`).
    pub id: String,
    /// Audit status (`compatible`, `incompatible`, or `NOT_FOUND`).
    pub status: String,
    /// Selected Marketplace version, if found.
    pub version: Option<String>,
    /// Declared `engines.vscode` range, if found.
    pub engine: Option<String>,
    /// Whether the selected version is a pre-release.
    pub prerelease: bool,
}

/// Summary counts for `vsp dev compat`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatSummary {
    /// Total unique extension IDs audited.
    pub total: usize,
    /// Number of compatible extensions.
    pub compatible: usize,
    /// Number of incompatible extensions.
    pub incompatible: usize,
    /// Number of extensions missing from Marketplace.
    pub not_found: usize,
}

/// Complete structured report emitted by `vsp dev compat`.
#[derive(Debug, Clone, Serialize)]
pub struct CompatReport {
    /// Target VS Code semver evaluated against.
    pub vscode_version: String,
    /// Aggregate summary counts.
    pub summary: CompatSummary,
    /// Per-extension compatibility findings.
    pub extensions: Vec<CompatEntry>,
}

/// Executes `vsp dev compat` against the VS Code Marketplace API.
///
/// # Errors
///
/// - [`VspError::TargetMissing`] (exit `2`): unknown profile or missing extensions file.
/// - [`VspError::IncompleteObservation`] (exit `7`): unable to determine VS Code version or
///   Marketplace verification unavailable.
pub fn run_dev_compat(
    catalog: &WorkspaceCatalog,
    opts: &CompatOptions,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    let vscode_ver = resolve_target_vscode_version(catalog, opts.vscode_version.as_deref())?;
    let audit_ids = collect_compat_extension_ids(catalog, opts)?;

    let api_url = marketplace_api_url();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();

    let mut mp_extensions: Vec<Value> = Vec::new();
    for chunk in audit_ids.chunks(40) {
        let resp = query_marketplace_compat_batch(&agent, &api_url, chunk, 529)?;
        if let Some(arr) = resp
            .pointer("/results/0/extensions")
            .and_then(Value::as_array)
        {
            mp_extensions.extend(arr.iter().cloned());
        }
    }

    let mut entries = Vec::with_capacity(audit_ids.len());
    let mut compatible = 0usize;
    let mut incompatible = 0usize;
    let mut not_found = 0usize;

    for id in &audit_ids {
        let id_lc = id.to_ascii_lowercase();
        let found = mp_extensions.iter().find(|ext| {
            let p = ext
                .pointer("/publisher/publisherName")
                .and_then(Value::as_str)
                .unwrap_or("");
            let n = ext
                .get("extensionName")
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("{p}.{n}").eq_ignore_ascii_case(&id_lc)
        });

        let Some(ext_obj) = found else {
            not_found += 1;
            entries.push(CompatEntry {
                id: id.clone(),
                status: "NOT_FOUND".to_string(),
                version: None,
                engine: None,
                prerelease: false,
            });
            continue;
        };

        let Some((mut ver, mut eng, mut pre)) = pick_extension_version(ext_obj) else {
            not_found += 1;
            entries.push(CompatEntry {
                id: id.clone(),
                status: "NOT_FOUND".to_string(),
                version: None,
                engine: None,
                prerelease: false,
            });
            continue;
        };

        if pre {
            // Query full version history (flags=17) to find latest stable if one exists.
            let fb_resp =
                query_marketplace_compat_batch(&agent, &api_url, std::slice::from_ref(id), 17)?;
            let fb_ext = fb_resp
                .pointer("/results/0/extensions")
                .and_then(Value::as_array)
                .and_then(|arr| {
                    arr.iter().find(|e| {
                        let p = e
                            .pointer("/publisher/publisherName")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let n = e
                            .get("extensionName")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        format!("{p}.{n}").eq_ignore_ascii_case(&id_lc)
                    })
                })
                .ok_or_else(|| {
                    VspError::IncompleteObservation(format!(
                        "compat: verification unavailable for '{id}' (missing stable history)"
                    ))
                })?;
            if let Some((fb_ver, fb_eng, false)) = pick_extension_version(fb_ext) {
                ver = fb_ver;
                eng = fb_eng;
                pre = false;
            }
        }

        let is_compat = semver_satisfies(&vscode_ver, &eng);
        let status = if is_compat {
            compatible += 1;
            "compatible"
        } else {
            incompatible += 1;
            "incompatible"
        };
        entries.push(CompatEntry {
            id: id.clone(),
            status: status.to_string(),
            version: Some(ver),
            engine: Some(eng),
            prerelease: pre,
        });
    }

    let report = CompatReport {
        vscode_version: vscode_ver,
        summary: CompatSummary {
            total: audit_ids.len(),
            compatible,
            incompatible,
            not_found,
        },
        extensions: entries,
    };

    if json {
        print!("{}", to_canonical_json_string(&report)?);
    } else if !quiet {
        println!(
            "VS Code version: {} | total={} compatible={} incompatible={} not_found={}",
            report.vscode_version,
            report.summary.total,
            report.summary.compatible,
            report.summary.incompatible,
            report.summary.not_found
        );
        for item in &report.extensions {
            if item.status != "compatible" {
                eprintln!(
                    "  {}: {} (version={}, engine={})",
                    item.status,
                    item.id,
                    item.version.as_deref().unwrap_or("none"),
                    item.engine.as_deref().unwrap_or("none")
                );
            }
        }
    }

    if report.summary.incompatible > 0 || report.summary.not_found > 0 {
        Ok(ExitCode::Operational)
    } else {
        Ok(ExitCode::Ok)
    }
}

fn resolve_target_vscode_version(
    catalog: &WorkspaceCatalog,
    override_ver: Option<&str>,
) -> VspResult<String> {
    if let Some(raw) = override_ver {
        return parse_clean_semver(raw);
    }
    if let Some(code_bin) = find_code_binary() {
        if let Ok(out) = Command::new(code_bin).arg("--version").output() {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(first) = stdout.lines().next() {
                    if let Ok(clean) = parse_clean_semver(first.trim()) {
                        return Ok(clean);
                    }
                }
            }
        }
    }
    if let Some(scopes) = &catalog.setting_scopes {
        if !scopes.vscode_version.is_empty() {
            if let Ok(clean) = parse_clean_semver(&scopes.vscode_version) {
                return Ok(clean);
            }
        }
    }
    Err(VspError::IncompleteObservation(
        "compat: unable to determine VS Code version (install 'code' or pass --vscode-version)"
            .to_string(),
    ))
}

fn parse_clean_semver(raw: &str) -> VspResult<String> {
    let base = raw.trim().trim_start_matches('v').split('-').next().unwrap_or("");
    let parsed = semver::Version::parse(base).map_err(|_| {
        VspError::UsageOrConfig(format!(
            "invalid VS Code semver version: '{raw}'"
        ))
    })?;
    Ok(format!("{}.{}.{}", parsed.major, parsed.minor, parsed.patch))
}

fn collect_compat_extension_ids(
    catalog: &WorkspaceCatalog,
    opts: &CompatOptions,
) -> VspResult<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    let mut push_ci = |id: &str| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return;
        }
        if seen.insert(trimmed.to_ascii_lowercase()) {
            ids.push(trimmed.to_string());
        }
    };

    if let Some(file) = &opts.extensions_file {
        if !file.is_file() {
            return Err(VspError::TargetMissing(format!(
                "extensions file not found: {}",
                file.display()
            )));
        }
        let raw = fs::read_to_string(file).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", file.display()))
        })?;
        if let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(&raw) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    push_ci(s);
                } else if let Some(s) = item.pointer("/identifier/id").and_then(Value::as_str) {
                    push_ci(s);
                }
            }
        } else {
            for line in raw.lines() {
                let no_comment = line.split('#').next().unwrap_or("").trim();
                push_ci(no_comment);
            }
        }
    }

    let target_profiles: Vec<String> = if opts.all || (opts.profiles.is_empty() && opts.extensions_file.is_none()) {
        catalog.profiles.keys().cloned().collect()
    } else {
        opts.profiles.clone()
    };

    for prof in &target_profiles {
        if !catalog.profiles.contains_key(prof) {
            return Err(VspError::TargetMissing(format!(
                "unknown profile '{prof}'"
            )));
        }
        let resolved =
            resolve_profile_recipe(catalog, prof, &[], &[], Some(UxPreset::Default), None)?;
        let exts = compile_profile_extensions(catalog, &resolved)?;
        for entry in exts {
            push_ci(&entry.identifier.id);
        }
    }

    Ok(ids)
}

fn query_marketplace_compat_batch(
    agent: &ureq::Agent,
    api_url: &str,
    ids: &[String],
    flags: u32,
) -> VspResult<Value> {
    let criteria: Vec<Value> = ids
        .iter()
        .map(|id| serde_json::json!({ "filterType": 7, "value": id }))
        .collect();
    let payload = serde_json::json!({
        "filters": [{
            "criteria": criteria,
            "pageNumber": 1,
            "pageSize": 100
        }],
        "assetTypes": [],
        "flags": flags
    });
    let mut resp = agent
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json;api-version=3.0-preview.1")
        .send_json(&payload)
        .map_err(|err| {
            VspError::IncompleteObservation(format!("compat: Marketplace API request failed: {err}"))
        })?;
    let val: Value = resp.body_mut().read_json().map_err(|err| {
        VspError::IncompleteObservation(format!(
            "compat: Marketplace API returned malformed JSON: {err}"
        ))
    })?;
    if !val.get("results").is_some_and(Value::is_array) {
        return Err(VspError::IncompleteObservation(
            "compat: Marketplace API response missing results array".to_string(),
        ));
    }
    Ok(val)
}

fn pick_extension_version(ext_obj: &Value) -> Option<(String, String, bool)> {
    let versions = ext_obj.get("versions")?.as_array()?;
    if versions.is_empty() {
        return None;
    }
    let is_pre = |v: &Value| {
        v.get("properties")
            .and_then(Value::as_array)
            .is_some_and(|props| {
                props.iter().any(|p| {
                    p.get("key").and_then(Value::as_str)
                        == Some("Microsoft.VisualStudio.Code.PreRelease")
                        && p.get("value").and_then(Value::as_str) == Some("true")
                })
            })
    };
    let (chosen, pre) = match versions.iter().find(|v| !is_pre(v)) {
        Some(stable) => (stable, false),
        None => (&versions[0], true),
    };
    let ver = chosen.get("version").and_then(Value::as_str)?.to_string();
    let engine = chosen
        .get("properties")
        .and_then(Value::as_array)
        .and_then(|props| {
            props.iter().find(|p| {
                p.get("key").and_then(Value::as_str) == Some("Microsoft.VisualStudio.Code.Engine")
            })
        })
        .and_then(|p| p.get("value").and_then(Value::as_str))
        .unwrap_or("*")
        .to_string();
    Some((ver, engine, pre))
}

/// Evaluates whether `vscode_ver` (`major.minor.patch`) satisfies a VS Code `engines.vscode`
/// semver range expression (supporting `||`, space-AND clauses, `^`, `~`, `>=`, `<=`, `>`, `<`,
/// `=`, `*`, and `x` wildcards).
#[must_use]
pub fn semver_satisfies(vscode_ver: &str, range: &str) -> bool {
    let trimmed = range.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return true;
    }
    let Ok(ver) = semver::Version::parse(vscode_ver.split('-').next().unwrap_or(vscode_ver)) else {
        return false;
    };

    // Normalize whitespace immediately following operators (e.g. `>= 1.80.0` -> `>=1.80.0`).
    let mut norm = trimmed.to_string();
    for op in [">=", "<=", ">", "<", "^", "~", "="] {
        let pat = format!("{op} ");
        while norm.contains(&pat) {
            norm = norm.replace(&pat, op);
        }
    }

    for branch in norm.split("||") {
        let clauses: Vec<&str> = branch.split_whitespace().collect();
        if !clauses.is_empty() && clauses.iter().all(|c| semver_clause_matches(&ver, c)) {
            return true;
        }
    }
    false
}

fn semver_clause_matches(ver: &semver::Version, raw_clause: &str) -> bool {
    let clause = raw_clause.trim().trim_start_matches('v');
    if clause.is_empty() || matches!(clause, "*" | "x" | "X") {
        return true;
    }

    let (op, target_raw) = if let Some(t) = clause.strip_prefix(">=") {
        (">=", t)
    } else if let Some(t) = clause.strip_prefix("<=") {
        ("<=", t)
    } else if let Some(t) = clause.strip_prefix('>') {
        (">", t)
    } else if let Some(t) = clause.strip_prefix('<') {
        ("<", t)
    } else if let Some(t) = clause.strip_prefix('^') {
        ("^", t)
    } else if let Some(t) = clause.strip_prefix('~') {
        ("~", t)
    } else if let Some(t) = clause.strip_prefix('=') {
        ("=", t)
    } else {
        ("=", clause)
    };

    let target = target_raw.trim_start_matches('v').split('-').next().unwrap_or("");
    let parts: Vec<&str> = target.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return false;
    }
    if matches!(parts[0], "*" | "x" | "X") {
        return true;
    }
    let Ok(n1) = parts[0].parse::<u64>() else {
        return false;
    };
    let wild_minor = parts.get(1).is_some_and(|p| matches!(*p, "*" | "x" | "X"));
    let wild_patch = parts.get(2).is_some_and(|p| matches!(*p, "*" | "x" | "X"));

    let n2 = if wild_minor || parts.len() < 2 {
        0
    } else if let Ok(v) = parts[1].parse::<u64>() {
        v
    } else {
        return false;
    };
    let n3 = if wild_minor || wild_patch || parts.len() < 3 {
        0
    } else if let Ok(v) = parts[2].parse::<u64>() {
        v
    } else {
        return false;
    };

    let target_ver = semver::Version::new(n1, n2, n3);
    if op == "=" && wild_minor {
        return *ver >= target_ver && *ver < semver::Version::new(n1 + 1, 0, 0);
    }
    if op == "=" && wild_patch {
        return *ver >= target_ver && *ver < semver::Version::new(n1, n2 + 1, 0);
    }

    match op {
        ">=" => *ver >= target_ver,
        ">" => *ver > target_ver,
        "<=" => *ver <= target_ver,
        "<" => *ver < target_ver,
        "=" => *ver == target_ver,
        "^" => {
            let upper = if n1 > 0 {
                semver::Version::new(n1 + 1, 0, 0)
            } else {
                semver::Version::new(0, n2 + 1, 0)
            };
            *ver >= target_ver && *ver < upper
        }
        "~" => *ver >= target_ver && *ver < semver::Version::new(n1, n2 + 1, 0),
        _ => false,
    }
}

/// Checks (`--check`, default) or updates (`--write`) both:
/// 1. The Rust docs policy sentinel block (`<!-- BEGIN/END RUST-DOCS-POLICY -->`) extracted
///    from §12 of `docs/rust.md` across `AGENTS.md`, `CLAUDE.md`, `CODEX.md`, and `GEMINI.md`.
/// 2. The generated Profile & Module Matrix sentinel block (`<!-- BEGIN/END GENERATED-MATRIX -->`)
///    in `README.md`.
///
/// # Errors
///
/// Returns [`VspError::UsageOrConfig`] if both `--check` and `--write` are specified, or
/// [`VspError::Operational`] on missing source files or malformed marker pairs.
pub fn run_dev_docs(
    catalog: &WorkspaceCatalog,
    check: bool,
    write: bool,
    json: bool,
    quiet: bool,
) -> VspResult<ExitCode> {
    if check && write {
        return Err(VspError::UsageOrConfig(
            "vsp dev docs: cannot combine --check and --write".to_string(),
        ));
    }
    let write_mode = write;
    let mut drifts = Vec::new();
    let mut updated = Vec::new();
    let docs_root = if catalog.root == Path::new(crate::schema_loader::EMBEDDED_ROOT_LABEL) {
        crate::schema_loader::resolve_repo_dir(None)?
    } else {
        catalog.root.clone()
    };

    // 1. Rust docs policy sentinel sync/check across agent docs.
    let rust_doc_path = docs_root.join("docs/rust.md");
    if rust_doc_path.is_file() {
        let policy_body = extract_rust_policy_block(&rust_doc_path)?;
        let expected_block = format!("{RUST_POLICY_BEGIN}\n{policy_body}\n{RUST_POLICY_END}");

        // Preflight inspect all target files before modifying any file.
        if write_mode {
            for rel in AGENT_DOC_FILES {
                let target = docs_root.join(rel);
                if target.is_file() {
                    let content = fs::read_to_string(&target).map_err(|err| {
                        VspError::Operational(format!("failed to read {}: {err}", target.display()))
                    })?;
                    if let MarkerState::Malformed(reason) =
                        inspect_marker_pair(&content, RUST_POLICY_BEGIN, RUST_POLICY_END)
                    {
                        return Err(VspError::Operational(format!(
                            "{}: {reason}; refusing to rewrite",
                            target.display()
                        )));
                    }
                }
            }
        }

        for rel in AGENT_DOC_FILES {
            let target = docs_root.join(rel);
            if !target.is_file() {
                if write_mode {
                    let out = format!("{expected_block}\n");
                    write_file_atomic(&target, out.as_bytes(), 0o644, false)?;
                    updated.push((*rel).to_string());
                } else {
                    drifts.push(format!("{rel} missing"));
                }
                continue;
            }
            let content = fs::read_to_string(&target).map_err(|err| {
                VspError::Operational(format!("failed to read {}: {err}", target.display()))
            })?;
            match inspect_marker_pair(&content, RUST_POLICY_BEGIN, RUST_POLICY_END) {
                MarkerState::Valid(current_block) => {
                    if current_block != expected_block {
                        if write_mode {
                            let new_content = replace_marker_block(
                                &content,
                                RUST_POLICY_BEGIN,
                                RUST_POLICY_END,
                                &expected_block,
                            );
                            write_file_atomic(&target, new_content.as_bytes(), 0o644, false)?;
                            updated.push((*rel).to_string());
                        } else {
                            drifts.push(format!(
                                "{rel} RUST-DOCS-POLICY block out of sync with docs/rust.md §12"
                            ));
                        }
                    }
                }
                MarkerState::None => {
                    if write_mode {
                        let mut new_content = content;
                        if !new_content.ends_with('\n') {
                            new_content.push('\n');
                        }
                        new_content.push('\n');
                        new_content.push_str(&expected_block);
                        new_content.push('\n');
                        write_file_atomic(&target, new_content.as_bytes(), 0o644, false)?;
                        updated.push((*rel).to_string());
                    } else {
                        drifts.push(format!("{rel} lacks RUST-DOCS-POLICY sentinels"));
                    }
                }
                MarkerState::Malformed(reason) => {
                    drifts.push(format!("{rel}: {reason}"));
                }
            }
        }
    }

    // 2. README.md generated matrix block sync/check.
    let readme_path = docs_root.join("README.md");
    if readme_path.is_file() {
        let expected_matrix = render_readme_matrix_block(catalog)?;
        let readme_content = fs::read_to_string(&readme_path).map_err(|err| {
            VspError::Operational(format!("failed to read {}: {err}", readme_path.display()))
        })?;
        match inspect_marker_pair(&readme_content, README_MATRIX_BEGIN, README_MATRIX_END) {
            MarkerState::Valid(current_block) => {
                if current_block != expected_matrix {
                    if write_mode {
                        let new_content = replace_marker_block(
                            &readme_content,
                            README_MATRIX_BEGIN,
                            README_MATRIX_END,
                            &expected_matrix,
                        );
                        write_file_atomic(&readme_path, new_content.as_bytes(), 0o644, false)?;
                        updated.push("README.md".to_string());
                    } else {
                        drifts.push("README.md GENERATED-MATRIX block is out of sync".to_string());
                    }
                }
            }
            MarkerState::None => {
                drifts.push("README.md lacks BEGIN/END GENERATED-MATRIX sentinels".to_string());
            }
            MarkerState::Malformed(reason) => {
                drifts.push(format!("README.md: {reason}"));
            }
        }
    }

    if json {
        let payload = serde_json::json!({
            "mode": if write_mode { "write" } else { "check" },
            "inSync": drifts.is_empty(),
            "updated": updated,
            "drifts": drifts,
        });
        print!("{}", to_canonical_json_string(&payload)?);
    } else if !quiet {
        if write_mode {
            if updated.is_empty() {
                println!("ok: all docs already in sync");
            } else {
                println!("updated: {}", updated.join(", "));
            }
        } else if drifts.is_empty() {
            println!("ok: RUST-DOCS-POLICY and README.md matrix blocks are in sync");
        } else {
            for d in &drifts {
                eprintln!("drift: {d}");
            }
        }
    }

    if drifts.is_empty() {
        Ok(ExitCode::Ok)
    } else {
        Ok(ExitCode::Operational)
    }
}

/// Extracts §12 (`# 12. Docs Policy for AI Agents`) inner ` ```md ... ``` ` block from `docs/rust.md`.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `rust.md` cannot be read or contains an empty §12 block.
pub fn extract_rust_policy_block(rust_md_path: &Path) -> VspResult<String> {
    let raw = fs::read_to_string(rust_md_path).map_err(|err| {
        VspError::Operational(format!("failed to read {}: {err}", rust_md_path.display()))
    })?;
    let mut found_section = false;
    let mut in_block = false;
    let mut inner_fence = false;
    let mut lines = Vec::new();

    for line in raw.lines() {
        if !found_section {
            if line.starts_with("# 12. Docs Policy for AI Agents") {
                found_section = true;
            }
            continue;
        }
        if !in_block {
            if line == "```md" {
                in_block = true;
            }
            continue;
        }
        if line.starts_with("```") && line.len() > 3 && line[3..].bytes().all(|b| b.is_ascii_lowercase()) {
            inner_fence = true;
            lines.push(line);
            continue;
        }
        if line == "```" {
            if inner_fence {
                inner_fence = false;
                lines.push(line);
                continue;
            }
            break;
        }
        lines.push(line);
    }

    if lines.is_empty() {
        return Err(VspError::Operational(format!(
            "empty or missing §12 Docs Policy block in {}",
            rust_md_path.display()
        )));
    }
    Ok(lines.join("\n"))
}

/// Renders the deterministic `<!-- BEGIN GENERATED-MATRIX --> ... <!-- END GENERATED-MATRIX -->`
/// block matching `scripts/generate-readme-matrix.sh`.
///
/// # Errors
///
/// Returns [`VspError`] if any profile fails module resolution.
pub fn render_readme_matrix_block(catalog: &WorkspaceCatalog) -> VspResult<String> {
    let mut out = String::new();
    out.push_str(README_MATRIX_BEGIN);
    out.push('\n');
    out.push_str("### Profiles (15 Compiled Recipes)\n\n");
    out.push_str("| Profile | Description | Included Modules | Extensions | Export Bundle |\n");
    out.push_str("| --- | --- | --- | ---: | --- |\n");

    for (prof_name, recipe) in &catalog.profiles {
        let resolved =
            resolve_profile_recipe(catalog, prof_name, &[], &[], Some(UxPreset::Default), None)?;
        let exts = compile_profile_extensions(catalog, &resolved)?;
        let mods_str = resolved.join(", ");
        let bundle_cell = format!("[`{prof_name}.code-profile`](exports/{prof_name}.code-profile)");
        out.push_str(&format!(
            "| `{prof_name}` | {} | `{mods_str}` | {} | {bundle_cell} |\n",
            recipe.description,
            exts.len()
        ));
    }

    out.push_str("\n### Modules (`modules/`)\n\n");
    out.push_str("| Kind | Module ID | Description | Requires | Conflicts |\n");
    out.push_str("| --- | --- | --- | --- | --- |\n");

    // Match scripts/lib/module-resolver.sh discover_modules order: capabilities, toolchains, ux.
    for kind in [
        crate::model::ModuleKind::Capability,
        crate::model::ModuleKind::Toolchain,
        crate::model::ModuleKind::Ux,
    ] {
        for def in catalog.modules.values() {
            if def.manifest.kind != kind {
                continue;
            }
            let reqs = if def.manifest.requires.is_empty() {
                "—".to_string()
            } else {
                def.manifest.requires.join(", ")
            };
            let confs = if def.manifest.conflicts.is_empty() {
                "—".to_string()
            } else {
                def.manifest.conflicts.join(", ")
            };
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {reqs} | {confs} |\n",
                def.manifest.kind.as_str(),
                def.manifest.id,
                def.manifest.description
            ));
        }
    }

    out.push_str(README_MATRIX_END);
    Ok(out)
}

#[derive(Debug, PartialEq, Eq)]
enum MarkerState {
    None,
    Valid(String),
    Malformed(String),
}

fn inspect_marker_pair(content: &str, begin_mark: &str, end_mark: &str) -> MarkerState {
    let b_raw_tok = begin_mark
        .trim_start_matches("<!-- ")
        .trim_end_matches(" -->");
    let e_raw_tok = end_mark
        .trim_start_matches("<!-- ")
        .trim_end_matches(" -->");

    let mut b_cnt = 0usize;
    let mut e_cnt = 0usize;
    let mut b_raw = 0usize;
    let mut e_raw = 0usize;
    let mut b_line = 0usize;
    let mut e_line = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let lineno = idx + 1;
        let bc = line.matches(begin_mark).count();
        if bc > 0 {
            if b_line == 0 {
                b_line = lineno;
            }
            b_cnt += bc;
        }
        let ec = line.matches(end_mark).count();
        if ec > 0 {
            if e_line == 0 {
                e_line = lineno;
            }
            e_cnt += ec;
        }
        b_raw += line.matches(b_raw_tok).count();
        e_raw += line.matches(e_raw_tok).count();
    }

    if b_raw != b_cnt || e_raw != e_cnt {
        return MarkerState::Malformed(format!(
            "corrupted sentinel (exact begin={b_cnt} end={e_cnt}, raw begin={b_raw} end={e_raw})"
        ));
    }
    if b_cnt == 0 && e_cnt == 0 {
        return MarkerState::None;
    }
    if b_cnt == 1 && e_cnt == 1 && b_line < e_line {
        let block: Vec<&str> = content
            .lines()
            .skip(b_line - 1)
            .take(e_line - b_line + 1)
            .collect();
        return MarkerState::Valid(block.join("\n"));
    }
    if b_cnt > 0 && e_cnt == 0 {
        return MarkerState::Malformed(format!("unmatched BEGIN marker (begin={b_cnt}, end=0)"));
    }
    if b_cnt == 0 && e_cnt > 0 {
        return MarkerState::Malformed(format!("unmatched END marker (begin=0, end={e_cnt})"));
    }
    if b_cnt != 1 || e_cnt != 1 {
        return MarkerState::Malformed(format!(
            "multiple policy markers (begin={b_cnt}, end={e_cnt})"
        ));
    }
    MarkerState::Malformed(format!(
        "marker order invalid (begin line {b_line} >= end line {e_line})"
    ))
}

fn replace_marker_block(
    content: &str,
    begin_mark: &str,
    end_mark: &str,
    replacement: &str,
) -> String {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.contains(begin_mark) {
            in_block = true;
            out.push(replacement);
            continue;
        }
        if line.contains(end_mark) {
            in_block = false;
            continue;
        }
        if !in_block {
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// Configures `git config core.hooksPath scripts/git-hooks` idempotently.
///
/// Refuses to overwrite an existing non-default `core.hooksPath` with
/// [`VspError::SafetyRefusal`] (exit `5`).
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if a different `core.hooksPath` is already
/// configured, or [`VspError::Operational`] if `git` fails or a hook script is not executable.
pub fn run_dev_hooks(repo_root: &Path, json: bool, quiet: bool) -> VspResult<ExitCode> {
    let target = "scripts/git-hooks";
    let get_out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--get", "core.hooksPath"])
        .output()
        .map_err(|err| VspError::Operational(format!("failed to invoke git: {err}")))?;

    let current = if get_out.status.success() {
        String::from_utf8_lossy(&get_out.stdout).trim().to_string()
    } else {
        String::new()
    };

    if current.is_empty() {
        let set_out = Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["config", "core.hooksPath", target])
            .output()
            .map_err(|err| VspError::Operational(format!("failed to invoke git config: {err}")))?;
        if !set_out.status.success() {
            return Err(VspError::Operational(format!(
                "git config core.hooksPath failed: {}",
                String::from_utf8_lossy(&set_out.stderr).trim()
            )));
        }
        if json {
            print!(
                "{}",
                to_canonical_json_string(&serde_json::json!({
                    "status": "installed",
                    "hooksPath": target,
                }))?
            );
        } else if !quiet {
            println!("vsp dev hooks: set core.hooksPath = {target}");
        }
        return Ok(ExitCode::Ok);
    }

    if current == target {
        let hooks_dir = repo_root.join(target);
        if hooks_dir.is_dir() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                for entry in fs::read_dir(&hooks_dir).into_iter().flatten().flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        let mode = fs::metadata(&p)
                            .map(|m| m.permissions().mode())
                            .unwrap_or(0);
                        if mode & 0o111 == 0 {
                            return Err(VspError::Operational(format!(
                                "hook {} is not executable; run 'chmod +x {}'",
                                p.display(),
                                p.display()
                            )));
                        }
                    }
                }
            }
        }
        if json {
            print!(
                "{}",
                to_canonical_json_string(&serde_json::json!({
                    "status": "already_installed",
                    "hooksPath": target,
                }))?
            );
        } else if !quiet {
            println!("vsp dev hooks: already installed (core.hooksPath = {target})");
        }
        return Ok(ExitCode::Ok);
    }

    Err(VspError::SafetyRefusal(format!(
        "refusing to overwrite existing core.hooksPath = {current} (expected unset or '{target}')"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_loader::load_legacy_catalog;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root")
            .to_path_buf()
    }

    #[test]
    fn semver_range_evaluator_matches_vscode_engines() {
        assert!(semver_satisfies("1.147.0", "^1.80.0"));
        assert!(!semver_satisfies("1.79.0", "^1.80.0"));
        assert!(semver_satisfies("1.147.0", ">= 1.80.0 <2.0.0"));
        assert!(semver_satisfies("1.147.0", "1.x"));
        assert!(semver_satisfies("1.80.5", "1.80.x"));
        assert!(!semver_satisfies("1.81.0", "1.80.x"));
        assert!(semver_satisfies("1.147.0", "*"));
        assert!(semver_satisfies("1.147.0", "^1.99.0 || ^1.100.0"));
    }

    #[test]
    fn dev_docs_check_passes_on_tracked_repo() {
        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let code = run_dev_docs(&catalog, true, false, false, true).expect("docs check");
        assert_eq!(code, ExitCode::Ok);
    }
}

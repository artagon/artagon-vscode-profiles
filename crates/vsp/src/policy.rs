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

use crate::jsonc::{parse_jsonc_settings_object, read_strict_json_value};
use crate::model::{
    ExtensionStatus, ModuleDefinition, ModuleKind, PinsLoadState, UxPreset, WorkspaceCatalog,
    COMPILER_UNION_KEYS,
};
use crate::resolver::{
    compile_canonical_profile_settings, compile_canonical_profile_settings_from_resolved,
    resolve_modules, resolve_profile_recipe, sha256_hex, ResolveRequest,
};
use crate::schema_loader::{
    embedded_snapshot_files, AutonomyDenyListPolicy, PolicyManifest, EMBEDDED_ROOT_LABEL,
};

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

/// Execution-override key prefixes denied anywhere in configuration. A key is denied when
/// it equals a prefix or continues it with `.` (for example
/// `terminal.integrated.profiles.osx`). Matching is case-insensitive and also covers nested
/// objects and `[language]` blocks.
///
/// These settings select the program, arguments, and environment of the integrated
/// terminal, so an untrusted bundle or override could use them to run an attacker binary.
/// `vsp` adds its own RTK terminal profile after the deny-list check.
pub const EXEC_OVERRIDE_KEY_PREFIXES: &[&str] = &[
    "terminal.integrated.profiles",
    "terminal.integrated.env",
    "terminal.integrated.automationProfile",
    "terminal.integrated.defaultProfile",
];

/// `claudeCode.initialPermissionMode` values that remove the per-action confirmation.
const DENIED_PERMISSION_MODES: &[&str] = &["bypassPermissions", "acceptEdits"];

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

/// Checks whether a leaf key or its joined dotted path equals `prefix` or continues it with
/// `.`, at the start or after a `.` (case-insensitive). This also matches a nested object
/// path (`terminal` -> `integrated` -> `profiles`) and a key inside a `[language]` block.
#[must_use]
pub fn matches_key_prefix(leaf_key: &str, joined_path: &str, prefix: &str) -> bool {
    let p = prefix.to_ascii_lowercase();
    [leaf_key, joined_path].iter().any(|candidate| {
        let c = candidate.to_ascii_lowercase();
        c == p
            || c.starts_with(&format!("{p}."))
            || c.ends_with(&format!(".{p}"))
            || c.contains(&format!(".{p}."))
    })
}

/// Inspects a VS Code `tasks.json` document for tasks that run without a user action.
///
/// A task with `runOptions.runOn == "folderOpen"` starts when the folder opens, so a
/// managed workspace file must never contain one. The check is recursive and
/// case-insensitive.
#[must_use]
pub fn check_tasks_denylist(tasks_doc: &Value, label: &str) -> Vec<PolicyViolation> {
    fn walk(value: &Value, label: &str, out: &mut Vec<PolicyViolation>) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    if key.eq_ignore_ascii_case("runOn")
                        && val
                            .as_str()
                            .is_some_and(|s| s.eq_ignore_ascii_case("folderOpen"))
                    {
                        out.push(PolicyViolation {
                            source: label.to_string(),
                            message: "forbidden auto-run task: runOptions.runOn=folderOpen"
                                .to_string(),
                        });
                    }
                    walk(val, label, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, label, out);
                }
            }
            _ => {}
        }
    }
    let mut violations = Vec::new();
    walk(tasks_doc, label, &mut violations);
    violations
}

/// One entry of [`flatten_setting_paths`].
#[derive(Debug, Clone, PartialEq)]
pub struct FlatSetting<'a> {
    /// Dotted setting key (for example `telemetry.telemetryLevel`).
    pub key: String,
    /// Value at that key.
    pub value: &'a Value,
    /// `true` when the entry is inside a `[language]` block.
    pub in_language_block: bool,
}

/// Flattens a settings map into dotted keys for key-based lints.
///
/// Each top-level entry is emitted. A nested object is also emitted with its joined dotted
/// path (`{"telemetry": {"telemetryLevel": ..}}` gives `telemetry.telemetryLevel`), and the
/// entries of a `[language]` block are emitted with their own keys. A lint that looks up
/// only top-level keys would miss both forms.
#[must_use]
pub fn flatten_setting_paths(map: &Map<String, Value>) -> Vec<FlatSetting<'_>> {
    fn walk<'a>(
        prefix: &str,
        in_lang: bool,
        map: &'a Map<String, Value>,
        out: &mut Vec<FlatSetting<'a>>,
    ) {
        for (key, value) in map {
            let is_lang_block = key.starts_with('[') && key.ends_with(']');
            let path = if prefix.is_empty() || is_lang_block {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            out.push(FlatSetting {
                key: path.clone(),
                value,
                in_language_block: in_lang,
            });
            if let Value::Object(inner) = value {
                if is_lang_block {
                    walk("", true, inner, out);
                } else {
                    walk(&path, in_lang, inner, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk("", false, map, &mut out);
    out
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

#[derive(Clone, Copy)]
enum PatternAtom {
    Any,
    Lit(char),
}

impl PatternAtom {
    fn matches(self, ch: char) -> bool {
        match self {
            Self::Any => true,
            Self::Lit(c) => c == ch,
        }
    }
}

#[derive(Clone, Copy)]
enum PatternQuant {
    One,
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
}

enum CompiledAutonomyPattern {
    Empty,
    ChatToolsAutoApprove,
    Nfa {
        anchored_start: bool,
        anchored_end: bool,
        tokens: Vec<(PatternAtom, PatternQuant)>,
    },
}

impl CompiledAutonomyPattern {
    fn compile(pattern: &str) -> Self {
        if pattern.is_empty() {
            return Self::Empty;
        }
        if pattern.eq_ignore_ascii_case(r"^chat\.tools\..*autoApprove$") {
            return Self::ChatToolsAutoApprove;
        }
        let pat_lc = pattern.to_ascii_lowercase();
        let mut pat_chars = pat_lc.chars().peekable();
        let mut anchored_start = false;
        if pat_chars.peek() == Some(&'^') {
            anchored_start = true;
            pat_chars.next();
        }

        let mut raw_tokens: Vec<(PatternAtom, PatternQuant, bool)> = Vec::new();
        while let Some(ch) = pat_chars.next() {
            let (atom, is_unescaped_dollar) = if ch == '\\' {
                match pat_chars.next() {
                    Some(escaped) => (PatternAtom::Lit(escaped), false),
                    None => (PatternAtom::Lit('\\'), false),
                }
            } else if ch == '.' {
                (PatternAtom::Any, false)
            } else if ch == '$' {
                (PatternAtom::Lit('$'), true)
            } else {
                (PatternAtom::Lit(ch), false)
            };

            let quant = match pat_chars.peek() {
                Some('*') => {
                    pat_chars.next();
                    PatternQuant::ZeroOrMore
                }
                Some('+') => {
                    pat_chars.next();
                    PatternQuant::OneOrMore
                }
                Some('?') => {
                    pat_chars.next();
                    PatternQuant::ZeroOrOne
                }
                _ => PatternQuant::One,
            };
            raw_tokens.push((atom, quant, is_unescaped_dollar));
        }

        let mut anchored_end = false;
        if let Some(&(_, PatternQuant::One, true)) = raw_tokens.last() {
            anchored_end = true;
            raw_tokens.pop();
        }

        let tokens = raw_tokens.into_iter().map(|(a, q, _)| (a, q)).collect();
        Self::Nfa {
            anchored_start,
            anchored_end,
            tokens,
        }
    }

    fn matches(&self, text: &str) -> bool {
        match self {
            Self::Empty => false,
            Self::ChatToolsAutoApprove => matches_chat_tools_autoapprove(text),
            Self::Nfa {
                anchored_start,
                anchored_end,
                tokens,
            } => {
                let chars: Vec<char> = text.to_ascii_lowercase().chars().collect();
                let mut dp = Vec::with_capacity(chars.len() + 1);
                let mut next = Vec::with_capacity(chars.len() + 1);

                let mut match_from = |start: usize| -> bool {
                    let slice = &chars[start..];
                    let n = slice.len();
                    dp.clear();
                    dp.resize(n + 1, false);
                    dp[0] = true;

                    for &(atom, quant) in tokens {
                        next.clear();
                        next.resize(n + 1, false);
                        match quant {
                            PatternQuant::One => {
                                for i in 0..n {
                                    if dp[i] && atom.matches(slice[i]) {
                                        next[i + 1] = true;
                                    }
                                }
                            }
                            PatternQuant::ZeroOrOne => {
                                for i in 0..=n {
                                    if dp[i] {
                                        next[i] = true;
                                        if i < n && atom.matches(slice[i]) {
                                            next[i + 1] = true;
                                        }
                                    }
                                }
                            }
                            PatternQuant::ZeroOrMore => {
                                next.copy_from_slice(&dp);
                                for i in 0..n {
                                    if next[i] && atom.matches(slice[i]) {
                                        next[i + 1] = true;
                                    }
                                }
                            }
                            PatternQuant::OneOrMore => {
                                for i in 0..n {
                                    if (dp[i] || next[i]) && atom.matches(slice[i]) {
                                        next[i + 1] = true;
                                    }
                                }
                            }
                        }
                        std::mem::swap(&mut dp, &mut next);
                    }

                    if *anchored_end {
                        dp[n]
                    } else {
                        dp.iter().any(|&b| b)
                    }
                };

                if *anchored_start {
                    if match_from(0) {
                        return true;
                    }
                    for (idx, &ch) in chars.iter().enumerate() {
                        if ch == '.' && match_from(idx + 1) {
                            return true;
                        }
                    }
                    false
                } else {
                    (0..=chars.len()).any(match_from)
                }
            }
        }
    }
}

struct CompiledAutonomyPolicy<'a> {
    raw: &'a AutonomyDenyListPolicy,
    extra_patterns: Vec<CompiledAutonomyPattern>,
    extra_substrings_lc: Vec<(String, String)>,
}

impl<'a> CompiledAutonomyPolicy<'a> {
    fn compile(raw: &'a AutonomyDenyListPolicy) -> Self {
        let extra_patterns = raw
            .patterns
            .iter()
            .filter(|pat| !pat.eq_ignore_ascii_case(r"^chat\.tools\..*autoApprove$"))
            .map(|pat| CompiledAutonomyPattern::compile(pat))
            .collect();
        let extra_substrings_lc = raw
            .deny_value_substrings
            .iter()
            .filter(|s| {
                !s.is_empty()
                    && !s.eq_ignore_ascii_case("bypassPermissions")
                    && !s.eq_ignore_ascii_case("--dangerously-skip-permissions")
            })
            .map(|s| (s.clone(), s.to_ascii_lowercase()))
            .collect();
        Self {
            raw,
            extra_patterns,
            extra_substrings_lc,
        }
    }
}

/// Matches a declarative regex-subset pattern from `catalog/policy.jsonc`
/// (`autonomyDenyList.patterns`) case-insensitively in linear-space DP (`O(P * N)`), supporting
/// `^`, `$`, `.`, `*`, `+`, `?`, and backslash-escaped literals without external regex
/// dependencies or backtracking blowup.
#[must_use]
pub fn matches_autonomy_pattern(pattern: &str, text: &str) -> bool {
    CompiledAutonomyPattern::compile(pattern).matches(text)
}

/// Recursively inspects a parsed JSON document against the built-in autonomy and
/// execution-override safety floor.
///
/// Defeats all 5 N1 nested/string/case-variation payloads:
/// 1. `{"chat.tools.autoApprove": true}`
/// 2. `{"chat": {"tools": {"autoApprove": true}}}`
/// 3. `{"chat": {"tools.terminal.autoApprove": true}}`
/// 4. `{"claudeCode": {"claudeProcessWrapper": "/tmp/x"}}`
/// 5. `{"claudeCode.allowDangerouslySkipPermissions": "true"}`
#[must_use]
pub fn check_autonomy_denylist(value: &Value, label: &str) -> Vec<PolicyViolation> {
    check_autonomy_denylist_with_policy(value, label, None)
}

/// Recursively inspects a parsed JSON document against the union of the built-in autonomy /
/// execution-override safety floor and any declarative rules declared in
/// `catalog/policy.jsonc` ([`AutonomyDenyListPolicy`]).
///
/// Declarative policy rules can only add restrictions on top of the built-in floor; omitting
/// a rule from `policy` never weakens built-in security checks.
#[must_use]
pub fn check_autonomy_denylist_with_policy(
    value: &Value,
    label: &str,
    policy: Option<&AutonomyDenyListPolicy>,
) -> Vec<PolicyViolation> {
    let mut violations = Vec::new();
    let mut path = Vec::new();
    let compiled = policy.map(CompiledAutonomyPolicy::compile);
    inspect_autonomy_node(value, &mut path, label, compiled.as_ref(), &mut violations);
    violations
}

/// Checks a Default-profile `settings.json` map against the union of the built-in security
/// floor (`telemetry.telemetryLevel == "off"`,
/// `security.workspace.trust.untrustedFiles == "prompt"`, `extensions.autoUpdate == false`)
/// and any declarative `defaultProfileExpectations` declared in `catalog/policy.jsonc`.
#[must_use]
pub fn check_default_profile_expectations(
    settings: &Map<String, Value>,
    policy: Option<&PolicyManifest>,
    label: &str,
) -> Vec<PolicyViolation> {
    let mut violations = Vec::new();
    let mut checked_keys: BTreeSet<String> = BTreeSet::new();

    // Built-in security floor (cannot be weakened by external policy).
    let floor: [(&str, Value, &str); 3] = [
        (
            "telemetry.telemetryLevel",
            Value::String("off".to_string()),
            "string",
        ),
        (
            "security.workspace.trust.untrustedFiles",
            Value::String("prompt".to_string()),
            "string",
        ),
        ("extensions.autoUpdate", Value::Bool(false), "boolean"),
    ];

    for (key, expected, expected_type) in &floor {
        checked_keys.insert((*key).to_string());
        let actual = settings.get(*key);
        if actual != Some(expected) {
            violations.push(PolicyViolation {
                source: label.to_string(),
                message: format!(
                    "default profile expectation failed for '{key}': expected {expected_type} {expected}, found {}",
                    actual.map_or_else(|| "missing".to_string(), Value::to_string)
                ),
            });
        }
    }

    if let Some(pol) = policy {
        for (key, exp) in &pol.default_profile_expectations {
            let actual = settings.get(key);
            let already_checked_same = checked_keys.contains(key)
                && floor
                    .iter()
                    .any(|(fk, fv, _)| fk == key && fv == &exp.expected);
            if already_checked_same {
                continue;
            }
            if actual != Some(&exp.expected) {
                violations.push(PolicyViolation {
                    source: label.to_string(),
                    message: format!(
                        "default profile expectation failed for '{key}': expected {} {}, found {}",
                        exp.value_type,
                        exp.expected,
                        actual.map_or_else(|| "missing".to_string(), Value::to_string)
                    ),
                });
            }
        }
    }

    violations
}

fn inspect_autonomy_node(
    value: &Value,
    path: &mut Vec<String>,
    label: &str,
    policy: Option<&CompiledAutonomyPolicy<'_>>,
    violations: &mut Vec<PolicyViolation>,
) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                path.push(key.clone());
                let joined_owned;
                let joined: &str = if path.len() == 1 {
                    key.as_str()
                } else {
                    joined_owned = path.join(".");
                    joined_owned.as_str()
                };
                let norm_b = normalize_bool(val);

                let matches_pattern = matches_chat_tools_autoapprove(key)
                    || (path.len() > 1 && matches_chat_tools_autoapprove(joined))
                    || policy.is_some_and(|p| {
                        p.extra_patterns
                            .iter()
                            .any(|pat| pat.matches(key) || (path.len() > 1 && pat.matches(joined)))
                    });

                let matches_deny_true = norm_b == Some(true)
                    && (matches_key_or_path(
                        key,
                        joined,
                        "claudeCode.allowDangerouslySkipPermissions",
                    ) || matches_key_or_path(key, joined, "geminicodeassist.agentYoloMode")
                        || policy.is_some_and(|p| {
                            p.raw
                                .deny_true_keys
                                .iter()
                                .any(|target| matches_key_or_path(key, joined, target))
                        }));

                let denied_mode: Option<String> = val.as_str().and_then(|s| {
                    if matches_key_or_path(key, joined, "claudeCode.initialPermissionMode") {
                        if let Some(mode) = DENIED_PERMISSION_MODES
                            .iter()
                            .find(|denied| s.eq_ignore_ascii_case(denied))
                        {
                            return Some((*mode).to_string());
                        }
                    }
                    if let Some(p) = policy {
                        for (target_key, disallowed_vals) in &p.raw.deny_values {
                            if matches_key_or_path(key, joined, target_key) {
                                if let Some(found) = disallowed_vals
                                    .iter()
                                    .find(|denied| s.eq_ignore_ascii_case(denied))
                                {
                                    return Some(found.clone());
                                }
                            }
                        }
                    }
                    None
                });

                let matches_prefix = EXEC_OVERRIDE_KEY_PREFIXES
                    .iter()
                    .any(|prefix| matches_key_prefix(key, joined, prefix))
                    || policy.is_some_and(|p| {
                        p.raw
                            .presence_key_prefixes
                            .iter()
                            .any(|prefix| matches_key_prefix(key, joined, prefix))
                    });

                let matches_presence = EXEC_OVERRIDE_KEYS
                    .iter()
                    .any(|target| matches_key_or_path(key, joined, target))
                    || policy.is_some_and(|p| {
                        p.raw
                            .presence_keys
                            .iter()
                            .any(|target| matches_key_or_path(key, joined, target))
                    });

                if matches_pattern {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy key: {joined}"),
                    });
                } else if matches_deny_true {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy setting: {joined}=true"),
                    });
                } else if let Some(mode) = denied_mode {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden autonomy setting: {joined}={mode}"),
                    });
                } else if matches_prefix || matches_presence {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!("forbidden exec-override key: {joined}"),
                    });
                }

                inspect_autonomy_node(val, path, label, policy, violations);
                path.pop();
            }
        }
        Value::Array(items) => {
            for item in items {
                inspect_autonomy_node(item, path, label, policy, violations);
            }
        }
        Value::String(s) => {
            let lc = s.to_ascii_lowercase();
            if lc.contains("bypasspermissions") || lc.contains("--dangerously-skip-permissions") {
                violations.push(PolicyViolation {
                    source: label.to_string(),
                    message: format!("forbidden bypassPermissions value at: {}", path.join(".")),
                });
            } else if let Some(p) = policy {
                if let Some((raw_sub, _)) = p
                    .extra_substrings_lc
                    .iter()
                    .find(|(_, sub_lc)| lc.contains(sub_lc.as_str()))
                {
                    violations.push(PolicyViolation {
                        source: label.to_string(),
                        message: format!(
                            "forbidden autonomy value substring '{raw_sub}' at: {}",
                            path.join(".")
                        ),
                    });
                }
            }
        }
        _ => {}
    }
}

/// Validates that `digest` is a lowercase 64-character hexadecimal SHA-256 string.
#[must_use]
pub fn is_valid_sha256_hex(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Host suffixes of the Visual Studio Marketplace asset CDNs (each needs a non-empty prefix).
const MARKETPLACE_CDN_SUFFIXES: &[&str] = &[".gallery.vsassets.io", ".gallerycdn.vsassets.io"];

/// The single VSIX URL validator, shared by catalog validation and the runtime downloader.
///
/// The URL is parsed with [`url::Url`] (RFC 3986 / WHATWG). A URL is accepted when:
///
/// - the scheme is `https`, there is no userinfo, and the host is
///   `marketplace.visualstudio.com`, `<name>.gallery.vsassets.io`, or
///   `<name>.gallerycdn.vsassets.io`; or
/// - `allow_test_urls` is `true` and the URL is `http://` with a host that is exactly
///   `localhost`, `127.0.0.1`, or `[::1]`, or is a `file://` URL with no host.
///
/// Raw input that contains whitespace, a control character, or a backslash is rejected
/// before parsing, so the HTTP client cannot read the URL differently from this validator.
///
/// # Errors
///
/// Returns a human-readable reason when the URL is rejected.
pub fn check_vsix_url(url: &str, allow_test_urls: bool) -> Result<(), String> {
    if url
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return Err(format!(
            "rejected URL with whitespace, control character, or backslash: {url:?}"
        ));
    }
    let parsed =
        url::Url::parse(url).map_err(|err| format!("rejected unparsable URL {url}: {err}"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!("rejected URL with userinfo: {url}"));
    }
    // The authority must end at a path (`/`) or at the end of the URL. A query or fragment
    // directly after the host (`https://host?x`) is rejected: no VSIX URL has that form.
    if let Some((_, after_scheme)) = url.split_once("://") {
        if let Some(pos) = after_scheme.find(['/', '?', '#']) {
            if !after_scheme[pos..].starts_with('/') {
                return Err(format!("rejected malformed URL authority: {url}"));
            }
        }
    }

    if allow_test_urls {
        match parsed.scheme() {
            "file" => {
                return match parsed.host() {
                    None => Ok(()),
                    Some(_) => Err(format!("rejected file:// test URL with a host: {url}")),
                };
            }
            "http" => {
                let is_loopback = match parsed.host() {
                    Some(url::Host::Domain(domain)) => domain == "localhost",
                    Some(url::Host::Ipv4(addr)) => addr == std::net::Ipv4Addr::LOCALHOST,
                    Some(url::Host::Ipv6(addr)) => addr == std::net::Ipv6Addr::LOCALHOST,
                    None => false,
                };
                return if is_loopback {
                    Ok(())
                } else {
                    Err(format!(
                        "rejected non-loopback http:// test URL (host must be localhost, 127.0.0.1, or [::1]): {url}"
                    ))
                };
            }
            _ => {}
        }
    }

    if parsed.scheme() != "https" {
        return Err(format!("rejected non-HTTPS URL: {url}"));
    }
    let Some(url::Host::Domain(host)) = parsed.host() else {
        return Err(format!("rejected URL without a DNS host name: {url}"));
    };
    let host_lc = host.to_ascii_lowercase();
    if host_lc == "marketplace.visualstudio.com" {
        return Ok(());
    }
    for suffix in MARKETPLACE_CDN_SUFFIXES {
        if let Some(prefix) = host_lc.strip_suffix(suffix) {
            if !prefix.is_empty() && !prefix.starts_with('.') && !prefix.ends_with('.') {
                return Ok(());
            }
        }
    }
    Err(format!("rejected off-host URL '{host_lc}' in {url}"))
}

/// Validates that a VSIX URL uses `https://` and an official Visual Studio Marketplace host
/// with no userinfo. This is [`check_vsix_url`] with test URLs disabled.
#[must_use]
pub fn is_valid_marketplace_vsix_url(url: &str) -> bool {
    check_vsix_url(url, false).is_ok()
}

fn module_manifest_label(catalog: &WorkspaceCatalog, def: &ModuleDefinition) -> String {
    if catalog.is_v2_layout() {
        if def.manifest.id == "default" {
            format!("{} (virtual)", def.dir.join("default").display())
        } else {
            def.dir
                .join(format!("{}.jsonc", def.manifest.id))
                .display()
                .to_string()
        }
    } else {
        def.dir.join("module.json").display().to_string()
    }
}

fn module_settings_label(
    catalog: &WorkspaceCatalog,
    def: &ModuleDefinition,
    scope: &str,
    rel: &str,
) -> String {
    if catalog.is_v2_layout() {
        format!("{} (settings.{scope})", def.dir.join(rel).display())
    } else {
        def.dir.join(rel).display().to_string()
    }
}

fn validate_policy_manifest(catalog: &WorkspaceCatalog, violations: &mut Vec<PolicyViolation>) {
    let Some(policy) = &catalog.policy else {
        return;
    };
    let src = "catalog/policy.jsonc";
    if !policy.vscode_version.is_empty() && semver::Version::parse(&policy.vscode_version).is_err()
    {
        violations.push(PolicyViolation {
            source: src.to_string(),
            message: format!("invalid semver vscodeVersion: {}", policy.vscode_version),
        });
    }
    for (key, exp) in &policy.default_profile_expectations {
        let type_matches = match exp.value_type.as_str() {
            "integer" => is_json_integer(&exp.expected),
            other => json_type_name(&exp.expected) == other,
        };
        if !type_matches {
            violations.push(PolicyViolation {
                source: src.to_string(),
                message: format!(
                    "defaultProfileExpectations['{key}']: expected value {} does not match declared type '{}'",
                    exp.expected, exp.value_type
                ),
            });
        }
    }
    for (key, floor_val) in [
        ("telemetry.telemetryLevel", Value::String("off".to_string())),
        (
            "security.workspace.trust.untrustedFiles",
            Value::String("prompt".to_string()),
        ),
        ("extensions.autoUpdate", Value::Bool(false)),
    ] {
        match policy.default_profile_expectations.get(key) {
            None => violations.push(PolicyViolation {
                source: src.to_string(),
                message: format!("defaultProfileExpectations missing required floor key '{key}'"),
            }),
            Some(exp) if exp.expected != floor_val => violations.push(PolicyViolation {
                source: src.to_string(),
                message: format!(
                    "defaultProfileExpectations['{key}'] cannot weaken safety floor (expected {floor_val}, got {})",
                    exp.expected
                ),
            }),
            Some(_) => {}
        }
    }
}

/// Runs the complete repository & security validation suite backing `vsp dev validate`.
///
/// Validates:
/// 1. Single-document strict JSON/JSONC syntax across `catalog/`, `packs/` / `modules/`,
///    `profiles/`, `_merged/`, and `exports/` (when present).
/// 2. Pack/module manifests, sibling file references, symmetric `conflicts[]`, and DAG acyclicity.
/// 3. Extension catalog schema, active-module parity, group-order coverage, and exact
///    `LC_ALL=C` sorted publisher allowlist parity.
/// 4. Fail-closed lockfile / pins manifest presence, SHA-256 format, and Marketplace URLs.
/// 5. Workspace & profile setting fragments against declarative + floor autonomy deny-list,
///    workspace lints, and setting-scope rules.
/// 6. Recursive scalar-leaf & object-vs-leaf prefix collision detection across jointly
///    selectable modules within the same scope.
/// 7. Exact UX symmetry (`crisp`, `retina`, `default`, disjoint from `core-editor`).
/// 8. Profile recipes, tracked `_merged/*.json`, `exports/*.code-profile`, `exports/SHA256SUMS`,
///    and forbidden security-bypass paths.
#[must_use]
pub fn validate_repository(catalog: &WorkspaceCatalog) -> ValidationReport {
    let mut violations = Vec::new();
    let mut checked_files = 0usize;
    let is_embedded = catalog.root == Path::new(EMBEDDED_ROOT_LABEL);

    // 1. Strict JSON / JSONC syntax pass.
    if is_embedded {
        checked_files += embedded_snapshot_files().len();
    } else {
        for dir_name in [
            "catalog", "packs", "modules", "profiles", "_merged", "exports",
        ] {
            let dir = catalog.root.join(dir_name);
            if dir.is_dir() {
                validate_tree_syntax(&catalog.root, &dir, &mut checked_files, &mut violations);
            }
        }
        let presets_path = catalog.root.join("presets.jsonc");
        if presets_path.is_file() {
            checked_files += 1;
            if let Err(err) = parse_jsonc_settings_object(&presets_path) {
                violations.push(PolicyViolation {
                    source: presets_path.display().to_string(),
                    message: err.to_string(),
                });
            }
        }
        let lock_path = catalog.root.join("vsp.lock.json");
        if lock_path.is_file() {
            checked_files += 1;
            if let Err(err) = read_strict_json_value(&lock_path, false) {
                violations.push(PolicyViolation {
                    source: lock_path.display().to_string(),
                    message: err.to_string(),
                });
            }
        }
    }

    // 2. Module sibling files, references, symmetric conflicts, and DAG acyclicity.
    for (mid, def) in &catalog.modules {
        let manifest_label = module_manifest_label(catalog, def);
        if is_embedded {
            if def.manifest.settings.workspace.is_some() && def.workspace_settings.is_none() {
                violations.push(PolicyViolation {
                    source: manifest_label.clone(),
                    message: "embedded pack declares settings.workspace but parsed map is missing"
                        .to_string(),
                });
            }
            if def.manifest.settings.profile.is_some() && def.profile_settings.is_none() {
                violations.push(PolicyViolation {
                    source: manifest_label.clone(),
                    message: "embedded pack declares settings.profile but parsed map is missing"
                        .to_string(),
                });
            }
            if def.manifest.tasks.is_some() && def.tasks_doc.is_none() {
                violations.push(PolicyViolation {
                    source: manifest_label.clone(),
                    message: "embedded pack declares tasks but parsed document is missing"
                        .to_string(),
                });
            }
        } else {
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
        }

        if let Some(tasks_doc) = &def.tasks_doc {
            violations.extend(check_tasks_denylist(
                tasks_doc,
                &format!("{manifest_label} (tasks)"),
            ));
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

    // 3. Catalog integrity, active-module parity, publisher allowlist parity, and group order.
    validate_catalog_and_publishers(catalog, &mut violations);

    // 4. Fail-closed pins/lockfile & declarative policy manifest validation.
    validate_pins(catalog, &mut violations);
    validate_policy_manifest(catalog, &mut violations);

    // 5. Settings fragment validation (autonomy deny-list, workspace lints, setting-scopes).
    let mut ux_key_set: BTreeSet<String> = catalog.ux_managed_keys().into_iter().collect();
    ux_key_set.extend(catalog.expected_ux_keys());
    let ux_keys: Vec<String> = ux_key_set.into_iter().collect();
    for def in catalog.modules.values() {
        if let Some(rel) = &def.manifest.settings.workspace {
            if let Some(ws_map) = &def.workspace_settings {
                let src = module_settings_label(catalog, def, "workspace", rel);
                validate_settings_fragment(catalog, &src, ws_map, true, &ux_keys, &mut violations);
            }
        }
        if let Some(rel) = &def.manifest.settings.profile {
            if let Some(prof_map) = &def.profile_settings {
                let src = module_settings_label(catalog, def, "profile", rel);
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
    let is_v2 = catalog.is_v2_layout();
    let ext_src = if is_v2 {
        "catalog/extensions.jsonc"
    } else {
        "catalog/extensions.json"
    };
    let pub_src = if is_v2 {
        "catalog/extensions.jsonc"
    } else {
        "catalog/publishers.json"
    };
    let group_order = catalog.effective_group_order();
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
            && !group_order
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
            source: pub_src.to_string(),
            message: format!(
                "{pub_src} publishers are not strictly lowercase, unique, and LC_ALL=C sorted"
            ),
        });
    }
    if normalized_actual != expected_pubs {
        violations.push(PolicyViolation {
            source: pub_src.to_string(),
            message: format!("{pub_src} drifted from active module publishers"),
        });
    }
}

fn validate_pins(catalog: &WorkspaceCatalog, violations: &mut Vec<PolicyViolation>) {
    let is_v2 = catalog.is_v2_layout();
    let pins_label = if is_v2 {
        "vsp.lock.json"
    } else {
        "catalog/extension-pins.json"
    };
    let scopes_label = if is_v2 {
        "catalog/policy.jsonc"
    } else {
        "catalog/setting-scopes.json"
    };

    match &catalog.pins {
        PinsLoadState::Missing(path) => {
            violations.push(PolicyViolation {
                source: path.display().to_string(),
                message: format!("{pins_label} is missing or unreadable"),
            });
        }
        PinsLoadState::Invalid { path, error } => {
            violations.push(PolicyViolation {
                source: path.display().to_string(),
                message: format!("{pins_label} parse failed: {error}"),
            });
        }
        PinsLoadState::Loaded(manifest) => {
            for (k, pin) in &manifest.pins {
                if k != &k.to_ascii_lowercase() {
                    violations.push(PolicyViolation {
                        source: pins_label.to_string(),
                        message: format!("non-lowercase pin key: {k}"),
                    });
                }
                if !is_valid_sha256_hex(&pin.sha256) {
                    violations.push(PolicyViolation {
                        source: pins_label.to_string(),
                        message: format!("invalid sha256 for pin {k}"),
                    });
                }
                if !is_valid_marketplace_vsix_url(&pin.vsix_url) {
                    violations.push(PolicyViolation {
                        source: pins_label.to_string(),
                        message: format!(
                            "invalid or non-Marketplace vsix_url for pin {k}: {}",
                            pin.vsix_url
                        ),
                    });
                }
                if !pin.version.is_empty() && semver::Version::parse(&pin.version).is_err() {
                    violations.push(PolicyViolation {
                        source: pins_label.to_string(),
                        message: format!("invalid semver version for pin {k}: {}", pin.version),
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
                source: scopes_label.to_string(),
                message: format!("invalid semver vscodeVersion: {}", scopes.vscode_version),
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
    let autonomy_policy = catalog.policy.as_ref().map(|p| &p.autonomy_deny_list);
    let val = Value::Object(map.clone());
    violations.extend(check_autonomy_denylist_with_policy(
        &val,
        source,
        autonomy_policy,
    ));

    if is_workspace {
        if map.contains_key("editor.defaultFormatter") {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: "top-level editor.defaultFormatter forbidden in workspace settings (scope inside [lang])".to_string(),
            });
        }
        for flat in flatten_setting_paths(map) {
            let k = &flat.key;
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

    if catalog.policy.is_none() && catalog.setting_scopes.is_none() {
        return;
    }

    let vscode_version = catalog
        .policy
        .as_ref()
        .map(|p| p.vscode_version.as_str())
        .or_else(|| {
            catalog
                .setting_scopes
                .as_ref()
                .map(|s| s.vscode_version.as_str())
        })
        .unwrap_or("");

    let mut forbidden_keys: BTreeSet<&str> = BTreeSet::new();
    if let Some(pol) = &catalog.policy {
        forbidden_keys.extend(pol.forbidden_keys.iter().map(String::as_str));
    }
    if let Some(scopes) = &catalog.setting_scopes {
        forbidden_keys.extend(scopes.forbidden_keys.iter().map(String::as_str));
    }

    let ws_fragment_label = if catalog.is_v2_layout() {
        "settings.workspace"
    } else {
        "settings.workspace.jsonc"
    };

    // Nested objects and `[language]` blocks are flattened to dotted keys first, so a nested
    // form cannot avoid the forbidden-key and scope lints.
    for flat in flatten_setting_paths(map) {
        let key = &flat.key;
        let value = flat.value;
        if forbidden_keys.contains(key.as_str()) {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!("forbidden/deprecated VS Code {vscode_version} key: {key}"),
            });
        }
        let spec = catalog
            .policy
            .as_ref()
            .and_then(|p| p.scopes.get(key))
            .or_else(|| {
                catalog
                    .setting_scopes
                    .as_ref()
                    .and_then(|s| s.settings.get(key))
            });
        let Some(spec) = spec else {
            continue;
        };

        // A `[language]` block in a workspace file is the intended place for a
        // language-specific override of a profile-layer editor setting (for example
        // `"[markdown]": {"editor.quickSuggestions": ..}`), so the `profile` scope is not
        // reported there. `machine` and `application` settings are reported in every form.
        let scope_forbidden_here = if flat.in_language_block {
            matches!(spec.scope.as_str(), "machine" | "application")
        } else {
            matches!(spec.scope.as_str(), "machine" | "profile" | "application")
        };
        if is_workspace && scope_forbidden_here {
            violations.push(PolicyViolation {
                source: source.to_string(),
                message: format!(
                    "key {key} has scope={} and is forbidden in {ws_fragment_label}",
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

type BorrowedScalarLeafMap<'a> = BTreeMap<Vec<&'a str>, &'a Value>;
type BorrowedObjectPathSet<'a> = BTreeSet<Vec<&'a str>>;

struct PrecomputedModuleSettings<'a> {
    ws: Option<(BorrowedScalarLeafMap<'a>, BorrowedObjectPathSet<'a>)>,
    pr: Option<(BorrowedScalarLeafMap<'a>, BorrowedObjectPathSet<'a>)>,
}

fn validate_module_collisions(catalog: &WorkspaceCatalog, violations: &mut Vec<PolicyViolation>) {
    // Precompute transitive requires[] ancestors for each module using borrowed &str keys.
    let mut ancestors: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for id in catalog.modules.keys() {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut stack: Vec<&str> = vec![id.as_str()];
        while let Some(cur) = stack.pop() {
            if let Some(def) = catalog.modules.get(cur) {
                for req in &def.manifest.requires {
                    if seen.insert(req.as_str()) {
                        stack.push(req.as_str());
                    }
                }
            }
        }
        ancestors.insert(id.as_str(), seen);
    }

    // Precompute borrowed scalar leaf maps and object-path sets ONCE per module & scope
    // before the O(N^2) pair comparison loop (zero Value/String cloning per pair).
    let mods: Vec<&ModuleDefinition> = catalog.modules.values().collect();
    let precomputed: Vec<PrecomputedModuleSettings<'_>> = mods
        .iter()
        .map(|def| PrecomputedModuleSettings {
            ws: def
                .workspace_settings
                .as_ref()
                .map(|m| (collect_scalar_leaves(m), collect_object_paths(m))),
            pr: def
                .profile_settings
                .as_ref()
                .map(|m| (collect_scalar_leaves(m), collect_object_paths(m))),
        })
        .collect();

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
                .get(a.manifest.id.as_str())
                .is_some_and(|set| set.contains(b.manifest.id.as_str()))
                || ancestors
                    .get(b.manifest.id.as_str())
                    .is_some_and(|set| set.contains(a.manifest.id.as_str()))
            {
                continue;
            }

            for (scope_label, scope_a, scope_b) in [
                ("ws", &precomputed[i].ws, &precomputed[j].ws),
                ("pr", &precomputed[i].pr, &precomputed[j].pr),
            ] {
                let (Some((la, oa)), Some((lb, ob))) = (scope_a, scope_b) else {
                    continue;
                };

                for (path_a, val_a) in la {
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
    let is_v2 = catalog.is_v2_layout();
    let ux_group_src = if is_v2 { "packs (ux)" } else { "modules/ux" };
    let default_src = if is_v2 {
        "packs/default (virtual)"
    } else {
        "modules/ux/default/settings.profile.jsonc"
    };
    let crisp_src = if is_v2 {
        "packs/crisp.jsonc"
    } else {
        "modules/ux/crisp/settings.profile.jsonc"
    };
    let retina_src = if is_v2 {
        "packs/retina.jsonc"
    } else {
        "modules/ux/retina/settings.profile.jsonc"
    };

    let (Some(crisp), Some(retina), Some(default_ux), Some(core)) = (
        catalog
            .modules
            .get("crisp")
            .and_then(|m| m.profile_settings.as_ref()),
        catalog
            .modules
            .get("retina")
            .and_then(|m| m.profile_settings.as_ref()),
        catalog
            .modules
            .get("default")
            .and_then(|m| m.profile_settings.as_ref()),
        catalog
            .modules
            .get("core-editor")
            .and_then(|m| m.profile_settings.as_ref()),
    ) else {
        violations.push(PolicyViolation {
            source: ux_group_src.to_string(),
            message: "missing required UX or core-editor settings fragments (crisp, retina, default, core-editor)".to_string(),
        });
        return;
    };

    let expected_ux_owned = catalog.expected_ux_keys();
    let expected_ux: Vec<&str> = expected_ux_owned.iter().map(String::as_str).collect();
    let crisp_keys: Vec<&str> = crisp.keys().map(String::as_str).collect();
    let retina_keys: Vec<&str> = retina.keys().map(String::as_str).collect();
    if crisp_keys != retina_keys {
        violations.push(PolicyViolation {
            source: ux_group_src.to_string(),
            message: "crisp and retina UX presets have different key sets".to_string(),
        });
    }
    if crisp_keys != expected_ux {
        violations.push(PolicyViolation {
            source: ux_group_src.to_string(),
            message: format!(
                "UX presets do not match the expected {} symmetric managed keys",
                expected_ux.len()
            ),
        });
    }
    if !default_ux.is_empty() {
        violations.push(PolicyViolation {
            source: default_src.to_string(),
            message: format!("{default_src} must be empty {{}}"),
        });
    }
    if retina.get("workbench.iconTheme").and_then(Value::as_str) != Some("catppuccin-mocha") {
        violations.push(PolicyViolation {
            source: retina_src.to_string(),
            message: "retina workbench.iconTheme must be 'catppuccin-mocha'".to_string(),
        });
    }
    if retina
        .get("workbench.productIconTheme")
        .and_then(Value::as_str)
        != Some("fluent-icons")
    {
        violations.push(PolicyViolation {
            source: retina_src.to_string(),
            message: "retina workbench.productIconTheme must be 'fluent-icons'".to_string(),
        });
    }
    if crisp
        .get("workbench.productIconTheme")
        .and_then(Value::as_str)
        != Some("material-product-icons")
    {
        violations.push(PolicyViolation {
            source: crisp_src.to_string(),
            message: "crisp workbench.productIconTheme must be 'material-product-icons'"
                .to_string(),
        });
    }
    for k in &crisp_keys {
        if core.contains_key(*k) {
            violations.push(PolicyViolation {
                source: ux_group_src.to_string(),
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
    let is_embedded = catalog.root == Path::new(EMBEDDED_ROOT_LABEL);
    let policy = catalog.policy.as_ref();
    let autonomy_policy = policy.map(|p| &p.autonomy_deny_list);

    for (name, recipe) in &catalog.profiles {
        let src = format!("preset:{name}");
        if let Ok(val) = serde_json::to_value(recipe) {
            violations.extend(check_autonomy_denylist_with_policy(
                &val,
                &src,
                autonomy_policy,
            ));
        }
        // Resolve each profile ONCE when its UX is already default/empty (all canonical presets).
        let compiled_res = if recipe.ux.is_empty() || recipe.ux == "default" {
            match resolve_profile_recipe(catalog, name, &[], &[], Some(UxPreset::Default), None) {
                Ok(resolved) => {
                    compile_canonical_profile_settings_from_resolved(catalog, recipe, &resolved)
                }
                Err(err) => {
                    violations.push(PolicyViolation {
                        source: src,
                        message: format!("profile '{name}' recipe failed resolution: {err}"),
                    });
                    continue;
                }
            }
        } else {
            if let Err(err) = resolve_profile_recipe(catalog, name, &[], &[], None, None) {
                violations.push(PolicyViolation {
                    source: src,
                    message: format!("profile '{name}' recipe failed resolution: {err}"),
                });
                continue;
            }
            compile_canonical_profile_settings(catalog, name)
        };

        match compiled_res {
            Ok(compiled) => {
                let val = Value::Object(compiled);
                violations.extend(check_autonomy_denylist_with_policy(
                    &val,
                    &src,
                    autonomy_policy,
                ));
                check_baseline_invariants(&val, &src, true, ux_keys, policy, violations);
            }
            Err(err) => {
                violations.push(PolicyViolation {
                    source: src,
                    message: format!(
                        "profile '{name}' canonical settings compilation failed: {err}"
                    ),
                });
            }
        }
    }

    if is_embedded {
        return;
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
                    violations.extend(check_autonomy_denylist_with_policy(
                        &val,
                        &src,
                        autonomy_policy,
                    ));
                    check_baseline_invariants(&val, &src, false, ux_keys, policy, violations);
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
                            violations.extend(check_autonomy_denylist_with_policy(
                                settings,
                                &src,
                                autonomy_policy,
                            ));
                            check_baseline_invariants(
                                settings, &src, true, ux_keys, policy, violations,
                            );
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
    policy: Option<&PolicyManifest>,
    violations: &mut Vec<PolicyViolation>,
) {
    let Some(obj) = settings.as_object() else {
        return;
    };
    if obj
        .get("workbench.sideBar.location")
        .and_then(Value::as_str)
        != Some("left")
    {
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
    // Enforce any additional compiled-profile-compatible expectations declared in catalog/policy.jsonc.
    if let Some(pol) = policy {
        for (key, exp) in &pol.default_profile_expectations {
            if matches!(
                key.as_str(),
                "security.workspace.trust.untrustedFiles" | "telemetry.telemetryLevel"
            ) {
                continue;
            }
            let disallowed_in_compiled = pol
                .scopes
                .get(key)
                .is_some_and(|spec| spec.allow_in_compiled_profile == Some(false));
            if disallowed_in_compiled {
                continue;
            }
            if obj.get(key) != Some(&exp.expected) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("{key} != {}", exp.expected),
                });
            }
        }
    }
    if obj.contains_key("$schema") || obj.contains_key("@extends") {
        violations.push(PolicyViolation {
            source: source.to_string(),
            message: "contains forbidden $schema or @extends metadata key".to_string(),
        });
    }
    if check_no_ux_leak {
        for flat in flatten_setting_paths(obj) {
            let k = &flat.key;
            if ux_keys.iter().any(|u| u == k) {
                violations.push(PolicyViolation {
                    source: source.to_string(),
                    message: format!("exported bundle leaks UX key: {k}"),
                });
            }
        }
    }
}

fn collect_scalar_leaves<'a>(map: &'a Map<String, Value>) -> BorrowedScalarLeafMap<'a> {
    fn walk<'a>(val: &'a Value, prefix: &mut Vec<&'a str>, out: &mut BorrowedScalarLeafMap<'a>) {
        match val {
            Value::Object(obj) => {
                for (k, v) in obj {
                    prefix.push(k.as_str());
                    walk(v, prefix, out);
                    prefix.pop();
                }
            }
            Value::Array(_) => {
                if prefix.len() == 1 && COMPILER_UNION_KEYS.contains(&prefix[0]) {
                    return;
                }
                out.insert(prefix.clone(), val);
            }
            _ => {
                out.insert(prefix.clone(), val);
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut prefix = Vec::new();
    for (k, v) in map {
        prefix.push(k.as_str());
        walk(v, &mut prefix, &mut out);
        prefix.pop();
    }
    out
}

fn collect_object_paths<'a>(map: &'a Map<String, Value>) -> BorrowedObjectPathSet<'a> {
    fn walk_obj<'a>(
        obj: &'a Map<String, Value>,
        prefix: &mut Vec<&'a str>,
        out: &mut BorrowedObjectPathSet<'a>,
    ) {
        for (k, v) in obj {
            if let Value::Object(sub) = v {
                prefix.push(k.as_str());
                out.insert(prefix.clone());
                walk_obj(sub, prefix, out);
                prefix.pop();
            }
        }
    }
    let mut out = BTreeSet::new();
    let mut prefix = Vec::new();
    walk_obj(map, &mut prefix, &mut out);
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

    /// Opus M4: terminal exec-override keys and `acceptEdits` are denied in every form.
    #[test]
    fn denylist_blocks_terminal_exec_overrides_and_permission_modes() {
        for payload in [
            json!({"terminal.integrated.profiles.osx": {"x": {"path": "/tmp/evil"}}}),
            json!({"terminal.integrated.env.linux": {"LD_PRELOAD": "/tmp/x.so"}}),
            json!({"terminal.integrated.automationProfile.osx": {"path": "/tmp/evil"}}),
            json!({"terminal.integrated.defaultProfile.windows": "evil"}),
            json!({"terminal": {"integrated": {"profiles": {"osx": {}}}}}),
            json!({"Terminal.Integrated.Env.OSX": {}}),
            json!({"[rust]": {"terminal.integrated.env.osx": {}}}),
            json!({"claudeCode.initialPermissionMode": "acceptEdits"}),
            json!({"claudeCode": {"initialPermissionMode": "ACCEPTEDITS"}}),
            json!({"claudeCode.initialPermissionMode": "bypassPermissions"}),
        ] {
            assert!(
                !check_autonomy_denylist(&payload, "t").is_empty(),
                "must be denied: {payload}"
            );
        }
        for allowed in [
            json!({"terminal.integrated.fontSize": 12}),
            json!({"terminal.integrated.profilesExtra": 1}),
            json!({"claudeCode.initialPermissionMode": "default"}),
        ] {
            assert!(
                check_autonomy_denylist(&allowed, "t").is_empty(),
                "must be allowed: {allowed}"
            );
        }
    }

    /// Opus M4: a managed tasks document must not contain an auto-run task.
    #[test]
    fn tasks_denylist_blocks_folder_open_auto_run() {
        let bad = json!({"version": "2.0.0", "tasks": [
            {"label": "ok", "command": "cargo build"},
            {"label": "auto", "command": "curl x | sh", "runOptions": {"runOn": "folderOpen"}}
        ]});
        assert_eq!(check_tasks_denylist(&bad, "t").len(), 1);
        let bad_case = json!({"tasks": [{"runOptions": {"RunOn": "FOLDEROPEN"}}]});
        assert_eq!(check_tasks_denylist(&bad_case, "t").len(), 1);
        let good = json!({"tasks": [{"label": "ok", "runOptions": {"runOn": "default"}}]});
        assert!(check_tasks_denylist(&good, "t").is_empty());

        let mut catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        catalog.modules.get_mut("rust").expect("rust").tasks_doc = Some(bad);
        let report = validate_repository(&catalog);
        assert!(report
            .violations
            .iter()
            .any(|v| v.message.contains("runOn=folderOpen")));
    }

    /// Opus L5: nested objects and `[language]` blocks reach the scope, forbidden-key, and
    /// UX lints.
    #[test]
    fn scope_and_ux_lints_see_nested_and_language_block_keys() {
        let map = json!({
            "a.b": 1,
            "telemetry": {"telemetryLevel": "all"},
            "[rust]": {"editor.fontSize": 9, "x": {"y": true}}
        });
        let flat_entries = flatten_setting_paths(map.as_object().expect("object"));
        assert!(flat_entries
            .iter()
            .any(|f| f.key == "editor.fontSize" && f.in_language_block));
        assert!(flat_entries
            .iter()
            .any(|f| f.key == "telemetry.telemetryLevel" && !f.in_language_block));
        let flat: Vec<String> = flat_entries.into_iter().map(|f| f.key).collect();
        for expected in [
            "a.b",
            "telemetry",
            "telemetry.telemetryLevel",
            "[rust]",
            "editor.fontSize",
            "x",
            "x.y",
        ] {
            assert!(
                flat.iter().any(|k| k == expected),
                "missing {expected}: {flat:?}"
            );
        }

        let catalog = load_legacy_catalog(&repo_root()).expect("catalog");
        let ux_keys = catalog.ux_managed_keys();
        let ux_key = ux_keys.first().expect("a UX key").clone();
        let (head, tail) = ux_key.split_once('.').expect("dotted UX key");

        // Workspace fragment: nested and `[language]` forms of a UX key and of the
        // application-scoped telemetry key are reported.
        let mut violations = Vec::new();
        let fragment = json!({
            head: {tail: 1},
            "[rust]": {ux_key.clone(): 1, "telemetry.telemetryLevel": "all"},
            "telemetry": {"telemetryLevel": "all"}
        });
        validate_settings_fragment(
            &catalog,
            "fragment",
            fragment.as_object().expect("object"),
            true,
            &ux_keys,
            &mut violations,
        );
        let ux_hits = violations
            .iter()
            .filter(|v| v.message.contains("personal UX key forbidden"))
            .count();
        assert_eq!(ux_hits, 2, "{violations:?}");
        // The application-scoped key is reported for the nested form and for the
        // `[language]` form.
        let telemetry_hits = violations
            .iter()
            .filter(|v| {
                v.message
                    .contains("key telemetry.telemetryLevel has scope=application")
            })
            .count();
        assert_eq!(telemetry_hits, 2, "{violations:?}");
    }

    /// The shared validator rejects a query or fragment directly after the host.
    #[test]
    fn vsix_url_validator_requires_a_path_after_the_host() {
        assert!(check_vsix_url("https://marketplace.visualstudio.com/x?y=1#z", false).is_ok());
        for bad in [
            "https://marketplace.visualstudio.com?evil=1",
            "https://marketplace.visualstudio.com#frag",
            "https://marketplace.visualstudio.com:443?x",
        ] {
            assert!(check_vsix_url(bad, false).is_err(), "{bad}");
        }
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
            .any(|v| v.message.contains("object-vs-leaf collision")
                && v.message.contains("test.conflict")));
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

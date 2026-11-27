//! VSIX supply-chain lock verifier, bounded 4-worker concurrent downloader with shared
//! connection pooling and per-hop HTTPS + host allowlist redirect validation, XDG VSIX
//! cache verification, and `--repair-pins`.

use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::exit_codes::{VspError, VspResult};
use crate::model::{ExtensionPin, PinsLoadState, WorkspaceCatalog};
use crate::policy::{check_vsix_url, is_valid_sha256_hex};
use crate::provenance::resolve_xdg_cache_home;
use crate::schema_loader::{
    embedded_snapshot_files, LockEntryStatus, LockfileManifest, EMBEDDED_ROOT_LABEL,
};
use crate::vscode::InstalledExtension;

/// Maximum number of concurrent VSIX download/verification worker threads.
pub const MAX_VSIX_WORKERS: usize = 4;

/// Result of one VSIX job: the install argument, or `(message, is_safety_refusal, kind)`.
type WorkerOutcome = Result<String, (String, bool, VsixFailureKind)>;

/// Maximum number of HTTP redirects followed while validating every hop.
const MAX_REDIRECT_HOPS: usize = 10;

/// Maximum VSIX archive download size (`1 GiB`) to accommodate large native AI extensions
/// (for example `openai.chatgpt` > 340 MiB) while still bounding disk usage during `64 KiB`
/// streaming verification.
const MAX_VSIX_BYTES: u64 = 1024 * 1024 * 1024;

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolved VSIX lock entry sourced from `vsp.lock.json` or `catalog/extension-pins.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPinEntry {
    /// Lowercase extension identifier (`publisher.name`).
    pub id: String,
    /// Pinned semver version string.
    pub version: String,
    /// Expected lowercase 64-hex SHA-256 digest.
    pub sha256: String,
    /// Official Marketplace HTTPS VSIX download URL.
    pub vsix_url: String,
    /// Whether this entry is strictly required (Tier 1 / `pinRequired`).
    pub required: bool,
    /// Lock tier from `vsp.lock.json` (`1` = hash-locked, `2` = version-locked). `None` for
    /// entries from a flat pin manifest.
    pub tier: Option<u8>,
    /// `true` when the lockfile entry is marked `status: "pending_refresh"`.
    pub status_pending: bool,
}

impl LockPinEntry {
    /// Returns `true` when the entry has both a SHA-256 digest and a download URL, so the
    /// VSIX can be fetched and verified. A `pending_refresh` entry never has them.
    #[must_use]
    pub fn has_verified_artifact(&self) -> bool {
        !self.sha256.is_empty() && !self.vsix_url.is_empty()
    }
}

/// Returns `true` when `catalog` was loaded from the compile-time embedded snapshot rather
/// than an explicit external `--catalog <DIR>`.
#[must_use]
pub fn catalog_is_embedded(catalog: &WorkspaceCatalog) -> bool {
    catalog.root == Path::new(EMBEDDED_ROOT_LABEL)
}

/// Returns the trimmed path from `ARTAGON_VSCODE_PINS_FILE` when set and non-empty.
#[must_use]
pub fn pins_file_override_path() -> Option<PathBuf> {
    env::var("ARTAGON_VSCODE_PINS_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Returns the Marketplace/lockfile target platform key for the running host.
#[must_use]
pub fn current_lock_platform() -> &'static str {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "win32-x64",
        _ => "universal",
    }
}

/// Builds a shared `ureq::Agent` configured for VSIX artifact downloads with manual
/// redirect handling (`max_redirects(0)`) so every hop is validated against the strict
/// Marketplace host allowlist.
#[must_use]
pub fn build_vsix_http_agent(allow_test_urls: bool) -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(!allow_test_urls)
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Builds a shared `ureq::Agent` for Visual Studio Marketplace JSON API queries.
#[must_use]
pub fn build_marketplace_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

/// Options controlling VSIX pin resolution and download verification.
#[derive(Debug, Clone, Default)]
pub struct VsixVerifyOptions {
    /// Explicit `--allow-unlocked` bypass (selectively relaxes `pending_refresh` / missing
    /// target entries while still verifying pinned SHA-256 digests).
    pub allow_unlocked: bool,
    /// Override VSIX cache directory (defaults to `VSCODE_VSIX_CACHE_DIR` or `${XDG_CACHE_HOME:-~/.cache}/vsp/vsix`).
    pub cache_dir: Option<PathBuf>,
}

/// Machine-readable cause category for a [`VsixFailure`], enabling grouped refusal diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VsixFailureKind {
    /// Tier 1 lock entry is marked `pending_refresh` (not yet refreshed with a verified digest).
    Tier1PendingRefresh,
    /// Tier 1 lock entry has no verified VSIX digest for the running host platform.
    Tier1MissingHostTarget,
    /// `pinRequired=true` lock policy has no pin entry for the extension.
    PinRequiredMissing,
    /// Version lock string is not valid semver.
    InvalidVersion,
    /// Cryptographic or URL policy refusal (SHA-256 mismatch, off-host URL/redirect, bad ID).
    VerificationRefusal,
    /// Non-safety operational download or I/O failure.
    Operational,
}

/// Failure detail for a single VSIX resolution/verification entry.
#[derive(Debug, Clone)]
pub struct VsixFailure {
    /// Extension identifier (`publisher.name`).
    pub id: String,
    /// Human-readable error message.
    pub message: String,
    /// `true` when the failure is a fail-closed security/policy refusal (exit `5`).
    pub is_safety_refusal: bool,
    /// Structured cause category for grouped diagnostics.
    pub kind: VsixFailureKind,
}

/// Result of resolving install targets for a list of extension IDs.
#[derive(Debug, Clone)]
pub struct ResolvedInstallTargets {
    /// Ordered `(extension_id, install_arg)` pairs where `install_arg` is either a verified
    /// local `.vsix` path or an extension ID (or `id@version` for Tier 2).
    pub targets: Vec<(String, String)>,
    /// Extensions that failed pin lookup or VSIX verification.
    pub failures: Vec<VsixFailure>,
    /// `true` if `--allow-unlocked` / `ARTAGON_VSCODE_ALLOW_UNLOCKED=1` actually relaxed at
    /// least one pending/missing lock entry or a missing/invalid pin manifest.
    pub unlocked_bypass: bool,
    /// `true` when `ARTAGON_VSCODE_PINS_FILE` overrode `vsp.lock.json` for this resolution.
    pub pins_file_override: bool,
    /// Sorted unique Tier 2 extension IDs that lacked a verified version pin (`pending_refresh`
    /// or `version == None`) and therefore resolved unpinned.
    pub tier2_unpinned: Vec<String>,
}

/// Planned pin-repair operation for `--repair-pins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRepairItem {
    /// Lowercase extension identifier.
    pub id: String,
    /// Currently installed version string.
    pub installed_version: String,
    /// Pinned target version string.
    pub pinned_version: String,
    /// `true` when `pinned_version < installed_version` by semver comparison.
    pub is_downgrade: bool,
    /// Lock entry backing the repair.
    pub pin: LockPinEntry,
}

/// Single version record returned by a Marketplace `extensionquery` response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketplaceVersionInfo {
    /// Extension version string.
    pub version: String,
    /// Target platform (`None` or `Some("universal")` for universal VSIXs, or e.g. `darwin-arm64`).
    pub target_platform: Option<String>,
    /// Marketplace `VSIXPackage` download URL, if present.
    pub vsix_url: Option<String>,
    /// `true` when `Microsoft.VisualStudio.Code.PreRelease == "true"`.
    pub is_prerelease: bool,
}

/// Resolves the XDG VSIX cache directory (`VSCODE_VSIX_CACHE_DIR` or `${XDG_CACHE_HOME:-~/.cache}/vsp/vsix`).
#[must_use]
pub fn resolve_vsix_cache_dir(override_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir.to_path_buf();
    }
    if let Ok(env_dir) = env::var("VSCODE_VSIX_CACHE_DIR") {
        let trimmed = env_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    resolve_xdg_cache_home().join("vsp/vsix")
}

/// Returns `true` when `ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1` enables local test URLs.
#[must_use]
pub fn allow_test_pin_urls() -> bool {
    env::var("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL")
        .ok()
        .as_deref()
        == Some("1")
}

/// Returns `true` when explicit unlocked bypass is active via CLI flag or env hook.
#[must_use]
pub fn unlocked_bypass_active(cli_allow_unlocked: bool) -> bool {
    cli_allow_unlocked
        || env::var("ARTAGON_VSCODE_ALLOW_UNLOCKED").ok().as_deref() == Some("1")
        || env::var("ARTAGON_VSCODE_BYPASS_PINS").ok().as_deref() == Some("1")
}

/// Validates a VSIX download URL against the strict Marketplace HTTPS allowlist:
/// - `https://marketplace.visualstudio.com/...`
/// - `https://*.gallerycdn.vsassets.io/...`
/// - `https://*.gallery.vsassets.io/...`
///
/// This is the same [`check_vsix_url`] validator that catalog validation uses. It rejects
/// HTTP downgrades, userinfo (`@`), and off-host URLs with [`VspError::SafetyRefusal`]
/// (exit `5`). When `allow_test_urls` is `true`, it also permits `http://` with a host that
/// is exactly `localhost`, `127.0.0.1`, or `[::1]`, and `file://`, for offline fixtures.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if the URL fails scheme or host validation.
pub fn validate_vsix_url(url: &str, allow_test_urls: bool) -> VspResult<()> {
    check_vsix_url(url, allow_test_urls)
        .map_err(|reason| VspError::SafetyRefusal(format!("vsix: {reason}")))
}

/// Unified fail-closed loader for the 4-tier `vsp.lock.json` lockfile shared by `vsix.rs`,
/// `doctor.rs`, and `dev_ops.rs`.
///
/// - For embedded catalogs, returns the already-parsed [`WorkspaceCatalog::lockfile`] directly
///   without re-parsing JSON.
/// - For external catalog directories, reads and strictly parses `<root>/vsp.lock.json` when
///   present on disk (failing closed with [`VspError::SafetyRefusal`] on unreadable or malformed
///   JSON, never silently returning empty), falling back to `catalog.lockfile` if no file exists
///   at `<root>/vsp.lock.json`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if `vsp.lock.json` exists but is unreadable
/// or malformed.
pub fn load_lockfile_strict(catalog: &WorkspaceCatalog) -> VspResult<Option<LockfileManifest>> {
    if catalog_is_embedded(catalog) {
        if let Some(lockfile) = &catalog.lockfile {
            return Ok(Some(lockfile.clone()));
        }
        if let Some((_, bytes)) = embedded_snapshot_files()
            .iter()
            .find(|(p, _)| *p == "vsp.lock.json")
        {
            let doc = parse_lockfile_bytes(bytes, "<embedded>/vsp.lock.json")?;
            return Ok(Some(doc));
        }
        return Ok(None);
    }

    let lock_path = catalog.root.join("vsp.lock.json");
    if lock_path.is_file() {
        let bytes = fs::read(&lock_path).map_err(|err| {
            VspError::SafetyRefusal(format!(
                "vsix: unreadable vsp.lock.json {}: {err}",
                lock_path.display()
            ))
        })?;
        let doc = parse_lockfile_bytes(&bytes, &lock_path.display().to_string())?;
        return Ok(Some(doc));
    }

    Ok(catalog.lockfile.clone())
}

fn parse_lockfile_bytes(raw_bytes: &[u8], label: &str) -> VspResult<LockfileManifest> {
    serde_json::from_slice::<LockfileManifest>(raw_bytes).map_err(|err| {
        VspError::SafetyRefusal(format!("vsix: malformed vsp.lock.json {label}: {err}"))
    })
}

/// Resolves a VSIX lock index combining `vsp.lock.json` and `catalog/extension-pins.json`
/// (or an explicit `ARTAGON_VSCODE_PINS_FILE` override).
///
/// Fails closed with [`VspError::SafetyRefusal`] (exit `5`) when the pins manifest or lockfile
/// is missing or invalid and `allow_unlocked` is `false`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) when lock metadata is missing/malformed and
/// unlocked bypass is not enabled.
pub fn load_combined_pin_index(
    catalog: &WorkspaceCatalog,
    allow_unlocked: bool,
) -> VspResult<(BTreeMap<String, LockPinEntry>, bool, bool)> {
    load_combined_pin_index_internal(catalog, allow_unlocked, true)
}

fn load_combined_pin_index_internal(
    catalog: &WorkspaceCatalog,
    allow_unlocked: bool,
    emit_override_warning: bool,
) -> VspResult<(BTreeMap<String, LockPinEntry>, bool, bool)> {
    let bypass = unlocked_bypass_active(allow_unlocked);
    let mut entries: BTreeMap<String, LockPinEntry> = BTreeMap::new();
    let mut pin_required = false;
    let override_path = pins_file_override_path();

    // 1. Authoritative 4-tier `vsp.lock.json` entries (unless explicitly overridden by ARTAGON_VSCODE_PINS_FILE).
    if let Some(ref path) = override_path {
        if emit_override_warning {
            eprintln!(
                "vsix: WARNING: ARTAGON_VSCODE_PINS_FILE={} overrides catalog vsp.lock.json; supply-chain lock verification is overridden",
                path.display()
            );
        }
    } else if let Some(lock_doc) = load_lockfile_strict(catalog)? {
        if lock_doc.pin_required {
            pin_required = true;
        }
        let host_platform = current_lock_platform();
        // Tier 1 and Tier 2 records always stay in the index, also when they are
        // `pending_refresh` or have no digest for this platform. Such a record has an empty
        // digest, and `resolve_and_verify_batch` fails closed on it for Tier 1 (or warns and
        // records provenance for Tier 2).
        for (k, rec) in &lock_doc.extensions {
            let tier = Some(rec.tier);
            let pending = rec.status == Some(LockEntryStatus::PendingRefresh);
            let native_only = rec.universal == Some(false);
            let (sha, url) = if pending {
                ("", "")
            } else if native_only {
                // Platform-specific record (`universal: false`): require a matching
                // host-platform target; never fall back to `"universal"` or flat
                // top-level `sha256`/`vsixUrl`.
                let target = (host_platform != "universal")
                    .then(|| rec.targets.get(host_platform))
                    .flatten();
                (
                    target.map_or("", |t| t.sha256.as_str()),
                    target.map_or("", |t| t.vsix_url.as_str()),
                )
            } else {
                let platform_target = rec
                    .targets
                    .get(host_platform)
                    .or_else(|| rec.targets.get("universal"));
                (
                    platform_target
                        .map(|t| t.sha256.as_str())
                        .or(rec.sha256.as_deref())
                        .unwrap_or(""),
                    platform_target
                        .map(|t| t.vsix_url.as_str())
                        .or(rec.vsix_url.as_deref())
                        .unwrap_or(""),
                )
            };
            let keep = matches!(rec.tier, 1 | 2);
            if !keep {
                continue;
            }
            let lc = k.to_ascii_lowercase();
            entries.insert(
                lc.clone(),
                LockPinEntry {
                    id: lc,
                    version: rec.version.clone().unwrap_or_default(),
                    sha256: sha.to_ascii_lowercase(),
                    vsix_url: url.to_string(),
                    required: rec.tier == 1 || pin_required,
                    tier,
                    status_pending: pending,
                },
            );
        }
    }

    // 2. Mapped pins (`catalog/extension-pins.json` / `ARTAGON_VSCODE_PINS_FILE`).
    match &catalog.pins {
        PinsLoadState::Loaded(manifest) => {
            if manifest.pin_required {
                pin_required = true;
            }
            for (k, pin) in &manifest.pins {
                let lc = k.to_ascii_lowercase();
                entries.entry(lc.clone()).or_insert_with(|| LockPinEntry {
                    id: lc,
                    version: pin.version.clone(),
                    sha256: pin.sha256.to_ascii_lowercase(),
                    vsix_url: pin.vsix_url.clone(),
                    required: manifest.pin_required,
                    tier: None,
                    status_pending: false,
                });
            }
            Ok((entries, pin_required, false))
        }
        PinsLoadState::Missing(path) => {
            if !entries.is_empty() {
                return Ok((entries, pin_required, false));
            }
            if bypass {
                eprintln!(
                    "vsix: WARNING: pins manifest missing ({}); proceeding unlocked (--allow-unlocked)",
                    path.display()
                );
                Ok((entries, false, true))
            } else {
                Err(VspError::SafetyRefusal(format!(
                    "vsix: pins manifest missing or unreadable: {} (pass --allow-unlocked to bypass)",
                    path.display()
                )))
            }
        }
        PinsLoadState::Invalid { path, error } => {
            if bypass {
                eprintln!(
                    "vsix: WARNING: pins manifest invalid ({}: {error}); proceeding unlocked (--allow-unlocked)",
                    path.display()
                );
                Ok((entries, false, true))
            } else {
                Err(VspError::SafetyRefusal(format!(
                    "vsix: invalid pins manifest {}: {error}",
                    path.display()
                )))
            }
        }
    }
}

/// Formats a grouped, multi-line, audience-aware VSIX refusal diagnostic.
///
/// Groups failures by cause (one extension ID per line) and appends a single actionable
/// remedy tailored to whether the user is running the embedded release catalog or an
/// external `--catalog <DIR>`.
#[must_use]
pub fn format_vsix_refusal_summary(
    catalog: &WorkspaceCatalog,
    target_label: &str,
    failures: &[VsixFailure],
) -> String {
    let host_platform = current_lock_platform();
    let mut pending_t1: Vec<&str> = Vec::new();
    let mut missing_target_t1: Vec<&str> = Vec::new();
    let mut missing_required: Vec<&str> = Vec::new();
    let mut other_safety: Vec<(&str, &str)> = Vec::new();
    let mut operational: Vec<(&str, &str)> = Vec::new();

    for f in failures {
        match f.kind {
            VsixFailureKind::Tier1PendingRefresh => pending_t1.push(f.id.as_str()),
            VsixFailureKind::Tier1MissingHostTarget => missing_target_t1.push(f.id.as_str()),
            VsixFailureKind::PinRequiredMissing => missing_required.push(f.id.as_str()),
            VsixFailureKind::InvalidVersion | VsixFailureKind::VerificationRefusal => {
                other_safety.push((f.id.as_str(), f.message.as_str()));
            }
            VsixFailureKind::Operational => {
                operational.push((f.id.as_str(), f.message.as_str()));
            }
        }
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "profile '{target_label}': VSIX verification refused {} extension(s) before any write:",
        failures.len()
    );

    if !pending_t1.is_empty() {
        let _ = writeln!(
            out,
            "  Tier 1 hash lock pending_refresh ({}):",
            pending_t1.len()
        );
        for id in &pending_t1 {
            let _ = writeln!(out, "    - {id}");
        }
    }
    if !missing_target_t1.is_empty() {
        let _ = writeln!(
            out,
            "  Tier 1 hash lock missing target for '{host_platform}' ({}):",
            missing_target_t1.len()
        );
        for id in &missing_target_t1 {
            let _ = writeln!(out, "    - {id}");
        }
    }
    if !missing_required.is_empty() {
        let _ = writeln!(
            out,
            "  pinRequired=true missing lock entry ({}):",
            missing_required.len()
        );
        for id in &missing_required {
            let _ = writeln!(out, "    - {id}");
        }
    }
    if !other_safety.is_empty() {
        let _ = writeln!(
            out,
            "  VSIX integrity / policy refusal ({}):",
            other_safety.len()
        );
        for (id, msg) in &other_safety {
            let _ = writeln!(out, "    - {id}: {msg}");
        }
    }
    if !operational.is_empty() {
        let _ = writeln!(out, "  VSIX download error ({}):", operational.len());
        for (id, msg) in &operational {
            let _ = writeln!(out, "    - {id}: {msg}");
        }
    }

    let has_pending_or_missing =
        !pending_t1.is_empty() || !missing_target_t1.is_empty() || !missing_required.is_empty();
    let remedy = if has_pending_or_missing {
        if catalog_is_embedded(catalog) {
            "This release ships unverified Tier 1 lock entries; pass `--allow-unlocked` to allow pending entries while still verifying pinned SHA-256 digests, or upgrade `vsp`.".to_string()
        } else {
            let dir = catalog.root.display();
            format!(
                "Run `vsp --catalog {dir} dev pins --refresh` and retry with `--catalog {dir}`."
            )
        }
    } else if !other_safety.is_empty() {
        "Verify catalog lockfile digests and Marketplace URLs before retrying; `--allow-unlocked` does not bypass verified SHA-256 or URL checks.".to_string()
    } else {
        "Check network connectivity and retry `vsp sync`.".to_string()
    };

    let _ = write!(out, "  Remedy: {remedy}");
    out
}

/// Downloads (or reuses from XDG cache) and SHA-256 verifies a single VSIX package,
/// returning the verified path inside `cache_dir`.
///
/// Performs:
/// 1. Strict extension ID & SHA-256 format validation.
/// 2. Direct streaming cache-hit verification: verifies SHA-256 directly from the open
///    read-only `File` handle inside the private `0700` XDG cache directory (avoiding
///    256 MiB staging copies on cache hits) and automatically evicts corrupt entries.
///    (Note: cache integrity assumes the same-UID XDG cache directory is trusted; a process
///    running as the same user already controls `$XDG_CACHE_HOME`.)
/// 3. Per-hop HTTPS + Marketplace host allowlist validation across every 3xx redirect
///    (manual redirect loop with `ureq` `max_redirects(0)`).
/// 4. Streaming download to a private `0600` staging file while hashing with `Sha256`.
/// 5. Atomic rename into `cache_dir/<id_lc>-<sha256>.vsix` after SHA-256 matches, with
///    unconditional staging file cleanup on any failure.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) on invalid URL, off-host redirect, or
/// SHA-256 digest mismatch, or [`VspError::Operational`] on network/IO failure.
pub fn fetch_and_verify_vsix(
    pin: &LockPinEntry,
    cache_dir: &Path,
    allow_test_urls: bool,
) -> VspResult<PathBuf> {
    let agent = build_vsix_http_agent(allow_test_urls);
    fetch_and_verify_vsix_with_agent(pin, cache_dir, allow_test_urls, &agent)
}

/// Same as [`fetch_and_verify_vsix`], reusing a caller-supplied [`ureq::Agent`] to share
/// TCP/TLS connections across a batch of downloads.
///
/// # Errors
///
/// Same as [`fetch_and_verify_vsix`].
pub fn fetch_and_verify_vsix_with_agent(
    pin: &LockPinEntry,
    cache_dir: &Path,
    allow_test_urls: bool,
    agent: &ureq::Agent,
) -> VspResult<PathBuf> {
    let id_lc = pin.id.to_ascii_lowercase();
    if !is_valid_extension_id_syntax(&id_lc) {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: invalid extension id '{}'",
            pin.id
        )));
    }
    let expected = pin.sha256.to_ascii_lowercase();
    if !is_valid_sha256_hex(&expected) {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: invalid expected sha256 for '{}': '{}'",
            pin.id, pin.sha256
        )));
    }
    validate_vsix_url(&pin.vsix_url, allow_test_urls)?;

    ensure_private_cache_dir(cache_dir)?;

    let final_cache_path = cache_dir.join(format!("{id_lc}-{expected}.vsix"));

    // Direct streaming cache-hit SHA-256 verification on the open read-only file handle
    // (avoids staging copy + fsync + rename back over itself on cache hits).
    if let Ok(sym_meta) = fs::symlink_metadata(&final_cache_path) {
        if sym_meta.file_type().is_file() {
            let verify_res = File::open(&final_cache_path).and_then(|mut src| {
                if !src.metadata()?.is_file() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "cache entry is not a regular file",
                    ));
                }
                hash_reader_sha256(&mut src, MAX_VSIX_BYTES)
            });
            match verify_res {
                Ok(actual) if actual == expected => {
                    return Ok(final_cache_path);
                }
                _ => {
                    let _ = fs::remove_file(&final_cache_path);
                }
            }
        }
    }

    // Stream download into a private 0600 staging file while validating every redirect hop.
    let stage = make_staging_path(cache_dir, "dl");
    let download_result = stream_verified_download(
        agent,
        &pin.vsix_url,
        &expected,
        &stage,
        allow_test_urls,
        &pin.id,
    );
    if let Err(err) = download_result {
        let _ = fs::remove_file(&stage);
        return Err(err);
    }

    if final_cache_path.is_symlink() || final_cache_path.is_dir() {
        let _ = fs::remove_file(&stage);
        return Err(VspError::SafetyRefusal(format!(
            "vsix: refusing to overwrite non-regular cache target: {}",
            final_cache_path.display()
        )));
    }

    if let Err(err) = fs::rename(&stage, &final_cache_path) {
        let _ = fs::remove_file(&stage);
        return Err(VspError::Operational(format!(
            "vsix: failed to publish verified VSIX for '{}' into {}: {err}",
            pin.id,
            final_cache_path.display()
        )));
    }

    Ok(final_cache_path)
}

fn ensure_private_cache_dir(cache_dir: &Path) -> VspResult<()> {
    if cache_dir.is_symlink() {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: refusing to use symlink cache directory: {}",
            cache_dir.display()
        )));
    }
    fs::create_dir_all(cache_dir).map_err(|err| {
        VspError::Operational(format!(
            "vsix: failed to create cache dir {}: {err}",
            cache_dir.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(cache_dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn stream_verified_download(
    agent: &ureq::Agent,
    initial_url: &str,
    expected_sha256: &str,
    stage_path: &Path,
    allow_test_urls: bool,
    ext_id: &str,
) -> VspResult<()> {
    let (actual, final_url) =
        download_to_stage_file(agent, initial_url, stage_path, allow_test_urls, ext_id)?;
    if actual != expected_sha256 {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: sha256 mismatch for '{ext_id}' (expected {expected_sha256}, got {actual}, url {final_url})"
        )));
    }
    Ok(())
}

/// Downloads `initial_url` (validating every redirect hop) into `stage_path` (mode `0600`)
/// and returns `(sha256_hex, final_url)`.
fn download_to_stage_file(
    agent: &ureq::Agent,
    initial_url: &str,
    stage_path: &Path,
    allow_test_urls: bool,
    ext_id: &str,
) -> VspResult<(String, String)> {
    if allow_test_urls {
        if let Some(file_path) = initial_url.strip_prefix("file://") {
            let mut src = File::open(file_path).map_err(|err| {
                VspError::Operational(format!(
                    "vsix: failed to open file:// fixture '{file_path}' for '{ext_id}': {err}"
                ))
            })?;
            let mut dst = create_private_file(stage_path).map_err(|err| {
                VspError::Operational(format!(
                    "vsix: failed to create staging file {}: {err}",
                    stage_path.display()
                ))
            })?;
            let actual = copy_and_sha256(&mut src, &mut dst, MAX_VSIX_BYTES).map_err(|err| {
                VspError::Operational(format!("vsix: stream error for '{ext_id}': {err}"))
            })?;
            dst.sync_all().map_err(|err| {
                VspError::Operational(format!("vsix: sync error for '{ext_id}': {err}"))
            })?;
            return Ok((actual, initial_url.to_string()));
        }
    }

    let mut current_url = initial_url.to_string();
    for hop in 0..=MAX_REDIRECT_HOPS {
        validate_vsix_url(&current_url, allow_test_urls)?;
        let resp = agent.get(&current_url).call().map_err(|err| {
            VspError::Operational(format!(
                "vsix: HTTP fetch failed for '{ext_id}' ({current_url}): {err}"
            ))
        })?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hop == MAX_REDIRECT_HOPS {
                return Err(VspError::SafetyRefusal(format!(
                    "vsix: exceeded {MAX_REDIRECT_HOPS} redirect hops for '{ext_id}'"
                )));
            }
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    VspError::SafetyRefusal(format!(
                        "vsix: redirect {status} missing Location header for '{ext_id}'"
                    ))
                })?;
            let next_url = resolve_redirect_url(&current_url, loc)?;
            validate_vsix_url(&next_url, allow_test_urls).map_err(|err| {
                VspError::SafetyRefusal(format!(
                    "vsix: rejected redirect hop for '{ext_id}' -> {next_url}: {err}"
                ))
            })?;
            current_url = next_url;
            continue;
        }
        if status != 200 {
            return Err(VspError::Operational(format!(
                "vsix: HTTP {status} downloading '{ext_id}' from {current_url}"
            )));
        }

        let mut reader = resp.into_body().into_reader();
        let mut dst = create_private_file(stage_path).map_err(|err| {
            VspError::Operational(format!(
                "vsix: failed to create staging file {}: {err}",
                stage_path.display()
            ))
        })?;
        let actual = copy_and_sha256(&mut reader, &mut dst, MAX_VSIX_BYTES).map_err(|err| {
            VspError::Operational(format!("vsix: stream read failed for '{ext_id}': {err}"))
        })?;
        dst.sync_all().map_err(|err| {
            VspError::Operational(format!("vsix: failed syncing staging file: {err}"))
        })?;
        return Ok((actual, current_url));
    }

    Err(VspError::SafetyRefusal(format!(
        "vsix: redirect loop while downloading '{ext_id}'"
    )))
}

/// Resolves an HTTP `Location` header against the current URL with [`url::Url::join`]
/// (RFC 3986 reference resolution). The caller validates the result with
/// [`validate_vsix_url`] before it follows the redirect.
fn resolve_redirect_url(base: &str, location: &str) -> VspResult<String> {
    let reject = |detail: String| {
        VspError::SafetyRefusal(format!(
            "vsix: cannot resolve redirect '{location}' against '{base}': {detail}"
        ))
    };
    let base_url = url::Url::parse(base).map_err(|err| reject(err.to_string()))?;
    let next = base_url
        .join(location)
        .map_err(|err| reject(err.to_string()))?;
    Ok(next.into())
}

/// Resolves and verifies a batch of extension IDs concurrently across at most
/// [`MAX_VSIX_WORKERS`] (`4`) bounded worker threads using a single shared [`ureq::Agent`].
///
/// - **Selective `--allow-unlocked`:** When `opts.allow_unlocked` (or env bypass) is `true`,
///   every Tier 1 pin that HAS a verified digest/URL for the host platform is STILL downloaded
///   and SHA-256 verified. Only `pending_refresh` / missing host-target / missing `pinRequired`
///   entries are relaxed (with a stderr warning and `unlocked_bypass = true`).
/// - **Tier 2 auditability:** Tier 2 entries without a verified version (`pending_refresh` or
///   `version == None`) emit a stderr `WARN` and are recorded in
///   [`ResolvedInstallTargets::tier2_unpinned`].
///
/// Preserves the exact input ordering of `ext_ids` in the returned [`ResolvedInstallTargets`].
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if the lockfile or pins manifest itself is
/// missing or invalid and `--allow-unlocked` is not enabled.
pub fn resolve_and_verify_batch(
    catalog: &WorkspaceCatalog,
    ext_ids: &[String],
    opts: &VsixVerifyOptions,
) -> VspResult<ResolvedInstallTargets> {
    let bypass = unlocked_bypass_active(opts.allow_unlocked);
    let pins_file_override = pins_file_override_path().is_some();
    let (pin_index, pin_required, manifest_bypassed) =
        load_combined_pin_index_internal(catalog, opts.allow_unlocked, true)?;
    let mut unlocked_bypass = manifest_bypassed;

    let cache_dir = resolve_vsix_cache_dir(opts.cache_dir.as_deref());
    let allow_test = allow_test_pin_urls();

    // Partition into jobs (verified hash-locked downloads), Tier 2 version pins, relaxed entries,
    // or fail-closed refusal slots.
    let mut slots: Vec<Option<WorkerOutcome>> = vec![None; ext_ids.len()];
    let mut jobs: VecDeque<(usize, LockPinEntry)> = VecDeque::new();
    let mut relaxed_ids: Vec<String> = Vec::new();
    let mut tier2_unpinned: Vec<String> = Vec::new();

    let host_platform = current_lock_platform();
    for (idx, id) in ext_ids.iter().enumerate() {
        let lc = id.to_ascii_lowercase();
        match pin_index.get(&lc) {
            // Hash-locked with a verified artifact: ALWAYS download & verify SHA-256,
            // even when `--allow-unlocked` is active!
            Some(pin) if pin.has_verified_artifact() => jobs.push_back((idx, pin.clone())),
            // Tier 1 without a verified digest (`pending_refresh`, or no target for this
            // platform and no universal target): fail closed unless `--allow-unlocked`
            // selectively relaxes this pending/missing-target entry.
            Some(pin) if pin.tier == Some(1) => {
                if bypass {
                    unlocked_bypass = true;
                    relaxed_ids.push(id.clone());
                    slots[idx] = Some(Ok(id.clone()));
                } else {
                    let kind = if pin.status_pending {
                        VsixFailureKind::Tier1PendingRefresh
                    } else {
                        VsixFailureKind::Tier1MissingHostTarget
                    };
                    let detail = if pin.status_pending {
                        format!(
                            "Tier 1 hash lock for '{id}' is pending_refresh (no verified VSIX digest yet)"
                        )
                    } else {
                        format!(
                            "Tier 1 hash lock for '{id}' has no verified VSIX digest for platform '{host_platform}'"
                        )
                    };
                    slots[idx] = Some(Err((detail, true, kind)));
                }
            }
            // Tier 2 with a recorded version: install exactly that version.
            Some(pin) if !pin.version.is_empty() => {
                slots[idx] = Some(if semver::Version::parse(&pin.version).is_ok() {
                    Ok(format!("{id}@{}", pin.version))
                } else {
                    Err((
                        format!(
                            "version lock for '{id}' is not a valid semver version: '{}'",
                            pin.version
                        ),
                        true,
                        VsixFailureKind::InvalidVersion,
                    ))
                });
            }
            // Tier 2 without a verified version (`pending_refresh` / `version == None`):
            // warn loudly and record in `tier2_unpinned` for provenance & `vsp doctor`.
            Some(pin) if pin.tier == Some(2) => {
                tier2_unpinned.push(lc);
                slots[idx] = Some(Ok(id.clone()));
            }
            _ if pin_required => {
                if bypass {
                    unlocked_bypass = true;
                    relaxed_ids.push(id.clone());
                    slots[idx] = Some(Ok(id.clone()));
                } else {
                    slots[idx] = Some(Err((
                        format!("pinRequired=true but no VSIX pin exists for '{id}'"),
                        true,
                        VsixFailureKind::PinRequiredMissing,
                    )));
                }
            }
            _ => slots[idx] = Some(Ok(id.clone())),
        }
    }

    if !relaxed_ids.is_empty() {
        eprintln!(
            "vsix: WARNING: --allow-unlocked relaxed {} unverified/missing lock entry(ies) (pinned SHA-256 digests are still enforced): {}",
            relaxed_ids.len(),
            relaxed_ids.join(", ")
        );
    }

    tier2_unpinned.sort();
    tier2_unpinned.dedup();
    if !tier2_unpinned.is_empty() {
        eprintln!(
            "vsix: WARN: {} Tier 2 extension(s) have no verified version pin (pending_refresh) and will install unpinned: {}",
            tier2_unpinned.len(),
            tier2_unpinned.join(", ")
        );
    }

    // Fail-fast before spawning network downloads: if any pre-download lock check already
    // failed closed with a safety refusal (e.g., pending_refresh Tier 1 pin, missing host
    // platform target, or invalid semver), do not download other VSIXs over the network only
    // to discard them on exit 5.
    let has_pre_download_safety_refusal = slots
        .iter()
        .any(|slot| matches!(slot, Some(Err((_, true, _)))));

    if !jobs.is_empty() && !has_pre_download_safety_refusal {
        let worker_count = jobs.len().min(MAX_VSIX_WORKERS);
        let queue = Mutex::new(jobs);
        let results: Mutex<Vec<(usize, WorkerOutcome)>> = Mutex::new(Vec::new());
        // Build ONE shared HTTP agent so all worker threads reuse TCP/TLS sessions.
        let agent = build_vsix_http_agent(allow_test);

        // A poisoned lock means that another worker panicked. The queue and the result list
        // are still valid, so the other workers continue. A job without a result is
        // reported as a failure below: a worker panic never installs an extension.
        thread::scope(|scope| {
            let workers: Vec<_> = (0..worker_count)
                .map(|_| {
                    let agent_ref = &agent;
                    let cache_ref = &cache_dir;
                    let queue_ref = &queue;
                    let results_ref = &results;
                    scope.spawn(move || loop {
                        let next = queue_ref
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .pop_front();
                        let Some((idx, pin)) = next else {
                            break;
                        };
                        let outcome = fetch_and_verify_vsix_with_agent(
                            &pin, cache_ref, allow_test, agent_ref,
                        )
                        .map(|path| path.display().to_string())
                        .map_err(|err| {
                            let is_safety = matches!(err, VspError::SafetyRefusal(_));
                            let kind = if is_safety {
                                VsixFailureKind::VerificationRefusal
                            } else {
                                VsixFailureKind::Operational
                            };
                            (err.to_string(), is_safety, kind)
                        });
                        results_ref
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push((idx, outcome));
                    })
                })
                .collect();
            // An explicit join keeps a worker panic out of this thread.
            for worker in workers {
                let _ = worker.join();
            }
        });

        for (idx, outcome) in results.into_inner().unwrap_or_else(PoisonError::into_inner) {
            if let Some(slot) = slots.get_mut(idx) {
                *slot = Some(outcome);
            }
        }
    }

    let mut targets = Vec::new();
    let mut failures = Vec::new();
    for (idx, id) in ext_ids.iter().enumerate() {
        match slots[idx].take() {
            Some(Ok(arg)) => targets.push((id.clone(), arg)),
            Some(Err((msg, is_safety, kind))) => failures.push(VsixFailure {
                id: id.clone(),
                message: msg,
                is_safety_refusal: is_safety,
                kind,
            }),
            None if has_pre_download_safety_refusal => {
                // Intentionally skipped downloading because batch already failed closed.
            }
            None => failures.push(VsixFailure {
                id: id.clone(),
                message: "internal VSIX worker missing result".to_string(),
                is_safety_refusal: false,
                kind: VsixFailureKind::Operational,
            }),
        }
    }

    Ok(ResolvedInstallTargets {
        targets,
        failures,
        unlocked_bypass,
        pins_file_override,
        tier2_unpinned,
    })
}

/// Computes planned `--repair-pins` items by comparing currently installed extension versions
/// against pinned versions in the lock index.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] if pins metadata is missing/invalid and unlocked bypass
/// is not enabled.
pub fn compute_pin_repairs(
    catalog: &WorkspaceCatalog,
    installed: &[InstalledExtension],
    allow_unlocked: bool,
) -> VspResult<Vec<PinRepairItem>> {
    let (pin_index, _, _) = load_combined_pin_index_internal(catalog, allow_unlocked, false)?;
    let mut repairs = Vec::new();

    for ext in installed {
        let Some(inst_ver) = &ext.version else {
            continue;
        };
        let lc = ext.id.to_ascii_lowercase();
        let Some(pin) = pin_index.get(&lc) else {
            continue;
        };
        // A repair needs a verified artifact. Pending and version-only entries have none.
        if !pin.has_verified_artifact() || pin.version.is_empty() || pin.version == *inst_ver {
            continue;
        }
        let is_downgrade = is_semver_downgrade(inst_ver, &pin.version);
        repairs.push(PinRepairItem {
            id: lc,
            installed_version: inst_ver.clone(),
            pinned_version: pin.version.clone(),
            is_downgrade,
            pin: pin.clone(),
        });
    }
    repairs.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(repairs)
}

/// Enforces confirmation before executing `--repair-pins` downgrades.
///
/// Previews every downgrade on stderr. When `assume_yes` is `false`:
/// - In a non-TTY stdin, fails closed with [`VspError::SafetyRefusal`] (exit `5`).
/// - In an interactive TTY, prompts `[y/N]` and refuses with exit `5` unless answered `y`/`yes`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) when downgrades are not confirmed.
pub fn confirm_pin_downgrades(repairs: &[PinRepairItem], assume_yes: bool) -> VspResult<()> {
    let stdin = io::stdin();
    let stdin_is_tty = stdin.is_terminal();
    confirm_pin_downgrades_with_io(
        repairs,
        assume_yes,
        stdin_is_tty,
        &mut stdin.lock(),
        &mut io::stderr(),
    )
}

/// [`confirm_pin_downgrades`] with explicit input and output streams.
///
/// The caller states whether standard input is a terminal (`stdin_is_tty`) and supplies the
/// answer `reader` and the message writer `err_writer`. The function never touches the
/// process standard streams, so a unit test cannot block on an interactive terminal: with
/// `stdin_is_tty = false` the `reader` is never read.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) when a downgrade is not confirmed: no
/// `assume_yes` and no terminal, an answer other than `y`/`yes`, or an unreadable answer.
pub fn confirm_pin_downgrades_with_io(
    repairs: &[PinRepairItem],
    assume_yes: bool,
    stdin_is_tty: bool,
    reader: &mut dyn io::BufRead,
    err_writer: &mut dyn Write,
) -> VspResult<()> {
    let downgrades: Vec<&PinRepairItem> = repairs.iter().filter(|r| r.is_downgrade).collect();
    if downgrades.is_empty() {
        return Ok(());
    }

    // Messages are best-effort: a closed stderr must not change the decision.
    let _ = writeln!(err_writer, "--repair-pins downgrade preview:");
    for item in &downgrades {
        let _ = writeln!(
            err_writer,
            "  downgrade {}: {} -> {}",
            item.id, item.installed_version, item.pinned_version
        );
    }

    if assume_yes {
        return Ok(());
    }

    if !stdin_is_tty {
        return Err(VspError::SafetyRefusal(
            "--repair-pins would downgrade pinned extension(s) in non-interactive mode; re-run with --yes to confirm".to_string(),
        ));
    }

    let _ = write!(err_writer, "Proceed with pin downgrade(s)? [y/N]: ");
    let _ = err_writer.flush();
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|err| {
        VspError::SafetyRefusal(format!("failed reading downgrade confirmation: {err}"))
    })?;
    let ans = line.trim().to_ascii_lowercase();
    if ans == "y" || ans == "yes" {
        Ok(())
    } else {
        Err(VspError::SafetyRefusal(
            "aborted --repair-pins downgrade by user".to_string(),
        ))
    }
}

/// Stages and SHA-256 verifies all VSIX packages required for `repairs` BEFORE any replacement
/// call to `code`, reusing one shared [`ureq::Agent`].
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] or [`VspError::Operational`] if any staged VSIX fails
/// download or SHA-256 verification.
pub fn stage_pin_repairs(
    repairs: &[PinRepairItem],
    cache_dir: &Path,
) -> VspResult<Vec<(PinRepairItem, PathBuf)>> {
    let allow_test = allow_test_pin_urls();
    let agent = build_vsix_http_agent(allow_test);
    let mut staged = Vec::with_capacity(repairs.len());
    for item in repairs {
        let path = fetch_and_verify_vsix_with_agent(&item.pin, cache_dir, allow_test, &agent)?;
        staged.push((item.clone(), path));
    }
    Ok(staged)
}

/// Returns `true` if `target_ver` is strictly older than `installed_ver` by semver comparison.
///
/// When either version is not valid semver, the order is unknown. The function then returns
/// `true`, so [`confirm_pin_downgrades`] asks for confirmation instead of skipping it.
#[must_use]
pub fn is_semver_downgrade(installed_ver: &str, target_ver: &str) -> bool {
    match (
        semver::Version::parse(installed_ver.trim_start_matches('v')),
        semver::Version::parse(target_ver.trim_start_matches('v')),
    ) {
        (Ok(inst), Ok(target)) => target < inst,
        _ => true,
    }
}

/// Returns `true` when each ID in `managed_ids` has a hash-locked entry with a verified
/// artifact and its installed version equals the locked version.
///
/// `vsp sync --repair-pins` uses this to decide whether a recorded unlocked bypass (or
/// `ARTAGON_VSCODE_PINS_FILE` override) is cleared: only a full re-verification of the managed
/// set clears it.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] if the lock metadata is missing or invalid.
pub fn managed_extensions_fully_pinned(
    catalog: &WorkspaceCatalog,
    installed: &[InstalledExtension],
    managed_ids: &[String],
) -> VspResult<bool> {
    let (pin_index, _, bypassed) = load_combined_pin_index_internal(catalog, false, false)?;
    if bypassed {
        return Ok(false);
    }
    for id in managed_ids {
        let lc = id.to_ascii_lowercase();
        let Some(pin) = pin_index.get(&lc) else {
            return Ok(false);
        };
        if !pin.has_verified_artifact() || pin.version.is_empty() {
            return Ok(false);
        }
        let matches_lock = installed.iter().any(|ext| {
            ext.id.eq_ignore_ascii_case(&lc) && ext.version.as_deref() == Some(&pin.version)
        });
        if !matches_lock {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Validates basic Marketplace extension ID syntax (`^[a-z0-9][a-z0-9._-]*\.[a-z0-9][a-z0-9._-]*$`
/// with no `..`).
#[must_use]
pub fn is_valid_extension_id_syntax(id: &str) -> bool {
    let lc = id.to_ascii_lowercase();
    if lc.contains("..") {
        return false;
    }
    let Some((pub_part, name_part)) = lc.split_once('.') else {
        return false;
    };
    let valid_part = |s: &str| {
        let mut bytes = s.bytes();
        let Some(first) = bytes.next() else {
            return false;
        };
        if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
            return false;
        }
        bytes.all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_' || b == b'-'
        })
    };
    valid_part(pub_part) && valid_part(name_part)
}

/// Queries the Marketplace API for all returned version entries (universal and per-platform)
/// of `ext_id`, reusing a caller-supplied [`ureq::Agent`].
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if the Marketplace API cannot be
/// reached or returns a malformed response.
pub fn query_marketplace_extension_versions(
    agent: &ureq::Agent,
    ext_id: &str,
    api_url: &str,
) -> VspResult<Vec<MarketplaceVersionInfo>> {
    let payload = serde_json::json!({
        "filters": [{
            "criteria": [{ "filterType": 7, "value": ext_id }],
            "pageNumber": 1,
            "pageSize": 1
        }],
        "assetTypes": ["Microsoft.VisualStudio.Services.VSIXPackage"],
        "flags": 103
    });

    let mut resp = agent
        .post(api_url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json;api-version=3.0-preview.1")
        .send_json(&payload)
        .map_err(|err| {
            VspError::IncompleteObservation(format!(
                "Marketplace API request failed for '{ext_id}': {err}"
            ))
        })?;

    let body: Value = resp
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_json()
        .map_err(|err| {
            VspError::IncompleteObservation(format!(
                "malformed Marketplace JSON for '{ext_id}': {err}"
            ))
        })?;

    let versions = body
        .pointer("/results/0/extensions/0/versions")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            VspError::IncompleteObservation(format!(
                "Marketplace response missing versions array for '{ext_id}'"
            ))
        })?;

    let mut out = Vec::with_capacity(versions.len());
    for ver_obj in versions {
        let is_prerelease = ver_obj
            .get("properties")
            .and_then(Value::as_array)
            .is_some_and(|props| {
                props.iter().any(|p| {
                    p.get("key").and_then(Value::as_str)
                        == Some("Microsoft.VisualStudio.Code.PreRelease")
                        && p.get("value").and_then(Value::as_str) == Some("true")
                })
            });
        let target_platform = ver_obj
            .get("targetPlatform")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let version = ver_obj
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if version.is_empty() {
            continue;
        }
        let vsix_url = ver_obj
            .get("files")
            .and_then(Value::as_array)
            .and_then(|files| {
                files.iter().find(|f| {
                    f.get("assetType").and_then(Value::as_str)
                        == Some("Microsoft.VisualStudio.Services.VSIXPackage")
                })
            })
            .and_then(|f| f.get("source").and_then(Value::as_str))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        out.push(MarketplaceVersionInfo {
            version,
            target_platform,
            vsix_url,
            is_prerelease,
        });
    }
    Ok(out)
}

/// Fetches a single extension's latest stable universal VSIX metadata from the Marketplace API.
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if the Marketplace API cannot be
/// reached or returns a malformed response.
pub fn query_marketplace_universal_pin(ext_id: &str, api_url: &str) -> VspResult<ExtensionPin> {
    let agent = build_marketplace_http_agent();
    query_marketplace_universal_pin_with_agent(&agent, ext_id, api_url)
}

/// Same as [`query_marketplace_universal_pin`], reusing a shared [`ureq::Agent`].
///
/// # Errors
///
/// Same as [`query_marketplace_universal_pin`].
pub fn query_marketplace_universal_pin_with_agent(
    agent: &ureq::Agent,
    ext_id: &str,
    api_url: &str,
) -> VspResult<ExtensionPin> {
    let versions = query_marketplace_extension_versions(agent, ext_id, api_url)?;
    let allow_test = allow_test_pin_urls();

    for ver in versions {
        let is_universal = ver
            .target_platform
            .as_deref()
            .is_none_or(|tp| tp.eq_ignore_ascii_case("universal"));
        if ver.is_prerelease || !is_universal {
            continue;
        }
        if let Some(vsix_url) = ver.vsix_url {
            validate_vsix_url(&vsix_url, allow_test)?;
            return Ok(ExtensionPin {
                sha256: String::new(),
                version: ver.version,
                vsix_url,
                verified_at: None,
            });
        }
    }

    Err(VspError::IncompleteObservation(format!(
        "no stable universal VSIX version found on Marketplace for '{ext_id}'"
    )))
}

/// Streams a VSIX URL, validates every redirect hop, computes its SHA-256 hex digest, and
/// optionally warms the XDG VSIX cache when `cache_dir` is `Some`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] on URL/redirect policy rejection or [`VspError::Operational`]
/// on download/cache error.
pub fn download_and_cache_vsix_with_agent(
    agent: &ureq::Agent,
    url: &str,
    ext_id: &str,
    cache_dir: Option<&Path>,
    allow_test: bool,
) -> VspResult<String> {
    validate_vsix_url(url, allow_test)?;

    if let Some(dir) = cache_dir {
        let id_lc = ext_id.to_ascii_lowercase();
        if !is_valid_extension_id_syntax(&id_lc) {
            return Err(VspError::SafetyRefusal(format!(
                "vsix: invalid extension id '{ext_id}'"
            )));
        }
        ensure_private_cache_dir(dir)?;
        let stage = make_staging_path(dir, "refresh");
        let res = download_to_stage_file(agent, url, &stage, allow_test, ext_id);
        let (digest, _) = match res {
            Ok(pair) => pair,
            Err(err) => {
                let _ = fs::remove_file(&stage);
                return Err(err);
            }
        };
        let final_cache_path = dir.join(format!("{id_lc}-{digest}.vsix"));
        if final_cache_path.is_symlink() || final_cache_path.is_dir() {
            let _ = fs::remove_file(&stage);
            return Err(VspError::SafetyRefusal(format!(
                "vsix: refusing to overwrite non-regular cache target: {}",
                final_cache_path.display()
            )));
        }
        if let Err(err) = fs::rename(&stage, &final_cache_path) {
            let _ = fs::remove_file(&stage);
            return Err(VspError::Operational(format!(
                "vsix: failed to warm cache for '{ext_id}' at {}: {err}",
                final_cache_path.display()
            )));
        }
        return Ok(digest);
    }

    // Dry-run / hash-only path: zero filesystem writes.
    if allow_test {
        if let Some(file_path) = url.strip_prefix("file://") {
            let mut src = File::open(file_path).map_err(|err| {
                VspError::Operational(format!(
                    "vsix: failed to open file:// fixture '{file_path}' for '{ext_id}': {err}"
                ))
            })?;
            return hash_reader_sha256(&mut src, MAX_VSIX_BYTES).map_err(|err| {
                VspError::Operational(format!("vsix: failed hashing '{ext_id}': {err}"))
            });
        }
    }

    let mut current_url = url.to_string();
    for hop in 0..=MAX_REDIRECT_HOPS {
        validate_vsix_url(&current_url, allow_test)?;
        let resp = agent.get(&current_url).call().map_err(|err| {
            VspError::Operational(format!("vsix download failed for '{ext_id}': {err}"))
        })?;
        let status = resp.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            if hop == MAX_REDIRECT_HOPS {
                return Err(VspError::SafetyRefusal(format!(
                    "vsix: too many redirects for '{ext_id}'"
                )));
            }
            let loc = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    VspError::SafetyRefusal(format!(
                        "vsix: redirect missing Location header for '{ext_id}'"
                    ))
                })?;
            let next_url = resolve_redirect_url(&current_url, loc)?;
            validate_vsix_url(&next_url, allow_test)?;
            current_url = next_url;
            continue;
        }
        if status != 200 {
            return Err(VspError::Operational(format!(
                "vsix: HTTP {status} downloading '{ext_id}'"
            )));
        }
        let mut reader = resp.into_body().into_reader();
        let digest = hash_reader_sha256(&mut reader, MAX_VSIX_BYTES).map_err(|err| {
            VspError::Operational(format!("vsix: failed hashing '{ext_id}': {err}"))
        })?;
        return Ok(digest);
    }

    Err(VspError::SafetyRefusal(format!(
        "vsix: redirect loop for '{ext_id}'"
    )))
}

/// Streams a VSIX URL, validates redirects, and returns its SHA-256 hex digest.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] on URL/redirect policy rejection or [`VspError::Operational`]
/// on download error.
pub fn download_and_hash_vsix_url(url: &str, ext_id: &str) -> VspResult<String> {
    let allow_test = allow_test_pin_urls();
    let agent = build_vsix_http_agent(allow_test);
    download_and_cache_vsix_with_agent(&agent, url, ext_id, None, allow_test)
}

fn make_staging_path(cache_dir: &Path, tag: &str) -> PathBuf {
    let seq = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    cache_dir.join(format!(".vsix-{tag}.{pid}.{seq}.tmp"))
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
}

fn hash_reader_sha256<R: Read>(reader: &mut R, max_bytes: u64) -> io::Result<String> {
    let mut hasher = Sha256::new();
    // Heap buffer: several worker threads run this function concurrently.
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("VSIX exceeded maximum size limit ({max_bytes} bytes)"),
            ));
        }
        hasher.update(&buf[..n]);
    }

    Ok(format_sha256_hex(hasher.finalize()))
}

fn copy_and_sha256<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    max_bytes: u64,
) -> io::Result<String> {
    let mut hasher = Sha256::new();
    // A heap buffer: several worker threads run this function, each on its own stack.
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n as u64);
        if total > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("VSIX exceeded maximum size limit ({max_bytes} bytes)"),
            ));
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n])?;
    }

    Ok(format_sha256_hex(hasher.finalize()))
}

fn format_sha256_hex(digest: impl AsRef<[u8]>) -> String {
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::sha256_hex;
    use tempfile::TempDir;

    #[test]
    fn url_validation_accepts_marketplace_hosts_and_rejects_off_host() {
        assert!(validate_vsix_url(
            "https://marketplace.visualstudio.com/_apis/public/gallery/publishers/a/vsextensions/b/1.0.0/vspackage",
            false
        )
        .is_ok());
        assert!(validate_vsix_url(
            "https://Catppuccin.gallerycdn.vsassets.io/extensions/catppuccin/catppuccin-vsc/3.19.0/1/pkg",
            false
        )
        .is_ok());
        // One shared validator: the runtime accepts the same hosts as catalog validation.
        assert!(validate_vsix_url("https://cdn.vsassets.io/pkg", false).is_err());
        assert!(validate_vsix_url("https://x.gallery.vsassets.io/pkg", false).is_ok());

        assert_eq!(
            validate_vsix_url("http://marketplace.visualstudio.com/pkg", false)
                .unwrap_err()
                .exit_code()
                .as_i32(),
            5
        );
        assert_eq!(
            validate_vsix_url("https://evil.example/pkg", false)
                .unwrap_err()
                .exit_code()
                .as_i32(),
            5
        );
        assert_eq!(
            validate_vsix_url(
                "https://marketplace.visualstudio.com@evil.example/pkg",
                false
            )
            .unwrap_err()
            .exit_code()
            .as_i32(),
            5
        );
    }

    #[test]
    fn file_fixture_verifies_sha256_populates_cache_and_cleans_temp_on_mismatch() {
        let tmp = TempDir::new().expect("tmp");
        let fixture = tmp.path().join("sample.vsix");
        let payload = b"fake-vsix-archive-bytes-v1";
        fs::write(&fixture, payload).expect("write fixture");
        let good_sha = sha256_hex(payload);

        let cache_dir = tmp.path().join("cache");
        let pin = LockPinEntry {
            id: "editorconfig.editorconfig".to_string(),
            version: "0.18.2".to_string(),
            sha256: good_sha.clone(),
            vsix_url: format!("file://{}", fixture.display()),
            required: true,
            tier: None,
            status_pending: false,
        };

        let cached = fetch_and_verify_vsix(&pin, &cache_dir, true).expect("verify ok");
        assert!(cached.is_file());
        assert_eq!(fs::read(&cached).expect("read cached"), payload);

        // Cache hit streams directly without creating staging temp files or rewriting the inode.
        #[cfg(unix)]
        let inode_before = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&cached).expect("meta").ino()
        };
        let cached_again = fetch_and_verify_vsix(&pin, &cache_dir, true).expect("cache hit");
        assert_eq!(cached_again, cached);
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                fs::metadata(&cached_again).expect("meta").ino(),
                inode_before,
                "cache hit must verify in-place without rewriting the cached file"
            );
        }

        // Corrupt cache entry must be evicted and re-fetched cleanly.
        fs::write(&cached, b"corrupted").expect("corrupt cache");
        let healed = fetch_and_verify_vsix(&pin, &cache_dir, true).expect("heal cache");
        assert_eq!(fs::read(&healed).expect("read healed"), payload);

        // Mismatch must fail with exit 5 and leave zero staging temp files behind.
        let bad_pin = LockPinEntry {
            sha256: "0".repeat(64),
            ..pin
        };
        let err = fetch_and_verify_vsix(&bad_pin, &cache_dir, true).expect_err("must fail");
        assert_eq!(err.exit_code().as_i32(), 5);
        let leftover: Vec<_> = fs::read_dir(&cache_dir)
            .expect("ls cache")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
            .collect();
        assert!(
            leftover.is_empty(),
            "temp staging files leaked: {leftover:?}"
        );
    }

    #[test]
    fn downgrade_detection_works() {
        assert!(is_semver_downgrade("2.0.0", "1.9.0"));
        assert!(!is_semver_downgrade("1.9.0", "2.0.0"));
        assert!(!is_semver_downgrade("1.9.0", "1.9.0"));
    }

    /// Opus L2: an unparsable version has an unknown order, so it needs confirmation.
    #[test]
    fn unparsable_versions_count_as_downgrade_and_require_confirmation() {
        assert!(is_semver_downgrade("2024.11", "1.9.0"));
        assert!(is_semver_downgrade("1.9.0", "nightly"));
        let item = PinRepairItem {
            id: "a.b".to_string(),
            installed_version: "2024.11".to_string(),
            pinned_version: "1.0.0".to_string(),
            is_downgrade: is_semver_downgrade("2024.11", "1.0.0"),
            pin: LockPinEntry {
                id: "a.b".to_string(),
                version: "1.0.0".to_string(),
                sha256: "0".repeat(64),
                vsix_url: "https://marketplace.visualstudio.com/x".to_string(),
                required: true,
                tier: Some(1),
                status_pending: false,
            },
        };
        let repairs = std::slice::from_ref(&item);
        let run = |assume_yes: bool, is_tty: bool, answer: &str| {
            let mut reader = io::Cursor::new(answer.as_bytes().to_vec());
            let mut messages: Vec<u8> = Vec::new();
            let result = confirm_pin_downgrades_with_io(
                repairs,
                assume_yes,
                is_tty,
                &mut reader,
                &mut messages,
            );
            (result, String::from_utf8_lossy(&messages).into_owned())
        };

        let (refused, messages) = run(false, false, "y\n");
        assert_eq!(
            refused.expect_err("non-interactive").exit_code().as_i32(),
            5
        );
        assert!(
            messages.contains("downgrade a.b: 2024.11 -> 1.0.0"),
            "{messages}"
        );
        assert!(
            !messages.contains("Proceed"),
            "no prompt without a terminal"
        );

        assert!(run(true, false, "").0.is_ok());

        let (confirmed, prompt) = run(false, true, "YES\n");
        assert!(confirmed.is_ok());
        assert!(prompt.contains("Proceed with pin downgrade(s)? [y/N]: "));
        assert!(run(false, true, "y\n").0.is_ok());
        for answer in ["n\n", "\n", "", "maybe\n"] {
            let err = run(false, true, answer).0.expect_err(answer);
            assert_eq!(err.exit_code().as_i32(), 5, "{answer:?}");
        }

        let mut not_downgrade = item.clone();
        not_downgrade.is_downgrade = false;
        let mut reader = io::Cursor::new(Vec::new());
        let mut messages: Vec<u8> = Vec::new();
        assert!(confirm_pin_downgrades_with_io(
            std::slice::from_ref(&not_downgrade),
            false,
            false,
            &mut reader,
            &mut messages
        )
        .is_ok());
        assert!(messages.is_empty());
    }

    /// Opus L3: test URLs need an exact loopback host. A prefix match is not enough.
    #[test]
    fn test_urls_require_exact_loopback_host() {
        for ok in [
            "http://localhost:8080/pkg",
            "http://127.0.0.1:9/pkg",
            "http://[::1]:9/pkg",
            "file:///tmp/sample.vsix",
        ] {
            assert!(validate_vsix_url(ok, true).is_ok(), "{ok}");
            assert!(
                validate_vsix_url(ok, false).is_err(),
                "{ok} needs the test flag"
            );
        }
        for bad in [
            "http://localhost.evil.com/pkg",
            "http://127.0.0.1.evil.com/pkg",
            "http://localhost@evil.com/pkg",
            "http://evil.com/localhost",
            "http://127.0.0.2/pkg",
            "file://evil.com/tmp/x.vsix",
            "https://marketplace.visualstudio.com\\@evil.com/pkg",
            "https://marketplace.visualstudio.com /pkg",
        ] {
            let err = validate_vsix_url(bad, true).expect_err(bad);
            assert_eq!(err.exit_code().as_i32(), 5, "{bad}");
        }
    }

    /// Opus H1 / Astra A08 / OPUS-01 / OPUS-04: Tier 1 entries without a verified digest fail
    /// closed, Tier 2 pending entries are recorded in `tier2_unpinned`, and `--allow-unlocked`
    /// is selective (relaxing pending/missing entries while still enforcing bad versions and
    /// verified SHA-256 artifacts).
    #[test]
    fn lock_tiers_fail_closed_and_selective_allow_unlocked_enforces_verified_pins() {
        use crate::model::ExtensionPinsManifest;

        let tmp = TempDir::new().expect("tmp");
        fs::write(
            tmp.path().join("vsp.lock.json"),
            r#"{
              "version": 2, "pinRequired": false,
              "extensions": {
                "a.pending": {"tier": 1, "status": "pending_refresh", "targets": {}},
                "a.stale": {"tier": 1, "status": "pending_refresh", "version": "1.0.0",
                  "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                  "vsixUrl": "https://marketplace.visualstudio.com/x"},
                "b.native": {"tier": 1, "status": "verified", "version": "1.0.0", "universal": false,
                  "targets": {"plan9-mips": {
                    "sha256": "1111111111111111111111111111111111111111111111111111111111111111",
                    "vsixUrl": "https://marketplace.visualstudio.com/y"}}},
                "c.versioned": {"tier": 2, "status": "verified", "version": "1.2.3"},
                "c.badversion": {"tier": 2, "version": "1.2.3 --install-extension=x.y"},
                "d.tier2pending": {"tier": 2, "status": "pending_refresh", "version": null},
                "e.tier3": {"tier": 3},
                "h.universal": {"tier": 1, "status": "verified", "version": "3.0.0", "universal": true,
                  "sha256": "2222222222222222222222222222222222222222222222222222222222222222",
                  "vsixUrl": "https://marketplace.visualstudio.com/h"}
              }
            }"#,
        )
        .expect("write lock");

        let host_lock = format!(
            r#"{{"version": 2, "extensions": {{"g.hostnative": {{"tier": 1, "status": "verified",
                "version": "1.0.0", "universal": false, "targets": {{"{}": {{
                "sha256": "3333333333333333333333333333333333333333333333333333333333333333",
                "vsixUrl": "https://marketplace.visualstudio.com/g"}}}}}}}}}}"#,
            current_lock_platform()
        );
        let host_dir = TempDir::new().expect("host tmp");
        fs::write(host_dir.path().join("vsp.lock.json"), host_lock).expect("write host lock");

        let mut catalog = crate::schema_loader::load_embedded().expect("embedded catalog");
        catalog.root = tmp.path().to_path_buf();
        catalog.pins = PinsLoadState::Loaded(ExtensionPinsManifest {
            version: 1,
            note: None,
            pin_required: false,
            pins: BTreeMap::new(),
        });

        let ids: Vec<String> = [
            "a.pending",
            "a.stale",
            "b.native",
            "c.versioned",
            "c.badversion",
            "d.tier2pending",
            "e.tier3",
            "f.unlisted",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();

        let strict = resolve_and_verify_batch(&catalog, &ids, &VsixVerifyOptions::default())
            .expect("resolve");
        assert!(!strict.unlocked_bypass);
        assert_eq!(strict.tier2_unpinned, vec!["d.tier2pending".to_string()]);
        let failed: Vec<&str> = strict.failures.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            failed,
            vec!["a.pending", "a.stale", "b.native", "c.badversion"]
        );
        assert!(strict.failures.iter().all(|f| f.is_safety_refusal));
        assert!(strict.failures[0].message.contains("Tier 1 hash lock"));
        assert_eq!(
            strict.targets,
            vec![
                ("c.versioned".to_string(), "c.versioned@1.2.3".to_string()),
                ("d.tier2pending".to_string(), "d.tier2pending".to_string()),
                ("e.tier3".to_string(), "e.tier3".to_string()),
                ("f.unlisted".to_string(), "f.unlisted".to_string()),
            ]
        );

        // Selective `--allow-unlocked` relaxes pending/missing Tier 1 entries while STILL
        // enforcing Tier 2 version pins (`c.versioned@1.2.3`) and rejecting invalid semver (`c.badversion`).
        let bypass_ids: Vec<String> = [
            "a.pending",
            "a.stale",
            "b.native",
            "c.versioned",
            "d.tier2pending",
            "e.tier3",
            "f.unlisted",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        let bypass = resolve_and_verify_batch(
            &catalog,
            &bypass_ids,
            &VsixVerifyOptions {
                allow_unlocked: true,
                cache_dir: None,
            },
        )
        .expect("resolve with bypass");
        assert!(bypass.unlocked_bypass);
        assert!(bypass.failures.is_empty());
        assert_eq!(bypass.targets.len(), bypass_ids.len());
        assert!(bypass
            .targets
            .contains(&("c.versioned".to_string(), "c.versioned@1.2.3".to_string())));
        assert_eq!(bypass.tier2_unpinned, vec!["d.tier2pending".to_string()]);

        // Invalid semver injection is STILL refused under `--allow-unlocked`.
        let bad_ver_bypass = resolve_and_verify_batch(
            &catalog,
            &["c.badversion".to_string()],
            &VsixVerifyOptions {
                allow_unlocked: true,
                cache_dir: None,
            },
        )
        .expect("bad version under allow-unlocked");
        assert_eq!(bad_ver_bypass.failures.len(), 1);
        assert!(bad_ver_bypass.failures[0].is_safety_refusal);

        // A pending entry is never "fully pinned", and it is never a repair candidate.
        let installed = vec![InstalledExtension {
            id: "a.stale".to_string(),
            version: Some("2.0.0".to_string()),
        }];
        assert!(compute_pin_repairs(&catalog, &installed, false)
            .expect("repairs")
            .is_empty());
        assert!(
            !managed_extensions_fully_pinned(&catalog, &installed, &["a.stale".to_string()])
                .expect("fully pinned")
        );

        let managed = vec!["h.universal".to_string()];
        let at_lock = vec![InstalledExtension {
            id: "h.universal".to_string(),
            version: Some("3.0.0".to_string()),
        }];
        let drifted = vec![InstalledExtension {
            id: "h.universal".to_string(),
            version: Some("3.1.0".to_string()),
        }];
        assert!(managed_extensions_fully_pinned(&catalog, &at_lock, &managed).expect("at lock"));
        assert!(!managed_extensions_fully_pinned(&catalog, &drifted, &managed).expect("drifted"));
        assert!(!managed_extensions_fully_pinned(&catalog, &[], &managed).expect("missing"));
        assert!(!managed_extensions_fully_pinned(
            &catalog,
            &at_lock,
            &["h.universal".to_string(), "f.unlisted".to_string()]
        )
        .expect("one unpinned ID"));

        let mut host_catalog = catalog.clone();
        host_catalog.root = host_dir.path().to_path_buf();
        let (host_index, _, _) = load_combined_pin_index(&host_catalog, false).expect("host index");
        let native = &host_index["g.hostnative"];
        assert_eq!(native.tier, Some(1));
        assert!(
            native.has_verified_artifact(),
            "this platform has a verified target"
        );
        let (index, _, _) = load_combined_pin_index(&catalog, false).expect("index");
        assert!(
            !index["b.native"].has_verified_artifact(),
            "another platform only"
        );
        assert!(
            !index["a.stale"].has_verified_artifact(),
            "pending digests are ignored"
        );
        assert!(!index.contains_key("e.tier3"), "Tier 3 is not a lock entry");
    }

    #[test]
    fn redirect_resolution_uses_rfc3986_join() {
        let base = "https://marketplace.visualstudio.com/a/b/pkg?x=1";
        assert_eq!(
            resolve_redirect_url(base, "https://x.gallerycdn.vsassets.io/p").expect("absolute"),
            "https://x.gallerycdn.vsassets.io/p"
        );
        assert_eq!(
            resolve_redirect_url(base, "/root").expect("root-relative"),
            "https://marketplace.visualstudio.com/root"
        );
        assert_eq!(
            resolve_redirect_url(base, "next").expect("relative"),
            "https://marketplace.visualstudio.com/a/b/next"
        );
        let off = resolve_redirect_url(base, "//evil.example/p").expect("scheme-relative");
        assert!(validate_vsix_url(&off, false).is_err());
    }

    /// Astra A16: a lock record with `universal: false` never falls back to a flat
    /// top-level `sha256`/`vsixUrl` or a `"universal"` target; it requires a matching
    /// host-platform target.
    #[test]
    fn non_universal_lock_never_falls_back_to_flat_digest() {
        use crate::model::ExtensionPinsManifest;

        let tmp = TempDir::new().expect("tmp");
        let host = current_lock_platform();
        fs::write(
            tmp.path().join("vsp.lock.json"),
            format!(
                r#"{{
                  "version": 2, "pinRequired": false,
                  "extensions": {{
                    "a.mixed-flat": {{
                      "tier": 1, "universal": false, "status": "verified", "version": "1.0.0",
                      "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                      "vsixUrl": "https://marketplace.visualstudio.com/flat",
                      "targets": {{
                        "universal": {{
                          "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                          "vsixUrl": "https://marketplace.visualstudio.com/univ"
                        }}
                      }}
                    }},
                    "b.host-target": {{
                      "tier": 1, "universal": false, "status": "verified", "version": "1.0.0",
                      "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                      "vsixUrl": "https://marketplace.visualstudio.com/wrong-flat",
                      "targets": {{
                        "{host}": {{
                          "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                          "vsixUrl": "https://marketplace.visualstudio.com/host-only"
                        }}
                      }}
                    }}
                  }}
                }}"#
            ),
        )
        .expect("write lock");

        let mut catalog = crate::schema_loader::load_embedded().expect("embedded catalog");
        catalog.root = tmp.path().to_path_buf();
        catalog.pins = PinsLoadState::Loaded(ExtensionPinsManifest {
            version: 1,
            note: None,
            pin_required: false,
            pins: BTreeMap::new(),
        });

        let (index, _, _) = load_combined_pin_index(&catalog, false).expect("pin index");
        assert!(
            !index["a.mixed-flat"].has_verified_artifact(),
            "universal=false must not fall back to top-level digest or universal target"
        );
        let resolved = resolve_and_verify_batch(
            &catalog,
            &["a.mixed-flat".to_string()],
            &VsixVerifyOptions::default(),
        )
        .expect("resolve");
        assert_eq!(resolved.failures.len(), 1);
        assert!(resolved.failures[0].is_safety_refusal);
        assert!(resolved.targets.is_empty());

        if host != "universal" {
            let host_pin = &index["b.host-target"];
            assert!(host_pin.has_verified_artifact());
            assert_eq!(
                host_pin.sha256,
                "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
            );
            assert_eq!(
                host_pin.vsix_url,
                "https://marketplace.visualstudio.com/host-only"
            );
        }
    }

    #[test]
    fn grouped_refusal_summary_is_audience_aware_for_embedded_vs_external() {
        let embedded = crate::schema_loader::load_embedded().expect("embedded");
        let failures = vec![
            VsixFailure {
                id: "anthropic.claude-code".to_string(),
                message: "pending".to_string(),
                is_safety_refusal: true,
                kind: VsixFailureKind::Tier1PendingRefresh,
            },
            VsixFailure {
                id: "github.copilot-chat".to_string(),
                message: "pending".to_string(),
                is_safety_refusal: true,
                kind: VsixFailureKind::Tier1PendingRefresh,
            },
        ];

        let emb_msg = format_vsix_refusal_summary(&embedded, "rust", &failures);
        assert!(emb_msg.contains("Tier 1 hash lock pending_refresh (2):"));
        assert!(emb_msg.contains("    - anthropic.claude-code"));
        assert!(emb_msg.contains("    - github.copilot-chat"));
        assert!(emb_msg.contains("pass `--allow-unlocked`"));
        assert!(emb_msg.contains("upgrade `vsp`"));
        assert!(!emb_msg.contains("dev pins --refresh"));

        let mut external = embedded;
        external.root = PathBuf::from("/opt/my-catalog");
        let ext_msg = format_vsix_refusal_summary(&external, "rust", &failures);
        assert!(ext_msg.contains("vsp --catalog /opt/my-catalog dev pins --refresh"));
        assert!(ext_msg.contains("retry with `--catalog /opt/my-catalog`"));
    }
}

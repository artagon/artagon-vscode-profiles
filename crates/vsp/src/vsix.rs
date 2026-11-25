//! VSIX supply-chain lock verifier, bounded 4-worker concurrent downloader with per-hop
//! HTTPS + host allowlist redirect validation, zero-TOCTOU XDG cache, and `--repair-pins`.

use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::exit_codes::{VspError, VspResult};
use crate::model::{ExtensionPin, PinsLoadState, WorkspaceCatalog};
use crate::policy::is_valid_sha256_hex;
use crate::provenance::resolve_xdg_cache_home;
use crate::vscode::InstalledExtension;

/// Maximum number of concurrent VSIX download/verification worker threads.
pub const MAX_VSIX_WORKERS: usize = 4;

/// Maximum number of HTTP redirects followed while validating every hop.
const MAX_REDIRECT_HOPS: usize = 10;

/// Maximum VSIX archive download size (`256 MiB`) to bound disk/memory usage.
const MAX_VSIX_BYTES: u64 = 256 * 1024 * 1024;

static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolved VSIX lock entry sourced from `catalog/extension-pins.json` or 4-tier `vsp.lock.json`.
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
}

/// Optional 4-tier `vsp.lock.json` document shape (forward-compatible with Phase 2 lockfile).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VspLockFile {
    #[serde(default)]
    pin_required: bool,
    #[serde(default)]
    pins: BTreeMap<String, LockPinRecord>,
    #[serde(default)]
    extensions: BTreeMap<String, LockPinRecord>,
    #[serde(default)]
    tier2: Option<Tier2LockSection>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tier2LockSection {
    #[serde(default)]
    pin_required: bool,
    #[serde(default)]
    pins: BTreeMap<String, LockPinRecord>,
}

#[derive(Debug, Clone, Deserialize)]
struct LockPinRecord {
    #[serde(default)]
    version: String,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    vsix_url: String,
    #[serde(default)]
    tier: Option<u8>,
}

/// Options controlling VSIX pin resolution and download verification.
#[derive(Debug, Clone, Default)]
pub struct VsixVerifyOptions {
    /// Explicit `--allow-unlocked` bypass (prints stderr warning & records unverified state).
    pub allow_unlocked: bool,
    /// Override VSIX cache directory (defaults to `VSCODE_VSIX_CACHE_DIR` or `${XDG_CACHE_HOME:-~/.cache}/vsp/vsix`).
    pub cache_dir: Option<PathBuf>,
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
}

/// Result of resolving install targets for a list of extension IDs.
#[derive(Debug, Clone)]
pub struct ResolvedInstallTargets {
    /// Ordered `(extension_id, install_arg)` pairs where `install_arg` is either a verified
    /// local `.vsix` path or an extension ID (when unpinned and permitted).
    pub targets: Vec<(String, String)>,
    /// Extensions that failed pin lookup or VSIX verification.
    pub failures: Vec<VsixFailure>,
    /// `true` if `--allow-unlocked` / `ARTAGON_VSCODE_ALLOW_UNLOCKED=1` bypassed pin checks.
    pub unlocked_bypass: bool,
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
    env::var("ARTAGON_VSCODE_ALLOW_TEST_PIN_URL").ok().as_deref() == Some("1")
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
/// - `https://*.vsassets.io/...`
///
/// Rejects HTTP downgrades, userinfo (`@`), authority spoofing (`?`, `#`), and off-host URLs
/// with [`VspError::SafetyRefusal`] (exit `5`). When `allow_test_urls` is `true`, permits
/// `http://127.0.0.1`, `http://localhost`, and `file://` for hermetic offline test fixtures.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if the URL fails scheme or host validation.
pub fn validate_vsix_url(url: &str, allow_test_urls: bool) -> VspResult<()> {
    if allow_test_urls
        && (url.starts_with("http://127.0.0.1")
            || url.starts_with("http://localhost")
            || url.starts_with("file://"))
    {
        return Ok(());
    }

    let Some(rest) = url.strip_prefix("https://") else {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: rejected non-HTTPS URL: {url}"
        )));
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
        return Err(VspError::SafetyRefusal(format!(
            "vsix: rejected malformed URL authority: {url}"
        )));
    }

    let (host, port_opt) = match authority.split_once(':') {
        Some((h, p)) => (h, Some(p)),
        None => (authority, None),
    };
    if let Some(port) = port_opt {
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            return Err(VspError::SafetyRefusal(format!(
                "vsix: rejected invalid port in URL: {url}"
            )));
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
        return Err(VspError::SafetyRefusal(format!(
            "vsix: rejected invalid host labels in URL: {url}"
        )));
    }

    let host_lc = host.to_ascii_lowercase();
    if host_lc == "marketplace.visualstudio.com" {
        return Ok(());
    }
    for suffix in [".gallerycdn.vsassets.io", ".gallery.vsassets.io", ".vsassets.io"] {
        if let Some(prefix) = host_lc.strip_suffix(suffix) {
            if !prefix.is_empty() && !prefix.starts_with('.') && !prefix.ends_with('.') {
                return Ok(());
            }
        }
    }

    Err(VspError::SafetyRefusal(format!(
        "vsix: rejected off-host URL '{host_lc}' in {url}"
    )))
}

/// Resolves a VSIX lock index combining `catalog/extension-pins.json` and optional `vsp.lock.json`.
///
/// Fails closed with [`VspError::SafetyRefusal`] (exit `5`) when the pins manifest is missing
/// or invalid and `allow_unlocked` is `false`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) when lock metadata is missing/malformed and
/// unlocked bypass is not enabled.
pub fn load_combined_pin_index(
    catalog: &WorkspaceCatalog,
    allow_unlocked: bool,
) -> VspResult<(BTreeMap<String, LockPinEntry>, bool, bool)> {
    let bypass = unlocked_bypass_active(allow_unlocked);
    let mut entries: BTreeMap<String, LockPinEntry> = BTreeMap::new();
    let mut pin_required = false;

    // 1. Optional 4-tier `vsp.lock.json` entries (forward-compatible).
    let lock_path = catalog.root.join("vsp.lock.json");
    if lock_path.is_file() {
        let raw = fs::read_to_string(&lock_path).map_err(|err| {
            VspError::SafetyRefusal(format!(
                "vsix: unreadable vsp.lock.json {}: {err}",
                lock_path.display()
            ))
        })?;
        let lock_doc: VspLockFile = serde_json::from_str(&raw).map_err(|err| {
            VspError::SafetyRefusal(format!(
                "vsix: malformed vsp.lock.json {}: {err}",
                lock_path.display()
            ))
        })?;
        if lock_doc.pin_required
            || lock_doc
                .tier2
                .as_ref()
                .is_some_and(|t2| t2.pin_required)
        {
            pin_required = true;
        }
        let mut insert_lock_map = |map: &BTreeMap<String, LockPinRecord>| {
            for (k, rec) in map {
                if rec.sha256.is_empty() && rec.vsix_url.is_empty() {
                    continue;
                }
                let lc = k.to_ascii_lowercase();
                entries.insert(
                    lc.clone(),
                    LockPinEntry {
                        id: lc,
                        version: rec.version.clone(),
                        sha256: rec.sha256.to_ascii_lowercase(),
                        vsix_url: rec.vsix_url.clone(),
                        required: rec.tier == Some(1) || pin_required,
                    },
                );
            }
        };
        insert_lock_map(&lock_doc.pins);
        insert_lock_map(&lock_doc.extensions);
        if let Some(t2) = &lock_doc.tier2 {
            insert_lock_map(&t2.pins);
        }
    }

    // 2. Authoritative `catalog/extension-pins.json`.
    match &catalog.pins {
        PinsLoadState::Loaded(manifest) => {
            if manifest.pin_required {
                pin_required = true;
            }
            for (k, pin) in &manifest.pins {
                let lc = k.to_ascii_lowercase();
                entries.insert(
                    lc.clone(),
                    LockPinEntry {
                        id: lc,
                        version: pin.version.clone(),
                        sha256: pin.sha256.to_ascii_lowercase(),
                        vsix_url: pin.vsix_url.clone(),
                        required: manifest.pin_required,
                    },
                );
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

/// Downloads (or reuses from XDG cache) and SHA-256 verifies a single VSIX package,
/// returning the verified path inside `cache_dir`.
///
/// Performs:
/// 1. Strict extension ID & SHA-256 format validation.
/// 2. Zero-TOCTOU cache lookup: copies cached file into a private `0600` staging file
///    BEFORE hashing, evicting corrupt cache entries automatically.
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

    let final_cache_path = cache_dir.join(format!("{id_lc}-{expected}.vsix"));

    // Zero-TOCTOU cache hit verification: copy cached bytes into private staging file FIRST,
    // then hash the private copy before accepting.
    if final_cache_path.is_file() && !final_cache_path.is_symlink() {
        let stage = make_staging_path(cache_dir, "cache");
        let verify_res = (|| -> io::Result<String> {
            let mut src = File::open(&final_cache_path)?;
            let mut dst = create_private_file(&stage)?;
            let digest = copy_and_sha256(&mut src, &mut dst, MAX_VSIX_BYTES)?;
            dst.sync_all()?;
            Ok(digest)
        })();
        match verify_res {
            Ok(actual) if actual == expected => {
                // Atomically publish verified private copy back to cache path.
                let _ = fs::rename(&stage, &final_cache_path);
                let _ = fs::remove_file(&stage);
                return Ok(final_cache_path);
            }
            _ => {
                let _ = fs::remove_file(&stage);
                if final_cache_path.is_file() && !final_cache_path.is_symlink() {
                    let _ = fs::remove_file(&final_cache_path);
                }
            }
        }
    }

    // Stream download into a private 0600 staging file while validating every redirect hop.
    let stage = make_staging_path(cache_dir, "dl");
    let download_result = stream_verified_download(
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

fn stream_verified_download(
    initial_url: &str,
    expected_sha256: &str,
    stage_path: &Path,
    allow_test_urls: bool,
    ext_id: &str,
) -> VspResult<()> {
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
            if actual != expected_sha256 {
                return Err(VspError::SafetyRefusal(format!(
                    "vsix: sha256 mismatch for '{ext_id}' (expected {expected_sha256}, got {actual})"
                )));
            }
            return Ok(());
        }
    }

    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(!allow_test_urls)
        .timeout_global(Some(Duration::from_secs(60)))
        .build();
    let agent: ureq::Agent = config.into();

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
            VspError::Operational(format!(
                "vsix: stream read failed for '{ext_id}': {err}"
            ))
        })?;
        dst.sync_all().map_err(|err| {
            VspError::Operational(format!("vsix: failed syncing staging file: {err}"))
        })?;
        if actual != expected_sha256 {
            return Err(VspError::SafetyRefusal(format!(
                "vsix: sha256 mismatch for '{ext_id}' (expected {expected_sha256}, got {actual}, url {current_url})"
            )));
        }
        return Ok(());
    }

    Err(VspError::SafetyRefusal(format!(
        "vsix: redirect loop while downloading '{ext_id}'"
    )))
}

fn resolve_redirect_url(base: &str, location: &str) -> VspResult<String> {
    if location.starts_with("https://") || location.starts_with("http://") {
        return Ok(location.to_string());
    }
    if location.starts_with("//") {
        let scheme = base.split("://").next().unwrap_or("https");
        return Ok(format!("{scheme}:{location}"));
    }
    let Some(scheme_end) = base.find("://") else {
        return Err(VspError::SafetyRefusal(format!(
            "vsix: cannot resolve relative redirect '{location}' against '{base}'"
        )));
    };
    let after_scheme = &base[scheme_end + 3..];
    let origin_end = after_scheme
        .find('/')
        .map(|i| scheme_end + 3 + i)
        .unwrap_or(base.len());
    let origin = &base[..origin_end];
    if location.starts_with('/') {
        Ok(format!("{origin}{location}"))
    } else {
        let dir = match base.rfind('/') {
            Some(i) if i >= origin_end => &base[..=i],
            _ => origin,
        };
        Ok(format!("{dir}{location}"))
    }
}

/// Resolves and verifies a batch of extension IDs concurrently across at most
/// [`MAX_VSIX_WORKERS`] (`4`) bounded worker threads.
///
/// Preserves the exact input ordering of `ext_ids` in the returned [`ResolvedInstallTargets`].
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if the pins manifest itself is missing or
/// invalid and `--allow-unlocked` is not enabled.
pub fn resolve_and_verify_batch(
    catalog: &WorkspaceCatalog,
    ext_ids: &[String],
    opts: &VsixVerifyOptions,
) -> VspResult<ResolvedInstallTargets> {
    let bypass = unlocked_bypass_active(opts.allow_unlocked);
    let (pin_index, pin_required, manifest_bypassed) =
        load_combined_pin_index(catalog, opts.allow_unlocked)?;
    let mut unlocked_bypass = manifest_bypassed;

    if bypass {
        if !manifest_bypassed {
            eprintln!("vsix: WARNING: bypassing VSIX pin verification (--allow-unlocked)");
        }
        unlocked_bypass = true;
        return Ok(ResolvedInstallTargets {
            targets: ext_ids.iter().map(|id| (id.clone(), id.clone())).collect(),
            failures: Vec::new(),
            unlocked_bypass,
        });
    }

    let cache_dir = resolve_vsix_cache_dir(opts.cache_dir.as_deref());
    let allow_test = allow_test_pin_urls();

    // Partition into jobs (pinned downloads) vs pass-through or missing-required errors.
    type WorkerOutcome = Result<String, (String, bool)>;
    let mut slots: Vec<Option<WorkerOutcome>> = vec![None; ext_ids.len()];
    let mut jobs: VecDeque<(usize, LockPinEntry)> = VecDeque::new();

    for (idx, id) in ext_ids.iter().enumerate() {
        let lc = id.to_ascii_lowercase();
        if let Some(pin) = pin_index.get(&lc) {
            jobs.push_back((idx, pin.clone()));
        } else if pin_required {
            slots[idx] = Some(Err((
                format!("pinRequired=true but no VSIX pin exists for '{id}'"),
                true,
            )));
        } else {
            slots[idx] = Some(Ok(id.clone()));
        }
    }

    if !jobs.is_empty() {
        let worker_count = jobs.len().min(MAX_VSIX_WORKERS);
        let queue = Mutex::new(jobs);
        let results: Mutex<Vec<(usize, WorkerOutcome)>> = Mutex::new(Vec::new());

        thread::scope(|scope| {
            for _ in 0..worker_count {
                scope.spawn(|| loop {
                    let next = {
                        let mut q = queue.lock().expect("vsix queue lock");
                        q.pop_front()
                    };
                    let Some((idx, pin)) = next else {
                        break;
                    };
                    let outcome = match fetch_and_verify_vsix(&pin, &cache_dir, allow_test) {
                        Ok(path) => Ok(path.display().to_string()),
                        Err(err) => Err((
                            err.to_string(),
                            matches!(err, VspError::SafetyRefusal(_)),
                        )),
                    };
                    results
                        .lock()
                        .expect("vsix results lock")
                        .push((idx, outcome));
                });
            }
        });

        for (idx, outcome) in results.into_inner().expect("vsix results") {
            slots[idx] = Some(outcome);
        }
    }

    let mut targets = Vec::new();
    let mut failures = Vec::new();
    for (idx, id) in ext_ids.iter().enumerate() {
        match slots[idx].take() {
            Some(Ok(arg)) => targets.push((id.clone(), arg)),
            Some(Err((msg, is_safety))) => failures.push(VsixFailure {
                id: id.clone(),
                message: msg,
                is_safety_refusal: is_safety,
            }),
            None => failures.push(VsixFailure {
                id: id.clone(),
                message: "internal VSIX worker missing result".to_string(),
                is_safety_refusal: false,
            }),
        }
    }

    Ok(ResolvedInstallTargets {
        targets,
        failures,
        unlocked_bypass,
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
    let (pin_index, _, _) = load_combined_pin_index(catalog, allow_unlocked)?;
    let mut repairs = Vec::new();

    for ext in installed {
        let Some(inst_ver) = &ext.version else {
            continue;
        };
        let lc = ext.id.to_ascii_lowercase();
        let Some(pin) = pin_index.get(&lc) else {
            continue;
        };
        if pin.version.is_empty() || pin.version == *inst_ver {
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
    let downgrades: Vec<&PinRepairItem> = repairs.iter().filter(|r| r.is_downgrade).collect();
    if downgrades.is_empty() {
        return Ok(());
    }

    eprintln!("--repair-pins downgrade preview:");
    for item in &downgrades {
        eprintln!(
            "  downgrade {}: {} -> {}",
            item.id, item.installed_version, item.pinned_version
        );
    }

    if assume_yes {
        return Ok(());
    }

    if !io::stdin().is_terminal() {
        return Err(VspError::SafetyRefusal(
            "--repair-pins would downgrade pinned extension(s) in non-interactive mode; re-run with --yes to confirm".to_string(),
        ));
    }

    eprint!("Proceed with pin downgrade(s)? [y/N]: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(|err| {
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
/// call to `code`.
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
    let mut staged = Vec::with_capacity(repairs.len());
    for item in repairs {
        let path = fetch_and_verify_vsix(&item.pin, cache_dir, allow_test)?;
        staged.push((item.clone(), path));
    }
    Ok(staged)
}

/// Returns `true` if `target_ver` is strictly older than `installed_ver` by semver comparison.
#[must_use]
pub fn is_semver_downgrade(installed_ver: &str, target_ver: &str) -> bool {
    match (
        semver::Version::parse(installed_ver.trim_start_matches('v')),
        semver::Version::parse(target_ver.trim_start_matches('v')),
    ) {
        (Ok(inst), Ok(target)) => target < inst,
        _ => false,
    }
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

/// Fetches a single extension's latest stable universal VSIX metadata from the Marketplace API.
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if the Marketplace API cannot be
/// reached or returns a malformed response.
pub fn query_marketplace_universal_pin(
    ext_id: &str,
    api_url: &str,
) -> VspResult<ExtensionPin> {
    let payload = serde_json::json!({
        "filters": [{
            "criteria": [{ "filterType": 7, "value": ext_id }],
            "pageNumber": 1,
            "pageSize": 1
        }],
        "assetTypes": [],
        "flags": 103
    });

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();

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

    let body: Value = resp.body_mut().read_json().map_err(|err| {
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
            .unwrap_or("");
        if is_prerelease || !target_platform.is_empty() {
            continue;
        }
        let version = ver_obj
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
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
            .unwrap_or("")
            .to_string();
        if !version.is_empty() && !vsix_url.is_empty() {
            validate_vsix_url(&vsix_url, allow_test_pin_urls())?;
            return Ok(ExtensionPin {
                sha256: String::new(),
                version,
                vsix_url,
                verified_at: None,
            });
        }
    }

    Err(VspError::IncompleteObservation(format!(
        "no stable universal VSIX version found on Marketplace for '{ext_id}'"
    )))
}

/// Streams a VSIX URL into a temporary file, validates redirects, and returns its SHA-256 hex digest.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] on URL/redirect policy rejection or [`VspError::Operational`]
/// on download error.
pub fn download_and_hash_vsix_url(url: &str, ext_id: &str) -> VspResult<String> {
    let allow_test = allow_test_pin_urls();
    validate_vsix_url(url, allow_test)?;
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(!allow_test)
        .timeout_global(Some(Duration::from_secs(60)))
        .build();
    let agent: ureq::Agent = config.into();

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
        let mut sink = io::sink();
        let digest = copy_and_sha256(&mut reader, &mut sink, MAX_VSIX_BYTES).map_err(|err| {
            VspError::Operational(format!("vsix: failed hashing '{ext_id}': {err}"))
        })?;
        return Ok(digest);
    }

    Err(VspError::SafetyRefusal(format!(
        "vsix: redirect loop for '{ext_id}'"
    )))
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

fn copy_and_sha256<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    max_bytes: u64,
) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
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

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok(hex)
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
        assert!(validate_vsix_url("https://cdn.vsassets.io/pkg", false).is_ok());

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
            validate_vsix_url("https://marketplace.visualstudio.com@evil.example/pkg", false)
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
        };

        let cached = fetch_and_verify_vsix(&pin, &cache_dir, true).expect("verify ok");
        assert!(cached.is_file());
        assert_eq!(fs::read(&cached).expect("read cached"), payload);

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
        assert!(leftover.is_empty(), "temp staging files leaked: {leftover:?}");
    }

    #[test]
    fn downgrade_detection_works() {
        assert!(is_semver_downgrade("2.0.0", "1.9.0"));
        assert!(!is_semver_downgrade("1.9.0", "2.0.0"));
        assert!(!is_semver_downgrade("1.9.0", "1.9.0"));
    }
}

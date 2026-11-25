//! Safe VS Code `storage.json` locking (`fs4` advisory lock), SHA-256 compare-and-swap (CAS),
//! running-editor detection, and symlink-escape path guards.
//!
//! Critical fast path: if a named profile already exists in `storage.json`, `vsp` skips
//! writing `storage.json` completely so everyday profile syncs succeed safely even while
//! VS Code is open.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs4::fs_std::FileExt;
use serde_json::{json, Map, Value};

use crate::exit_codes::{VspError, VspResult};
use crate::legacy_loader::read_strict_json_value;
use crate::provenance::{
    backup_file_bounded, check_path_contained, write_file_atomic, DEFAULT_BACKUP_RETENTION,
};
use crate::resolver::{sha256_hex, to_canonical_json_string};
use crate::vscode::{find_storage_profile_location, validate_profile_name};

/// Default lock acquisition timeout (`10s`; overridable via `VSP_LOCK_TIMEOUT_MS` for tests).
pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

static ID_NONCE: AtomicU64 = AtomicU64::new(0);

/// RAII guard holding an OS-level exclusive advisory lock (`fs4`) on a lock file.
///
/// The lock is automatically released when dropped.
#[derive(Debug)]
pub struct AdvisoryLock {
    file: File,
    path: PathBuf,
}

impl AdvisoryLock {
    /// Returns the filesystem path of the underlying lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AdvisoryLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// Resolves the effective lock acquisition timeout, honoring `VSP_LOCK_TIMEOUT_MS` when set.
#[must_use]
pub fn effective_lock_timeout() -> Duration {
    if let Ok(raw) = env::var("VSP_LOCK_TIMEOUT_MS") {
        if let Ok(ms) = raw.trim().parse::<u64>() {
            return Duration::from_millis(ms);
        }
    }
    DEFAULT_LOCK_TIMEOUT
}

/// Acquires an OS-level exclusive advisory lock (`fs4`) at `lock_path` within `timeout`.
///
/// Refuses symlinks at `lock_path` with [`VspError::SafetyRefusal`] (exit `5`) and returns
/// [`VspError::RetryableLock`] (exit `6`) on contention timeout.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): `lock_path` is a symlink.
/// - [`VspError::RetryableLock`] (exit `6`): another process held the lock past `timeout`.
/// - [`VspError::Operational`] (exit `1`): filesystem error opening the lock file.
pub fn acquire_advisory_lock(lock_path: &Path, timeout: Duration) -> VspResult<AdvisoryLock> {
    if lock_path.is_symlink() {
        return Err(VspError::SafetyRefusal(format!(
            "refusing to lock symlink path: {}",
            lock_path.display()
        )));
    }
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            VspError::Operational(format!(
                "failed to create lock directory {}: {err}",
                parent.display()
            ))
        })?;
    }

    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(lock_path).map_err(|err| {
        VspError::Operational(format!(
            "failed to open lock file {}: {err}",
            lock_path.display()
        ))
    })?;

    let start = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(true) => {
                return Ok(AdvisoryLock {
                    file,
                    path: lock_path.to_path_buf(),
                });
            }
            Ok(false) => {
                if start.elapsed() >= timeout {
                    return Err(VspError::RetryableLock(format!(
                        "lock busy after {}ms: {} (another vsp process holds this target)",
                        timeout.as_millis(),
                        lock_path.display()
                    )));
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(err) => {
                return Err(VspError::Operational(format!(
                    "advisory lock syscall failed on {}: {err}",
                    lock_path.display()
                )));
            }
        }
    }
}

/// Detects whether the VS Code desktop application is currently running.
///
/// Honors the `ARTAGON_VSCODE_TEST_RUNNING` test hook (`"1"` = running, `"0"` = not running).
/// On macOS and Linux, scans `ps -eo args=` and fails closed (treats VS Code as running) if
/// `ps` fails to execute.
#[must_use]
pub fn is_vscode_running() -> bool {
    if let Ok(hook) = env::var("ARTAGON_VSCODE_TEST_RUNNING") {
        if hook == "1" {
            return true;
        }
        if hook == "0" {
            return false;
        }
    }

    let is_macos = cfg!(target_os = "macos");
    let is_linux = cfg!(target_os = "linux");
    if !is_macos && !is_linux {
        // No process scan exists for this OS. Fail closed: treat VS Code as running, so
        // the caller needs an explicit `--force` (or the test hook) to change storage.json.
        eprintln!(
            "warning: no VS Code process scan on this OS; treating VS Code as running (fail-closed)"
        );
        return true;
    }

    let output = match Command::new("ps").args(["-eo", "args="]).output() {
        Ok(out) if out.status.success() => out,
        _ => {
            eprintln!("warning: ps process scan failed; treating VS Code as running (fail-closed)");
            return true;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    ps_output_indicates_vscode_running(&stdout, is_macos)
}

/// Evaluates raw `ps -eo args=` output for running VS Code desktop processes.
///
/// - On macOS: matches the main executable of a recognized VS Code product bundle
///   ([`MACOS_VSCODE_BUNDLES`]), for example
///   `/Users/Jane Doe/Applications/Visual Studio Code.app/Contents/MacOS/Code` (parent
///   directories can contain spaces). Other Electron applications (Slack, Discord, and
///   similar) do not match. Transient `out/cli.js` CLI invocations are excluded, and so are
///   unrelated commands that mention the path in `argv[1+]`.
/// - On Linux: matches a line whose executable path ends in `/code` or `/code-insiders`,
///   also when the install path contains spaces, while excluding `out/cli.js` and
///   `/remote-cli/`.
#[must_use]
pub fn ps_output_indicates_vscode_running(ps_output: &str, is_macos: bool) -> bool {
    for raw_line in ps_output.lines() {
        let line = raw_line.trim_start();
        if line.is_empty() || line.contains("out/cli.js") {
            continue;
        }
        if is_macos {
            if macos_line_is_vscode(line) {
                return true;
            }
        } else if linux_line_is_vscode(line) {
            return true;
        }
    }
    false
}

/// macOS application bundles that are VS Code products. A bundle with another name is a
/// different application, also when its executable is named `Electron`.
pub const MACOS_VSCODE_BUNDLES: &[&str] = &[
    "Visual Studio Code.app",
    "Visual Studio Code - Insiders.app",
    "VSCodium.app",
];

/// Main executable names inside a VS Code product bundle (`Contents/MacOS/<name>`).
const MACOS_VSCODE_EXECUTABLES: &[&str] = &["Code", "Code - Insiders", "Electron", "VSCodium"];

fn macos_line_is_vscode(line: &str) -> bool {
    for bundle in MACOS_VSCODE_BUNDLES {
        for exe in MACOS_VSCODE_EXECUTABLES {
            let needle = format!("/{bundle}/Contents/MacOS/{exe}");
            let Some(pos) = line.find(&needle) else {
                continue;
            };
            let after = &line[pos + needle.len()..];
            let end_ok = after.is_empty() || after.chars().next().is_some_and(char::is_whitespace);
            if !end_ok {
                continue;
            }
            let prefix = &line[..pos];
            // Must be an absolute executable path in argv[0] (not an argument to another tool
            // like `grep /Applications/Visual Studio Code.app/Contents/MacOS/Code` or
            // `/usr/bin/cat /Applications/Visual Studio Code.app/Contents/MacOS/Code`).
            let is_argv0 = if prefix.is_empty() {
                true
            } else {
                prefix.starts_with('/') && !prefix.contains(" /") && !prefix.contains("\t/")
            };
            if is_argv0 {
                return true;
            }
        }
    }
    false
}

fn linux_line_is_vscode(line: &str) -> bool {
    if line.contains("/remote-cli/") {
        return false;
    }
    if !line.starts_with('/') {
        return false;
    }
    // `ps -eo args=` joins argv with spaces, so an install path with spaces cannot be split
    // on whitespace. Accept the line when a prefix that ends at an argument boundary is an
    // absolute path to the `code` executable and no earlier boundary starts a second
    // absolute path (which would make `code` an argument of another program).
    for exe in ["/code", "/code-insiders"] {
        let mut search_from = 0;
        while let Some(rel) = line[search_from..].find(exe) {
            let end = search_from + rel + exe.len();
            let at_boundary = line[end..].chars().next().is_none_or(char::is_whitespace);
            let prefix = &line[..end];
            if at_boundary && !prefix.contains(" /") && !prefix.contains("\t/") {
                return true;
            }
            search_from = end;
        }
    }
    false
}

/// Outcome of [`ensure_storage_profile`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureProfileOutcome {
    /// Profile directory location ID under `<user_dir>/profiles/<location>`.
    pub location: String,
    /// `true` if `storage.json` was mutated to register a brand-new profile; `false` when the
    /// profile already existed and `storage.json` was intentionally left untouched.
    pub created: bool,
    /// Full path to `<user_dir>/profiles/<location>`.
    pub profile_dir: PathBuf,
}

/// Ensures `profile_name` is registered in `<user_dir>/globalStorage/storage.json` and that
/// its directory `<user_dir>/profiles/<location>` exists.
///
/// - **Fast path (no `storage.json` write):** If `profile_name` ALREADY exists in
///   `storage.json`, skips acquiring the storage lock and skips writing `storage.json`
///   completely (succeeds safely even when VS Code is running).
/// - **New profile creation:** Enforces symlink containment (exit `5`), running-VS-Code guard
///   unless `force` is set (exit `5`), acquires an OS advisory lock (`fs4`) with timeout
///   (exit `6`), re-checks running VS Code under lock, computes pre-write SHA-256, reserves an
///   unused 8-hex location ID, creates a bounded `0600` `.bak` backup (last 5), performs a
///   pre-publish SHA-256 CAS check (exit `6` on external modification), and atomically writes
///   `storage.json` with mode `0600`.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): symlink escape or VS Code running without `--force`.
/// - [`VspError::RetryableLock`] (exit `6`): lock contention or external CAS modification.
/// - [`VspError::Operational`] (exit `1`): malformed `storage.json` or filesystem error.
pub fn ensure_storage_profile(
    user_dir: &Path,
    profile_name: &str,
    force: bool,
    timeout: Duration,
) -> VspResult<EnsureProfileOutcome> {
    ensure_storage_profile_with_cas_hook(user_dir, profile_name, force, timeout, None)
}

/// Internal testable entrypoint allowing deterministic pre-publish CAS fault injection.
///
/// # Errors
///
/// The same as [`ensure_storage_profile`].
pub fn ensure_storage_profile_with_cas_hook(
    user_dir: &Path,
    profile_name: &str,
    force: bool,
    timeout: Duration,
    pre_cas_hook: Option<&dyn Fn(&Path)>,
) -> VspResult<EnsureProfileOutcome> {
    // The name is written to storage.json and later passed to `code`: validate it first.
    validate_profile_name(profile_name)?;

    let storage_dir = user_dir.join("globalStorage");
    let storage_path = storage_dir.join("storage.json");
    let profiles_dir = user_dir.join("profiles");

    // Pre-flight symlink-escape guards on user_dir boundaries.
    check_path_contained(user_dir, &storage_dir)?;
    check_path_contained(user_dir, &storage_path)?;
    check_path_contained(user_dir, &profiles_dir)?;

    // Critical optimization: if profile already exists in storage.json, skip touching storage.json!
    if let Some(existing_loc) = find_storage_profile_location(user_dir, profile_name)? {
        let profile_dir = profiles_dir.join(&existing_loc);
        check_path_contained(user_dir, &profile_dir)?;
        ensure_private_dir(&profile_dir)?;
        return Ok(EnsureProfileOutcome {
            location: existing_loc,
            created: false,
            profile_dir,
        });
    }

    // Creating a new profile requires mutating storage.json -> enforce running-VS-Code guard.
    if !force && is_vscode_running() {
        return Err(VspError::SafetyRefusal(
            "VS Code appears to be running; creating a new profile in storage.json while VS Code is open risks lost updates (close VS Code or pass --force)".to_string(),
        ));
    }

    fs::create_dir_all(&storage_dir).map_err(|err| {
        VspError::Operational(format!("failed to create {}: {err}", storage_dir.display()))
    })?;

    let lock_path = storage_dir.join(".storage.json.lock");
    let _lock = acquire_advisory_lock(&lock_path, timeout)?;

    // Re-check VS Code running state under advisory lock.
    if !force && is_vscode_running() {
        return Err(VspError::SafetyRefusal(
            "VS Code started while waiting for storage.json lock; aborting to prevent lost update (close VS Code or pass --force)".to_string(),
        ));
    }

    // Re-check storage.json under lock in case another process registered it concurrently.
    if let Some(existing_loc) = find_storage_profile_location(user_dir, profile_name)? {
        let profile_dir = profiles_dir.join(&existing_loc);
        check_path_contained(user_dir, &profile_dir)?;
        ensure_private_dir(&profile_dir)?;
        return Ok(EnsureProfileOutcome {
            location: existing_loc,
            created: false,
            profile_dir,
        });
    }

    // Pre-write SHA-256 snapshot for CAS check.
    let pre_hash = file_sha256_opt(&storage_path)?;

    let mut root_map = if storage_path.is_file() {
        match read_strict_json_value(&storage_path, false)? {
            Value::Object(m) => m,
            _ => {
                return Err(VspError::Operational(format!(
                    "malformed storage.json (expected object): {}",
                    storage_path.display()
                )));
            }
        }
    } else {
        Map::new()
    };

    let existing_locs: Vec<String> = root_map
        .get("userDataProfiles")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("location").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();

    let new_id = reserve_unused_profile_id(&profiles_dir, &existing_locs)?;
    let profile_dir = profiles_dir.join(&new_id);
    check_path_contained(user_dir, &profile_dir)?;

    let mut profiles_arr = root_map
        .get("userDataProfiles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    profiles_arr.push(json!({
        "name": profile_name,
        "location": new_id,
    }));
    root_map.insert("userDataProfiles".to_string(), Value::Array(profiles_arr));

    let serialized = to_canonical_json_string(&root_map)?;

    // Backup existing storage.json (bounded 0600, last 5).
    if storage_path.is_file() {
        backup_file_bounded(&storage_path, DEFAULT_BACKUP_RETENTION)?;
    }

    ensure_private_dir(&profile_dir)?;

    if let Some(hook) = pre_cas_hook {
        hook(&storage_path);
    }

    // Pre-publish SHA-256 CAS verification (guards against independent writers even under --force).
    let now_hash = file_sha256_opt(&storage_path)?;
    if now_hash != pre_hash {
        return Err(VspError::RetryableLock(format!(
            "storage.json changed concurrently by an independent writer (CAS mismatch on {}); aborting to avoid lost update",
            storage_path.display()
        )));
    }

    write_file_atomic(&storage_path, serialized.as_bytes(), 0o600, true)?;

    Ok(EnsureProfileOutcome {
        location: new_id,
        created: true,
        profile_dir,
    })
}

fn ensure_private_dir(dir: &Path) -> VspResult<()> {
    fs::create_dir_all(dir).map_err(|err| {
        VspError::Operational(format!(
            "failed to create directory {}: {err}",
            dir.display()
        ))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn file_sha256_opt(path: &Path) -> VspResult<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|err| {
        VspError::Operational(format!(
            "failed to read {} for CAS hash: {err}",
            path.display()
        ))
    })?;
    Ok(Some(sha256_hex(&bytes)))
}

fn reserve_unused_profile_id(profiles_dir: &Path, existing_locs: &[String]) -> VspResult<String> {
    for attempt in 0..32u32 {
        let cand = generate_hex_8(attempt);
        if profiles_dir.join(&cand).exists() {
            continue;
        }
        if existing_locs.iter().any(|loc| loc == &cand) {
            continue;
        }
        return Ok(cand);
    }
    Err(VspError::Operational(
        "unable to reserve unused 8-hex profile location ID".to_string(),
    ))
}

fn generate_hex_8(attempt: u32) -> String {
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = File::open("/dev/urandom") {
            let mut buf = [0u8; 4];
            if f.read_exact(&mut buf).is_ok() {
                return format!("{:02x}{:02x}{:02x}{:02x}", buf[0], buf[1], buf[2], buf[3]);
            }
        }
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let seq = ID_NONCE.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{}:{}:{}:{attempt}", std::process::id(), nanos, seq);
    sha256_hex(seed.as_bytes())[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn macos_spaced_path_and_cli_exclusion_work() {
        let spaced = "/Users/Jane Doe/Applications/Visual Studio Code.app/Contents/MacOS/Code --folder-uri file:///tmp";
        assert!(ps_output_indicates_vscode_running(spaced, true));

        let cli = "/Applications/Visual Studio Code.app/Contents/Resources/app/out/cli.js --list-extensions";
        assert!(!ps_output_indicates_vscode_running(cli, true));

        let grep_arg = "grep /Applications/Visual Studio Code.app/Contents/MacOS/Code";
        assert!(!ps_output_indicates_vscode_running(grep_arg, true));

        let abs_cat = "/bin/cat /Applications/Visual Studio Code.app/Contents/MacOS/Code";
        assert!(!ps_output_indicates_vscode_running(abs_cat, true));

        let linux_ok = "/usr/share/code/code --new-window";
        assert!(ps_output_indicates_vscode_running(linux_ok, false));
        let linux_remote = "/home/u/.vscode-server/bin/abc/bin/remote-cli/code file.rs";
        assert!(!ps_output_indicates_vscode_running(linux_remote, false));
    }

    /// Astra A18: only recognized VS Code product bundles count on macOS.
    #[test]
    fn macos_scan_ignores_unrelated_electron_apps() {
        for other in [
            "/Applications/Slack.app/Contents/MacOS/Electron --type=renderer",
            "/Applications/Discord.app/Contents/MacOS/Discord",
            "/Applications/Obsidian.app/Contents/MacOS/Electron",
            "/Applications/Code Helper.app/Contents/MacOS/Code",
            "/Applications/Not Visual Studio Code.app.bak/Contents/MacOS/Electron",
        ] {
            assert!(!ps_output_indicates_vscode_running(other, true), "{other}");
        }
        for product in [
            "/Applications/Visual Studio Code.app/Contents/MacOS/Electron",
            "/Applications/Visual Studio Code.app/Contents/MacOS/Code",
            "/Applications/Visual Studio Code - Insiders.app/Contents/MacOS/Electron --x",
            "/Users/Jane Doe/Applications/VSCodium.app/Contents/MacOS/VSCodium",
        ] {
            assert!(
                ps_output_indicates_vscode_running(product, true),
                "{product}"
            );
        }
        // A mixed process list is "running" when one line is a VS Code product.
        let mixed = "/Applications/Slack.app/Contents/MacOS/Electron\n/Applications/Visual Studio Code.app/Contents/MacOS/Code\n";
        assert!(ps_output_indicates_vscode_running(mixed, true));
    }

    /// Opus L7: a Linux install path with spaces is still detected.
    #[test]
    fn linux_scan_handles_spaced_install_paths() {
        for running in [
            "/opt/Visual Studio Code/code --unity-launch",
            "/home/jane doe/apps/VSCode-linux-x64/code",
            "/usr/share/code-insiders/code-insiders --new-window",
            "/usr/share/code/code",
        ] {
            assert!(
                ps_output_indicates_vscode_running(running, false),
                "{running}"
            );
        }
        for not_running in [
            "/usr/bin/cat /usr/share/code/code",
            "grep /usr/share/code/code",
            "/usr/bin/codex --flag",
            "/usr/share/code/codeium-helper",
            "/home/u/.vscode-server/bin/abc/bin/remote-cli/code file.rs",
            "/usr/bin/node /usr/share/code/resources/app/out/cli.js --list-extensions",
        ] {
            assert!(
                !ps_output_indicates_vscode_running(not_running, false),
                "{not_running}"
            );
        }
    }

    /// Opus H2: storage.json is never changed for an unsafe profile name.
    #[test]
    fn ensure_storage_profile_rejects_unsafe_name_before_any_write() {
        let tmp = TempDir::new().expect("tmp");
        let user_dir = tmp.path().join("User");
        fs::create_dir_all(&user_dir).expect("mkdir");
        let err = ensure_storage_profile(
            &user_dir,
            "--install-extension=evil.ext",
            true,
            Duration::from_millis(50),
        )
        .expect_err("unsafe name");
        assert_eq!(err.exit_code().as_i32(), 5);
        assert!(!user_dir.join("globalStorage").exists());
        assert!(!user_dir.join("profiles").exists());
    }

    #[test]
    fn cas_mismatch_aborts_with_exit_6() {
        let tmp = TempDir::new().expect("tmp");
        let user_dir = tmp.path().join("User");
        let gs = user_dir.join("globalStorage");
        fs::create_dir_all(&gs).expect("mkdir");
        fs::write(gs.join("storage.json"), "{\"userDataProfiles\":[]}\n").expect("write");

        let hook = |storage: &Path| {
            fs::write(
                storage,
                "{\"userDataProfiles\":[{\"name\":\"external\",\"location\":\"deadbeef\"}]}\n",
            )
            .expect("external write");
        };

        let err = ensure_storage_profile_with_cas_hook(
            &user_dir,
            "rust",
            true,
            Duration::from_secs(2),
            Some(&hook),
        )
        .expect_err("CAS must reject external mutation");
        assert_eq!(err.exit_code().as_i32(), 6);
    }

    #[test]
    fn advisory_lock_contention_returns_exit_6() {
        let tmp = TempDir::new().expect("tmp");
        let lock_file = tmp.path().join("test.lock");
        let _guard = acquire_advisory_lock(&lock_file, Duration::from_secs(1)).expect("first lock");

        let err = acquire_advisory_lock(&lock_file, Duration::from_millis(80))
            .expect_err("second lock must fail with contention");
        assert_eq!(err.exit_code().as_i32(), 6);
    }
}

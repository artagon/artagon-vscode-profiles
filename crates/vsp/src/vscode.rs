//! Read-only VS Code data directory, `storage.json` profile registry, XDG path helpers,
//! and bounded `code` CLI subprocess execution.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::exit_codes::{VspError, VspResult};
use crate::jsonc::read_strict_json_value;
use crate::provenance::check_path_contained;

/// Default maximum wall-clock duration for a single `code` CLI subprocess (`120s`).
///
/// Overridable via `VSP_CODE_TIMEOUT_SECS` (seconds) or `VSP_CODE_TIMEOUT_MS` (milliseconds).
pub const DEFAULT_CODE_TIMEOUT_SECS: u64 = 120;

/// Maximum bytes captured from a single `code` subprocess stdout or stderr stream (`16 MiB`).
const MAX_CODE_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;

/// Resolved VS Code user-data store paths.
#[derive(Debug, Clone)]
pub struct VsCodeStore {
    /// Path to the `User` directory containing `settings.json`, `profiles/`, and `globalStorage/`.
    pub user_dir: PathBuf,
    /// Optional explicit `--user-data-dir` root passed to `code` when overriding the default store.
    pub data_root: Option<PathBuf>,
}

/// Structured result of executing a bounded VS Code `code` CLI subprocess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeExecOutput {
    /// `true` when the subprocess exited with status code `0`.
    pub success: bool,
    /// Process exit status code, or `None` when terminated by a signal.
    pub exit_code: Option<i32>,
    /// Captured standard output decoded lossily as UTF-8.
    pub stdout: String,
    /// Captured standard error decoded lossily as UTF-8 with normalized whitespace.
    pub stderr: String,
}

impl CodeExecOutput {
    /// Formats a compact single-line failure summary combining exit code and trimmed stderr
    /// so caller diagnostics never discard child stderr.
    #[must_use]
    pub fn format_failure(&self) -> String {
        let rc = self
            .exit_code
            .map_or_else(|| "signal".to_string(), |c| c.to_string());
        let cleaned = normalize_stderr(&self.stderr);
        if cleaned.is_empty() {
            format!("exit={rc}")
        } else {
            format!("exit={rc}: {cleaned}")
        }
    }
}

/// Resolves the effective `code` CLI subprocess timeout.
///
/// Honors `VSP_CODE_TIMEOUT_SECS` (positive integer seconds) first, then optional
/// `VSP_CODE_TIMEOUT_MS` (positive integer milliseconds), defaulting to
/// [`DEFAULT_CODE_TIMEOUT_SECS`] (`120s`).
#[must_use]
pub fn effective_code_timeout() -> Duration {
    if let Ok(raw) = env::var("VSP_CODE_TIMEOUT_SECS") {
        if let Ok(secs) = raw.trim().parse::<u64>() {
            if secs > 0 {
                return Duration::from_secs(secs);
            }
        }
    }
    if let Ok(raw) = env::var("VSP_CODE_TIMEOUT_MS") {
        if let Ok(ms) = raw.trim().parse::<u64>() {
            if ms > 0 {
                return Duration::from_millis(ms);
            }
        }
    }
    Duration::from_secs(DEFAULT_CODE_TIMEOUT_SECS)
}

/// Executes a VS Code `code` CLI command using [`effective_code_timeout`].
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if the subprocess fails to spawn,
/// fails while waiting, or exceeds the timeout (in which case the child process tree is killed
/// cleanly).
pub fn run_code_command(cmd: &mut Command, context: &str) -> VspResult<CodeExecOutput> {
    run_code_command_with_timeout(cmd, effective_code_timeout(), context)
}

/// Executes a VS Code `code` CLI command with an explicit wall-clock `timeout`.
///
/// Captures `stdout` and `stderr` concurrently on reader threads so large outputs cannot
/// deadlock on pipe buffers. On Unix, isolates the child in its own process group so a
/// timed-out wrapper script and all of its descendants are terminated cleanly without leaving
/// orphan processes or blocking pipe readers.
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if the subprocess fails to spawn,
/// encounters a wait syscall error, or exceeds `timeout`.
pub fn run_code_command_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
    context: &str,
) -> VspResult<CodeExecOutput> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().map_err(|err| {
        VspError::IncompleteObservation(format!("failed to invoke {context}: {err}"))
    })?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_reader = thread::spawn(move || read_bounded_pipe(stdout_pipe));
    let stderr_reader = thread::spawn(move || read_bounded_pipe(stderr_pipe));

    let deadline = Instant::now() + timeout;
    let mut poll_ms = 5u64;

    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                let now = Instant::now();
                if now >= deadline {
                    kill_child_tree(&mut child);
                    let timeout_label = if timeout.as_secs() > 0 && timeout.subsec_millis() == 0 {
                        format!("{}s", timeout.as_secs())
                    } else {
                        format!("{}ms", timeout.as_millis())
                    };
                    return Err(VspError::IncompleteObservation(format!(
                        "{context} timed out after {timeout_label} (override with VSP_CODE_TIMEOUT_SECS)"
                    )));
                }
                let remaining = deadline.saturating_duration_since(now);
                thread::sleep(Duration::from_millis(poll_ms).min(remaining));
                poll_ms = (poll_ms * 2).min(50);
            }
            Err(err) => {
                kill_child_tree(&mut child);
                return Err(VspError::IncompleteObservation(format!(
                    "failed waiting for {context}: {err}"
                )));
            }
        }
    };

    let stdout_bytes = stdout_reader.join().unwrap_or_default();
    let stderr_bytes = stderr_reader.join().unwrap_or_default();

    Ok(CodeExecOutput {
        success: status.success(),
        exit_code: status.code(),
        stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_bytes).trim().to_string(),
    })
}

fn read_bounded_pipe<R: Read>(pipe: Option<R>) -> Vec<u8> {
    let Some(reader) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = reader.take(MAX_CODE_CAPTURE_BYTES).read_to_end(&mut buf);
    buf
}

fn kill_child_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(raw_pid) = i32::try_from(child.id()) {
            if let Some(pid) = rustix::process::Pid::from_raw(raw_pid) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn normalize_stderr(raw: &str) -> String {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolves XDG config home per the XDG Base Directory Specification (ignoring relative paths).
#[must_use]
pub fn resolve_xdg_config_home() -> PathBuf {
    if let Ok(val) = env::var("XDG_CONFIG_HOME") {
        let trimmed = val.trim_end_matches('/');
        if trimmed.starts_with('/') {
            return PathBuf::from(trimmed);
        }
    }
    home_dir().join(".config")
}

/// Returns the XDG config home only when it does not depend on the working directory:
/// an absolute `XDG_CONFIG_HOME`, or `$HOME/.config` for an absolute `HOME`.
///
/// `None` means that `XDG_CONFIG_HOME` is absent or relative and `HOME` is absent, empty,
/// or relative. [`resolve_xdg_config_home`] would then give a path beneath `/` or beneath
/// the working directory, which a security-relevant file must never use.
#[must_use]
pub fn trusted_xdg_config_home() -> Option<PathBuf> {
    if let Ok(val) = env::var("XDG_CONFIG_HOME") {
        let trimmed = val.trim_end_matches('/');
        if trimmed.starts_with('/') {
            return Some(PathBuf::from(trimmed));
        }
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".config"))
}

/// Resolves the user's home directory from `HOME`.
#[must_use]
pub fn home_dir() -> PathBuf {
    env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Resolves the default OS-specific VS Code `User` directory.
#[must_use]
pub fn default_vscode_user_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir().join("Library/Application Support/Code/User")
    } else {
        resolve_xdg_config_home().join("Code/User")
    }
}

/// Resolves the VS Code store honoring `--vscode-data-dir`, `VSCODE_USER_DATA_DIR`,
/// `VSCODE_USER_DIR`, and OS defaults while detecting split-brain misconfiguration and
/// symlink escapes out of the data directory root.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`) if `VSCODE_USER_DIR` disagrees with
///   `VSCODE_USER_DATA_DIR/User` or does not end in `/User` when `require_data_root` is set.
/// - [`VspError::SafetyRefusal`] (exit `5`) if `<data_root>/User` escapes `<data_root>` via symlink.
pub fn resolve_vscode_store(
    cli_data_dir: Option<&Path>,
    require_data_root: bool,
) -> VspResult<VsCodeStore> {
    if let Some(dir) = cli_data_dir {
        let raw = dir.to_string_lossy();
        let trimmed = raw.trim_end_matches('/');
        let clean = PathBuf::from(if trimmed.is_empty() { "/" } else { trimmed });
        let user_dir = clean.join("User");
        check_path_contained(&clean, &user_dir)?;
        return Ok(VsCodeStore {
            user_dir,
            data_root: Some(clean),
        });
    }

    let env_data = env::var("VSCODE_USER_DATA_DIR")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());
    let env_user = env::var("VSCODE_USER_DIR")
        .ok()
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());

    if let Some(data_str) = env_data {
        let data_root = PathBuf::from(&data_str);
        let expected_user = data_root.join("User");
        check_path_contained(&data_root, &expected_user)?;
        if let Some(user_str) = env_user {
            let actual_user = PathBuf::from(&user_str);
            let c_exp = fs::canonicalize(&expected_user).unwrap_or_else(|_| expected_user.clone());
            let c_act = fs::canonicalize(&actual_user).unwrap_or_else(|_| actual_user.clone());
            if c_exp != c_act {
                return Err(VspError::UsageOrConfig(format!(
                    "VSCODE_USER_DIR ({}) disagrees with VSCODE_USER_DATA_DIR/User ({})",
                    actual_user.display(),
                    expected_user.display()
                )));
            }
            check_path_contained(&data_root, &actual_user)?;
            return Ok(VsCodeStore {
                user_dir: actual_user,
                data_root: Some(data_root),
            });
        }
        return Ok(VsCodeStore {
            user_dir: expected_user,
            data_root: Some(data_root),
        });
    }

    if let Some(user_str) = env_user {
        let user_dir = PathBuf::from(&user_str);
        if user_dir.file_name().and_then(|n| n.to_str()) == Some("User") {
            let data_root = user_dir.parent().map(Path::to_path_buf);
            if let Some(ref dr) = data_root {
                check_path_contained(dr, &user_dir)?;
            }
            return Ok(VsCodeStore {
                user_dir,
                data_root,
            });
        }
        if require_data_root {
            return Err(VspError::UsageOrConfig(format!(
                "VSCODE_USER_DIR='{}' does not end in '/User'; set VSCODE_USER_DIR='<root>/User' or VSCODE_USER_DATA_DIR='<root>'",
                user_dir.display()
            )));
        }
        return Ok(VsCodeStore {
            user_dir,
            data_root: None,
        });
    }

    Ok(VsCodeStore {
        user_dir: default_vscode_user_dir(),
        data_root: None,
    })
}

/// Lists all registered VS Code profile display names in `<user_dir>/globalStorage/storage.json`,
/// sorted uniquely in `LC_ALL=C` order.
///
/// Returns an empty list if `storage.json` does not exist.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `storage.json` exists but is malformed JSON.
pub fn list_storage_profiles(user_dir: &Path) -> VspResult<Vec<String>> {
    let storage = user_dir.join("globalStorage/storage.json");
    if !storage.is_file() {
        return Ok(Vec::new());
    }
    let val = read_strict_json_value(&storage, false)?;
    let mut names = BTreeSet::new();
    if let Some(arr) = val.get("userDataProfiles").and_then(Value::as_array) {
        for entry in arr {
            if let Some(name) = entry.get("name").and_then(Value::as_str) {
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
    }
    Ok(names.into_iter().collect())
}

/// Looks up the storage location directory ID for a named profile in `storage.json`.
///
/// Returns `Ok(None)` if `storage.json` or the named profile does not exist.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if `storage.json` is malformed, has duplicate entries
/// for `profile_name`, or contains an unsafe location ID.
pub fn find_storage_profile_location(
    user_dir: &Path,
    profile_name: &str,
) -> VspResult<Option<String>> {
    let storage = user_dir.join("globalStorage/storage.json");
    if !storage.is_file() {
        return Ok(None);
    }
    let val = read_strict_json_value(&storage, false)?;
    let mut matches = Vec::new();
    if let Some(arr) = val.get("userDataProfiles").and_then(Value::as_array) {
        for entry in arr {
            if entry.get("name").and_then(Value::as_str) == Some(profile_name) {
                let loc = entry
                    .get("location")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                matches.push(loc);
            }
        }
    }
    if matches.len() > 1 {
        return Err(VspError::Operational(format!(
            "duplicate userDataProfiles entries named '{profile_name}' in {}",
            storage.display()
        )));
    }
    let Some(loc) = matches.into_iter().next() else {
        return Ok(None);
    };
    if loc.is_empty()
        || !loc
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(VspError::Operational(format!(
            "invalid profile location '{loc}' for '{profile_name}' in {}",
            storage.display()
        )));
    }
    Ok(Some(loc))
}

/// Maximum accepted length of a VS Code profile name, in bytes.
pub const MAX_PROFILE_NAME_LEN: usize = 128;

/// Validates a VS Code profile name before it reaches the `code` command line or
/// `storage.json`.
///
/// A profile name can come from an untrusted source: `-p/--profile`, the `.name` field of a
/// `--from` bundle, or `storage.json`. The name is rejected when it is empty, starts with
/// `-` (it would be parsed as a `code` option), contains a control character, `/`, `\`, or
/// `..`, or is longer than [`MAX_PROFILE_NAME_LEN`] bytes.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) for a rejected name.
pub fn validate_profile_name(name: &str) -> VspResult<()> {
    let reason = if name.is_empty() {
        Some("it is empty")
    } else if name.len() > MAX_PROFILE_NAME_LEN {
        Some("it is longer than 128 bytes")
    } else if name.starts_with('-') {
        Some("it starts with '-'")
    } else if name.chars().any(char::is_control) {
        Some("it contains a control character")
    } else if name.contains('/') || name.contains('\\') {
        Some("it contains a path separator")
    } else if name.contains("..") {
        Some("it contains '..'")
    } else {
        None
    };
    match reason {
        None => Ok(()),
        Some(why) => Err(VspError::SafetyRefusal(format!(
            "unsafe VS Code profile name {name:?}: {why}"
        ))),
    }
}

/// Locates an executable on `PATH` with [`which::which`].
#[must_use]
pub fn find_binary(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

/// Locates the `code` executable on `PATH` (if available).
#[must_use]
pub fn find_code_binary() -> Option<PathBuf> {
    find_binary("code")
}

/// Builds `code [--user-data-dir <root>] [--profile=<name>]` for one profile.
///
/// The profile name is validated with [`validate_profile_name`] and passed as the single
/// argument `--profile=<name>`, so a name can never be read as a separate `code` option.
/// The Default profile (`"default"`) needs no `--profile` argument.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) for an unsafe profile name.
pub fn code_command(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
) -> VspResult<Command> {
    validate_profile_name(profile_name)?;
    let mut cmd = Command::new(code_bin);
    if let Some(root) = &store.data_root {
        cmd.arg("--user-data-dir").arg(root);
    }
    if profile_name != "default" {
        cmd.arg(format!("--profile={profile_name}"));
    }
    Ok(cmd)
}

/// Installed VS Code extension parsed from `code --list-extensions --show-versions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledExtension {
    /// Lowercase extension ID (`publisher.name`).
    pub id: String,
    /// Optional installed version string after `@`.
    pub version: Option<String>,
}

/// Queries installed extensions for `profile_name` (`"default"` or a named profile)
/// by executing `code [--user-data-dir <root>] [--profile=<name>] --list-extensions --show-versions`
/// under [`effective_code_timeout`].
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`) if `profile_name` is unsafe (`code` is not run).
/// - [`VspError::IncompleteObservation`] (exit `7`) if `code` fails to spawn, times out, or
///   exits nonzero.
pub fn list_installed_extensions(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
) -> VspResult<Vec<InstalledExtension>> {
    let mut cmd = code_command(code_bin, store, profile_name)?;
    cmd.arg("--list-extensions").arg("--show-versions");

    let context = format!("{} for profile '{profile_name}'", code_bin.display());
    let output = run_code_command(&mut cmd, &context).map_err(|err| {
        VspError::IncompleteObservation(format!(
            "audit incomplete — failed to list extensions for profile '{profile_name}' ({err})"
        ))
    })?;

    if !output.success {
        let detail = output.format_failure();
        return Err(VspError::IncompleteObservation(format!(
            "audit incomplete — failed to list extensions for profile '{profile_name}' ({detail})"
        )));
    }

    let mut exts = Vec::new();
    for line in output.stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (id_part, ver_part) = match trimmed.split_once('@') {
            Some((id, ver)) => (id, Some(ver.to_string())),
            None => (trimmed, None),
        };
        exts.push(InstalledExtension {
            id: id_part.to_ascii_lowercase(),
            version: ver_part,
        });
    }
    Ok(exts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opus H2: a profile name can never become a separate `code` option.
    #[test]
    fn profile_name_validation_rejects_argv_injection_and_path_forms() {
        for ok in [
            "rust",
            "My Profile",
            "java-spring",
            "rust-profile-crisp",
            "a.b_c",
        ] {
            assert!(validate_profile_name(ok).is_ok(), "{ok}");
        }
        let too_long = "a".repeat(MAX_PROFILE_NAME_LEN + 1);
        for bad in [
            "",
            "-p",
            "--install-extension=attackerpub.backdoor",
            "a/b",
            "a\\b",
            "..",
            "a..b",
            "new\nline",
            "nul\0byte",
            "tab\tname",
            "del\x7fname",
            too_long.as_str(),
        ] {
            let err = validate_profile_name(bad).expect_err(bad);
            assert_eq!(err.exit_code().as_i32(), 5, "{bad:?}");
        }
        assert!(validate_profile_name(&"a".repeat(MAX_PROFILE_NAME_LEN)).is_ok());
    }

    #[test]
    fn code_command_passes_profile_as_one_argument() {
        let store = VsCodeStore {
            user_dir: PathBuf::from("/data/User"),
            data_root: Some(PathBuf::from("/data")),
        };
        let cmd = code_command(Path::new("/bin/code"), &store, "my rust").expect("command");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--user-data-dir", "/data", "--profile=my rust"]);

        let default_cmd = code_command(Path::new("/bin/code"), &store, "default").expect("default");
        assert_eq!(default_cmd.get_args().count(), 2);

        let err = code_command(Path::new("/bin/code"), &store, "--install-extension=x.y")
            .expect_err("unsafe name");
        assert_eq!(err.exit_code().as_i32(), 5);
    }

    #[test]
    fn find_binary_uses_path_lookup() {
        assert!(find_binary("vsp-definitely-not-a-real-binary-name").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn code_timeout_kills_hung_child_and_returns_exit_7() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let start = Instant::now();
        let err = run_code_command_with_timeout(&mut cmd, Duration::from_millis(120), "stub code")
            .expect_err("hung child must time out");
        assert_eq!(err.exit_code().as_i32(), 7);
        assert!(err.to_string().contains("timed out"), "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timeout wrapper hung: {:?}",
            start.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn code_exec_output_captures_exit_code_and_trimmed_stderr() {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo 'out line'; echo '  bad profile  ' >&2; exit 23"]);
        let out = run_code_command_with_timeout(&mut cmd, Duration::from_secs(5), "stub code")
            .expect("spawned");
        assert!(!out.success);
        assert_eq!(out.exit_code, Some(23));
        assert_eq!(out.stdout.trim(), "out line");
        assert_eq!(out.stderr, "bad profile");
        assert_eq!(out.format_failure(), "exit=23: bad profile");
    }
}

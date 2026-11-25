//! Read-only VS Code data directory, `storage.json` profile registry, XDG path helpers,
//! and `code --list-extensions` subprocess observer.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::exit_codes::{VspError, VspResult};
use crate::legacy_loader::read_strict_json_value;
use crate::provenance::check_path_contained;

/// Resolved VS Code user-data store paths.
#[derive(Debug, Clone)]
pub struct VsCodeStore {
    /// Path to the `User` directory containing `settings.json`, `profiles/`, and `globalStorage/`.
    pub user_dir: PathBuf,
    /// Optional explicit `--user-data-dir` root passed to `code` when overriding the default store.
    pub data_root: Option<PathBuf>,
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

/// Resolves the user's home directory from `HOME`.
#[must_use]
pub fn home_dir() -> PathBuf {
    env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
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

/// Locates the `code` executable on `PATH` (if available).
#[must_use]
pub fn find_code_binary() -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join("code");
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(path) {
        Ok(meta) => meta.is_file() && (meta.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
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
/// by executing `code [--user-data-dir <root>] [--profile <name>] --list-extensions --show-versions`.
///
/// # Errors
///
/// Returns [`VspError::IncompleteObservation`] (exit `7`) if `code` fails to spawn or exits nonzero.
pub fn list_installed_extensions(
    code_bin: &Path,
    store: &VsCodeStore,
    profile_name: &str,
) -> VspResult<Vec<InstalledExtension>> {
    let mut cmd = Command::new(code_bin);
    if let Some(root) = &store.data_root {
        cmd.arg("--user-data-dir").arg(root);
    }
    if profile_name != "default" {
        cmd.arg("--profile").arg(profile_name);
    }
    cmd.arg("--list-extensions").arg("--show-versions");

    let output = cmd.output().map_err(|err| {
        VspError::IncompleteObservation(format!(
            "failed to invoke {} for profile '{profile_name}': {err}",
            code_bin.display()
        ))
    })?;

    if !output.status.success() {
        let rc = output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |c| c.to_string());
        let stderr = String::from_utf8_lossy(&output.stderr)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let detail = if stderr.is_empty() {
            format!("exit={rc}")
        } else {
            format!("exit={rc}: {stderr}")
        };
        return Err(VspError::IncompleteObservation(format!(
            "audit incomplete — failed to list extensions for profile '{profile_name}' ({detail})"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut exts = Vec::new();
    for line in stdout.lines() {
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

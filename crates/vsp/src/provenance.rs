//! XDG state provenance manifests, bounded `0600` settings backups, atomic file writes,
//! and symlink-escape path containment guards.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::exit_codes::{VspError, VspResult};
use crate::model::{AiMode, UxPreset};
use crate::resolver::{sha256_hex, to_canonical_json_string};
use crate::vscode::home_dir;

/// Default maximum number of bounded `.bak` backups retained per file.
pub const DEFAULT_BACKUP_RETENTION: usize = 5;

/// Maximum symlink hops permitted during canonical path containment checks.
const MAX_SYMLINK_HOPS: usize = 40;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Persistent per-profile provenance record stored under
/// `${XDG_STATE_HOME:-~/.local/state}/vsp/profiles/<hash>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileProvenance {
    /// Provenance schema version (`1`).
    #[serde(default = "default_version")]
    pub version: u32,
    /// Target VS Code profile name (`"default"` or named profile).
    pub profile: String,
    /// Canonical VS Code `User` directory backing this profile.
    pub user_dir: String,
    /// Optional preset/recipe name used during the last sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Ordered resolved module IDs applied during the last sync.
    #[serde(default)]
    pub resolved_modules: Vec<String>,
    /// Top-level settings keys managed by `vsp` in the profile's `settings.json`.
    #[serde(default)]
    pub managed_settings_keys: Vec<String>,
    /// Extension IDs managed by `vsp` in the profile.
    #[serde(default)]
    pub managed_extensions: Vec<String>,
    /// Effective UX preset applied during the last sync, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ux: Option<UxPreset>,
    /// Effective AI mode applied during the last sync, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_mode: Option<AiMode>,
    /// Whether `--allow-unlocked` bypassed VSIX pin verification during the last sync.
    #[serde(default)]
    pub unlocked_bypass: bool,
    /// RFC 3339 UTC timestamp of the last sync.
    pub last_sync_at: String,
}

/// Persistent per-workspace provenance record stored under
/// `${XDG_STATE_HOME:-~/.local/state}/vsp/workspaces/<hash>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceProvenance {
    /// Provenance schema version (`1`).
    #[serde(default = "default_version")]
    pub version: u32,
    /// Canonical workspace directory path.
    pub workspace: String,
    /// Path + inode generation fingerprint invalidating stale state if directory is recreated.
    pub workspace_fingerprint: String,
    /// Ordered resolved module IDs applied during the last sync.
    #[serde(default)]
    pub resolved_modules: Vec<String>,
    /// Top-level settings keys managed by `vsp` in `.vscode/settings.json`.
    #[serde(default)]
    pub managed_settings_keys: Vec<String>,
    /// Extension recommendations inserted/managed by `vsp` in `.vscode/extensions.json`.
    #[serde(default)]
    pub managed_recommendations: Vec<String>,
    /// Effective UX preset applied, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ux: Option<UxPreset>,
    /// Effective AI mode applied, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_mode: Option<AiMode>,
    /// RFC 3339 UTC timestamp of the last sync.
    pub last_sync_at: String,
}

fn default_version() -> u32 {
    1
}

/// Resolves `${XDG_STATE_HOME:-~/.local/state}` per the XDG Base Directory Specification
/// (ignoring relative paths).
#[must_use]
pub fn resolve_xdg_state_home() -> PathBuf {
    if let Ok(val) = env::var("XDG_STATE_HOME") {
        let trimmed = val.trim_end_matches('/');
        if trimmed.starts_with('/') {
            return PathBuf::from(trimmed);
        }
    }
    home_dir().join(".local/state")
}

/// Resolves `${XDG_CACHE_HOME:-~/.cache}` per the XDG Base Directory Specification
/// (ignoring relative paths).
#[must_use]
pub fn resolve_xdg_cache_home() -> PathBuf {
    if let Ok(val) = env::var("XDG_CACHE_HOME") {
        let trimmed = val.trim_end_matches('/');
        if trimmed.starts_with('/') {
            return PathBuf::from(trimmed);
        }
    }
    home_dir().join(".cache")
}

/// Returns the deterministic XDG provenance manifest path for a VS Code profile.
#[must_use]
pub fn profile_provenance_path(user_dir: &Path, profile_name: &str) -> PathBuf {
    let key = format!("{}|{}", user_dir.display(), profile_name);
    let hash = sha256_hex(key.as_bytes());
    resolve_xdg_state_home()
        .join("vsp/profiles")
        .join(format!("{hash}.json"))
}

/// Returns the deterministic XDG provenance manifest path for a canonical workspace directory.
#[must_use]
pub fn workspace_provenance_path(ws_canon: &Path) -> PathBuf {
    let hash = sha256_hex(ws_canon.to_string_lossy().as_bytes());
    resolve_xdg_state_home()
        .join("vsp/workspaces")
        .join(format!("{hash}.json"))
}

/// Computes a workspace generation fingerprint combining canonical path and directory inode
/// so recreating a checkout at the same path invalidates stale managed-key state.
#[must_use]
pub fn compute_workspace_fingerprint(ws_canon: &Path) -> String {
    let inode = directory_inode(ws_canon);
    let payload = format!("{}|{inode}", ws_canon.display());
    sha256_hex(payload.as_bytes())
}

#[cfg(unix)]
fn directory_inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).map(|m| m.ino()).unwrap_or(0)
}

#[cfg(not(unix))]
fn directory_inode(_path: &Path) -> u64 {
    0
}

/// Loads a [`ProfileProvenance`] manifest if present and valid JSON; returns `None` when absent.
#[must_use]
pub fn load_profile_provenance(user_dir: &Path, profile_name: &str) -> Option<ProfileProvenance> {
    let path = profile_provenance_path(user_dir, profile_name);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Saves a [`ProfileProvenance`] manifest atomically with mode `0600` (never following symlinks).
///
/// # Errors
///
/// Returns [`VspError::Operational`] or [`VspError::SafetyRefusal`] if serialization or atomic
/// file publishing fails.
pub fn save_profile_provenance(prov: &ProfileProvenance) -> VspResult<PathBuf> {
    let path = profile_provenance_path(Path::new(&prov.user_dir), &prov.profile);
    let json = to_canonical_json_string(prov)?;
    write_file_atomic(&path, json.as_bytes(), 0o600, true)?;
    Ok(path)
}

/// Loads a [`WorkspaceProvenance`] manifest if present, valid JSON, and matching the current
/// workspace generation fingerprint.
#[must_use]
pub fn load_workspace_provenance(ws_canon: &Path) -> Option<WorkspaceProvenance> {
    let path = workspace_provenance_path(ws_canon);
    let raw = fs::read_to_string(path).ok()?;
    let prov: WorkspaceProvenance = serde_json::from_str(&raw).ok()?;
    let expected_fp = compute_workspace_fingerprint(ws_canon);
    if prov.workspace_fingerprint == expected_fp {
        Some(prov)
    } else {
        None
    }
}

/// Saves a [`WorkspaceProvenance`] manifest atomically with mode `0600` (never following symlinks).
///
/// # Errors
///
/// Returns [`VspError::Operational`] or [`VspError::SafetyRefusal`] if serialization or atomic
/// file publishing fails.
pub fn save_workspace_provenance(prov: &WorkspaceProvenance) -> VspResult<PathBuf> {
    let path = workspace_provenance_path(Path::new(&prov.workspace));
    let json = to_canonical_json_string(prov)?;
    write_file_atomic(&path, json.as_bytes(), 0o600, true)?;
    Ok(path)
}

/// Formats the current system wall clock as an RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`).
#[must_use]
pub fn now_rfc3339_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_secs_rfc3339(secs)
}

/// Formats a compact UTC backup timestamp (`YYYYMMDDTHHMMSSZ`).
#[must_use]
pub fn now_compact_utc() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (y, m, d, hh, mm, ss) = civil_from_unix_secs(dur.as_secs());
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z.{:09}.{seq}",
        dur.subsec_nanos()
    )
}

/// Formats a Unix epoch second timestamp as RFC 3339 UTC (`YYYY-MM-DDTHH:MM:SSZ`).
#[must_use]
pub fn format_unix_secs_rfc3339(secs: u64) -> String {
    let (y, m, d, hh, mm, ss) = civil_from_unix_secs(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Howard Hinnant's proleptic Gregorian civil-from-days algorithm (zero external deps).
fn civil_from_unix_secs(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hh = (rem / 3600) as u32;
    let mm = ((rem % 3600) / 60) as u32;
    let ss = (rem % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32, hh, mm, ss)
}

/// Creates a bounded timestamped `0600` backup of `target` (if it exists as a regular file)
/// and prunes older `<name>.bak.*` files, retaining the newest `keep` backups.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if copying or setting permissions fails.
pub fn backup_file_bounded(target: &Path, keep: usize) -> VspResult<Option<PathBuf>> {
    if !target.is_file() {
        return Ok(None);
    }
    let parent = target.parent().ok_or_else(|| {
        VspError::Operational(format!(
            "cannot determine parent directory for backup of {}",
            target.display()
        ))
    })?;
    let base = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            VspError::Operational(format!("non-UTF-8 filename: {}", target.display()))
        })?;

    let stamp = now_compact_utc();
    let pid = std::process::id();
    let backup_path = parent.join(format!("{base}.bak.{stamp}.{pid}"));

    let data = fs::read(target).map_err(|err| {
        VspError::Operational(format!(
            "failed to read {} for backup: {err}",
            target.display()
        ))
    })?;
    write_file_atomic(&backup_path, &data, 0o600, true)?;
    prune_backups(target, keep)?;
    Ok(Some(backup_path))
}

/// Removes older `<name>.bak.*` backups alongside `target`, retaining the newest `keep` files.
///
/// # Errors
///
/// Returns [`VspError::Operational`] if the backup directory cannot be read.
pub fn prune_backups(target: &Path, keep: usize) -> VspResult<()> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    let Some(base) = target.file_name().and_then(|n| n.to_str()) else {
        return Ok(());
    };
    if !parent.is_dir() {
        return Ok(());
    }
    let prefix = format!("{base}.bak.");
    let mut backups: Vec<PathBuf> = fs::read_dir(parent)
        .map_err(|err| {
            VspError::Operational(format!("failed to scan backups in {}: {err}", parent.display()))
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
        })
        .collect();
    backups.sort();
    if backups.len() <= keep {
        return Ok(());
    }
    let drop_count = backups.len() - keep;
    for old in backups.into_iter().take(drop_count) {
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// Atomically writes `bytes` to `dest` via a sibling temporary file and `rename`.
///
/// - When `no_follow` is `true`, refuses to write through a symlink at `dest` (exit `5`).
/// - Sets file permissions to `mode` (e.g. `0o600` or `0o644`) on Unix before renaming.
/// - Cleans up temporary staging files on any error.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] if `no_follow` is set and `dest` is a symlink,
/// or [`VspError::Operational`] on filesystem errors.
pub fn write_file_atomic(dest: &Path, bytes: &[u8], mode: u32, no_follow: bool) -> VspResult<()> {
    if no_follow && dest.is_symlink() {
        return Err(VspError::SafetyRefusal(format!(
            "refusing to write through symlink: {}",
            dest.display()
        )));
    }

    let write_path = if !no_follow && dest.is_symlink() {
        resolve_canonical_path(dest)?
    } else {
        dest.to_path_buf()
    };

    let dir = write_path.parent().ok_or_else(|| {
        VspError::Operational(format!(
            "cannot determine parent directory for {}",
            write_path.display()
        ))
    })?;
    fs::create_dir_all(dir).map_err(|err| {
        VspError::Operational(format!("failed to create directory {}: {err}", dir.display()))
    })?;

    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tmp_path = dir.join(format!(".vsp.tmp.{pid}.{seq}"));

    let write_res = (|| -> std::io::Result<()> {
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(mode);
        }
        let mut file = opts.open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(mode))?;
        }
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(VspError::Operational(format!(
            "failed to stage atomic write for {}: {err}",
            write_path.display()
        )));
    }

    if let Err(err) = fs::rename(&tmp_path, &write_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(VspError::Operational(format!(
            "failed to publish atomic write to {}: {err}",
            write_path.display()
        )));
    }

    Ok(())
}

/// Resolves a path canonically even when trailing segments do not exist yet, following
/// existing and dangling symlinks with a 40-hop loop guard and collapsing lexical `.` / `..`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if a symlink loop exceeds 40 hops or
/// resolution fails.
pub fn resolve_canonical_path(path: &Path) -> VspResult<PathBuf> {
    resolve_canonical_inner(path, 0)
}

fn resolve_canonical_inner(path: &Path, hops: usize) -> VspResult<PathBuf> {
    if hops > MAX_SYMLINK_HOPS {
        return Err(VspError::SafetyRefusal(format!(
            "symlink loop detected (exceeded {MAX_SYMLINK_HOPS} hops): {}",
            path.display()
        )));
    }

    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let cwd = env::current_dir().map_err(|err| {
            VspError::Operational(format!("cannot read current directory: {err}"))
        })?;
        cwd.join(path)
    };

    // Walk components incrementally so intermediate symlinks (including dangling ones)
    // and lexical `..` segments are resolved in strict POSIX order.
    let mut current = PathBuf::new();
    let comps: Vec<Component<'_>> = abs.components().collect();
    let mut idx = 0usize;
    let mut cur_hops = hops;

    while idx < comps.len() {
        match comps[idx] {
            Component::RootDir => {
                current.push(Path::new("/"));
            }
            Component::Prefix(p) => {
                current.push(p.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(part) => {
                current.push(part);
                if let Ok(meta) = fs::symlink_metadata(&current) {
                    if meta.file_type().is_symlink() {
                        cur_hops += 1;
                        if cur_hops > MAX_SYMLINK_HOPS {
                            return Err(VspError::SafetyRefusal(format!(
                                "symlink loop detected (exceeded {MAX_SYMLINK_HOPS} hops): {}",
                                path.display()
                            )));
                        }
                        let link_target = fs::read_link(&current).map_err(|err| {
                            VspError::SafetyRefusal(format!(
                                "failed to read symlink {}: {err}",
                                current.display()
                            ))
                        })?;
                        let base_dir = current.parent().unwrap_or_else(|| Path::new("/"));
                        let mut next_path = if link_target.is_absolute() {
                            link_target
                        } else {
                            base_dir.join(link_target)
                        };
                        for rem in &comps[idx + 1..] {
                            next_path.push(rem.as_os_str());
                        }
                        return resolve_canonical_inner(&next_path, cur_hops);
                    }
                }
            }
        }
        idx += 1;
    }

    Ok(current)
}

/// Verifies that `target` (after resolving all existing/dangling symlinks and `..`) remains
/// strictly contained within `root_canon`.
///
/// # Errors
///
/// Returns [`VspError::SafetyRefusal`] (exit `5`) if `target` escapes `root_canon` or hits a
/// symlink loop.
pub fn check_path_contained(root_canon: &Path, target: &Path) -> VspResult<PathBuf> {
    let root_resolved = resolve_canonical_path(root_canon)?;
    let target_resolved = resolve_canonical_path(target)?;

    if target_resolved == root_resolved || target_resolved.starts_with(&root_resolved) {
        Ok(target_resolved)
    } else {
        Err(VspError::SafetyRefusal(format!(
            "path escapes boundary '{}': {} -> {}",
            root_resolved.display(),
            target.display(),
            target_resolved.display()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn backup_retention_keeps_only_newest_five_with_0600_mode() {
        let tmp = TempDir::new().expect("tmp");
        let target = tmp.path().join("settings.json");
        fs::write(&target, "{\"a\": 1}\n").expect("write");

        for i in 0..8 {
            fs::write(&target, format!("{{\"a\": {i}}}\n")).expect("update");
            backup_file_bounded(&target, 5).expect("backup");
        }

        let backups: Vec<_> = fs::read_dir(tmp.path())
            .expect("ls")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("settings.json.bak."))
            })
            .collect();
        assert_eq!(backups.len(), 5);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for b in backups {
                let mode = fs::metadata(&b).expect("meta").permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "backup {} must be 0600", b.display());
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn check_path_contained_blocks_symlink_escape_with_exit_5() {
        let outer = TempDir::new().expect("outer");
        let root = outer.path().join("workspace");
        let outside = outer.path().join("outside");
        fs::create_dir_all(&root).expect("mkdir root");
        fs::create_dir_all(&outside).expect("mkdir outside");

        let escape_link = root.join(".vscode");
        std::os::unix::fs::symlink(&outside, &escape_link).expect("symlink");

        let err = check_path_contained(&root, &root.join(".vscode/settings.json"))
            .expect_err("must reject symlink escape");
        assert_eq!(err.exit_code().as_i32(), 5);
    }

    #[test]
    fn rfc3339_timestamp_formats_known_epoch() {
        assert_eq!(format_unix_secs_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_unix_secs_rfc3339(1_700_000_000),
            "2023-11-14T22:13:20Z"
        );
    }
}

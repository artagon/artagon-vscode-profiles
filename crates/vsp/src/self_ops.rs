//! `vsp self {path, install, uninstall, update}` — management of the `vsp` installation.
//!
//! `install` and `uninstall` change symbolic links only. They never replace or delete a
//! regular file or a directory, and `update` never uses the network.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::exit_codes::{VspError, VspResult};
use crate::vscode::home_dir;

/// Name of the primary command link created by `vsp self install`.
pub const VSP_LINK_NAME: &str = "vsp";

/// Name of the optional legacy compatibility link created by `--with-vspcli`.
pub const VSPCLI_LINK_NAME: &str = "vspcli";

/// Homebrew command that upgrades a formula-managed installation.
pub const HOMEBREW_UPGRADE_COMMAND: &str = "brew upgrade artagon/aitools/vsp";

/// GitHub Releases page with signed release archives.
pub const RELEASES_URL: &str = "https://github.com/trumpyla/artagon-vscode-profiles/releases";

/// What `vsp self install` or `vsp self uninstall` did (or would do) to one link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkAction {
    /// A new symbolic link was created.
    Created,
    /// A symbolic link with a different target was replaced.
    Replaced,
    /// The symbolic link already had the correct target.
    Unchanged,
    /// The symbolic link was removed.
    Removed,
    /// No entry existed, so nothing was removed.
    Absent,
    /// A symbolic link that this installation does not own was left in place.
    Skipped,
}

/// Result for one link path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkOutcome {
    /// Absolute path of the link.
    pub link: String,
    /// Action that was done, or that a dry run would do.
    pub action: LinkAction,
}

/// Report returned by [`install`] and [`uninstall`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelfLinkReport {
    /// `true` when no filesystem change was made.
    pub dry_run: bool,
    /// Directory that contains the links.
    pub bin_dir: String,
    /// Canonical executable that the links point to (install only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// One entry for each examined link.
    pub links: Vec<LinkOutcome>,
}

/// Upgrade guidance returned by [`update_guidance`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateGuidance {
    /// Version of the running binary.
    pub current_version: String,
    /// Canonical path of the running binary, when it can be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Homebrew upgrade command.
    pub homebrew: String,
    /// GitHub Releases URL.
    pub releases_url: String,
    /// Always `false`: `vsp self update` does not use the network.
    pub network_used: bool,
}

/// Returns the canonical path of the running `vsp` executable.
///
/// # Errors
///
/// Returns [`VspError::Operational`] (exit `1`) when the operating system cannot report the
/// executable path or the path cannot be resolved.
pub fn current_executable() -> VspResult<PathBuf> {
    let exe = env::current_exe().map_err(|err| {
        VspError::Operational(format!("cannot determine current executable path: {err}"))
    })?;
    fs::canonicalize(&exe).map_err(|err| {
        VspError::Operational(format!(
            "cannot resolve executable path {}: {err}",
            exe.display()
        ))
    })
}

/// Returns the default link directory (`${XDG_BIN_HOME:-~/.local/bin}`).
///
/// A relative `XDG_BIN_HOME` value is ignored.
#[must_use]
pub fn default_bin_dir() -> PathBuf {
    if let Ok(val) = env::var("XDG_BIN_HOME") {
        let trimmed = val.trim_end_matches('/');
        if trimmed.starts_with('/') {
            return PathBuf::from(trimmed);
        }
    }
    home_dir().join(".local/bin")
}

/// Creates `<bin_dir>/vsp` (and `<bin_dir>/vspcli` when `with_vspcli` is set) as symbolic
/// links to the running executable.
///
/// An existing symbolic link is replaced atomically. With `dry_run`, the function reports
/// the planned actions and changes nothing.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): a link path exists and is not a symbolic link.
///   The function examines all link paths before it changes any of them.
/// - [`VspError::Operational`] (exit `1`): filesystem failure, or a platform without
///   symbolic link support.
pub fn install(
    bin_dir: Option<&Path>,
    with_vspcli: bool,
    dry_run: bool,
) -> VspResult<SelfLinkReport> {
    let target = current_executable()?;
    install_links(&target, &resolve_bin_dir(bin_dir)?, with_vspcli, dry_run)
}

/// Removes the `<bin_dir>/vsp` symbolic link.
///
/// A `<bin_dir>/vspcli` symbolic link is also removed when it has the same link target as
/// the removed `vsp` link. A `vspcli` link with a different target is left in place.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): `<bin_dir>/vsp` exists and is not a symbolic
///   link.
/// - [`VspError::Operational`] (exit `1`): filesystem failure.
pub fn uninstall(bin_dir: Option<&Path>, dry_run: bool) -> VspResult<SelfLinkReport> {
    let dir = resolve_bin_dir(bin_dir)?;
    let vsp_link = dir.join(VSP_LINK_NAME);
    let vspcli_link = dir.join(VSPCLI_LINK_NAME);
    let mut links = Vec::new();

    let removed_target = match link_state(&vsp_link)? {
        LinkState::Absent => {
            links.push(outcome(&vsp_link, LinkAction::Absent));
            None
        }
        LinkState::Other => return Err(refuse_non_symlink(&vsp_link, "remove")),
        LinkState::Symlink(link_target) => {
            if !dry_run {
                remove_link(&vsp_link)?;
            }
            links.push(outcome(&vsp_link, LinkAction::Removed));
            Some(link_target)
        }
    };

    // Remove the compatibility link only when it is clearly the companion of the `vsp` link.
    if let LinkState::Symlink(cli_target) = link_state(&vspcli_link)? {
        if removed_target.as_ref() == Some(&cli_target) {
            if !dry_run {
                remove_link(&vspcli_link)?;
            }
            links.push(outcome(&vspcli_link, LinkAction::Removed));
        } else {
            links.push(outcome(&vspcli_link, LinkAction::Skipped));
        }
    }

    Ok(SelfLinkReport {
        dry_run,
        bin_dir: dir.display().to_string(),
        target: None,
        links,
    })
}

/// Returns the current version and safe upgrade guidance. This function has no side effects.
#[must_use]
pub fn update_guidance() -> UpdateGuidance {
    UpdateGuidance {
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        executable: current_executable().ok().map(|p| p.display().to_string()),
        homebrew: HOMEBREW_UPGRADE_COMMAND.to_string(),
        releases_url: RELEASES_URL.to_string(),
        network_used: false,
    }
}

/// Formats [`UpdateGuidance`] as human-readable lines.
#[must_use]
pub fn format_update_guidance(guidance: &UpdateGuidance) -> String {
    let mut out = format!("vsp {}\n", guidance.current_version);
    if let Some(exe) = &guidance.executable {
        out.push_str(&format!("Installed at: {exe}\n"));
    }
    out.push_str("vsp does not update itself and this command does not use the network.\n");
    out.push_str("To upgrade:\n");
    out.push_str(&format!("  Homebrew:        {}\n", guidance.homebrew));
    out.push_str(&format!(
        "  GitHub Release:  download the archive for your platform from {}\n",
        guidance.releases_url
    ));
    out.push_str(
        "                   verify it against the release SHA256SUMS, replace the binary,\n",
    );
    out.push_str("                   then run 'vsp self install' to refresh the links.\n");
    out
}

enum LinkState {
    Absent,
    Symlink(PathBuf),
    Other,
}

fn resolve_bin_dir(bin_dir: Option<&Path>) -> VspResult<PathBuf> {
    let dir = bin_dir.map_or_else(default_bin_dir, Path::to_path_buf);
    if dir.as_os_str().is_empty() {
        return Err(VspError::UsageOrConfig(
            "--bin-dir must not be empty".to_string(),
        ));
    }
    if dir.is_absolute() {
        return Ok(dir);
    }
    let cwd = env::current_dir().map_err(|err| {
        VspError::Operational(format!("cannot determine current directory: {err}"))
    })?;
    Ok(cwd.join(dir))
}

fn link_state(path: &Path) -> VspResult<LinkState> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let target = fs::read_link(path).map_err(|err| {
                VspError::Operational(format!("cannot read link {}: {err}", path.display()))
            })?;
            Ok(LinkState::Symlink(target))
        }
        Ok(_) => Ok(LinkState::Other),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(LinkState::Absent),
        Err(err) => Err(VspError::Operational(format!(
            "cannot examine {}: {err}",
            path.display()
        ))),
    }
}

fn refuse_non_symlink(path: &Path, verb: &str) -> VspError {
    VspError::SafetyRefusal(format!(
        "refusing to {verb} {}: it exists and is not a symbolic link",
        path.display()
    ))
}

fn outcome(path: &Path, action: LinkAction) -> LinkOutcome {
    LinkOutcome {
        link: path.display().to_string(),
        action,
    }
}

fn remove_link(path: &Path) -> VspResult<()> {
    fs::remove_file(path).map_err(|err| {
        VspError::Operational(format!("failed to remove link {}: {err}", path.display()))
    })
}

fn install_links(
    target: &Path,
    dir: &Path,
    with_vspcli: bool,
    dry_run: bool,
) -> VspResult<SelfLinkReport> {
    let mut names = vec![VSP_LINK_NAME];
    if with_vspcli {
        names.push(VSPCLI_LINK_NAME);
    }

    // Examine every link path first so a refusal leaves the directory unchanged.
    let mut planned: Vec<(PathBuf, LinkAction)> = Vec::with_capacity(names.len());
    for name in names {
        let link = dir.join(name);
        let action = match link_state(&link)? {
            LinkState::Absent => LinkAction::Created,
            LinkState::Symlink(existing) if existing == target => LinkAction::Unchanged,
            LinkState::Symlink(_) => LinkAction::Replaced,
            LinkState::Other => return Err(refuse_non_symlink(&link, "replace")),
        };
        planned.push((link, action));
    }

    if !dry_run {
        fs::create_dir_all(dir).map_err(|err| {
            VspError::Operational(format!(
                "failed to create directory {}: {err}",
                dir.display()
            ))
        })?;
        for (link, action) in &planned {
            if *action != LinkAction::Unchanged {
                place_symlink(target, link)?;
            }
        }
    }

    Ok(SelfLinkReport {
        dry_run,
        bin_dir: dir.display().to_string(),
        target: Some(target.display().to_string()),
        links: planned
            .iter()
            .map(|(link, action)| outcome(link, *action))
            .collect(),
    })
}

/// Creates or atomically replaces the symbolic link `link` so that it points to `target`.
#[cfg(unix)]
fn place_symlink(target: &Path, link: &Path) -> VspResult<()> {
    use std::os::unix::fs::symlink;

    let dir = link.parent().ok_or_else(|| {
        VspError::Operational(format!(
            "cannot determine parent directory for {}",
            link.display()
        ))
    })?;
    let name = link
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = dir.join(format!(".{name}.vsp-link.{}", std::process::id()));
    // A leftover staging link from an interrupted run is ours to replace.
    if let Ok(meta) = fs::symlink_metadata(&staging) {
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&staging);
        }
    }
    symlink(target, &staging).map_err(|err| {
        VspError::Operational(format!(
            "failed to create link {}: {err}",
            staging.display()
        ))
    })?;

    // Re-check immediately before the rename: never replace a non-symlink entry.
    let recheck = match link_state(link) {
        Ok(LinkState::Other) => Err(refuse_non_symlink(link, "replace")),
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    };
    if let Err(err) = recheck {
        let _ = fs::remove_file(&staging);
        return Err(err);
    }
    if let Err(err) = fs::rename(&staging, link) {
        let _ = fs::remove_file(&staging);
        return Err(VspError::Operational(format!(
            "failed to publish link {}: {err}",
            link.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn place_symlink(_target: &Path, link: &Path) -> VspResult<()> {
    Err(VspError::Operational(format!(
        "'vsp self install' cannot create the symbolic link {} on this platform",
        link.display()
    )))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::exit_codes::ExitCode;

    fn fake_target(dir: &Path) -> PathBuf {
        let target = dir.join("real-vsp");
        fs::write(&target, b"#!/bin/sh\n").expect("write target");
        target
    }

    #[test]
    fn install_creates_replaces_and_keeps_links() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let target = fake_target(tmp.path());
        let bin = tmp.path().join("bin");

        let first = install_links(&target, &bin, true, false).expect("install");
        assert_eq!(first.links.len(), 2);
        assert!(first.links.iter().all(|l| l.action == LinkAction::Created));
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), target);
        assert_eq!(fs::read_link(bin.join("vspcli")).expect("link"), target);

        let second = install_links(&target, &bin, true, false).expect("reinstall");
        assert!(second
            .links
            .iter()
            .all(|l| l.action == LinkAction::Unchanged));

        let other = tmp.path().join("other-vsp");
        fs::write(&other, b"x").expect("write other");
        let third = install_links(&other, &bin, false, false).expect("replace");
        assert_eq!(third.links[0].action, LinkAction::Replaced);
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), other);
    }

    #[test]
    fn install_dry_run_changes_nothing() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let target = fake_target(tmp.path());
        let bin = tmp.path().join("bin");
        let report = install_links(&target, &bin, true, true).expect("dry run");
        assert!(report.dry_run);
        assert!(!bin.exists());
    }

    #[test]
    fn install_refuses_regular_file_and_leaves_directory_unchanged() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let target = fake_target(tmp.path());
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).expect("mkdir");
        fs::write(bin.join("vspcli"), b"user file").expect("write");

        let err = install_links(&target, &bin, true, false).expect_err("must refuse");
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
        assert!(!bin.join("vsp").exists(), "no partial install");
        assert_eq!(fs::read(bin.join("vspcli")).expect("read"), b"user file");
    }

    #[test]
    fn uninstall_removes_links_and_refuses_regular_file() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let target = fake_target(tmp.path());
        let bin = tmp.path().join("bin");
        install_links(&target, &bin, true, false).expect("install");

        let report = uninstall(Some(&bin), false).expect("uninstall");
        assert_eq!(report.links.len(), 2);
        assert!(report.links.iter().all(|l| l.action == LinkAction::Removed));
        assert!(fs::symlink_metadata(bin.join("vsp")).is_err());
        assert!(fs::symlink_metadata(bin.join("vspcli")).is_err());

        let again = uninstall(Some(&bin), false).expect("idempotent uninstall");
        assert_eq!(again.links[0].action, LinkAction::Absent);

        fs::write(bin.join("vsp"), b"user file").expect("write");
        let err = uninstall(Some(&bin), false).expect_err("must refuse");
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
        assert_eq!(fs::read(bin.join("vsp")).expect("read"), b"user file");
    }

    #[test]
    fn uninstall_keeps_foreign_vspcli_link() {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let target = fake_target(tmp.path());
        let bin = tmp.path().join("bin");
        install_links(&target, &bin, false, false).expect("install");
        let foreign = tmp.path().join("legacy-vspcli");
        fs::write(&foreign, b"x").expect("write");
        std::os::unix::fs::symlink(&foreign, bin.join("vspcli")).expect("foreign link");

        let report = uninstall(Some(&bin), false).expect("uninstall");
        assert_eq!(report.links[1].action, LinkAction::Skipped);
        assert_eq!(fs::read_link(bin.join("vspcli")).expect("link"), foreign);
    }

    #[test]
    fn update_guidance_reports_version_without_network() {
        let guidance = update_guidance();
        assert_eq!(guidance.current_version, env!("CARGO_PKG_VERSION"));
        assert!(!guidance.network_used);
        let text = format_update_guidance(&guidance);
        assert!(text.contains(HOMEBREW_UPGRADE_COMMAND));
        assert!(text.contains(RELEASES_URL));
    }
}

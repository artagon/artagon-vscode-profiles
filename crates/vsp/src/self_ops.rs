//! `vsp self {path, install, uninstall, update}` — management of the `vsp` installation.
//!
//! `install` and `uninstall` change symbolic links only. They never replace or delete a
//! regular file or a directory, and `update` never uses the network.
//!
//! # Link ownership
//!
//! A link is owned by this installation when it resolves to the running executable, or
//! when its text is the install target. `install` and `uninstall` change an owned link
//! only. A link that some other tool made (for example a Homebrew link, or a link to a
//! different binary) and a dangling link are foreign: `install` refuses to replace one
//! (exit `5`) and `uninstall` keeps it, unless `--force` is given.
//!
//! # Stable link target
//!
//! A Homebrew executable has the versioned path `<prefix>/Cellar/vsp/<version>/bin/vsp`,
//! which `brew upgrade` and `brew cleanup` delete. For such an executable the link target
//! is the stable path `<prefix>/opt/vsp/bin/vsp`, so the links stay valid after an upgrade.

use std::env;
use std::fmt::Write as _;
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
    /// Executable path that the links point to (install only). This is the canonical path
    /// of the running executable, or its stable package-manager path when one exists
    /// (see [`stable_executable_path`]).
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

/// Returns the stable path of the executable `canonical_exe`, for use as a link target.
///
/// For a Homebrew executable `<prefix>/Cellar/<formula>/<version>/<tail>`, the result is
/// `<prefix>/opt/<formula>/<tail>` when that path resolves to `canonical_exe`. Homebrew
/// moves the `opt/<formula>` link on each upgrade, so the path stays valid. For each other
/// executable the result is `canonical_exe`.
#[must_use]
pub fn stable_executable_path(canonical_exe: &Path) -> PathBuf {
    homebrew_opt_path(canonical_exe)
        .filter(|candidate| {
            fs::canonicalize(candidate).is_ok_and(|resolved| resolved == canonical_exe)
        })
        .unwrap_or_else(|| canonical_exe.to_path_buf())
}

/// Maps `<prefix>/Cellar/<formula>/<version>/<tail>` to `<prefix>/opt/<formula>/<tail>`.
fn homebrew_opt_path(canonical_exe: &Path) -> Option<PathBuf> {
    let components: Vec<_> = canonical_exe.components().collect();
    let cellar = components.iter().rposition(|c| c.as_os_str() == "Cellar")?;
    // After `Cellar`: the formula, the version, and at least one tail component.
    let formula = components.get(cellar + 1)?;
    let tail = components.get(cellar + 3..).filter(|t| !t.is_empty())?;
    let mut out: PathBuf = components[..cellar].iter().collect();
    out.push("opt");
    out.push(formula);
    out.extend(tail);
    Some(out)
}

/// Creates `<bin_dir>/vsp` (and `<bin_dir>/vspcli` when `with_vspcli` is set) as symbolic
/// links to the running executable (see [`stable_executable_path`] for the link target).
///
/// An existing link that this installation owns is replaced atomically. With `dry_run`,
/// the function reports the planned actions and changes nothing.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): a link path exists and is not a symbolic link,
///   or it is a symbolic link that this installation does not own and `force` is `false`.
///   The function examines all link paths before it changes any of them. `force` never
///   permits the replacement of a regular file or a directory.
/// - [`VspError::Operational`] (exit `1`): filesystem failure, or a platform without
///   symbolic link support.
pub fn install(
    bin_dir: Option<&Path>,
    with_vspcli: bool,
    force: bool,
    dry_run: bool,
) -> VspResult<SelfLinkReport> {
    let owner = current_executable()?;
    let plan = LinkPlan {
        target: stable_executable_path(&owner),
        owner,
        force,
        dry_run,
    };
    install_links(&plan, &resolve_bin_dir(bin_dir)?, with_vspcli)
}

/// Removes the `<bin_dir>/vsp` and `<bin_dir>/vspcli` symbolic links that this installation
/// owns.
///
/// A link that this installation does not own is kept ([`LinkAction::Skipped`]), unless
/// `force` is `true`. A `vspcli` link with the same link text as the removed `vsp` link is
/// its companion, and it is removed too.
///
/// # Errors
///
/// - [`VspError::SafetyRefusal`] (exit `5`): `<bin_dir>/vsp` exists and is not a symbolic
///   link. `force` never permits the removal of a regular file or a directory.
/// - [`VspError::Operational`] (exit `1`): filesystem failure.
pub fn uninstall(bin_dir: Option<&Path>, force: bool, dry_run: bool) -> VspResult<SelfLinkReport> {
    let owner = current_executable()?;
    let plan = LinkPlan {
        target: stable_executable_path(&owner),
        owner,
        force,
        dry_run,
    };
    uninstall_links(&plan, &resolve_bin_dir(bin_dir)?)
}

/// Inputs that are common to the link operations.
struct LinkPlan {
    /// Canonical path of the running executable. A link that resolves to it is owned.
    owner: PathBuf,
    /// Path that new links point to.
    target: PathBuf,
    /// Also change links that this installation does not own.
    force: bool,
    /// Report only; change nothing.
    dry_run: bool,
}

impl LinkPlan {
    /// Returns `true` when the symbolic link `link` with link text `link_text` belongs to
    /// this installation. A dangling link resolves to nothing, so it is not owned.
    fn owns(&self, link: &Path, link_text: &Path) -> bool {
        link_text == self.target
            || fs::canonicalize(link).is_ok_and(|resolved| resolved == self.owner)
    }
}

fn uninstall_links(plan: &LinkPlan, dir: &Path) -> VspResult<SelfLinkReport> {
    let vsp_link = dir.join(VSP_LINK_NAME);
    let vspcli_link = dir.join(VSPCLI_LINK_NAME);
    let mut links = Vec::new();

    let removed_text = match link_state(&vsp_link)? {
        LinkState::Absent => {
            links.push(outcome(&vsp_link, LinkAction::Absent));
            None
        }
        LinkState::Other => return Err(refuse_non_symlink(&vsp_link, "remove")),
        LinkState::Symlink(link_text) if plan.force || plan.owns(&vsp_link, &link_text) => {
            // The companion check below resolves `vspcli` before `vsp` is removed.
            Some(link_text)
        }
        LinkState::Symlink(_) => {
            links.push(outcome(&vsp_link, LinkAction::Skipped));
            None
        }
    };

    // Remove the compatibility link when it is ours, or the companion of the `vsp` link.
    let vspcli_action = match link_state(&vspcli_link)? {
        LinkState::Symlink(cli_text)
            if plan.force
                || removed_text.as_ref() == Some(&cli_text)
                || plan.owns(&vspcli_link, &cli_text) =>
        {
            Some(LinkAction::Removed)
        }
        LinkState::Symlink(_) => Some(LinkAction::Skipped),
        LinkState::Absent | LinkState::Other => None,
    };

    if removed_text.is_some() {
        if !plan.dry_run {
            remove_link(&vsp_link)?;
        }
        links.push(outcome(&vsp_link, LinkAction::Removed));
    }
    if let Some(action) = vspcli_action {
        if action == LinkAction::Removed && !plan.dry_run {
            remove_link(&vspcli_link)?;
        }
        links.push(outcome(&vspcli_link, action));
    }

    Ok(SelfLinkReport {
        dry_run: plan.dry_run,
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
    // A write to a `String` cannot fail.
    let mut out = format!("vsp {}\n", guidance.current_version);
    if let Some(exe) = &guidance.executable {
        let _ = writeln!(out, "Installed at: {exe}");
    }
    out.push_str("vsp does not update itself and this command does not use the network.\n");
    out.push_str("To upgrade:\n");
    let _ = writeln!(out, "  Homebrew:        {}", guidance.homebrew);
    let _ = writeln!(
        out,
        "  GitHub Release:  download the archive for your platform from {}",
        guidance.releases_url
    );
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

fn refuse_foreign_link(path: &Path, link_text: &Path) -> VspError {
    VspError::SafetyRefusal(format!(
        "refusing to replace {}: it is a symbolic link to {}, which is not this vsp installation (pass --force to replace it)",
        path.display(),
        link_text.display()
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

fn install_links(plan: &LinkPlan, dir: &Path, with_vspcli: bool) -> VspResult<SelfLinkReport> {
    let target = plan.target.as_path();
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
            LinkState::Symlink(existing) if plan.force || plan.owns(&link, &existing) => {
                LinkAction::Replaced
            }
            LinkState::Symlink(existing) => return Err(refuse_foreign_link(&link, &existing)),
            LinkState::Other => return Err(refuse_non_symlink(&link, "replace")),
        };
        planned.push((link, action));
    }

    if !plan.dry_run {
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
        dry_run: plan.dry_run,
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

    use std::os::unix::fs::symlink;

    /// A canonical temporary directory (on macOS, `/var` is a link to `/private/var`).
    fn canonical_tmp() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::TempDir::new().expect("tmp");
        let root = fs::canonicalize(tmp.path()).expect("canonical tmp");
        (tmp, root)
    }

    fn fake_target(dir: &Path) -> PathBuf {
        let target = dir.join("real-vsp");
        fs::write(&target, b"#!/bin/sh\n").expect("write target");
        target
    }

    /// A plan for an installation whose executable (and link target) is `target`.
    fn plan_for(target: &Path, force: bool, dry_run: bool) -> LinkPlan {
        LinkPlan {
            owner: fs::canonicalize(target).expect("canonical target"),
            target: target.to_path_buf(),
            force,
            dry_run,
        }
    }

    #[test]
    fn install_creates_and_keeps_links() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        let plan = plan_for(&target, false, false);

        let first = install_links(&plan, &bin, true).expect("install");
        assert_eq!(first.links.len(), 2);
        assert!(first.links.iter().all(|l| l.action == LinkAction::Created));
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), target);
        assert_eq!(fs::read_link(bin.join("vspcli")).expect("link"), target);

        let second = install_links(&plan, &bin, true).expect("reinstall");
        assert!(second
            .links
            .iter()
            .all(|l| l.action == LinkAction::Unchanged));
    }

    /// R2-L1: `install` replaces a link that resolves to this executable, and refuses a
    /// foreign or dangling link unless `force` is set.
    #[test]
    fn install_replaces_owned_link_and_refuses_foreign_link_without_force() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("mkdir");

        // Owned: a different link text that resolves to the same executable.
        let alias = root.join("alias-vsp");
        symlink(&target, &alias).expect("alias");
        symlink(&alias, bin.join("vsp")).expect("owned link");
        let owned = install_links(&plan_for(&target, false, false), &bin, false).expect("owned");
        assert_eq!(owned.links[0].action, LinkAction::Replaced);
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), target);

        // Foreign: the link of another tool (for example a Homebrew link).
        let foreign = root.join("other-tool-vsp");
        fs::write(&foreign, b"x").expect("write foreign");
        fs::remove_file(bin.join("vsp")).expect("rm");
        symlink(&foreign, bin.join("vsp")).expect("foreign link");
        let err = install_links(&plan_for(&target, false, false), &bin, true)
            .expect_err("foreign link must be refused");
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), foreign);
        assert!(
            fs::symlink_metadata(bin.join("vspcli")).is_err(),
            "no partial install"
        );

        // Dangling: ownership cannot be shown, so it is foreign too.
        fs::remove_file(bin.join("vsp")).expect("rm");
        symlink(root.join("gone"), bin.join("vsp")).expect("dangling link");
        let err = install_links(&plan_for(&target, false, false), &bin, false)
            .expect_err("dangling link must be refused");
        assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);

        // `force` replaces a foreign link, and a dry run with `force` changes nothing.
        let preview = install_links(&plan_for(&target, true, true), &bin, false).expect("preview");
        assert_eq!(preview.links[0].action, LinkAction::Replaced);
        assert_eq!(
            fs::read_link(bin.join("vsp")).expect("link"),
            root.join("gone")
        );
        let forced = install_links(&plan_for(&target, true, false), &bin, false).expect("forced");
        assert_eq!(forced.links[0].action, LinkAction::Replaced);
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), target);
    }

    #[test]
    fn install_dry_run_changes_nothing() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        let report = install_links(&plan_for(&target, false, true), &bin, true).expect("dry run");
        assert!(report.dry_run);
        assert!(!bin.exists());
    }

    #[test]
    fn install_refuses_regular_file_and_leaves_directory_unchanged() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("mkdir");
        fs::write(bin.join("vspcli"), b"user file").expect("write");

        // `force` never permits the replacement of a regular file.
        for force in [false, true] {
            let err = install_links(&plan_for(&target, force, false), &bin, true)
                .expect_err("must refuse");
            assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
            assert!(!bin.join("vsp").exists(), "no partial install");
            assert_eq!(fs::read(bin.join("vspcli")).expect("read"), b"user file");
        }
    }

    #[test]
    fn uninstall_removes_links_and_refuses_regular_file() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        let plan = plan_for(&target, false, false);
        install_links(&plan, &bin, true).expect("install");

        let report = uninstall_links(&plan, &bin).expect("uninstall");
        assert_eq!(report.links.len(), 2);
        assert!(report.links.iter().all(|l| l.action == LinkAction::Removed));
        assert!(fs::symlink_metadata(bin.join("vsp")).is_err());
        assert!(fs::symlink_metadata(bin.join("vspcli")).is_err());

        let again = uninstall_links(&plan, &bin).expect("idempotent uninstall");
        assert_eq!(again.links[0].action, LinkAction::Absent);

        fs::write(bin.join("vsp"), b"user file").expect("write");
        for force in [false, true] {
            let err =
                uninstall_links(&plan_for(&target, force, false), &bin).expect_err("must refuse");
            assert_eq!(err.exit_code(), ExitCode::SafetyRefusal);
            assert_eq!(fs::read(bin.join("vsp")).expect("read"), b"user file");
        }
    }

    #[test]
    fn uninstall_keeps_foreign_vspcli_link() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        let plan = plan_for(&target, false, false);
        install_links(&plan, &bin, false).expect("install");
        let foreign = root.join("legacy-vspcli");
        fs::write(&foreign, b"x").expect("write");
        symlink(&foreign, bin.join("vspcli")).expect("foreign link");

        let report = uninstall_links(&plan, &bin).expect("uninstall");
        assert_eq!(report.links[0].action, LinkAction::Removed);
        assert_eq!(report.links[1].action, LinkAction::Skipped);
        assert_eq!(fs::read_link(bin.join("vspcli")).expect("link"), foreign);
    }

    /// R2-L1: `uninstall` keeps a `vsp` link that another tool owns, unless `force` is set.
    #[test]
    fn uninstall_keeps_foreign_vsp_link_without_force() {
        let (_tmp, root) = canonical_tmp();
        let target = fake_target(&root);
        let bin = root.join("bin");
        fs::create_dir_all(&bin).expect("mkdir");
        let foreign = root.join("homebrew-vsp");
        fs::write(&foreign, b"x").expect("write");
        symlink(&foreign, bin.join("vsp")).expect("foreign vsp");
        symlink(&foreign, bin.join("vspcli")).expect("foreign vspcli");

        let kept = uninstall_links(&plan_for(&target, false, false), &bin).expect("uninstall");
        assert!(kept.links.iter().all(|l| l.action == LinkAction::Skipped));
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), foreign);
        assert_eq!(fs::read_link(bin.join("vspcli")).expect("link"), foreign);

        let preview = uninstall_links(&plan_for(&target, true, true), &bin).expect("preview");
        assert!(preview
            .links
            .iter()
            .all(|l| l.action == LinkAction::Removed));
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), foreign);

        let forced = uninstall_links(&plan_for(&target, true, false), &bin).expect("forced");
        assert!(forced.links.iter().all(|l| l.action == LinkAction::Removed));
        assert!(fs::symlink_metadata(bin.join("vsp")).is_err());
        assert!(fs::symlink_metadata(bin.join("vspcli")).is_err());
    }

    /// R2-L2: a Homebrew Cellar executable is linked through the stable `opt/<formula>`
    /// path. A path that is not in a Cellar, or has no matching `opt` link, stays as it is.
    #[test]
    fn stable_executable_path_prefers_homebrew_opt_link() {
        let (_tmp, root) = canonical_tmp();
        let cellar_bin = root.join("Cellar/vsp/3.0.0/bin");
        fs::create_dir_all(&cellar_bin).expect("mkdir cellar");
        let exe = cellar_bin.join("vsp");
        fs::write(&exe, b"#!/bin/sh\n").expect("write exe");

        // No `opt` link yet: the canonical path is the only safe target.
        assert_eq!(stable_executable_path(&exe), exe);

        fs::create_dir_all(root.join("opt")).expect("mkdir opt");
        symlink(root.join("Cellar/vsp/3.0.0"), root.join("opt/vsp")).expect("opt link");
        let stable = root.join("opt/vsp/bin/vsp");
        assert_eq!(stable_executable_path(&exe), stable);

        // An `opt` link to a different version is not this executable.
        let other_bin = root.join("Cellar/vsp/3.1.0/bin");
        fs::create_dir_all(&other_bin).expect("mkdir other");
        let other_exe = other_bin.join("vsp");
        fs::write(&other_exe, b"#!/bin/sh\n").expect("write other exe");
        assert_eq!(stable_executable_path(&other_exe), other_exe);

        // Not a Cellar path.
        let plain = fake_target(&root);
        assert_eq!(stable_executable_path(&plain), plain);

        // The links made for a Cellar executable use the stable path, and the installation
        // owns them after the version directory moves.
        let bin = root.join("bin");
        let plan = LinkPlan {
            owner: exe.clone(),
            target: stable_executable_path(&exe),
            force: false,
            dry_run: false,
        };
        install_links(&plan, &bin, false).expect("install");
        assert_eq!(fs::read_link(bin.join("vsp")).expect("link"), stable);
        let after_upgrade = LinkPlan {
            owner: other_exe,
            target: stable.clone(),
            force: false,
            dry_run: false,
        };
        let again = install_links(&after_upgrade, &bin, false).expect("reinstall after upgrade");
        assert_eq!(again.links[0].action, LinkAction::Unchanged);
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

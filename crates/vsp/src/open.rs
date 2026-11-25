//! `vsp open` (hidden alias `use`) — Single-window `--user-data-dir`-aware VS Code launcher
//! with optional `--sync` pre-convergence.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::exit_codes::{ExitCode, VspError, VspResult};
use crate::model::{AiMode, UxPreset, WorkspaceCatalog};
use crate::resolver::to_canonical_json_string;
use crate::sync::{run_sync, SyncOptions};
use crate::user_config::UserOverlay;
use crate::vscode::{find_code_binary, resolve_vscode_store, validate_profile_name};

/// Input options for [`run_open`].
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Optional workspace path to open.
    pub path: Option<PathBuf>,
    /// Optional named VS Code profile to activate (`-p/--profile`).
    pub profile: Option<String>,
    /// Converge target via [`run_sync`] before launching VS Code; aborts launch on failure.
    pub sync_first: bool,
    /// Preview actions without mutating files or launching VS Code.
    pub dry_run: bool,
    /// Emit machine-readable JSON output.
    pub json: bool,
    /// Suppress non-essential stdout output.
    pub quiet: bool,
    /// Optional `--vscode-data-dir` override.
    pub vscode_data_dir: Option<PathBuf>,
    /// Assume yes for sync confirmations (`-y/--yes`).
    pub yes: bool,
    /// Allow storage mutation when VS Code is running during `--sync`.
    pub force: bool,
    /// Resolved user-profile data, so `--sync` applies `extraExtensions` and last-layer
    /// `overrides` of a custom user profile exactly as `vsp sync -p <name>` does.
    pub user_overlay: UserOverlay,
    /// Effective UX preset for the profile sync (personal `defaults.ux`, if it applies).
    pub ux: Option<UxPreset>,
    /// Effective AI mode for the profile sync (personal `defaults.ai`).
    pub ai: Option<AiMode>,
    /// Extra pack IDs for the profile sync (personal `defaults.rtkTerminal = true`).
    pub with_modules: Vec<String>,
    /// Skip RTK wrapper installation (personal `defaults.rtkTerminal = false`).
    pub no_rtk: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenPreview {
    dry_run: bool,
    profile: Option<String>,
    path: Option<String>,
    sync_first: bool,
    command: Vec<String>,
}

/// Launches VS Code targeting `opts.path` and/or `opts.profile` in a single window while
/// forwarding `--user-data-dir` whenever a custom store is active.
///
/// When `opts.sync_first` is `true`, runs [`run_sync`] before invoking `code` and aborts
/// launch immediately if synchronization fails.
///
/// # Errors
///
/// - [`VspError::UsageOrConfig`] (exit `4`): neither `path` nor `profile` was provided.
/// - [`VspError::TargetMissing`] (exit `2`): `path` does not exist.
/// - [`VspError::UsageOrConfig`] (exit `4`): `VSCODE_USER_DIR` cannot be represented as a
///   `code --user-data-dir` root. This is checked before any sync or launch.
/// - [`VspError::SafetyRefusal`] (exit `5`): unsafe profile name.
/// - Any [`VspError`] from [`run_sync`] when `--sync` fails (launch is aborted).
/// - [`VspError::Operational`] (exit `1`): `code` CLI missing or returned non-zero.
pub fn run_open(catalog: &WorkspaceCatalog, opts: &OpenOptions) -> VspResult<ExitCode> {
    if opts.profile.is_none() && opts.path.is_none() {
        return Err(VspError::UsageOrConfig(
            "vsp open requires -p/--profile <NAME> or <PATH>".to_string(),
        ));
    }

    if let Some(ref p) = opts.path {
        if !p.exists() {
            return Err(VspError::TargetMissing(format!(
                "path does not exist: {}",
                p.display()
            )));
        }
    }

    let (resolved_profile, legacy_ux) = match opts.profile.as_deref() {
        Some(raw) => match catalog.resolve_alias(raw) {
            Some((canon, ux)) => (Some(canon), Some(ux)),
            None => (Some(raw.to_string()), None),
        },
        None => (None, None),
    };

    // Pre-flight, before any sync or launch:
    // - the profile name must be safe for the `code` command line;
    // - the VS Code store must be representable as `--user-data-dir`. A `VSCODE_USER_DIR`
    //   that does not end in `/User` cannot be passed to `code`, and a silent launch against
    //   the default data root would open (and sync) a different store.
    if let Some(name) = &resolved_profile {
        validate_profile_name(name)?;
    }
    let store = resolve_vscode_store(opts.vscode_data_dir.as_deref(), true)?;
    let launch_args = build_code_open_args(
        store.data_root.as_deref(),
        resolved_profile.as_deref(),
        opts.path.as_deref(),
    );

    if opts.dry_run {
        let mut full_cmd = vec!["code".to_string()];
        full_cmd.extend(launch_args);
        if opts.json {
            let preview = OpenPreview {
                dry_run: true,
                profile: resolved_profile,
                path: opts.path.as_ref().map(|p| p.display().to_string()),
                sync_first: opts.sync_first,
                command: full_cmd,
            };
            print!("{}", to_canonical_json_string(&preview)?);
        } else if !opts.quiet {
            println!("DRY-RUN: {}", full_cmd.join(" "));
        }
        return Ok(ExitCode::Ok);
    }

    // Converge target BEFORE launching VS Code when `--sync` is requested.
    if opts.sync_first {
        if let Some(ref ws_path) = opts.path {
            let ws_opts = SyncOptions {
                workspace_path: Some(ws_path.clone()),
                yes: opts.yes,
                force: opts.force,
                vscode_data_dir: opts.vscode_data_dir.clone(),
                ..Default::default()
            };
            let _ = run_sync(catalog, &ws_opts)?;
        }
        if let Some(ref prof_name) = resolved_profile {
            let prof_opts = SyncOptions {
                profile: Some(prof_name.clone()),
                // A legacy alias name selects its own UX preset; otherwise use the caller's.
                ux: legacy_ux.or(opts.ux),
                ai: opts.ai,
                with_modules: opts.with_modules.clone(),
                no_rtk: opts.no_rtk,
                yes: opts.yes,
                force: opts.force,
                vscode_data_dir: opts.vscode_data_dir.clone(),
                user_overlay: opts.user_overlay.clone(),
                ..Default::default()
            };
            let _ = run_sync(catalog, &prof_opts)?;
        }
    }

    let code_bin = find_code_binary()
        .ok_or_else(|| VspError::Operational("VS Code 'code' CLI not found on PATH".to_string()))?;

    let status = Command::new(&code_bin)
        .args(&launch_args)
        .status()
        .map_err(|err| {
            VspError::Operational(format!("failed to launch {}: {err}", code_bin.display()))
        })?;

    if !status.success() {
        let code = status
            .code()
            .map_or_else(|| "signal".to_string(), |c| c.to_string());
        return Err(VspError::Operational(format!(
            "VS Code exited with status {code}"
        )));
    }

    if opts.json {
        let mut full_cmd = vec![code_bin.display().to_string()];
        full_cmd.extend(launch_args);
        let payload = OpenPreview {
            dry_run: false,
            profile: resolved_profile,
            path: opts.path.as_ref().map(|p| p.display().to_string()),
            sync_first: opts.sync_first,
            command: full_cmd,
        };
        print!("{}", to_canonical_json_string(&payload)?);
    }

    Ok(ExitCode::Ok)
}

/// Builds the exact argument vector passed to `code` for single-window opening.
#[must_use]
pub fn build_code_open_args(
    data_root: Option<&Path>,
    profile: Option<&str>,
    workspace_path: Option<&Path>,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(root) = data_root {
        args.push("--user-data-dir".to_string());
        args.push(root.display().to_string());
    }
    if let Some(prof) = profile {
        if prof != "default" {
            // One argument: the name can never be parsed as a separate `code` option.
            args.push(format!("--profile={prof}"));
        }
    }
    if let Some(path) = workspace_path {
        args.push("--".to_string());
        args.push(path.display().to_string());
    }
    args
}

## MODIFIED Requirements

### Requirement: Extension installation

The system SHALL install extensions listed in `extensions.enabled` using
`code --profile <name> --install-extension` and continue even if an
extension install fails. The system SHALL track which extension IDs
failed to install and surface that list to subsequent post-install
processing (the theme-fallback rewrite in "Settings import"), as well as
in the final error report when at least one install failed.

#### Scenario: Extensions list missing

- **WHEN** the bundle has no `extensions.enabled` list
- **THEN** the script skips extension installation.

#### Scenario: Extension install failure

- **WHEN** an extension install fails
- **THEN** the script continues with the remaining extensions, records
  the failed extension ID in an in-memory failure list, and exits
  non-zero at the end of the script with the failure list printed
  to stderr.

#### Scenario: Failure list available to settings rewrite

- **WHEN** the script reaches the post-install settings-rewrite step
- **THEN** the in-memory failure list is available so theme-related
  settings keys whose backing extensions failed can be removed before
  VS Code first reads the profile.

### Requirement: Settings import

The system SHALL write the bundle's `settings` object (or `{}` when
absent) into the profile's cached `settings.json`. After the bundle's
`extensions.enabled` list has been processed, the system SHALL inspect
the failed-install list (per "Extension installation") and rewrite the
imported `settings.json` to remove `workbench.colorTheme` and
`workbench.iconTheme` whose target extensions failed to install. The
removal MUST happen before VS Code first reads the profile so the
workbench resolves to its built-in default theme rather than silently
falling through a missing theme name.

The mapping from a setting value to its providing extension is
established by these rules:

1. The system SHALL recognise a value of the form `<publisher>.<name>`
   as an explicit extension ID (no resolution needed).
2. For human-readable theme labels (e.g., "Tokyo Night",
   "Catppuccin Mocha"), the system SHALL consult a small built-in
   table mapping known theme/icon-theme labels to their
   `<publisher>.<name>` extension IDs. The table covers at least the
   themes referenced by the project's own profile bundles
   (`enkia.tokyo-night`, `catppuccin.catppuccin-vsc`,
   `catppuccin.catppuccin-vsc-icons`, `pkief.material-icon-theme`,
   `zhuangtongfa.Material-theme`, `sdras.night-owl`,
   `akamud.vscode-theme-onedark`, `dracula-theme.theme-dracula`,
   `vscode-icons-team.vscode-icons`).
3. When the value cannot be mapped to an extension ID, the system
   SHALL leave the key in place (no removal); a value rtk does not
   recognise is presumed to be a built-in theme name VS Code will
   resolve normally.

#### Scenario: Settings imported

- **WHEN** the bundle contains a `settings` object
- **THEN** the profile cache receives those settings.

#### Scenario: All theme extensions installed

- **WHEN** every extension in `extensions.enabled` installed
  successfully
- **THEN** the imported `settings.json` is unchanged from the bundle's
  `settings` object — no theme keys are removed.

#### Scenario: Color-theme extension failed to install

- **WHEN** the bundle's `workbench.colorTheme` resolves (via the
  built-in label-to-ID table or as an explicit extension ID) to an
  extension that appears in the failed-install list
- **THEN** the script removes the `workbench.colorTheme` key from the
  imported `settings.json` before exiting, so VS Code falls back to
  its built-in default theme on first launch of the profile.

#### Scenario: Icon-theme extension failed to install

- **WHEN** the bundle's `workbench.iconTheme` resolves to an
  extension that appears in the failed-install list
- **THEN** the script removes the `workbench.iconTheme` key from
  the imported `settings.json`.

#### Scenario: Theme value is unrecognised

- **WHEN** the value of `workbench.colorTheme` or
  `workbench.iconTheme` is neither in the built-in label-to-ID
  table nor an explicit `<publisher>.<name>` form
- **THEN** the script leaves the key in place — VS Code is presumed
  to recognise it as a built-in theme.

#### Scenario: Theme extensions installed but some other extension failed

- **WHEN** the failed-install list is non-empty but does NOT
  contain any extension that backs the bundle's theme keys
- **THEN** the imported `settings.json` retains its theme keys
  unchanged. The script still exits non-zero (per "Extension
  installation") so the failure is visible to the caller.

# Changelog

All notable changes to this project will be documented in this file.

## Unreleased — `harden-profile-tooling-and-pipeline`

### Security (BREAKING)

- All shipped profiles now default `security.workspace.trust.untrustedFiles` to `"prompt"` (was `"open"`). The first time you open an untrusted folder under a managed profile, VS Code shows its Workspace Trust prompt before running language servers, tasks, debug launches, or formatters.
- Reporting channel updated in `SECURITY.md` to use GitHub Security Advisories with an email fallback.

### Changed

- `editor.fontVariations` restored to `true` in the shared editor bases (regression fix).
- `scripts/compose-settings.sh` writes `_merged/*.json` atomically (temp file + `mv -f`) so VS Code never reads a partial file via the profile symlink.
- `scripts/compose-settings.sh` rejects `@extends` values that contain `..`, leading `/`, leading `~`, backslashes, or NUL bytes; cycle detection now uses resolved real paths so symlinked overrides cannot trick the detector.

(Further entries will land as the change progresses; this section will be finalized at archive time.)

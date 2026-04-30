# Changelog

All notable changes to this project will be documented in this file.

## Unreleased — `harden-profile-tooling-and-pipeline`

### Security (BREAKING)

- All shipped profiles now default `security.workspace.trust.untrustedFiles` to `"prompt"` (was `"open"`). The first time you open an untrusted folder under a managed profile, VS Code shows its Workspace Trust prompt before running language servers, tasks, debug launches, or formatters.
- Reporting channel updated in `SECURITY.md` to use GitHub Security Advisories with an email fallback.
- Extension identifiers from `extensions.json` and `.code-profile` bundles are validated against an allowlist regex (`scripts/lib/extension-id.sh`) before being passed to `code --install-extension`. `install-extensions.sh`, `import-profile.sh`, and `vspcli` all share this single helper. Injection-shaped values (e.g. `evil; rm -rf …`) are rejected before any install attempt.

### Changed

- `editor.fontVariations` restored to `true` in the shared editor bases (regression fix).
- `scripts/compose-settings.sh` writes `_merged/*.json` atomically (temp file + `mv -f`) so VS Code never reads a partial file via the profile symlink.
- `scripts/compose-settings.sh` rejects `@extends` values containing `..`, leading `/`, leading `~`, backslashes, or NUL bytes, and only accepts a bare filename or a single sub-directory under `_overrides/`. Cycle detection now uses resolved real paths so symlinked overrides cannot trick the detector.
- `scripts/import-profile.sh` PROFILE_ID generator now uses `openssl rand -hex 4` (with `python3 secrets` fallback). The previous `tr | head -c` form aborted under `set -euo pipefail` from SIGPIPE.
- `scripts/check-extension-compatibility.sh` repaired: replaces the bogus `code --show-extension` flag with `code --list-extensions --show-versions --profile <name>`; per-extension compat captures no longer abort the loop under `set -e`; documented exit code contract `0` clean / `1` findings / `2` CLI misuse.
- `scripts/open-profiles.sh` no longer swallows `code --new-window` failures with `|| true`; failed profiles are collected and surface as a non-zero exit.
- `scripts/validate-json.sh` defaults `TMPDIR=/tmp` so it runs in stripped environments where `set -u` would otherwise abort.

### Added

- `.github/workflows/ci.yml` runs on every push and PR (Ubuntu, SHA-pinned `actions/checkout@v6.0.2`, `permissions: contents: read`). Step order: regen → assert `git diff --exit-code _merged/ exports/` → validate-json → tests, so tests always see freshly composed artifacts.
- `scripts/install-hooks.sh` — idempotent installer that sets `core.hooksPath = scripts/git-hooks` only when no other value is configured. Refuses to clobber a foreign value (husky, pre-commit, lefthook, etc.) and verifies hook scripts are executable.
- `scripts/git-hooks/pre-commit` now also refuses to complete when compose/export regenerated tracked files that were not staged, with a clear "run X, stage Y" message.
- `CHANGELOG.md` (this file) and `secure-shared-defaults` + `audit-profile-extensions` OpenSpec capabilities.

### Removed

- `.cache/extensions-installed/` no longer tracked. Per-machine state was leaking into git; `.cache/` is now in `.gitignore` and the four committed markers were removed from the index.

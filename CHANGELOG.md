# Changelog

All notable changes to this project will be documented in this file.

## Unreleased — `workspace-toolchain-and-ux-layering`

### Changed (BREAKING — catalog reshape)

- Profile catalog collapsed from 22 dirs to 11. `<flavor>-{crisp,retina}` directories renamed to `<flavor>` (e.g., `rust-profile-crisp` → `rust`, `web-astro-crisp` → `astro`, `ai-profile-crisp` → `ai`). The 11 surviving flavors: `rust`, `astro`, `java-maven`, `java-gradle`, `java-spring`, `cpp-clangd`, `cpp-intellisense`, `ai`, `ai-plus`, `github-workflows`, `general`. Run `vspcli --migrate-catalog` once to perform the rename + regenerate `_merged/`, `exports/`, and `_overrides/`.
- UX (font, color theme, icon theme) moved from profile baseline (`_shared/editor-{crisp,retina}.jsonc`) to a workspace overlay (`_shared/ux/{crisp,retina}.jsonc`). Apply via `vspcli --detect --target=workspace --ux=<crisp|retina>` or raw `--font/--font-size/--theme/--icon-theme` flags. Profiles themselves are now UX-agnostic.
- Non-UX shared keys (telemetry, update mode, autoFetch, format-on-save, GPU acceleration, ~65 total) extracted to `_shared/editor-base.jsonc` and merged into every `_merged/<flavor>.json`.
- AI extensions consolidated into `_shared/extensions/{ai,ai-plus}.json` only. Toolchain layers (`rust.json`, `cpp-clangd.json`, etc.) no longer ship Copilot/Claude/Continue. Users wanting AI in non-AI workspaces pass `--toolchain rust --toolchain ai`.

### Added

- `vspcli --detect [PATH]` — workspace toolchain detection from signals (`Cargo.toml` → rust, `package.json` + `astro.config.*` → astro, `pom.xml` → java-maven, `build.gradle*` + `*Application.java`@SpringBootApplication → java-spring, `CMakeLists.txt` + `.clangd` → cpp-clangd, etc.). Stacks polyglot layers; `--toolchain <flavor>` overrides; `--no-detect` disables; `--json` emits structured output.
- `vspcli --detect --target=workspace [--ux=PRESET] [--font=...] [--no-rtk]` writes `.vscode/{extensions,settings,tasks}.json`: extensions as `recommendations` (set-union with existing), settings = editor-base + UX overlay + rust hover (when rust detected) + rtk terminal profile, tasks = rust doc tasks (when rust detected).
- `_shared/extensions/{base,<flavor>}.json` — layered extension lists. `scripts/lib/compose-extensions.sh` produces the composed install list (base + each toolchain layer, deduplicated).
- `scripts/lib/jsonc-merge.sh` — key-level JSONC merge helper. Strips comments via `scripts/lib/jsonc-strip.awk`, deep-merges via `jq`, preserves the leading file-comment block as a string prefix on write-back. Interior comments are NOT preserved (documented non-goal); `.bak` sibling written on first merge into a comment-bearing file.
- `scripts/lib/legacy-profile-name.sh` — resolves `<flavor>-{crisp,retina}` legacy names to `<flavor>` plus implied `--ux=<look>`. Sourced from `vspcli` and `install-extensions.sh` so direct script callers also get the soft-landing. Rate-limited deprecation warning (once per process tree); `ARTAGON_VSCODE_DEPRECATION_ACK=1` suppresses; `ARTAGON_VSCODE_DEPRECATION_SUMMARY=1` defers to an end-of-run summary.
- `_shared/rtk/rtk-init.{fish,bash}` — function-based wrappers routing common commands (`rg`, `grep`, `find`, `git`, `cargo`, `npm`, etc.) through `rtk <cmd>`. Sourced by VS Code's `rtk-fish` / `rtk-bash` terminal profiles. Process-local re-wrap guard (children get their own wrappers when rtk is on PATH there). Workspace Trust diagnostic line printed on interactive shell start.
- Rust API documentation auto-wired for any rust-detected workspace: rust-analyzer hover/signature settings (per `docs/rust.md` §2) written to `.vscode/settings.json`; two `cargo doc` tasks (rtk-prefixed) written to `.vscode/tasks.json`. The "Documentation Policy" block (per `docs/rust.md` §12) is appended (between sentinels) to `AGENTS.md`, `CLAUDE.md`, `CODEX.md`, `GEMINI.md` via `scripts/lib/apply-rust-policy.sh`. CI staleness gate prevents drift.
- `scripts/migrate-catalog.sh` (`vspcli --migrate-catalog`): one-shot, idempotent migration. Modes: default (run), `--doctor` (detect partial state), `--finalize` (remove deprecation symlinks post-deprecation). `mkdir`-based lock prevents concurrent invocations; backup written to `.cache/migrate-catalog-backup-<pid>/`. Legacy bundle filenames (`exports/<flavor>-{crisp,retina}.code-profile`) replaced with symlinks to canonical `<flavor>.code-profile` so external bookmarks continue to resolve through the deprecation window.
- **Curated theme + font picks** (research-driven, all on Open VSX so Cursor/Windsurf-compatible): added 7 new theme/icon extensions to `_shared/extensions/base.json` — `GitHub.github-vscode-theme` (official GitHub palettes), `mvllow.rose-pine` (Rosé Pine + Moon + Dawn), `fawwazfirdaus.poimandres-darker` (Pmndrs/Theatre.js community), `monokai.theme-monokai-pro-vscode` (free filter-octagon variant), `teabyii.ayu` (Light/Mirage/Dark), `liviuschera.noctis` (12 variants in one package), `miguelsolorio.symbols` (minimal line-art icons by VS Code core engineer). Existing themes preserved (Tokyo Night, Catppuccin, Dracula, One Dark Pro, Night Owl, Material Icon Theme, Catppuccin Icons, vscode-icons).
- **Updated font fallback chains** in `_shared/editor-base.jsonc` to add modern free fonts: `editor.fontFamily` now `'JetBrains Mono', 'Monaspace Neon', 'Fira Code', 'Cascadia Code', 'Iosevka', 'IBM Plex Mono', Menlo, monospace` (Monaspace adds GitHub Next's texture-healing tech; IBM Plex Mono adds CJK fallback). `terminal.integrated.fontFamily` adds Nerd Font variants for Monaspace/FiraCode/CaskaydiaCove/Iosevka. Paid fonts (Berkeley Mono, MonoLisa, Operator Mono, Comic Code) deliberately NOT in defaults — opt-in via `--font` flag with documented marketplace vendors.
- **Added `--ux=default` preset** alongside crisp/retina. Empty `_shared/ux/default.jsonc` overlays no keys, so the workspace inherits the user's VS Code User-scope settings + VS Code's built-in defaults for font/theme/iconTheme. Use when toolchain-driven settings (rust hover, rtk profile) are wanted but UX should be left untouched.

### Deprecated

- Legacy profile names (`<flavor>-crisp`, `<flavor>-retina`) resolve to `<flavor>` + implied `--ux=<look>` for one release with rate-limited deprecation warning. Removed in the release after.
- `java-profile` flavor dropped; legacy `java-profile-{crisp,retina}` aliases to `java-maven`.

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

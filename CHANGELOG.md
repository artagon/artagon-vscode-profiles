# Changelog

All notable changes to this project will be documented in this file.

## Unreleased — `refactor-capability-modules-and-defaults`

### Changed (BREAKING — vertical capability-module architecture & default AI agents)

- Refactored repository from horizontal `_shared/` + `_overrides/` + `_catalog/` layers to vertical self-contained modules under `modules/{capabilities,toolchains,ux}/` backed by a unified lowercase-keyed `catalog/` registry and compiled by a single pure Bash 3.2+ + `jq` engine (`scripts/lib/{common,catalog,module-resolver,compiler,vscode-storage}.sh`).
- Expanded compiled recipes from 13 to 15 (`ai-copilot` for strict Copilot-only policy and `general-minimal` for zero-AI `"chat.disableAIFeatures": true`).
- Added `modules/capabilities/ai-agents` (**Google Antigravity** `google.google-antigravity` as default primary agent alongside `anthropic.claude-code`, `openai.chatgpt`, and unified `github.copilot-chat` owning inline completions & Next Edit Suggestions) to default compiled profiles and 18 legacy toolchain export symlinks; superseded `google.geminicodeassist`. Legacy `ai-profile-{crisp,retina}` aliases and symlinks strictly point to `ai-copilot.code-profile` (never silently upgraded to Antigravity, Claude, or Codex).
- Added universal shell tooling (`modules/capabilities/shell`: Bash IDE, ShellCheck, `shfmt`, Fish, Better ShellScript syntax, BATS Test Runner) and universal data-format tooling (`core-editor`: JSON/JSONC, YAML, TOML, XML, CSV via `rainbow-csv`, DotEnv, INI/Properties, EditorConfig) across all 15 compiled profiles.
- Added IntelliJ-grade developer ergonomics across `core-editor` (24+ native settings including Project Explorer file nesting, inlay hints, sticky scroll, semantic highlighting, VS Code `1.147.0` Terminal IntelliSense, Kitty image protocol, macOS Option-as-Meta, SCM Graph incoming/outgoing changes, inline diff view, and Git merge/prune/smart-commit tuning), `modules/capabilities/dx-pro` (Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io; included in `ai-plus`), opt-in `modules/capabilities/intellij-keybindings`, and expanded `modules/capabilities/themes-pack` to 30 curated theme/icon/product-icon packs with `themeLabels`.
- Refined `retina` and `crisp` 15-key symmetric UX presets with verified macOS CoreText + Linux font stacks (`Maple Mono NF`, `JetBrains Mono`, `Monaspace Neon/Krypton NF`, `Geist Mono`, `CommitMono`, `Lilex`, `Fira Code`) and OpenType ligatures (`retina`: `'calt', 'liga', 'zero', 'ss03', 'ss07', 'ss08'`; `crisp`: `'calt', 'liga', 'zero'`).
- Tuned VS Code `1.120`–`1.147` AI chat ergonomics (`chat.tools.compressOutput.enabled`, `chat.tools.riskAssessment.enabled`, `chat.tools.terminal.outputReflow`, `chat.tools.autoExpandFailures`, `chat.notifyWindowOnResponseReceived`), `editor.inlineSuggest.fontFamily` (`Monaspace Krypton NF`), Claude Code (`preferredLocation: "sidebar"`, `respectGitIgnore`, `autosave`), and Codex (`followUpQueueMode: "queue"`, `composerEnterBehavior: "cmdIfMultiline"`, `reviewDelivery: "inline"`).
- Hardened extension installation and VSIX pinning: zero-TOCTOU copy-before-hash content-addressed local VSIX cache (`VSCODE_VSIX_CACHE_DIR`, `0700`/`0600` modes, BSD/GNU POSIX `chmod --`, symlink/traversal guards), batched `--install-extension` with per-extension retry fallback (`--no-batch` opt-out), `DX` group completeness gate in `validate-json.sh`, refined `--no-ai` stripping (preserves shared `core-editor` keys while removing all `chat.*`/`inlineChat.*`/`editor.inlineSuggest.{fontFamily,edits.*}`), and anchored VS Code `1.147.0` macOS/Linux `is_vscode_running` detection.
- Added deprecated setting keys (`json.colorDecorators.enable`, `scratchpads.useSubfolders`, `ruff.nativeServer`, `chat.tools.terminal.shellIntegrationTimeout`, `chatgpt.runCodexInWindowsSubsystemForLinux`) to `catalog/setting-scopes.json` `forbiddenKeys` and extended the bundle autonomy deny-list with Antigravity bypass keys (`antigravity.serverArgs`, `antigravity.releaseBaseUrl`, `antigravity.caCertificates`).
- Added [`ABOUT.md`](ABOUT.md) and refreshed [`README.md`](README.md) with badges, security contract, capability highlights, UX/AI breakdowns, and troubleshooting guide.
- Enforced strict workspace vs personal separation: `vspcli apply --target=workspace` writes only `extensions.workspace[]`, `settings.workspace.jsonc`, and portable plain `tasks.json`. Personal UX presets require `--ux-target=user|profile:<name>` or explicit `--ux-in-workspace`; workspace RTK wiring requires explicit `--rtk-in-workspace`.
- Replaced compromised `JScearcy.rust-doc-viewer` (Oct 2025 GlassWorm campaign) with `status: "compromised"` in `catalog/extensions.json`, removed 5 Marketplace 404 extension IDs, updated VS Code `1.147.0` setting types/enums/scopes (`editor.hover.enabled: "on"`, `catppuccin-mocha`, `errorLens.messageMaxChars`, `gitlens.plusFeatures.enabled`, `testing.automaticallyOpenTestResults`, native `java.jdt.ls.lombokSupport.enabled`).
- Removed tracked security-bypass files (`agents`, `.agents/settings.local.json`), circular symlink `.agents/skills/skills`, and legacy `_shared/`, `_overrides/`, `_catalog/` directories.

### Added

- Unified `vspcli` verbs: `build`, `validate`, `detect`, `apply`, `reconcile`, `doctor`, `install`, `import`, `open`, `list`, `migrate`.
- `vspcli reconcile`: plan-first (`--dry-run` zero mutations), unconditional Step 0 compromised-extension uninstall, `--as <recipe>`, `--compromised-only`, `--prune-retired`, `--prune-unmanaged` with `packMembers[]` (`openai.codex-audio`) protection.
- `vspcli doctor`: audits tools, catalog, XDG RTK wrappers, Default-profile application-scope settings (`telemetry.telemetryLevel`, `security.workspace.trust.untrustedFiles`), pin version drift, and `FAIL`s on any installed compromised extension across all profiles including Default.
- Live install-time SHA-256 VSIX pins in `catalog/extension-pins.json` refreshed via `scripts/refresh-pins.sh`.
- Full Zsh `ZDOTDIR` proxy RTK wrapper (`modules/capabilities/rtk-terminal/scripts/rtk-init.zsh` + `zsh/.*`) alongside hardened Fish and Bash wrappers.
- `scripts/generate-readme-matrix.sh` for deterministic Profile & Module Matrix rendering and CI sync checks.
- Dual-OS CI (`ubuntu-latest` + `macos-latest` with PATH-pinned `/bin/bash` 3.2 shim), static Bash 3.2 construct lint, full `shellcheck`, `zsh -n`, `fish -n`, and hermetic offline BATS suite.

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

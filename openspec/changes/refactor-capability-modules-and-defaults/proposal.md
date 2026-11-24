# Proposal: Vertical Capability-Module Architecture, Default AI Agent Trio, Extension Refresh & Visual Optimization

## Why

The current `artagon-vscode-profiles` architecture fragments each toolchain horizontally across `_shared/extensions/`, `_overrides/`, `profiles/`, and 8+ hardcoded shell tables, causing high change amplification (~18–20 files per new toolchain, ~48+ files per cross-cutting capability). Profile compilation (`compose-settings.sh`) and workspace setup (`install-workspace.sh`) run split-brain pipelines: workspace mode ignores `_overrides/*.jsonc`, depends at runtime on an unarchived OpenSpec snapshot, and emits broken external RTK paths. Live verification against the VS Code Marketplace and VS Code `1.147.0` also confirmed 5 removed extensions (`NOT_FOUND`), 1 supply-chain compromised extension (`JScearcy.rust-doc-viewer`, Oct 2025 GlassWorm campaign), 16 invalid or deprecated settings, a bloated 39-extension `base.json`, and two git-tracked `bypassPermissions` files (`agents` and `.agents/settings.local.json`).

Refactoring to a vertical, strictly Bash 3.2 + `jq` capability-module compiler resolves all P0/P1 defects, makes Claude Code + OpenAI Codex + GitHub Copilot Chat a clean default capability (`ai-agents`) with explicit opt-outs at build, workspace, and import time, refreshes all extensions and 14-key visual presets against VS Code `1.147.0`, and adds `reconcile` and `doctor` lifecycle management for native VS Code Profiles.

## What Changes

### 1. Vertical Capability-Module Architecture (Strictly Bash 3.2 + `jq`)
- Introduce a single catalog source of truth under `catalog/` (`extensions.json`, `publishers.json`, `extension-pins.json`, `defaults.json`), retiring `_shared/`, `_overrides/`, and `_catalog/`.
- Introduce self-contained vertical modules under `modules/{capabilities,toolchains,ux}/<id>/` (`module.json` + optional `settings.workspace.jsonc`, `settings.profile.jsonc`, `tasks.json`, scripts) supporting `requires[]`, `defaultWith[]`, `conflicts[]`, `detect`, and strict separation of BOTH settings (`workspace` vs `profile`) and extensions (`extensions.workspace[]` vs `extensions.profile[]` vs `extensions.unwanted[]`).
- Convert `profiles/<name>/` into thin recipes (`profile.json`) compiled by one unified Bash 3.2 + `jq` engine (`scripts/lib/{common,catalog,module-resolver,compiler,vscode-storage}.sh`) into UX-agnostic `_merged/<name>.json`, `profiles/<name>/settings.json`, `profiles/<name>/extensions.json`, `exports/<name>.code-profile`, 22 legacy symlinks (from ONE authoritative mapping helper shared by `export-profiles.sh` and `migrate-catalog.sh --refresh-symlinks`), and `exports/SHA256SUMS` (`0644`).
- Unify workspace (`--target=workspace`), native profile (`--target=profile`, with `storage.json` lookup, consistent `--user-data-dir`, and locked writes), and global (`--target=global`) workflows under `vspcli` (`build`, `apply`, `detect`, `reconcile`, `doctor`, `validate`, `export`, `import`, `completions`, `list`, `open`, `install-ext`) while preserving backwards-compatible flags and script entry points.
- Add `vspcli reconcile` (compromised-first unconditional pruning, install-first active reconciliation, explicit `--as <recipe>`, 5 lifecycle statuses, `--prune-retired`, `--prune-unmanaged` with `packMembers[]`) and `vspcli doctor` (read-only diagnostics with explicit `FAIL` vs `WARN` severity and `--strict`).

### 2. Default AI Agent Trio (`ai-agents`) & Explicit Breaking-Change Policy
- **BREAKING (supersedes OpenSpec Decision 15 / Copilot-only default):** Create `modules/capabilities/ai-agents/` (`requires: ["core-editor"]`, `conflicts: ["ai-copilot"]`) bundling the verified 2026 trio in `extensions.profile[]` (never leaking into committed `.vscode/extensions.json`):
  - `anthropic.claude-code` (Claude Code)
  - `openai.chatgpt` (OpenAI Codex IDE; `openai` added to `catalog/publishers.json`; pack member `openai.codex-audio` tracked in catalog)
  - `github.copilot-chat` (unified GitHub Copilot; deprecated `github.copilot` marked `superseded`)
- Include `ai-agents` in `catalog/defaults.json` for standard profiles (`general`, `ai`, `ai-plus`, and the 10 toolchain profiles).
- **Explicit Breaking Decision for Legacy Toolchain Bundles (Option B):** The 18 legacy toolchain symlinks (`rust-profile-*`, `java-*`, `web-astro-*`, `cpp-*`, `github-workflows-*`) point to their canonical toolchain bundles, which now include `ai-agents` by default (documented prominently as a **BREAKING** change in `README.md` and `CHANGELOG.md`). Three first-class opt-outs protect AI-free and Copilot-only consumers:
  1. **Legacy Copilot-only links preserved:** `exports/ai-profile-{crisp,retina}.code-profile` and `legacy-profile-name.sh` `ai-profile*` aliases point to `ai-copilot` (NEVER silently upgraded to Claude + Codex), enforced by ONE authoritative link table across both `export-profiles.sh` and `migrate-catalog.sh --refresh-symlinks`.
  2. **Import-time AI opt-out:** `import-profile.sh --no-ai` (alias `--without-group AI`) strips all `AI`-group extensions when importing ANY `.code-profile` bundle (including legacy toolchain links).
  3. **AI-free baseline & CLI opt-out:** `profiles/general-minimal/` excludes `ai-agents` AND sets `"chat.disableAIFeatures": true` (disabling VS Code's built-in Copilot runtime as well), and `--without ai-agents` excludes the trio on any `vspcli apply` composition.
- Configure verified VS Code `1.147.0` coexistence settings and enforce a parsed-JSON/JSONC autonomy/exec deny-list (`^chat\.tools\..*autoApprove$`, `claudeCode.allowDangerouslySkipPermissions == true`, `claudeCode.initialPermissionMode == "bypassPermissions"`, `claudeCode.claudeProcessWrapper`, `claudeCode.environmentVariables`, string value `"bypassPermissions"`).

### 3. Live-Verified Extension Refresh, Supply-Chain Remediation & Base Slimming
- Remediate GlassWorm-compromised `JScearcy.rust-doc-viewer`: mark `status: "compromised"` in `catalog/extensions.json`, add to `rust` `extensions.unwanted[]`, remove `jscearcy` from `catalog/publishers.json`, unconditionally uninstall FIRST in `vspcli reconcile`, and flag as `FAIL` (exit 1) across all profiles (including Default) in `vspcli doctor`.
- Mark 5 live-confirmed `NOT_FOUND` Marketplace extensions (`gabrielbb.vscode-lombok`, `pmd.pmd`, `shirdows.vscode-maven-dependency-explorer`, `pivotal.vscode-spring-boot`, `vscjava.vscode-spring-cloud`) as `removed`/`superseded` and replace or retire stale tools.
- Eliminate undefined conflict edges (`ts-base` + `ts-eslint-prettier` <-> `biome`, with `astro` `defaultWith: ["ts-eslint-prettier"]` suppressed automatically when `biome` is present; `python-base` + `python` <-> `python-ty`), layer `java-spring` over detected `java-gradle`/`java-maven`, make `github-workflows` additive (so Rust+workflows keeps Actions), keep `redhat.vscode-yaml` in `core-editor` (`workspace`), `github.vscode-pull-request-github` in `git-pro` (`profile`), and the 4 pinned universal crisp/retina theme/icon packs in `core-editor` (`profile`).
- Populate live-verified SHA-256 VSIX pins in `catalog/extension-pins.json` for the 6 universal core/UX extensions (`catppuccin.catppuccin-vsc`, `catppuccin.catppuccin-vsc-icons`, `enkia.tokyo-night`, `pkief.material-icon-theme`, `editorconfig.editorconfig`, `tamasfe.even-better-toml`) and document clearly that pins are install-time verified.

### 4. Visual Refresh, VS Code `1.147.0` Schema Accuracy & Hardening
- Refresh `modules/ux/{crisp,retina,default}/` with **14 exact symmetric keys** (`catppuccin-mocha`, `editor.lineHeight` `1.6`/`1.65`, typography + Nerd Font fallback) disjoint from `core-editor`, explicit `--ux-target` routing (`workspace` / `--ux-in-workspace`, `profile:<name>`, or `user`), timestamped `.bak` (`0600`) before `<CODE_USER_DIR>` writes, workspace symlink containment (refusing escapes outside canonical repo root), and `--ux=default` 14-key reset.
- Align every setting with verified VS Code `1.147.0` and extension manifests & scopes: `editor.hover.enabled: "on"`, `[rust].editor.hover.delay: 300`, `window.titleBarStyle: "custom"` + `window.nativeTabs: false`, `errorLens.messageMaxChars: 160`, `gitlens.plusFeatures.enabled: false`, `testing.automaticallyOpenTestResults: "openOnTestStart"`, machine-scoped `python-envs.alwaysUseUv` & `java.import.gradle.arguments` in `settings.profile.jsonc` (keeping workspace portable when `uv` is absent), built-in `shellscript` preserved for Zsh with `[shellscript].editor.formatOnSave: false` + `shellcheck.ignorePatterns`, omit `extensions.autoUpdate` from `core-editor`, plain `cargo doc` default in portable workspace `tasks.json`, and `vspcli doctor` Default-profile checks for application-scoped settings.
- Fix all P0/P1 Bash 3.2, `jq` 1.7.1-apple, `jsonc-strip.awk`, `jsonc-merge.sh`, `rtk-init.{fish,bash,zsh}`, `storage.json` exclusivity/locking, `pre-commit` porcelain col-2/` T`, isolated `run.sh` + `test_helper.bash`, and `ubuntu-latest` + `macos-latest` (PATH-pinned `/bin/bash` 3.2) CI defects.

## Supersession & Archive Disposition of Overlapping Active Changes

Five pre-existing active changes under `openspec/changes/` overlap the capabilities modified by this change (`harden-vscode-config-skill` is disjoint and untouched):

| Active Change | Current State | Disposition Before Archiving This Change | Carried-Forward Requirements Retained Here |
|---|---|---|---|
| `harden-profile-tooling-and-pipeline` | 39/39 tasks complete | Archive with `--skip-specs` (or withdraw to `archive/superseded/`) in Phase 0 | `EXTENSION_ID_REGEX`, atomic writes, `security.workspace.trust.untrustedFiles: "prompt"`, `validate-json`, `open-profiles` caching (`@extends` retired). |
| `modernize-extension-registry-and-toolchains` | 27/27 tasks complete | Archive with `--skip-specs` in Phase 0 | Python, Shell, Java, C++ toolchain modernization (`_shared/`/`_overrides/`, `ms-pyright.pyright`, `foxundermoon.shell-format`, `${command:jenv.javaHome}` retired). |
| `workspace-toolchain-and-ux-layering` | 99/99 tasks complete | Archive with `--skip-specs` in Phase 0 | Workspace detection, UX symmetry, Rust docs policy, RTK wrappers (Decision 15 Copilot-only, `_shared/rtk/`, Spring-only suppression, `snapshots/rust-tasks.json` superseded). |
| `harden-extension-supply-chain` | 12/22 tasks complete | Fold remaining tasks here; archive with `--skip-specs` in Phase 0 | `SHA256SUMS`, publisher allowlist, VSIX SHA-256 pinning (migrated `_catalog/` -> `catalog/`). |
| `profile-graceful-defaults` | 8/29 tasks complete | Fold remaining tasks here; archive with `--skip-specs` in Phase 0 | Failed theme/icon-theme stripping (extended with `workbench.preferredDarkColorTheme` & catalog lookup). |

Archiving the 5 superseded changes with `--skip-specs` before archiving `refactor-capability-modules-and-defaults` guarantees ONE conflict-free authoritative specification set in `openspec/specs/` with zero `_shared/`, `_overrides/`, or `_catalog/` residue.

## Capabilities

### New Capabilities
- `capability-module-architecture`: Vertical modules, Bash 3.2 + `jq` topological DAG (`requires[]`, `defaultWith[]`, `conflicts[]`), dual scope separation, `XDG_STATE_HOME` workspace provenance, and `jq` 1.7.1-safe collision checking.
- `extension-catalog-and-reconcile`: Unified `catalog/` SSOT, 5 lifecycle statuses, `packMembers[]`, fail-closed VSIX pinning, `vspcli reconcile` (compromised-first + `--as` + `--prune-unmanaged`), and `vspcli doctor`.
- `default-ai-agents-capability`: Default `ai-agents` trio (`extensions.profile[]`), coexistence settings, parsed-config autonomy deny-list, `general-minimal` (`chat.disableAIFeatures: true`), `ai-copilot`, and `import-profile.sh --no-ai`.

### Modified Capabilities
- `compose-profile-settings`: Retires `_shared/` + `_overrides/`; UX-agnostic `_merged/` + `profiles/*/extensions.json`, 14-key UX symmetry, XDG RTK (`fish`/`bash`/`zsh` `ZDOTDIR`).
- `detect-workspace-toolchain`: Data-driven detection across `modules/*/*/module.json`, `match`, `supersedes`, Spring + build-tool layering, additive `github-workflows`, BSD-safe draining `find`.
- `install-profile-extensions`: Catalog-driven grouping/order, `all` flag propagation, consistent `--user-data-dir`, VSIX pins, and catalog+`packMembers[]` `--uninstall`.
- `export-profile-bundles`: Dynamic 14-key UX stripping, single authoritative legacy link table, staged locked atomic export + `SHA256SUMS`.
- `import-profile-bundles`: Canonical-bound TOCTOU-free `SHA256SUMS` check, optional `.extensions.enabled`, skip all 4 non-active catalog statuses, `--no-ai`, pre-mutation checks, exclusive `storage.json` locking + backups, theme fallback.
- `open-profiles`: Pin-manifest + `CODE_USER_DIR` cache key, `--user-data-dir` consistency, error propagation, UX preservation, `-- "$path"`.
- `manage-profile-cli`: Unified `vspcli` verbs, explicit `--ux-target` routing, workspace symlink containment, fail-closed `--check-compat=block`, Bash 3.2 hardening.
- `validate-profile-json`: Renamed & updated for `catalog/`/`modules/`/`profiles/`/`_merged/`/`exports/`; single-document JSON validation, exact active-module publishers, parsed-config autonomy deny-list, schema/scope checks, Rust policy.
- `audit-profile-extensions`: Stable-first with pre-release fallback, `--extensions-file`, full semver/range comparator, `curl --max-time`, `jq -s` JSONL.

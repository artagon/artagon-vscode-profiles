# Artagon VS Code Profiles

[![CI](https://github.com/Artagon/artagon-vscode-profiles/actions/workflows/ci.yml/badge.svg)](https://github.com/Artagon/artagon-vscode-profiles/actions/workflows/ci.yml)
[![Platforms: macOS & Linux](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-0969da)](ABOUT.md#overview)
[![Runtime: Bash + jq](https://img.shields.io/badge/runtime-Bash%20%2B%20jq-4a7bbb)](ABOUT.md#design-guarantees)
[![VS Code: 1.147+](https://img.shields.io/badge/VS%20Code-1.147%2B-007acc)](#profile--module-matrix)
[![Profiles: 15](https://img.shields.io/badge/profiles-15-8250df)](#profile--module-matrix)
[![Modules: 32](https://img.shields.io/badge/modules-32-bf3989)](#profile--module-matrix)
[![VSIX Pins: SHA-256](https://img.shields.io/badge/VSIX%20pins-SHA--256-2ea44f)](#security--supply-chain-controls)
[![About](https://img.shields.io/badge/about-design%20%26%20verification-57606a)](ABOUT.md)

Vertical capability-module architecture for Visual Studio Code (`1.147+`) profiles, team-shareable workspace toolchains, IntelliJ-grade developer ergonomics, multi-agent AI coexistence, and supply-chain-hardened extension management. Runs on **portable Bash + `jq`** (stock macOS and Linux Bash) with zero Python, Node, or Rust runtime dependencies.

> [!IMPORTANT]
> **Security & Non-Invasiveness Contract**
> - **Clean team workspaces**: `vspcli apply --target=workspace` writes only team-shareable toolchain recommendations, workspace settings, and portable plain `tasks.json` into `.vscode/`; personal AI agents, UX presets (`crisp`/`retina`), and RTK terminal wiring stay out of `.vscode/` unless explicitly opted in.
> - **Hardened local-first defaults**: Profiles default `security.workspace.trust.untrustedFiles` to `"prompt"`, disable telemetry (`telemetry.telemetryLevel: "off"`), block auto-approve / CLI-wrapper bypass settings during bundle import, and provide first-class Copilot-only (`ai-copilot`) and zero-AI (`general-minimal`, `--no-ai`) opt-outs.
> - **Verified supply chain**: Universal core/UX extensions install via SHA-256-pinned official `https://` Marketplace VSIX packages through a zero-TOCTOU local cache; publisher allowlists gate every install; and compromised `jscearcy.rust-doc-viewer` (GlassWorm) is uninstalled first by `vspcli reconcile` and flagged `FAIL` across all profiles by `vspcli doctor`.

[Quick Start](#quick-start) · [Highlights](#architecture--capability-highlights) · [Profiles & Modules](#profile--module-matrix) · [AI Agents & Opt-Outs](#ai-agent-coexistence--explicit-opt-outs) · [UX & Typography](#ux-presets-typography--themes) · [Security Controls](#security--supply-chain-controls) · [CLI Reference](#cli-quick-reference) · [Troubleshooting](#troubleshooting--faq)

---

## Quick Start

### 1. Import a Compiled Profile Bundle (Recommended)

```bash
# Full stack with default AI agent suite (Antigravity + Claude Code + Codex + Copilot Chat)
bash scripts/import-profile.sh rust exports/rust.code-profile

# Import a profile, then apply your personal Retina or Crisp UX preset
bash scripts/import-profile.sh rust exports/rust.code-profile
bash scripts/vspcli apply --target=profile --profile rust --ux=retina

bash scripts/import-profile.sh java-gradle exports/java-gradle.code-profile
bash scripts/vspcli apply --target=profile --profile java-gradle --ux=crisp

# Strict Copilot-only (never adds Antigravity, Claude Code, or Codex)
bash scripts/import-profile.sh ai-copilot exports/ai-copilot.code-profile

# Zero-AI minimal profile, or strip AI from any compiled bundle
bash scripts/import-profile.sh general-minimal exports/general-minimal.code-profile
bash scripts/import-profile.sh --no-ai rust-clean exports/rust.code-profile
```

> [!NOTE]
> Always import via `scripts/import-profile.sh` (or `bash scripts/vspcli import`), not the VS Code UI (`Settings → Profiles → Import`). The VS Code UI skips extensions that are not already installed, whereas `import-profile.sh` verifies `exports/SHA256SUMS`, enforces the autonomy deny-list and publisher allowlist, installs pinned VSIX packages with SHA-256 verification, batches installs with per-extension retry fallback (`--no-batch` to disable), and applies graceful theme fallbacks if any theme extension fails.

### 2. Apply Team-Shareable Workspace Configuration

```bash
# Auto-detect toolchains and write team-shareable .vscode/{extensions,settings,tasks}.json
bash scripts/vspcli apply --target=workspace /path/to/repo

# Java Maven + Spring Boot (layers cleanly with either Gradle or Maven)
bash scripts/vspcli apply --target=workspace --toolchain java-maven --with java-spring /path/to/maven-spring-repo

# Astro + Biome (automatic formatter settings-key cleanup from ESLint+Prettier)
bash scripts/vspcli apply --target=workspace --with biome --without ts-eslint-prettier /path/to/astro-repo

# Python with Astral ty instead of BasedPyright
bash scripts/vspcli apply --target=workspace --with python-ty --without python /path/to/python-repo

# Apply personal UX preset to VS Code User settings (keeps team .vscode/ clean)
bash scripts/vspcli apply --target=workspace --ux=retina --ux-target=user /path/to/repo

# Explicitly opt in to workspace UX or RTK terminal wiring when desired
bash scripts/vspcli apply --target=workspace --ux=crisp --ux-in-workspace --rtk-in-workspace /path/to/repo
```

### 3. Reconcile & Audit Installed Profiles

```bash
# Audit tools, catalog, Default-profile application scopes, pin drift, and compromised extensions
bash scripts/vspcli doctor --strict

# Preview reconciliation with zero mutations (including Step 0)
bash scripts/vspcli reconcile --profile rust --dry-run

# Remove compromised extensions across all registered profiles (including Default)
bash scripts/vspcli reconcile --all --compromised-only

# Sync a profile to a canonical recipe, or switch an existing profile to Copilot-only
bash scripts/vspcli reconcile --profile ai-copilot --as ai-copilot --prune-unmanaged
```

---

## Architecture & Capability Highlights

| Domain | Included Capabilities & Tuning | Modules |
| :--- | :--- | :--- |
| **Core Editor & IntelliJ-Grade DX** | Native inlay hints, sticky scroll, semantic highlighting, bracket-pair guides, Project Explorer file nesting (`explorer.fileNesting.*`), Terminal IntelliSense (`terminal.integrated.suggest.*`), Kitty graphics protocol (`enableImages`), macOS Option-as-Meta, SCM Graph & inline diffs, Error Lens, spell check; opt-in Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io, and IntelliJ keymap | `core-editor`, `dx-pro`, `intellij-keybindings` |
| **Universal Shell & Data Formats** | Built into **all 15 profiles**: Bash IDE, ShellCheck, `shfmt`, Fish, Better ShellScript syntax, BATS Test Runner, JSON/JSONC, YAML (`redhat.vscode-yaml`), TOML (`even-better-toml`), XML (`redhat.vscode-xml`), CSV (`rainbow-csv`), DotEnv, INI/Properties, EditorConfig, and Markdown + Mermaid | `core-editor`, `shell`, `markdown` |
| **AI Coding Agents** | **Google Antigravity** (default primary agent), **Claude Code**, **OpenAI Codex**, and unified **GitHub Copilot Chat** (owning inline completions & Next Edit Suggestions) with clean non-conflicting defaults, `Monaspace Krypton NF` ghost-text font, and safe VS Code `1.120`–`1.147` chat ergonomics | `ai-agents`, `ai-copilot` |
| **Language Toolchains** | Auto-detected composable stacks for **Rust** (`rust-analyzer`, CodeLLDB, Dependi, Coverage Gutters, strict rustdoc tasks), **Java** (`java-base` native Lombok + `java-gradle` / `java-maven` / `java-spring`), **Python** (Ruff, `uv`/venv Environments, Debugpy, pytest + BasedPyright or Astral `ty`), **TypeScript / Astro** (`ts-base` + `astro` + ESLint/Prettier or Biome), and **C/C++** (`cpp-base` + `cpp-clangd` or `cpp-intellisense`) | `rust`, `java-*`, `python*`, `ts-*`, `astro`, `biome`, `cpp-*` |
| **DevOps, Git & Quality** | GitLens (Plus disabled) + GitHub Pull Requests, Container Tools (replaces deprecated `vscode-docker`), GitHub Actions (`github-workflows` covering all `.github`/`.gitea`/`.forgejo` globs), SonarQube for IDE (`quality`), Live Share (`collaboration`), and portable XDG RTK terminal wrappers (`rtk-terminal`) | `git-pro`, `containers`, `github-workflows`, `quality`, `collaboration`, `rtk-terminal` |
| **UX Presets & Themes** | 15-key exact-symmetry `retina` (Maple Mono NF + Catppuccin Mocha + Fluent Icons) and `crisp` (JetBrains Mono + Tokyo Night + Material Icons) presets, UX-agnostic `default`, plus 30 curated color/file-icon/product-icon packs in `themes-pack` | `retina`, `crisp`, `default`, `themes-pack` |

---

## Profile & Module Matrix

<!-- BEGIN GENERATED-MATRIX -->
### Profiles (15 Compiled Recipes)

| Profile | Description | Included Modules | Extensions | Export Bundle |
| --- | --- | --- | ---: | --- |
| `ai` | AI-first multi-agent workflow profile (Google Antigravity default, Claude Code, OpenAI Codex, and unified GitHub Copilot Chat) | `core-editor, ai-agents, markdown, shell` | 28 | [`ai.code-profile`](exports/ai.code-profile) |
| `ai-copilot` | Strict Copilot-only AI profile (unified GitHub Copilot Chat without Antigravity, Claude Code, or Codex; target of legacy ai-profile-* links) | `core-editor, markdown, shell, ai-copilot` | 25 | [`ai-copilot.code-profile`](exports/ai-copilot.code-profile) |
| `ai-plus` | Full-stack AI power profile adding GitLens + GitHub PRs, Container Tools, SonarQube for IDE, DX Pro (Bookmarks/Scratchpads/Project Manager/Bruno), and curated extra themes | `core-editor, ai-agents, markdown, shell, containers, dx-pro, git-pro, quality, themes-pack` | 64 | [`ai-plus.code-profile`](exports/ai-plus.code-profile) |
| `astro` | Astro + TypeScript web development with Tailwind CSS, Stylelint, and ESLint+Prettier (swap with --with biome) | `core-editor, ai-agents, markdown, shell, ts-base, astro, ts-eslint-prettier` | 39 | [`astro.code-profile`](exports/astro.code-profile) |
| `cpp-clangd` | C/C++ development with LLVM clangd, CMake Tools, and CodeLLDB | `core-editor, ai-agents, markdown, shell, cpp-base, cpp-clangd` | 32 | [`cpp-clangd.code-profile`](exports/cpp-clangd.code-profile) |
| `cpp-intellisense` | C/C++ development with Microsoft C/C++ IntelliSense, CMake Tools, and CodeLLDB | `core-editor, ai-agents, markdown, shell, cpp-base, cpp-intellisense` | 32 | [`cpp-intellisense.code-profile`](exports/cpp-intellisense.code-profile) |
| `general` | General-purpose developer profile with slim core editor, default AI agent suite (Antigravity + Claude + Codex + Copilot), Markdown, and Shell | `core-editor, ai-agents, markdown, shell` | 28 | [`general.code-profile`](exports/general.code-profile) |
| `general-minimal` | AI-free general developer profile (excludes ai-agents and disables built-in VS Code AI features) | `core-editor, markdown, shell` | 24 | [`general-minimal.code-profile`](exports/general-minimal.code-profile) |
| `github-workflows` | GitHub Actions CI/CD workflow authoring with YAML schema validation and Shell support | `core-editor, ai-agents, markdown, shell, github-workflows` | 29 | [`github-workflows.code-profile`](exports/github-workflows.code-profile) |
| `java-gradle` | Java development with Red Hat JDT LS (native Lombok), Gradle, JUnit, and Checkstyle | `core-editor, ai-agents, markdown, shell, java-base, java-gradle` | 34 | [`java-gradle.code-profile`](exports/java-gradle.code-profile) |
| `java-maven` | Java development with Red Hat JDT LS (native Lombok), Apache Maven, JUnit, and Checkstyle | `core-editor, ai-agents, markdown, shell, java-base, java-maven` | 34 | [`java-maven.code-profile`](exports/java-maven.code-profile) |
| `java-spring` | Spring Boot development layered on Gradle (for Maven Spring, compose java-maven --with java-spring) | `core-editor, ai-agents, markdown, shell, java-base, java-gradle, java-spring` | 37 | [`java-spring.code-profile`](exports/java-spring.code-profile) |
| `python` | Python development with Ruff, BasedPyright, Python Environments (venv/uv), Debugpy, and pytest (swap BasedPyright with --with python-ty --without python) | `core-editor, ai-agents, markdown, shell, python-base, python` | 33 | [`python.code-profile`](exports/python.code-profile) |
| `rust` | Rust development with rust-analyzer, CodeLLDB, Dependi, Coverage Gutters, and strict docs policy | `core-editor, ai-agents, markdown, shell, rust` | 32 | [`rust.code-profile`](exports/rust.code-profile) |
| `shell` | Shell scripting profile with Bash IDE, ShellCheck, shfmt, Fish, and BATS Test Runner | `core-editor, ai-agents, markdown, shell` | 28 | [`shell.code-profile`](exports/shell.code-profile) |

### Modules (`modules/`)

| Kind | Module ID | Description | Requires | Conflicts |
| --- | --- | --- | --- | --- |
| `capability` | `ai-agents` | Default AI agent suite: Google Antigravity (default), Claude Code, OpenAI Codex IDE, and unified GitHub Copilot Chat (inline completions & NES) | core-editor | ai-copilot |
| `capability` | `ai-copilot` | Strict Copilot-only AI capability (unified GitHub Copilot Chat, excludes Antigravity, Claude Code, and Codex) | core-editor | ai-agents |
| `capability` | `collaboration` | Real-time collaborative editing via Visual Studio Live Share | core-editor | — |
| `capability` | `containers` | Container Tools (replaces deprecated ms-azuretools.vscode-docker) | core-editor | — |
| `capability` | `core-editor` | Slim core editor ergonomics, universal data-format tooling (JSON/YAML/TOML/XML/CSV/DotEnv/EditorConfig), Error Lens, spell checking, and pinned crisp/retina theme packs | — | — |
| `capability` | `dx-pro` | IntelliJ-grade developer experience toolkit (Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io) | core-editor | — |
| `capability` | `git-pro` | Enhanced Git and GitHub PR workflow (GitLens with Plus disabled + GitHub Pull Requests) | core-editor | — |
| `capability` | `github-workflows` | GitHub Actions workflow schema validation, IntelliSense, and run inspection (additive on repos with .github/workflows) | core-editor | — |
| `capability` | `intellij-keybindings` | Opt-in IntelliJ IDEA keyboard shortcuts for JetBrains muscle memory (pass --with intellij-keybindings) | core-editor | — |
| `capability` | `markdown` | Markdown authoring, linting, Mermaid diagrams, and GitHub preview styles | core-editor | — |
| `capability` | `quality` | Optional SonarQube for IDE (SonarLint) static analysis | core-editor | — |
| `capability` | `rtk-terminal` | Portable XDG-installed RTK terminal wrappers for Fish, Bash, and Zsh (ZDOTDIR proxy) | core-editor | — |
| `capability` | `shell` | Bash, POSIX sh, Fish, ShellCheck, shfmt, and BATS testing support | core-editor | — |
| `capability` | `themes-pack` | Optional curated extra color, file icon, and product icon themes beyond crisp (Tokyo Night) and retina (Catppuccin Mocha) | core-editor | — |
| `toolchain` | `astro` | Astro web development with Tailwind CSS, Stylelint, and automatic ESLint+Prettier (or Biome when biome.json exists) | ts-base | — |
| `toolchain` | `biome` | Biome formatter and linter stack for TypeScript/JavaScript/JSON/CSS (mutually exclusive with ts-eslint-prettier) | ts-base | ts-eslint-prettier |
| `toolchain` | `cpp-base` | Shared C/C++ CMake Tools, CodeLLDB debugger, and syntax highlighting (portable, zero hardcoded host paths) | core-editor | — |
| `toolchain` | `cpp-clangd` | C/C++ powered by LLVM clangd (mutually exclusive with cpp-intellisense) | cpp-base | cpp-intellisense |
| `toolchain` | `cpp-intellisense` | C/C++ powered by Microsoft C/C++ IntelliSense (mutually exclusive with cpp-clangd) | cpp-base | cpp-clangd |
| `toolchain` | `java-base` | Shared Java language support (Red Hat JDT LS with native Lombok, XML, Debug, Test, Dependencies, Checkstyle) | core-editor | — |
| `toolchain` | `java-gradle` | Java with Gradle build tool (mutually exclusive with java-maven; supersedes java-maven when both manifests exist) | java-base | java-maven |
| `toolchain` | `java-maven` | Java with Apache Maven build tool (mutually exclusive with java-gradle) | java-base | java-gradle |
| `toolchain` | `java-spring` | Spring Boot framework layer (layers cleanly with either java-gradle or java-maven) | java-base | — |
| `toolchain` | `python` | Default Python toolchain with BasedPyright type checking (mutually exclusive with python-ty) | python-base | python-ty |
| `toolchain` | `python-base` | Shared Python runtime, debugger, Python Environments (venv/uv), Ruff formatter/linter, and pytest configuration | core-editor | — |
| `toolchain` | `python-ty` | Optional Python toolchain using Astral ty type checker (mutually exclusive with python/BasedPyright) | python-base | python |
| `toolchain` | `rust` | Rust development with rust-analyzer, CodeLLDB, Dependi, Coverage Gutters, and strict docs tasks | core-editor | — |
| `toolchain` | `ts-base` | Shared TypeScript/JavaScript base (Pretty TypeScript Errors, Vitest, tsserver tuning) | core-editor | — |
| `toolchain` | `ts-eslint-prettier` | ESLint + Prettier formatter and lint stack for TypeScript/JavaScript/Astro (mutually exclusive with biome) | ts-base | biome |
| `ux` | `crisp` | High-density crisp preset for external / standard-DPI monitors (Tokyo Night + Material Icons + Material Product Icons) | core-editor | retina, default |
| `ux` | `default` | UX-agnostic default preset (removes all 15 managed UX keys on explicit apply) | core-editor | crisp, retina |
| `ux` | `retina` | Comfortable Retina / HiDPI display preset (Catppuccin Mocha + Catppuccin Icons + Fluent Product Icons) | core-editor | crisp, default |
<!-- END GENERATED-MATRIX -->

---

## AI Agent Coexistence & Explicit Opt-Outs

Default compiled profiles (`general`, `ai`, `ai-plus`, `rust`, `python`, `astro`, `java-*`, `cpp-*`, `github-workflows`, `shell`) and the 18 legacy toolchain export symlinks include [`modules/capabilities/ai-agents`](modules/capabilities/ai-agents):

| Extension | Role in `ai-agents` | Coexistence Defaults |
| :--- | :--- | :--- |
| `google.google-antigravity` | Default primary AI agent | Sidebar-first agent workflow; replaces superseded `google.geminicodeassist` |
| `anthropic.claude-code` | Claude Code IDE agent | `preferredLocation: "sidebar"`, `respectGitIgnore: true`, `autosave: true` |
| `openai.chatgpt` (+ `openai.codex-audio`) | OpenAI Codex IDE agent | `followUpQueueMode: "queue"`, `composerEnterBehavior: "cmdIfMultiline"`, `reviewDelivery: "inline"` |
| `github.copilot-chat` | Inline ghost-text completions & Next Edit Suggestions (NES) | Replaces legacy `github.copilot`; paired with `Monaspace Krypton NF` inline ghost text |

- **Strict Copilot-only preserved**: Legacy `ai-profile-crisp` / `ai-profile-retina` aliases and export symlinks strictly resolve to [`exports/ai-copilot.code-profile`](exports/ai-copilot.code-profile) (`github.copilot-chat` only — never silently upgraded to Antigravity, Claude, or Codex).
- **Zero-AI profile**: Use [`exports/general-minimal.code-profile`](exports/general-minimal.code-profile), which excludes `ai-agents` and sets `"chat.disableAIFeatures": true`.
- **Strip AI on any import or apply**: Pass `--no-ai` to `scripts/import-profile.sh` (strips all AI extensions and `ai-agents`/`chat.*`/`inlineChat.*` settings while preserving shared `core-editor` keys, and sets `"chat.disableAIFeatures": true`), or pass `--without ai-agents` to `vspcli apply`.

---

## UX Presets, Typography & Themes

Profiles compile **UX-agnostic** (`_merged/*.json`) so you can pair any toolchain profile with your display preset via `--ux=retina`, `--ux=crisp`, or `--ux=default`. Both `retina` and `crisp` manage the exact same **15 symmetric keys** (enforced by [`scripts/tests/check-ux-symmetry.sh`](scripts/tests/check-ux-symmetry.sh)):

| Aspect | `retina` (HiDPI / Liquid Retina XDR) | `crisp` (External QHD / 1x DPI) |
| :--- | :--- | :--- |
| Color Theme | `Catppuccin Mocha` (`catppuccin.catppuccin-vsc`) | `Tokyo Night` (`enkia.tokyo-night`) |
| File Icon Theme | `catppuccin-mocha` (`catppuccin.catppuccin-vsc-icons`) | `material-icon-theme` (`pkief.material-icon-theme`) |
| Product Icon Theme | `fluent-icons` (`miguelsolorio.fluent-icons`) | `material-product-icons` (`pkief.material-product-icons`) |
| Editor Font Stack | `'Maple Mono NF', 'Monaspace Neon NF', 'Geist Mono', 'JetBrains Mono', ...` | `'JetBrains Mono', 'JetBrainsMono NF', 'CommitMono', 'Monaspace Neon', ...` |
| Terminal Font Stack | `'Maple Mono NF', 'Monaspace Neon NF', 'GeistMono NF', 'JetBrainsMono NF', ...` | `'JetBrainsMono NF', 'JetBrainsMono Nerd Font', 'Monaspace Neon NF', ...` |
| OpenType Ligatures | `'calt', 'liga', 'zero', 'ss03', 'ss07', 'ss08'` (`fontVariations: true`) | `'calt', 'liga', 'zero'` (`fontVariations: true`) |
| Sizing & Density | Editor `14px` / line-height `1.65`, Terminal `13px`, indent `16`, tabs `default` | Editor `13px` / line-height `1.55`, Terminal `12px`, indent `14`, tabs `compact` |

<details>
<summary><strong>Recommended free Homebrew fonts & extra theme packs (`themes-pack`)</strong></summary>

Install the exact macOS/Linux font stacks used by `retina`, `crisp`, and `ai-agents` ghost text (`Monaspace Krypton NF`):

```bash
brew install --cask \
  font-maple-mono-nf \
  font-jetbrains-mono \
  font-jetbrains-mono-nerd-font \
  font-monaspace \
  font-monaspace-nf \
  font-geist-mono \
  font-geist-mono-nerd-font \
  font-commit-mono \
  font-lilex \
  font-fira-code
```

Need extra color or icon themes beyond Tokyo Night and Catppuccin Mocha? Include `themes-pack` (`ai-plus` includes all 30 curated packs automatically, or pass `--with themes-pack` to `vspcli apply`).

</details>

---

## Security & Supply-Chain Controls

- **GlassWorm Compromise Remediation (`jscearcy.rust-doc-viewer`)**: Marked `status: "compromised"` in [`catalog/extensions.json`](catalog/extensions.json), listed in `rust` `unwantedRecommendations`, unconditionally uninstalled first (Step 0) by `vspcli reconcile` even when network installs fail, and flagged **`FAIL`** by `vspcli doctor` across every profile including Default.
- **Install-Time SHA-256 VSIX Pins & Verified Local Cache ([`catalog/extension-pins.json`](catalog/extension-pins.json))**: The 8 universal core/UX extensions (`catppuccin.catppuccin-vsc`, `catppuccin.catppuccin-vsc-icons`, `editorconfig.editorconfig`, `enkia.tokyo-night`, `miguelsolorio.fluent-icons`, `pkief.material-icon-theme`, `pkief.material-product-icons`, `tamasfe.even-better-toml`) download from official `https://` Marketplace URLs, verify SHA-256 digests before installation, and reuse a zero-TOCTOU copy-before-hash content-addressed cache (`VSCODE_VSIX_CACHE_DIR`, `0700`/`0600` private permissions). Refresh pins via `bash scripts/refresh-pins.sh`.
- **Trusted Bundle Settings Guard**: `scripts/import-profile.sh` verifies `<bundle-dir>/SHA256SUMS` and blocks autonomy/process-wrapper bypass keys (`chat.tools.*.autoApprove`, `bypassPermissions`, `claudeProcessWrapper`, `antigravity.serverArgs`, `antigravity.releaseBaseUrl`, `antigravity.caCertificates`, `chatgpt.cliExecutable`, `geminicodeassist.agentYoloMode`, etc.) unless `--allow-unsafe-settings` is explicitly passed.
- **Exact Active-Module Publisher Allowlist ([`catalog/publishers.json`](catalog/publishers.json))**: Enforced across `install-extensions.sh`, `import-profile.sh`, `vspcli install-ext`, and `vspcli reconcile` (`ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` for explicit per-run override).
- **Application-Scope Settings (VS Code `1.147.0`)**: `telemetry.telemetryLevel`, `security.workspace.trust.untrustedFiles`, `extensions.autoUpdate`, `extensions.autoCheckUpdates`, `update.mode`, `update.showReleaseNotes`, `window.titleBarStyle`, `window.nativeTabs`, `window.autoDetectHighContrast`, `catppuccin.*`, and `antigravity.keybindingScheme` are `APPLICATION`-scoped (`scope: 1`) and take effect from the **Default** VS Code profile (`<CODE_USER_DIR>/settings.json`). Run `bash scripts/vspcli doctor --strict` to verify your host Default profile.
- **Graceful Theme Fallback on Import**: If a theme, icon-theme, or product-icon-theme extension fails to install during `import-profile.sh`, dangling `workbench.colorTheme`, `workbench.preferredDarkColorTheme`, `workbench.iconTheme`, and `workbench.productIconTheme` keys are stripped before VS Code reads the profile.

---

## Shell Formatting & RTK Terminal Wrappers

<details>
<summary><strong>Shell / Zsh formatting safety (`.editorconfig`) & portable XDG RTK terminal wrappers</strong></summary>

### Shell & Zsh Formatting (`modules/capabilities/shell`)

Every compiled profile includes `bash-ide-vscode`, `shellcheck`, `mkhl.shfmt`, `bmalehorn.vscode-fish`, `jeff-hykin.better-shellscript-syntax`, and `kherring.bats-test-runner` while preserving VS Code's built-in `shellscript` grammar.

Because `shfmt` does not parse Zsh syntax reliably, `modules/capabilities/shell` sets `[shellscript].editor.formatOnSave: false`, `shfmt.formatIgnored: false`, and excludes Zsh files from `shellcheck.ignorePatterns`. To opt into format-on-save for POSIX/Bash scripts while protecting Zsh files per repository, add `.editorconfig`:

```ini
root = true

[*.{sh,bash}]
indent_style = space
indent_size = 2

[*.zsh]
shfmt_ignore = true
```

### RTK Terminal Wrappers (Zsh, Fish, Bash)

[`modules/capabilities/rtk-terminal`](modules/capabilities/rtk-terminal) installs wrappers atomically (`--no-follow`) into `${XDG_CONFIG_HOME:-~/.config}/artagon-vscode/rtk/`:
- **Zsh (`ZDOTDIR` proxy)**: Preserves user `ARTAGON_ORIG_ZDOTDIR` and restores it cleanly in `.zshrc` (non-login) or `.zlogin` (login).
- **Bash (`rtk-init.bash`)**: Uses an isolated `_ARTAGON_RTK_RC_LOADED` recursion guard before sourcing `~/.bash_profile` / `~/.bashrc` and wraps commands via `type -P` + `unalias`.
- **Fish (`rtk-init.fish`)**: Resolves `command -v rtk` safely even when paths contain spaces.

Workspace `.vscode/tasks.json` uses portable plain `cargo doc` commands by default; pass `--rtk-in-workspace` to emit `rtk`-prefixed tasks and workspace terminal profiles.

</details>

---

## CLI Quick Reference

| Command | Purpose |
| :--- | :--- |
| `bash scripts/vspcli list` | List all 15 compiled profile recipes |
| `bash scripts/vspcli build` | Compile `_merged/`, `profiles/*/`, `exports/`, 22 legacy symlinks, and `exports/SHA256SUMS` |
| `bash scripts/vspcli validate` | Run all JSON/JSONC, catalog/publisher, `GROUP_ORDER`, DAG, collision, scope, and security checks |
| `bash scripts/vspcli detect [--json] <path>` | Detect workspace toolchains from `modules/*/*/module.json` |
| `bash scripts/vspcli apply [flags] <path>` | Compile & write workspace, profile, or global config |
| `bash scripts/vspcli import [--no-ai] [--no-batch] <name> <bundle>` | Verify, install extensions (batched + SHA-256 VSIX pins), and register a VS Code profile (follow with `vspcli apply --target=profile --profile <name> --ux=retina` for UX presets) |
| `bash scripts/vspcli reconcile [flags]` | Sync installed profile extensions & remove compromised extensions first (Step 0) |
| `bash scripts/vspcli doctor [--strict]` | Audit tools, catalog, Default-profile application scopes, pin drift, and security |
| `bash scripts/refresh-pins.sh` | Refresh live SHA-256 VSIX pins in `catalog/extension-pins.json` |
| `bash scripts/check-extension-compatibility.sh --all --marketplace-only --json` | Live VS Code Marketplace compatibility audit across all 15 profiles |
| `bash scripts/install-hooks.sh` | Configure `core.hooksPath = scripts/git-hooks` |
| `bash scripts/tests/run.sh` | Run full hermetic offline script + 128-test BATS suite |

---

## Repository Layout & Maintainer Workflow

```text
.
├─ catalog/          # Authoritative registries (extensions, publishers, defaults, pins, setting-scopes)
├─ modules/
│  ├─ capabilities/  # 14 vertical capabilities (core-editor, ai-agents, ai-copilot, dx-pro, shell, rtk-terminal, ...)
│  ├─ toolchains/    # 15 language toolchains (rust, java-*, python*, ts-base, astro, biome, cpp-*)
│  └─ ux/            # 15-key symmetric UX presets (crisp, retina) + empty default
├─ profiles/         # 15 thin profile.json recipes + compiled settings.json symlink & extensions.json
├─ _merged/          # Compiled UX-agnostic profile settings (generated; do not edit)
├─ exports/          # Compiled .code-profile bundles, 22 legacy v1 symlinks, and SHA256SUMS (generated)
├─ scripts/          # vspcli, scripts/lib/ (portable Bash libraries), hooks, completions, and BATS test suite
├─ docs/             # Stack documentation (e.g. docs/rust.md)
└─ openspec/         # OpenSpec specifications, research notes, and adversarial review ledgers
```

1. Edit source definitions under [`catalog/`](catalog), [`modules/`](modules), or [`profiles/<name>/profile.json`](profiles) — never hand-edit `_merged/`, `profiles/*/extensions.json`, or `exports/`.
2. Run `bash scripts/vspcli build` to regenerate compiled artifacts and `exports/SHA256SUMS`.
3. Run `bash scripts/generate-readme-matrix.sh --write` whenever profiles or modules change.
4. Run `bash scripts/vspcli validate` and `bash scripts/tests/run.sh`.
5. Stage source and generated changes together (`git add catalog/ modules/ profiles/ _merged/ exports/`).

---

## Troubleshooting & FAQ

| Symptom | Cause | Fix |
| :--- | :--- | :--- |
| Extensions are missing after importing a `.code-profile` | Imported via VS Code UI instead of `import-profile.sh` | Re-import via `bash scripts/import-profile.sh <name> exports/<name>.code-profile` |
| `vspcli doctor --strict` warns on Default-profile settings | `APPLICATION`-scoped keys (`telemetry.telemetryLevel`, `security.workspace.trust.untrustedFiles`, `extensions.autoUpdate`) are read from the Default profile | Set those keys in your local VS Code Default `User/settings.json` and re-run `vspcli doctor --strict` |
| `vspcli doctor` reports `FAIL` for `jscearcy.rust-doc-viewer` | Compromised GlassWorm extension is installed in a profile or Default | Run `bash scripts/vspcli reconcile --all --compromised-only` |
| `import-profile.sh` rejects a bundle with unsafe settings | Bundle contains an autonomy or CLI-wrapper bypass setting | Inspect the reported key; only pass `--allow-unsafe-settings` for explicitly trusted local overrides |
| Batched extension install hits a Marketplace hiccup | A single extension error can trigger per-extension retry fallback | Pass `--no-batch` to force serial installation, or rerun `vspcli reconcile --profile <name>` |
| `import-profile.sh` refuses to overwrite while VS Code is open | VS Code `1.147+` holds `storage.json` while running | Quit VS Code cleanly (detected across macOS `Contents/MacOS/Code` and Linux) and re-run |

See [`ABOUT.md`](ABOUT.md), [`CHANGELOG.md`](CHANGELOG.md), [`CONTRIBUTING.md`](CONTRIBUTING.md), and [`SECURITY.md`](SECURITY.md) for project guarantees, release history, contribution rules, and vulnerability reporting.

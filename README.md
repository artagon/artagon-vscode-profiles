# Artagon VS Code Profiles

[![CI](https://github.com/Artagon/artagon-vscode-profiles/actions/workflows/ci.yml/badge.svg)](https://github.com/Artagon/artagon-vscode-profiles/actions/workflows/ci.yml)
[![Platforms: macOS & Linux](https://img.shields.io/badge/platforms-macOS%20%7C%20Linux-0969da)](ABOUT.md#overview)
[![Runtime: Rust](https://img.shields.io/badge/runtime-Rust-4a7bbb)](ABOUT.md#design-guarantees)
[![VS Code: 1.147+](https://img.shields.io/badge/VS%20Code-1.147%2B-007acc)](#profile--pack-matrix)
[![VSIX Pins: SHA-256](https://img.shields.io/badge/VSIX%20pins-SHA--256-2ea44f)](#security--supply-chain-controls)

**vsp** is the unified CLI for Visual Studio Code profiles, workspace toolchains, and extension management. The v3.0.0 cutover uses a native Rust binary with an embedded catalog. It composes 15 profiles from editor, AI, language, and UX packs.

[Installation](#installation) · [Quick Start](#quick-start) · [Composition](#composition) · [Personal Configuration](#personal-configuration) · [Profiles & Packs](#profile--pack-matrix) · [Security](#security--supply-chain-controls) · [CLI Reference](#cli-quick-reference) · [Migrating from v2](#migrating-from-v2)

## Installation

Install the v3.0.0 binary with Homebrew once the tag release is published. The [release checklist](openspec/changes/unify-vsp-cli-and-composition/tasks.md#phase-3-legacy-retirement-homebrew-launch--final-cutover-v300) tracks publication.

```bash
brew install artagon/aitools/vsp
vsp --version
```

To build from the v3.0.0 source checkout, use Cargo and install the compiled binary into your user bin directory:

```bash
cargo build --release --locked
./target/release/vsp self install
```

Add the user bin directory (normally `~/.local/bin`) to your `PATH`. Use the VS Code CLI (`code`) for live profile operations.

## Quick Start

```bash
# Browse recipes and inspect the Rust preset
vsp list presets
vsp show rust

# Detect workspace toolchains and preview .vscode changes
vsp detect /path/to/repo --explain
vsp diff --workspace /path/to/repo --preset rust
vsp sync --workspace /path/to/repo --preset rust

# Sync the Rust profile directly from the built-in catalog, then open it
vsp sync -p rust
vsp use rust
vsp open . -p rust

# Audit installed profiles, including Default
vsp doctor --strict
```

- Use `vsp use <profile> [path]` to open a profile unambiguously even when a local directory shares the profile name (such as `./shell`). Use `vsp open [path] -p <profile>` for workspace-first opening.
- Use `vsp sync --dry-run` with the same selector and composition flags to preview a standard sync. Use `vsp diff --check` to detect drift in automation.
- `vsp doctor --strict` audits Default-profile application settings, catalog and lock integrity, and installed extensions. On a clean install it passes without opt-in `rtk-terminal` wrappers unless `rtkTerminal: true` is configured or a synced profile enables `rtk-terminal`.
- If `vsp.lock.json` contains `pending_refresh` Tier 1 entries for your platform, `vsp sync` fails closed before writing (exit `5`). Pass `--allow-unlocked` to relax only pending or missing Tier 1 entries while continuing to enforce verified SHA-256 digests, approved HTTPS Marketplace URLs, and Tier 0 blocks.

To import from a local bundle, first export it with `vsp export` or download and extract the profile archive from [GitHub Releases](https://github.com/Artagon/artagon-vscode-profiles/releases). Keep `SHA256SUMS` beside the bundle.

```bash
vsp export rust --out ./bundles
vsp sync --from ./bundles/rust.code-profile -p rust
```

## Composition

`vsp` distinguishes three concepts:

1. **Shipped presets** — the 15 built-in recipes defined in [`presets.jsonc`](presets.jsonc) (`vsp list presets`, `vsp show <name>`).
2. **Custom user recipes** — personal recipes declared under `profiles` in `~/.config/vsp/config.jsonc` (`vsp init`) that extend shipped presets or packs with overrides and `extraExtensions`.
3. **Installed VS Code profiles** — live profiles registered in VS Code's `storage.json` (`vsp list installed`), targeted with `-p/--profile <name>` and launched via `vsp use <profile> [path]` or `vsp open [path] -p <profile>`.

- Use `vsp show --pack <id>` (for example `vsp show --pack rust` or `vsp show --pack shell`) to inspect a pack explicitly when a shipped preset shares the same name.
- `--with` and `--without` add or exclude packs. The resolver loads required packs and rejects conflicting combinations. For example, `vsp diff /path/to/astro-repo --preset astro --with biome --without ts-eslint-prettier` previews changing the Astro formatter.
- `--profile NAME` selects a named VS Code profile. `--preset NAME` selects a recipe and can be combined with a profile or workspace selector. Use `--user` for the Default VS Code profile.
- Use `--ux retina` or `--ux crisp` on a profile or user target for personal typography and themes. Workspace sync can apply an explicitly selected UX preset. Leave `--ux` out of team workspace commands to keep personal display settings out of the workspace.

## Personal Configuration

Run `vsp init` to create `~/.config/vsp/config.jsonc`, or `$XDG_CONFIG_HOME/vsp/config.jsonc` when `XDG_CONFIG_HOME` is an absolute path. To share team recipes explicitly, pass `vsp --config <file>` (for example `vsp --config ./team-vsp.jsonc sync -p team-rust`) to replace the personal config source for that invocation. For security, `vsp` ignores repo-local `.vsp.toml` and `.vsp.jsonc` files automatically.

```jsonc
{
  "version": 1,
  "defaults": { "ux": "retina", "ai": "copilot", "rtkTerminal": false },
  "profiles": {
    "my-rust": {
      "description": "Rust with Git tooling",
      "extends": "rust",
      "with": ["git-pro"],
      "without": [],
      "overrides": { "editor.fontSize": 14, "[rust]": { "editor.formatOnSave": true } },
      "extraExtensions": ["editorconfig.editorconfig"]
    }
  }
}
```

`version` must be the integer `1`. `defaults` and `profiles` are optional objects. The parser also permits a top-level `$schema` key.

Defaults apply to personal targets:

- `ux` accepts `crisp`, `retina`, or `default`. An explicit `--ux` or a UX preset implied by a legacy alias takes precedence over the default.
- `ai` accepts `full`, `copilot`, or `off`. Explicit `--ai` or `--no-ai` takes precedence.
- `rtkTerminal: true` adds the pack unless `--no-rtk` or `--without rtk-terminal` is present. `rtkTerminal: false` skips wrapper installation.

Specify a catalog preset or pack in `extends`. Use `with` and `without` for arrays of pack IDs. Run `vsp diff -p my-rust` to preview and `vsp sync -p my-rust` to apply the example profile. Its overrides apply after UX settings.

- Use literal dotted VS Code setting IDs or `[language]` blocks in `overrides`. Bare keys such as `chat` exit `4`. A setting that matches the autonomy deny-list exits `5`, even with `--allow-unsafe-settings`.
- Use exact `publisher.name` IDs in `extraExtensions`. Wildcards and publisher trust keys (`trust`, `publishers`, `trustedPublishers`) exit `4`. Compromised or removed IDs exit `5`.
- A catalog-external exact ID is Tier 3 `user-unlocked` and uses no VSIX pin. `vsp doctor` prints `WARN` and includes the ID in `user_unlocked_extensions` in JSON. Under `--strict`, the warning fails the audit with exit `1` when observation completes. The finding still displays as `WARN`.

## Architecture & Capability Highlights

The Rust engine in [`crates/vsp`](crates/vsp) resolves capability and toolchain packs from `packs/*.jsonc`. It reads recipes and legacy aliases from [`presets.jsonc`](presets.jsonc), and reads extension metadata and settings policy from [`catalog/`](catalog). The embedded snapshot lets the compiled binary work outside a clone. Use `--catalog DIR` to test a local catalog.

The 83 generated files formerly tracked in `_merged/`, `exports/`, and `profiles/*/{extensions,settings}.json` are built in memory on demand. Use `vsp export rust --out ./bundles` for a single bundle or `vsp dev dist --out ./dist` for all 15 canonical bundles, 22 legacy aliases, and their `SHA256SUMS`. The tag release workflow publishes binary and profile archives as GitHub Release assets with `SHA256SUMS` and signed build provenance attestations. [GitHub Releases](https://github.com/Artagon/artagon-vscode-profiles/releases)

| Domain | Included Capabilities & Tuning | Packs |
| :--- | :--- | :--- |
| **Core Editor & IntelliJ-Grade DX** | Native inlay hints, sticky scroll, semantic highlighting, bracket-pair guides, Project Explorer file nesting (`explorer.fileNesting.*`), Terminal IntelliSense (`terminal.integrated.suggest.*`), Kitty graphics protocol (`enableImages`), macOS Option-as-Meta, SCM Graph & inline diffs, Error Lens, spell check; opt-in Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io, and IntelliJ keymap | `core-editor`, `dx-pro`, `intellij-keybindings` |
| **Universal Shell & Data Formats** | Built into **all 15 profiles**: Bash IDE, ShellCheck, `shfmt`, Fish, Better ShellScript syntax, BATS Test Runner, JSON/JSONC, YAML (`redhat.vscode-yaml`), TOML (`even-better-toml`), XML (`redhat.vscode-xml`), CSV (`rainbow-csv`), DotEnv, INI/Properties, EditorConfig, and Markdown + Mermaid | `core-editor`, `shell`, `markdown` |
| **AI Coding Agents** | **Google Antigravity** (default primary agent), **Claude Code**, **OpenAI Codex**, and unified **GitHub Copilot Chat** (owning inline completions & Next Edit Suggestions) with clean non-conflicting defaults, `Monaspace Krypton NF` ghost-text font, and safe VS Code `1.120`–`1.147` chat ergonomics | `ai-agents`, `ai-copilot` |
| **Language Toolchains** | Auto-detected composable stacks for **Rust** (`rust-analyzer`, CodeLLDB, Dependi, Coverage Gutters, strict rustdoc tasks), **Java** (`java-base` native Lombok + `java-gradle` / `java-maven` / `java-spring`), **Python** (Ruff, `uv`/venv Environments, Debugpy, pytest + BasedPyright or Astral `ty`), **TypeScript / Astro** (`ts-base` + `astro` + ESLint/Prettier or Biome), and **C/C++** (`cpp-base` + `cpp-clangd` or `cpp-intellisense`) | `rust`, `java-*`, `python*`, `ts-*`, `astro`, `biome`, `cpp-*` |
| **DevOps, Git & Quality** | GitLens (Plus disabled) + GitHub Pull Requests, Container Tools (replaces deprecated `vscode-docker`), GitHub Actions (`github-workflows` covering all `.github`/`.gitea`/`.forgejo` globs), SonarQube for IDE (`quality`), Live Share (`collaboration`), and portable XDG RTK terminal wrappers (`rtk-terminal`) | `git-pro`, `containers`, `github-workflows`, `quality`, `collaboration`, `rtk-terminal` |
| **UX Presets & Themes** | 15-key exact-symmetry `retina` (Maple Mono NF + Catppuccin Mocha + Fluent Icons) and `crisp` (JetBrains Mono + Tokyo Night + Material Icons) presets, UX-agnostic `default`, plus 30 curated color/file-icon/product-icon packs in `themes-pack` | `retina`, `crisp`, `default`, `themes-pack` |

---

## Profile & Pack Matrix

Bundle artifacts are not tracked in git. Generate any bundle locally with `vsp export <name> --out ./bundles` (or all bundles with `vsp dev dist --out ./dist`), or download the verified profile archive from [GitHub Releases](https://github.com/Artagon/artagon-vscode-profiles/releases).

<!-- BEGIN GENERATED-MATRIX -->
### Profiles (15 Compiled Recipes)

| Profile | Description | Included Packs | Extensions | Bundle Artifact |
| --- | --- | --- | ---: | --- |
| `ai` | AI-first multi-agent workflow profile (Google Antigravity default, Claude Code, OpenAI Codex, and unified GitHub Copilot Chat) | `core-editor, ai-agents, markdown, shell` | 28 | `ai.code-profile` |
| `ai-copilot` | Strict Copilot-only AI profile (unified GitHub Copilot Chat without Antigravity, Claude Code, or Codex; target of legacy ai-profile-* links) | `core-editor, markdown, shell, ai-copilot` | 25 | `ai-copilot.code-profile` |
| `ai-plus` | Full-stack AI power profile adding GitLens + GitHub PRs, Container Tools, SonarQube for IDE, DX Pro (Bookmarks/Scratchpads/Project Manager/Bruno), and curated extra themes | `core-editor, ai-agents, markdown, shell, containers, dx-pro, git-pro, quality, themes-pack` | 64 | `ai-plus.code-profile` |
| `astro` | Astro + TypeScript web development with Tailwind CSS, Stylelint, and ESLint+Prettier (swap with --with biome) | `core-editor, ai-agents, markdown, shell, ts-base, astro, ts-eslint-prettier` | 39 | `astro.code-profile` |
| `cpp-clangd` | C/C++ development with LLVM clangd, CMake Tools, and CodeLLDB | `core-editor, ai-agents, markdown, shell, cpp-base, cpp-clangd` | 32 | `cpp-clangd.code-profile` |
| `cpp-intellisense` | C/C++ development with Microsoft C/C++ IntelliSense, CMake Tools, and CodeLLDB | `core-editor, ai-agents, markdown, shell, cpp-base, cpp-intellisense` | 32 | `cpp-intellisense.code-profile` |
| `general` | General-purpose developer profile with slim core editor, default AI agent suite (Antigravity + Claude + Codex + Copilot), Markdown, and Shell | `core-editor, ai-agents, markdown, shell` | 28 | `general.code-profile` |
| `general-minimal` | AI-free general developer profile (excludes ai-agents and disables built-in VS Code AI features) | `core-editor, markdown, shell` | 24 | `general-minimal.code-profile` |
| `github-workflows` | GitHub Actions CI/CD workflow authoring with YAML schema validation and Shell support | `core-editor, ai-agents, markdown, shell, github-workflows` | 29 | `github-workflows.code-profile` |
| `java-gradle` | Java development with Red Hat JDT LS (native Lombok), Gradle, JUnit, and Checkstyle | `core-editor, ai-agents, markdown, shell, java-base, java-gradle` | 34 | `java-gradle.code-profile` |
| `java-maven` | Java development with Red Hat JDT LS (native Lombok), Apache Maven, JUnit, and Checkstyle | `core-editor, ai-agents, markdown, shell, java-base, java-maven` | 34 | `java-maven.code-profile` |
| `java-spring` | Spring Boot development layered on Gradle (for Maven Spring, compose java-maven --with java-spring) | `core-editor, ai-agents, markdown, shell, java-base, java-gradle, java-spring` | 37 | `java-spring.code-profile` |
| `python` | Python development with Ruff, BasedPyright, Python Environments (venv/uv), Debugpy, and pytest (swap BasedPyright with --with python-ty --without python) | `core-editor, ai-agents, markdown, shell, python-base, python` | 33 | `python.code-profile` |
| `rust` | Rust development with rust-analyzer, CodeLLDB, Dependi, Coverage Gutters, and strict docs policy | `core-editor, ai-agents, markdown, shell, rust` | 32 | `rust.code-profile` |
| `shell` | Shell scripting profile with Bash IDE, ShellCheck, shfmt, Fish, and BATS Test Runner | `core-editor, ai-agents, markdown, shell` | 28 | `shell.code-profile` |

### Packs (`packs/`)

| Kind | Pack ID | Description | Requires | Conflicts |
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

Default compiled profiles (`general`, `ai`, `ai-plus`, `rust`, `python`, `astro`, `java-*`, `cpp-*`, `github-workflows`, `shell`) and the 18 legacy toolchain bundle aliases include [`packs/ai-agents.jsonc`](packs/ai-agents.jsonc):

| Extension | Role in `ai-agents` | Coexistence Defaults |
| :--- | :--- | :--- |
| `google.google-antigravity` | Default primary AI agent | Sidebar-first agent workflow; replaces superseded `google.geminicodeassist` |
| `anthropic.claude-code` | Claude Code IDE agent | `preferredLocation: "sidebar"`, `respectGitIgnore: true`, `autosave: true` |
| `openai.chatgpt` (+ `openai.codex-audio`) | OpenAI Codex IDE agent | `followUpQueueMode: "queue"`, `composerEnterBehavior: "cmdIfMultiline"`, `reviewDelivery: "inline"` |
| `github.copilot-chat` | Inline ghost-text completions & Next Edit Suggestions (NES) | Replaces legacy `github.copilot`; paired with `Monaspace Krypton NF` inline ghost text |

- **Strict Copilot-only.** Legacy `ai-profile-crisp` and `ai-profile-retina` aliases resolve to `ai-copilot`, which uses `github.copilot-chat`.
- **Zero-AI profile.** Use `general-minimal`, which excludes `ai-agents` and sets `"chat.disableAIFeatures": true`.
- **AI modes.** Use `--ai off` (or `--no-ai`) with `vsp sync` to disable AI, or `--ai copilot` for Copilot only.

---

## UX Presets, Typography & Themes

Profile bundles are UX-agnostic. Apply `--ux retina`, `--ux crisp`, or `--ux default` when you sync a profile. Both `retina` and `crisp` manage the same 15 settings keys.

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

Need extra color or icon themes beyond Tokyo Night and Catppuccin Mocha? Include `themes-pack` (`ai-plus` includes all 30 curated packs automatically, or pass `--with themes-pack` to `vsp sync`).

</details>

---

## Security & Supply-Chain Controls

- **Autonomy policy.** The settings deny-list checks dotted and nested paths, including case-insensitive execution overrides and boolean strings. It catches automatic approval and process-wrapper bypass settings before writes. Profile defaults prompt for Workspace Trust and disable telemetry. Use `vsp doctor --strict` to audit the Default profile's application-scoped settings; it passes cleanly without opt-in `rtk-terminal` wrappers unless `rtkTerminal: true` or a synced profile enables them.
- **GlassWorm remediation.** Named-profile sync removes installed compromised or removed extensions in Step 0 before settings writes and extension installs. `vsp sync --all --compromised-only` sweeps every registered profile and Default. `vsp doctor` reports compromised extensions such as `jscearcy.rust-doc-viewer` and returns exit `7` if a required extension listing fails.
- **Four-tier VSIX lock & selective `--allow-unlocked`.** Tier 0 records compromised or removed IDs in `catalog/extensions.jsonc` for Step 0. [`vsp.lock.json`](vsp.lock.json) records tiers 1–3. Tier 1 uses SHA-256 hashes and platform-specific VSIX URLs. Tier 2 records version locks for executable extensions. Tier 3 covers unlocked declarative extensions such as themes, grammars, and keymaps. Pre-write VSIX lock preflight runs before mutating `storage.json`, settings, extensions, or provenance and exits `5` with zero mutations on refusal. Passing `--allow-unlocked` selectively relaxes only `pending_refresh` or missing Tier 1 entries (recorded in provenance as `WARN` in `vsp doctor`) while continuing to enforce verified SHA-256 digests, approved HTTPS Marketplace hosts, and Tier 0 blocks.
- **Bundle integrity.** Keep `SHA256SUMS` beside exported bundles. `vsp sync --from BUNDLE -p NAME` checks a matching checksum and rejects mismatches. Verify the release archive against its published checksum and attestation before extracting it.
- **Lossless JSONC writes.** The concrete syntax tree (CST) merge preserves comments and formatting on untouched settings and keeps user-owned keys. Sync checks workspace symlink containment. Advisory locks and a SHA-256 compare-and-swap guard protect VS Code's `storage.json` updates.

## Shell Formatting & RTK Terminal Wrappers

The shell pack ships syntax support for Bash, Fish, and Zsh. Use `shfmt` for POSIX and Bash sources, and exclude `*.zsh` files from automatic `shfmt` formatting. The optional [RTK terminal pack](packs/rtk-terminal) keeps its Bash, Fish, and Zsh wrappers under `packs/rtk-terminal/scripts/`. Profile or user sync with `--with rtk-terminal` installs wrappers under `${XDG_CONFIG_HOME:-~/.config}/vsp/rtk/`. Workspace tasks use portable shell commands.

## CLI Quick Reference

| Command | Purpose |
| --- | --- |
| `vsp sync --workspace PATH` | Sync a workspace. Other selectors include positional `PATH`, `-p/--profile NAME`, `--preset NAME`, `--user`, `--from BUNDLE -p NAME`, `--extensions-only` (profile/`--all`), and `--all --compromised-only`. |
| `vsp diff --workspace PATH --check` | Preview a workspace, profile, preset, or user plan and report confirmed drift with exit `3`. |
| `vsp use PROFILE [PATH]` | Launch a named VS Code profile unconditionally (avoids ambiguity when `./<profile>` exists). |
| `vsp open [PATH] -p NAME` | Open a workspace in a named profile. Add `--sync` to sync first. |
| `vsp doctor --strict` | Audit VS Code profiles, extensions, and Default-profile settings (passes cleanly without opt-in `rtk-terminal` wrappers). |
| `vsp list presets` | List shipped presets. Use `packs`, `ux`, or `installed` for other categories. |
| `vsp show NAME`, `vsp show --pack ID` | Inspect a preset, pack, UX preset, or legacy alias. Use `--pack` when a pack shares a preset name. |
| `vsp detect PATH --explain` | Inspect workspace signals without writing files. |
| `vsp export rust --out DIR` | Export a bundle and `SHA256SUMS`. Omit names to export all canonical profiles. |
| `vsp init` | Create `~/.config/vsp/config.jsonc` for personal defaults and custom user recipes. |
| `vsp --config FILE ...` | Use an explicit team or alternate JSONC config file for one invocation. |
| `vsp completions zsh` | Generate completions for Bash, Zsh, or Fish. |
| `vsp self path`, `vsp self install` | Locate the running binary or install the v3.0.0 binary in your user bin directory. |
| `vsp dev validate`, `vsp dev dist --out DIR`, `vsp dev docs --check` | Validate canonical catalog inputs, build deterministic distribution bundles, or check/regenerate docs. |

Both `sync` and `diff` require an explicit target selector. A missing selector exits `4`.

| Exit | Meaning |
| ---: | --- |
| `0` | Success, clean check, preview, help, or version |
| `1` | Operational, audit, or validation failure |
| `2` | Target missing |
| `3` | Complete negative check, such as no detection match or confirmed drift |
| `4` | Incorrect usage, invalid configuration, or composition conflict |
| `5` | Safety guard refused before write (zero mutations) |
| `6` | Retryable lock/CAS conflict or mid-apply partial failure |
| `7` | Incomplete observation or unavailable verification |

## Migrating from v2

The native binary enters compatibility mode when the basename of `argv[0]` is `vspcli` (or `vspcli.exe`, ignoring case), when `VSP_COMPAT_VSPCLI=1`, or when invoked as `vsp compat-vspcli …`. A source install can add the `vspcli` symlink with `vsp self install --with-vspcli`.

In compatibility mode, `doctor`, `reconcile`, and `install-ext` map exits `5`, `6`, and `7` to `1`. All other exits pass through. Use the commands below to migrate, including replacing the retired `build` verb, which returns `4` in compatibility mode.

| v2 workflow | `vsp` workflow |
| --- | --- |
| `vspcli apply --target=workspace PATH` | `vsp sync --workspace PATH` |
| `vspcli reconcile --profile rust [--as <recipe>]` | `vsp sync -p rust --extensions-only [--preset <recipe>]` |
| `vspcli import rust BUNDLE` | `vsp sync --from BUNDLE --profile rust` |
| `vspcli doctor --strict` | `vsp doctor --strict` |
| `vspcli build` | `vsp dev dist --out ./dist` |
| `vspcli validate` | `vsp dev validate` |

`vspcli reconcile` also remains available through the compatibility route and maps to `--extensions-only` (changing only extensions, never `settings.json`, `storage.json`, or RTK wrappers). Drop `--extensions-only` only when you want `vsp sync -p rust` to manage profile settings, registration, and enabled RTK wrappers as well.

Move raw branch export downloads to the release profile archive. The archive materializes the 22 legacy bundle filenames. Both `ai-profile-crisp.code-profile` and `ai-profile-retina.code-profile` contain the Copilot-only bundle.

The v3.0.0 workflow retires `scripts/.vspcli-completion.zsh` and `scripts/.vspcli-completion.fish` in favor of `vsp completions zsh` and `vsp completions fish`. It retires `scripts/setup-rust-toolchain.sh`, `scripts/setup-cpp-toolchain.sh`, and `profiles/rust/tasks.example.json` as setup entry points. Install language toolchains separately and use `vsp sync --workspace PATH` for workspace settings, extension recommendations, and Rust documentation tasks where applicable.

See [Contributing](CONTRIBUTING.md) for source paths and checks, [About](ABOUT.md) for architecture, and [Security](SECURITY.md) to report a vulnerability.

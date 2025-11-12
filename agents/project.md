# VS Code Profiles: Project Overview

> Canonical/global architecture guidance still lives at `~/.config/agents/project.md`. Use that file for cross-editor policies and keep this VS Code–specific document in sync whenever shared conventions change.

Purpose
- Centralized management of multiple VS Code profiles (Java, Rust, C/C++, Web/Astro) with a shared editor baseline and per‑profile overrides.
- Keep rendering/typography consistent via shared files, while language tools and workflow live in profile overrides.

Layout (repo root: `~/.config/vscode`)
- _shared/
  - editor-crisp.jsonc — shared editor/terminal config for “crisp” variants
  - editor-retina.jsonc — shared editor/terminal config for “retina” variants
- _overrides/
  - <profile>.jsonc — per‑profile settings (language/build/tooling); strict JSON only
  - Overrides can declare `"@extends": ["other-base.jsonc"]` to reuse settings (e.g., Spring profiles extend `java-profile-base.jsonc` plus `java-spring-base.jsonc` extras).
- _merged/
  - <profile>.json — auto-generated merged settings (do not edit)
- profiles/
  - <profile>/extensions.json — declared extensions per profile
  - <profile>/settings.json — symlink to _merged/<profile>.json (managed by composer)
- scripts/
  - compose-settings.sh — merges shared base + override into _merged and refreshes symlinks in profiles/
  - export-profiles.sh — builds exports/<profile>.code-profile from _merged + extensions
  - validate-json.sh — runs jq over every JSON/JSONC fragment and extensions list
  - install-extensions.sh — installs the extensions declared in `profiles/<name>/extensions.json` via `code --profile ... --install-extension`
  - tests/run.sh — smoke-tests helper scripts (validate, compose, export, open-profiles via mock `code`)
  - git-hooks/pre-commit — auto-runs validation, composition, and exports before commits (enable via `git config core.hooksPath ~/.config/vscode/scripts/git-hooks`)
- open-profiles.sh — installs extensions (once per profile) and opens each profile so it appears in the VS Code profile switcher
- Shell helpers (bash/zsh via `~/.config/shell/xdg.sh`, fish via `~/.config/fish/conf.d/xdg-vscode.fish`):
  - `vsp <profile> [path]` / `vspi <profile> [path]` — call `code --profile` / `code-insiders --profile`.
  - `vspl` / `vspli` — list the managed profiles (reads `profiles/`).
  - `vsext-list <profile>` — list extensions installed for a given profile.
  - `vsext-install <profile>` — sync the profile’s declared extensions (`profiles/<name>/extensions.json`).
  - `vsregen [<profiles...>]` — run validation + composition. No args = all profiles.
  - `vsexport [<profiles...>]` — export `.code-profile` bundles. No args = all profiles.
- exports/
  - <profile>.code-profile — portable exports for “Profiles: Import Profile”
- agents/
  - README.md, project.md, instructions.md — VS Code–specific docs for LLMs/maintainers

Shared Themes & Icons
- Theme extensions installed by default: Catppuccin (catppuccin.catppuccin-vsc), Tokyo Night (enkia.tokyo-night), Material Theme (zhuangtongfa.Material-theme), Night Owl (sdras.night-owl), One Dark Pro (akamud.vscode-theme-onedark), Dracula Official (dracula-theme.theme-dracula).
- Icon themes installed by default: Material Icon Theme (pkief.material-icon-theme), vscode-icons (vscode-icons-team.vscode-icons), Catppuccin Icons (catppuccin.catppuccin-vsc-icons).
- `workbench.colorTheme` and `workbench.iconTheme` defaults live in `_shared/editor-*.jsonc`; individual profiles override in `_overrides/<profile>.jsonc` when specialization is needed.
- Rendering defaults (state-of-the-art readability):
  - Fonts: JetBrains Mono (preferred) with fallbacks (JetBrainsMono Nerd Font, Cascadia Code, IBM Plex Mono, SF Mono, system monospace) and ligatures/variable axes enabled.
  - GPU: terminal + editor leverage VS Code's GPU pipeline (`terminal.integrated.gpuAcceleration=on`, renderer auto) plus minimum contrast ratio for legibility.
  - Font smoothing: crisp profile forces `workbench.fontAliasing=antialiased`; retina uses `auto` so macOS/Wayland HiDPI pick subpixel AA.
  - Inline docs & IntelliSense: hover previews, inline suggestions, code lens, parameter hints, detailed suggestion UI are enabled globally (see `_shared/editor-*.jsonc`).

Profiles
- cpp-clangd — Clangd workspace with LLVM toolchain, CMake Tools, clang-tidy, CodeLLDB, Better C++ Syntax, Resource Monitor, SonarLint, GitLens, shared themes/icons.
- cpp-intellisense — Microsoft cpptools + cpptools extension pack, CMake Tools, CodeLLDB, Better C++ Syntax, shared themes/icons.
- web-astro-crisp / web-astro-retina — Astro + TypeScript/JavaScript + HTML/CSS with Prettier/ESLint, Tailwind, HTML/CSS IntelliSense, npm/path completion, Emmet in `.astro`.
- java-profile-crisp / java-profile-retina — General-purpose Java dev (VS Code Java Pack, Maven+Gradle helpers, Docker, GitLens, SonarLint, Java Debug, Lombok, Checkstyle, PMD, XML/YAML).
- java-gradle-crisp / java-gradle-retina — Gradle-first Java (profile base plus Gradle language/completion extensions, Lombok/Checkstyle/PMD/XML/YAML).
- java-maven-crisp / java-maven-retina — Maven-first Java (profile base plus Maven dependency explorer, Lombok/Checkstyle/PMD/XML/YAML).
- java-spring-crisp / java-spring-retina — Spring Boot–focused profiles (Java Pack + Debug, Spring Boot dashboard, Spring Initializr/Cloud, Lombok, Checkstyle, PMD, Maven + Gradle helpers, XML/YAML, Docker, SonarLint).
- rust-profile-crisp / rust-profile-retina — Rust Analyzer, CodeLLDB, Dependi, Even Better TOML, Cargo tooling, plus shared productivity extensions. Requires `rustup component add rust-src rustfmt clippy` on each machine.
- ai-profile-crisp / ai-profile-retina — Shared AI assistant workspace (Copilot, Copilot Chat, Claude Code, Gemini AI Studio, Continue, Codeium, Tabnine, YAML/GitHub Actions helpers) with the chat command center enabled by default.

Tooling Notes
- Java
  - Uses `${env:JAVA_HOME}`; shell init enables `jenv export` plugin so the active jenv JDK feeds JDT LS, Gradle, and Maven.
  - `scripts/setup-java-toolchain.sh <profile> [workspace]` syncs extensions and can copy the tasks template (`profiles/java-*/tasks.example.json`) into your project’s `.vscode/tasks.json`.
  - Hovers/Javadoc enabled, sources auto-download
- Rust
  - rust-analyzer proc macros enabled; check-on-save; all features
  - CodeLLDB for debugging
  - Use `scripts/setup-rust-toolchain.sh [profile]` to install rustup components + VS Code extensions. Cargo task templates live under `profiles/rust-profile-*/tasks.example.json` for quick build/test bindings.
- C/C++
  - Both profiles assume the system toolchain on `$PATH` (clang/gcc). Use `scripts/setup-cpp-toolchain.sh <profile>` to copy kit/toolchain templates into `.vscode/`.
  - Copy `profiles/<profile>/cmake/tasks.example.json` if you want ready-made configure/build/test tasks per workspace.
  - cpp-clangd: clangd settings with clang-tidy, CodeLLDB, watcher excludes for build dirs.
  - cpp-intellisense: cpptools + CMake Tools, consistent CMake logging/parallel jobs, watcher excludes.

Build/Debug Templates (User scope)
- tasks: Library/Application Support/Code/User/tasks.json
- launch: Library/Application Support/Code/User/launch.json

Conventions
- Crisp vs Retina is determined by the profile name (contains “retina” → editor-retina.jsonc; otherwise editor-crisp.jsonc).
- Do not place comments in override files; jq merges strict JSON only.

Plan-Test-Commit Workflow
- Apply the plan step-by-step: after each planned step, rerun `scripts/validate-json.sh` (and `code --version` if a VS Code CLI call is involved) before proceeding so regressions surface immediately.
1. **Plan**: Break down requested work into explicit steps and record/communicate the plan before editing (mirrors the LLM planning tool).
2. **Validate JSON**: After each configuration edit, run `bash ~/.config/vscode/scripts/validate-json.sh` so jq catches syntax mistakes immediately.
3. **Compose Profiles**: Run `bash ~/.config/vscode/scripts/compose-settings.sh <profiles...>` (or without args for all) to refresh `_merged` outputs and profile symlinks.
4. **Check VS Code CLI**: Run `code --version` (or `code-insiders --version`) to ensure the CLI is available before relying on tasks/hooks.
5. **Export Profiles**: Run `bash ~/.config/vscode/scripts/export-profiles.sh` to regenerate `exports/*.code-profile`.
6. **Commit**: Only commit after the above steps succeed; the pre-commit hook enforces validation/composition/export automatically, but manual runs keep iterations tight.

Common Operations
- Recompose all profiles after edits
  - bash "~/.config/vscode/scripts/compose-settings.sh"
- Recompose selected profiles
  - bash "~/.config/vscode/scripts/compose-settings.sh" cpp-clangd java-gradle-retina
- Validate JSON fragments quickly
  - bash "~/.config/vscode/scripts/validate-json.sh"
- Export all profiles for import sharing
  - bash "~/.config/vscode/scripts/export-profiles.sh"
- Script smoke tests (validate + compose + export + mocked open-profiles log)
  - bash "~/.config/vscode/scripts/tests/run.sh"
- Open all profiles once so they appear in UI (reads from `profiles/`)
  - bash "~/.config/vscode/scripts/open-profiles.sh"
- Import a profile into VS Code (UI)
  - Profiles: Import Profile → Import from a file → pick exports/<profile>.code-profile

Git Hooks
- Hooks live in `scripts/git-hooks/`. Set them up with:
  - `git config core.hooksPath ~/.config/vscode/scripts/git-hooks`
- `pre-commit` runs `validate-json.sh`, `compose-settings.sh`, and `export-profiles.sh` automatically. The hook blocks commits on failures so merged settings/exports never drift.

Extensibility
- To add a new profile:
  - Create folder profiles/<profile>/ with extensions.json (IDs list objects)
  - Create _overrides/<profile>.jsonc with language/tooling settings
  - Recompose to generate and link settings.json
  - Run export-profiles.sh to publish a .code-profile for sharing
- CLI helper:
  - `scripts/vspcli` — standalone command-line interface to list profiles, install extensions, open workspaces, compose/export without invoking multiple scripts manually (install completions via `vspcli --completion bash|zsh|fish` when you want shell-native tab completion).

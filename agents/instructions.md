# Maintenance Instructions for LLM Agent

> Global/host-wide runbook lives at `~/.config/agents/instructions.md`. Mirror any policy changes there and treat this file as the VS Code–specific supplement.

Scope
- Maintain VS Code profiles stored under `~/.config/vscode/profiles`. Keep shared editor settings DRY via `_shared/`, and language/tool specifics in `_overrides/`. Always use the composer to regenerate symlinked `profiles/<profile>/settings.json`.

Golden Rules
- Do NOT edit any file in _merged — it is generated.
- Keep _overrides/*.jsonc strictly valid JSON (no comments). jq cannot parse JSONC comments.
- Shared bases (_shared/editor-*.jsonc) should also be valid JSON; they are applied first, then overrides win on conflicts.

Edit Paths
- Shared editor (typography/rendering) changes that affect many profiles:
  - _shared/editor-crisp.jsonc and/or _shared/editor-retina.jsonc
- Per‑profile language/build/tool settings:
  - _overrides/<profile>.jsonc
- Extensions per profile:
  - profiles/<profile>/extensions.json (list of extension objects with identifier.id, version optional)
- To avoid duplication, an override file may declare `["@extends": ["other-base.jsonc"]]`; the composer resolves those chains before merging (Spring profiles extend `java-profile-base.jsonc` this way).

Themes & Icons
- Shared theme pack (installed for every profile):
  - Catppuccin (catppuccin.catppuccin-vsc), Tokyo Night (enkia.tokyo-night), Material Theme (zhuangtongfa.Material-theme), Night Owl (sdras.night-owl), One Dark Pro (akamud.vscode-theme-onedark), Dracula Official (dracula-theme.theme-dracula)
- Shared icon pack:
  - Material Icon Theme (pkief.material-icon-theme), vscode-icons (vscode-icons-team.vscode-icons), Catppuccin Icons (catppuccin.catppuccin-vsc-icons)
- Default selections (per _shared editor files) can be overridden in _overrides/<profile>.jsonc when needed.
- Typography, GPU & IntelliSense defaults:
  - Fonts: JetBrains Mono + JetBrainsMono Nerd Font fallback (ligatures + variable axes enabled) with Cascadia Code, IBM Plex Mono, SF Mono, monospace fallback.
  - GPU: terminal + editor renderers force hardware acceleration with auto renderer choice and a minimum contrast ratio of 4.5 for accessibility.
  - Font smoothing: crisp profiles pin `workbench.fontAliasing=antialiased`; retina profiles leave it `auto` for HiDPI subpixel AA.
  - Inline docs/IntelliSense: hover definition preview, inline suggestions, code lens, parameter hints, linked editing, suggest preview/status bar/locality bonus all enabled globally. Override per profile only if tooling conflicts.

Plan-Test-Commit Procedure
- Execute every plan step sequentially. After finishing a step, immediately run `bash ~/.config/vscode/scripts/validate-json.sh` (and `code --version` if the step touches VS Code CLI workflows) before moving on so regressions are caught at the exact step that introduced them.
1. **Plan** every non-trivial change and record the plan steps before touching files (mirrors the planning tool requirements).
2. After each edit, run `bash ~/.config/vscode/scripts/validate-json.sh` to ensure _shared, _overrides, and profiles/*/extensions.json remain valid JSON.
3. Run `bash ~/.config/vscode/scripts/compose-settings.sh <profiles...>` (or no args for all) to refresh `_merged` outputs and `profiles/<profile>/settings.json` symlinks.
4. Verify the VS Code CLI is available with `code --version` (or `code-insiders --version`) so downstream tasks and hooks can succeed.
5. Run `bash ~/.config/vscode/scripts/export-profiles.sh` to keep `exports/*.code-profile` in sync.
6. Install/update extensions for any touched profile (new machines or when extension sets change):
   - `bash ~/.config/vscode/scripts/install-extensions.sh <profile>`
7. Run `bash ~/.config/vscode/scripts/tests/run.sh` for a quick smoke test (validates JSON, compose, export, and open-profiles with a mocked `code` CLI + log verification).
8. Only commit after steps 2‑7 succeed; the git pre-commit hook will enforce them but manual runs keep debugging quick.

Apply Changes
- Recompose all profiles after edits:
  - bash "~/.config/vscode/scripts/compose-settings.sh"
- Recompose a subset (faster iteration):
  - bash "~/.config/vscode/scripts/compose-settings.sh" <profile> [more profiles]
- Verify symlinks (ensures each profile points at _merged output):
  - ls -l "~/.config/vscode/profiles/"*/settings.json

Validation Helpers
- JSON validation (shared + overrides + extensions):
  - bash "~/.config/vscode/scripts/validate-json.sh"
- Ensure `jq` is installed; the script fails fast if missing.

VS Code Tasks
- Profiles: Compose All
  - Runs the composer over every profile.
- Profiles: Compose (Prompt)
  - Prompts for space-separated profile names; blank runs all.
  - Command Palette → “Tasks: Run Task” → choose one of the above.

Profiles & Intent
- C/C++ (cpp-*)
  - `cpp-clangd`: ships clangd, LLVM toolchain helpers, CMake Tools, clang-tidy, CodeLLDB, Better C++ Syntax, SonarLint, GitLens, Docker, Resource Monitor.
  - `cpp-intellisense`: Microsoft cpptools + extension pack, CMake Tools, CodeLLDB, Better C++ Syntax, GitLens/SonarLint/Docker.
- Java (java-*)
  - Use jenv-managed JDKs: ensure `jenv enable-plugin export` and add JDKs (`jenv add ...`). VS Code settings resolve JDK via `${command:jenv.javaHome}` for runtimes and Gradle/Maven tooling.
  - Hovers/Javadoc: editor.hover.enabled=true, java.signatureHelp.description.enabled=true, sources auto-download.
  - General profiles install Java Pack plus Java Debug, Lombok, Checkstyle, PMD, XML/YAML tooling, Docker, SonarLint, GitLens.
  - Spring profiles (`java-spring-*`) add Spring Boot dashboard, Spring Initializr/Cloud, Lombok, Checkstyle, PMD, and both Maven + Gradle helpers.
  - Gradle/Maven profiles restrict build-tool extensions accordingly; general java-profile-* includes both.
  - Gradle profiles add Gradle language/completion extensions; Maven profiles include VS Code Maven + dependency explorer.
  - Quick setup: `bash ~/.config/vscode/scripts/setup-java-toolchain.sh <profile> [workspace]` ensures extensions are installed and optionally copies the profile’s `tasks.example.json` into your workspace `.vscode/tasks.json`.
- Rust (rust-*)
  - rust-analyzer.procMacro.enable=true; rust-analyzer.check.command=check; rust-analyzer.cargo.features=all.
  - Keep CodeLLDB + Dependi + Even Better TOML installed for debugging, dependency insights, and manifest editing. (Deprecated `serayuzgur.crates` was removed; Dependi + rust-analyzer cover dependency views.)
  - Before using the profile on a new machine run `rustup component add rust-src rustfmt clippy`; rust-analyzer relies on those components for completion, hover docs, formatting, and clippy-on-save.
  - Ensure the VS Code extension `rust-lang.rust-analyzer` remains enabled for these profiles (profiles ship with it, but disabling it once disables it for that profile).
  - Quick setup: `bash ~/.config/vscode/scripts/setup-rust-toolchain.sh [profile]` installs the components and re-syncs extensions. Optional Cargo task templates live under `profiles/rust-profile-*/tasks.example.json`.
- Web (web-astro-*)
  - Astro + TypeScript/JavaScript + HTML/CSS. Defaults: Prettier on save for JS/TS/HTML/CSS/JSON; Astro files use the Astro formatter.
  - IntelliSense: HTML/CSS (class/id), path and npm script completion, Tailwind IntelliSense (mapped for Astro), Emmet inside `.astro`.
  - Linting: ESLint and Stylelint enabled by default; TOML formatting via Even Better TOML.
  - Open with `vsp web-astro-crisp <path>` (or retina variant). Install with `scripts/install-extensions.sh web-astro-crisp`.
- AI (ai-profile-*)
  - Bundles GitHub Copilot + Copilot Chat only. Other assistants (Claude, Gemini, Continue, Codeium, Tabnine) are intentionally excluded.
  - Chat command center enabled by default (`workbench.startupEditor=chatView`). Open with `vsp ai-profile-crisp <path>` or retina variant.
  - Run `bash ~/.config/vscode/scripts/install-extensions.sh ai-profile-crisp` (or retina) on new machines and sign in to Copilot.
- C/C++
  - cpp-intellisense: Microsoft C/C++ IntelliSense. CMake Tools provider; compile_commands at ${workspaceFolder}/build.
  - cpp-clangd: clangd with background index + clang-tidy; compile_commands dir set to ${workspaceFolder}/build.
- User scope includes tasks/launch templates for CMake.

Opening Profiles
- Use `vspcli --open-profiles` to open one or more profiles; add `--skip-install` to skip extension installation when you only want to register or preview themes.

Tasks/Debug Templates (User scope)
- tasks.json provides:
  - CMake: Configure (Debug)
  - Compile Commands: Link to workspace root
  - CMake: Build (Debug) → depends on Configure + Link
- launch.json provides:
  - (lldb) Launch CMake Target → uses ${command:cmake.launchTargetPath}

Profile Exports
- Regenerate portable exports for importing in VS Code:
  - bash "~/.config/vscode/scripts/export-profiles.sh"
  - Outputs live in exports/*.code-profile
- Format: `{ "settings": { ... }, "extensions": { "enabled": ["ext.id"...] } }`

Open/Import Profiles
- Open all once (adds to VS Code profile switcher):
  - bash "~/.config/vscode/scripts/open-profiles.sh" (ensures extensions are installed the first time each profile is opened; set `VSCODE_SKIP_EXTENSION_INSTALL=1` to skip)
- Import via VS Code UI:
  - Profiles: Import Profile → Import from a file → select exports/<profile>.code-profile
- CLI helpers:
  - Bash/Zsh source `~/.config/shell/xdg.sh`; Fish loads `~/.config/fish/conf.d/xdg-vscode.fish`.
  - `vsp <profile> [path]` / `vspi <profile> [path]` wrap `code --profile` / `code-insiders --profile`.
  - `vspl` / `vspli` list the managed profiles by enumerating `profiles/`.
  - `vsext-list <profile>` outputs the extensions currently installed for that profile.
  - `vsext-install <profile>` runs `scripts/install-extensions.sh` to ensure declared extensions are installed.
  - `vsregen [<profiles...>]` runs validation + composition (wrapper over `validate-json.sh` + `compose-settings.sh`).
  - `vsexport [<profiles...>]` runs `export-profiles.sh` to rebuild `.code-profile` bundles.
  - `scripts/vspcli` is a standalone CLI for listing profiles, installing extensions, opening workspaces, composing or exporting from a single command (use `vspcli --completion bash|zsh|fish` to install tab-completion snippets quickly).

Quality & Safety
- Validate JSON before composing: jq . <file> (or run validate-json.sh).
- Keep ${env:JAVA_HOME} not hardcoded; do not replace with absolute JDK paths.
- For clangd, prefer /opt/homebrew/opt/llvm/bin/clangd on Apple Silicon; fall back to /usr/bin/clangd if needed.
- When adding global editor changes, prefer updating _shared files to avoid divergence.

Git Hooks
- Hooks live in `~/.config/vscode/scripts/git-hooks/`. Enable them with:
  - `git config core.hooksPath ~/.config/vscode/scripts/git-hooks`
- `pre-commit` automatically runs `validate-json.sh`, `compose-settings.sh`, and `export-profiles.sh`. The hook blocks commits if any step fails, guaranteeing merged settings and exports stay current.

Adding a New Profile
1) Create folder profiles/<profile>/ and add extensions.json
2) Create _overrides/<profile>.jsonc (strict JSON)
3) Run composer to create symlinked settings.json
4) Run export-profiles.sh to build exports/<profile>.code-profile
5) Commit only after validation + hooks steps succeed

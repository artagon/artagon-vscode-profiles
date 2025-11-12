# VS Code Profiles (XDG Managed)

This repository tracks all Visual Studio Code profiles, shared settings, and helper scripts under `~/.config/vscode`. It replaces the stock `User/` folder with a reproducible, profile-aware layout and scripting toolkit. Clone it anywhere (e.g., `~/src/vscode-profiles`) if you prefer—profile symlinks stay relative, so the layout remains portable even when teammates do not follow the same XDG conventions.

## Quick Start (First‑Time Users)
- Requirements: VS Code CLI on PATH (`code --version`) and `jq`.
- Compose + export:
  - `bash scripts/compose-settings.sh && bash scripts/export-profiles.sh`
- Open a profile without installing extensions (registers it):
  - `scripts/open-profiles.sh --skip-install web-astro-crisp`
- Install extensions for a profile:
  - `bash scripts/install-extensions.sh web-astro-crisp`
- Tip: `vspcli --open-profiles --skip-install <profiles...>` to open multiple at once.

### Profiles Overview
- Web (Astro/Node): `web-astro-(crisp|retina)` with Astro syntax, Prettier/ESLint, Tailwind, HTML/CSS/npm/path IntelliSense, Emmet in `.astro`.
- Java: `java-*-*` (jenv-driven via `${command:jenv.javaHome}`), Spring/Maven/Gradle focused variants.
- Rust: `rust-profile-(crisp|retina)` (rust-analyzer, CodeLLDB, Dependi, TOML).
- C/C++: `cpp-clangd-(crisp|retina)` and `cpp-intellisense-(crisp|retina)`.
- AI bundle: `ai-profile-(crisp|retina)` with GitHub Copilot + Copilot Chat only.

## Layout Overview
- `_shared/` — base editor/terminal settings (`editor-crisp` & `editor-retina` variants).
- `_overrides/` — per-profile JSON fragments. Overrides may declare `"@extends"` to inherit other fragments (e.g., Spring profiles extend `java-profile-base.jsonc`).
- `_merged/` — generated settings; never edit directly.
- `profiles/<name>/` — per-profile `settings.json` symlink (into `_merged/`) plus `extensions.json` describing the extension bundle.
- `scripts/` — helper scripts (`validate-json.sh`, `compose-settings.sh`, `export-profiles.sh`, `open-profiles.sh`, git hooks, tests).
 - compose-settings.sh — merges shared base + override into _merged and refreshes symlinks in profiles/
  - export-profiles.sh — builds exports/<profile>.code-profile from merged settings + extensions
  - validate-json.sh — runs jq over every JSON/JSONC fragment and extensions list
  - install-extensions.sh — installs the extensions declared for a profile via `code --install-extension`
  - open-profiles.sh — opens each profile once so it appears in the VS Code profile switcher
 - setup-cpp-toolchain.sh / setup-rust-toolchain.sh / setup-java-toolchain.sh — quick-start scripts for language-specific toolchains
  - vspcli — command-line helper for listing profiles, opening workspaces, installing extensions, composing, exporting
  - scripts/tests/run.sh — smoke-tests helper scripts (validate, compose, export, open-profiles via mock `code`)
- `exports/` — portable `.code-profile` bundles for importing via VS Code’s “Profiles: Import Profile”.
- `agents/` — documentation used by LLM assistants (`README.md`, `project.md`, `instructions.md`).

## Standalone install (non-XDG environments)
1. Clone the repo wherever it is convenient (no need for `~/.config`). Example: `git clone https://… ~/src/vscode-profiles`.
2. From that directory, run `bash scripts/validate-json.sh` and `bash scripts/compose-settings.sh` to refresh `_merged/` and recreate the relative `profiles/*/settings.json` links.
3. Export the profiles you need via `bash scripts/export-profiles.sh` so coworkers can import `exports/<profile>.code-profile` directly inside VS Code (Profiles: Import Profile).
4. Optionally run `bash scripts/install-extensions.sh <profile>` first so VS Code installs the declared extensions before importing.
5. Use CLI helpers via absolute paths (e.g., `~/src/vscode-profiles/scripts/vspcli --list`) or add the repo’s `scripts/` directory to `$PATH`. No additional environment variables or host-level symlinks are required.

## Common Tasks
> Workflow rule: break non-trivial requests into plan steps and, after finishing each step, immediately run `scripts/validate-json.sh` (plus `code --version` when CLI usage is involved) before continuing. This keeps JSON/CLI regressions localized to the step that introduced them.
1. **Plan & edit**: For non-trivial work, outline steps, then edit `_shared/`, `_overrides/`, or `profiles/*/extensions.json` as needed.
2. **Validate JSON**: `bash ~/.config/vscode/scripts/validate-json.sh`
3. **Compose settings**: `bash ~/.config/vscode/scripts/compose-settings.sh` (optionally pass profile names).
4. **Export profiles**: `bash ~/.config/vscode/scripts/export-profiles.sh`
5. **Install extensions for a profile** (first-time setup / new machine): `bash ~/.config/vscode/scripts/install-extensions.sh <profile>` (or `vsext-install <profile>`). The installer groups output by stack (AI, CMake, Java, Rust, General) so it’s easy to see what’s being installed. Use `--group <name>` (repeatable) to install only a subset (e.g., `--group AI --group Java`), or pass `all` as the profile name to cover every profile at once. Run this the first time you open a profile so VS Code has the declared extensions ready.
6. **Smoke tests**: `bash ~/.config/vscode/scripts/tests/run.sh` (validates, composes, exports, then mocks `code --profile` calls and checks logs).
7. **Open profiles**: `bash ~/.config/vscode/scripts/open-profiles.sh [profile ...]` to register either every profile (no args) or a filtered list with the VS Code CLI. The script installs extensions (set `VSCODE_SKIP_EXTENSION_INSTALL=1` to skip during automation) **and** now copies the freshly merged `profiles/<name>/settings.json` into VS Code’s cached profile directory (uses `jq` + `~/Library/Application Support/Code/User` on macOS; override with `VSCODE_USER_DIR` if needed) so the editor immediately reflects the latest fonts/theme tweaks even if you previously imported a profile snapshot.
8. **Import a `.code-profile` bundle via CLI**: `bash ~/.config/vscode/scripts/import-profile.sh <profile> <path/to/file>` (or `vspcli --profile-import <profile> <file>`). The importer writes the bundle’s settings into VS Code’s cached profile store and installs every listed extension so teammates can restore shared profiles headlessly.
9. **Add AI tooling everywhere**: run `bash ~/.config/vscode/scripts/install-extensions.sh <profile>` after pulling the repo to ensure AI extensions (Copilot, Claude, Gemini, Continue, Codeium, OpenAI, Tabnine, GitHub Actions helpers, etc.) are installed for that profile. All non-AI profiles now include the AI bundle directly in their `extensions.json`.

### Shell helpers
- Bash/Zsh: defined in `~/.config/shell/xdg.sh` and sourced from the respective rc files.
- Fish: defined in `~/.config/fish/conf.d/xdg-vscode.fish`.
- Commands:
- `vsp <profile> [path]` / `vspi <profile> [path]` — call `code --profile` / `code-insiders --profile`.
- `vspl` / `vspli` — list the managed profiles (reads `profiles/`).
- `vsext-list <profile>` — list extensions currently installed for a profile via `code --profile ... --list-extensions`.
- `vsext-install <profile>` — run the installer script to synchronize extensions declared in `profiles/<profile>/extensions.json`.
- `vsregen [<profiles...>]` — run `validate-json.sh` + `compose-settings.sh` (all profiles if none specified).
- `vsexport [<profiles...>]` — run `export-profiles.sh` (all profiles if none specified).
- `scripts/vspcli` provides the same workflows via a getopt-style CLI.

## Git Hooks
Enable the provided hooks so commits automatically validate and rebuild profiles:
```bash
git config core.hooksPath ~/.config/vscode/scripts/git-hooks
```

## Java & jenv
Shell init enables `jenv enable-plugin export`, so `JAVA_HOME` always matches the active jenv JDK. Every Java-oriented profile (including Gradle, Maven, and Spring variants) reads `${env:JAVA_HOME}` for JDT LS, Gradle, Maven, and Spring tooling—no manual path edits required.

## Rust toolchain prerequisites
Rust-oriented profiles expect the following components to be installed once per host:
```bash
rustup component add rust-src rustfmt clippy
```
`rust-src` powers IntelliSense/hover docs, while `rustfmt` and `clippy` back format-on-save and linting tasks.
Make sure the VS Code extension `rust-lang.rust-analyzer` remains enabled for the Rust profiles (disabling it once disables it per-profile).

### Quick setup
- Run `bash ~/.config/vscode/scripts/setup-rust-toolchain.sh [profile]` to install the required rustup components and synchronize VS Code extensions for the given profile (defaults to `rust-profile-crisp`).
- Copy `profiles/rust-profile-*/tasks.example.json` into `.vscode/tasks.json` if you want Cargo build/test/clippy commands available in the VS Code Tasks UI.

## Java toolchain automation
- Run `bash ~/.config/vscode/scripts/setup-java-toolchain.sh <profile> [workspace]` to ensure extensions are installed for the Java profile and (optionally) copy the tasks template into your workspace `.vscode/tasks.json`. The script checks `jenv`/`JAVA_HOME` and reminds you to enable the export plugin if needed.
- Tasks templates live under `profiles/java-*/tasks.example.json` (Gradle-only, Maven-only, or combined depending on the profile).

## AI collaboration profiles
- `ai-profile-crisp` and `ai-profile-retina` ship with the popular AI coding extensions (GitHub Copilot + Copilot Chat, Claude Code, Gemini AI Studio, Continue, Codeium, Tabnine, YAML/GitHub Actions helpers, etc.).
- Use `vsp ai-profile-crisp <path>` (or retina) to open a workspace with the command center targeting AI chats by default.
- First-time setup: `bash ~/.config/vscode/scripts/install-extensions.sh ai-profile-crisp` so VS Code installs the bundle on your machine. Sign in to Copilot, Claude, Gemini, etc., via their respective panels afterward.

## CLI helper (vspcli)
`scripts/vspcli` exposes common operations from the terminal:

```bash
vspcli --list
vspcli --install ai-profile-crisp
vspcli --open ai-profile-crisp /path/to/workspace
vspcli --compose cpp-clangd cpp-intellisense
vspcli --export java-profile-crisp
vspcli --install-ext rust-profile-retina github.copilot
vspcli --profile-import rust-profile-retina ~/Downloads/team-standard.code-profile
vspcli --open-profiles rust-profile-retina java-profile-retina
vspcli --install all
vspcli --install-groups java-profile-crisp AI Rust
```

Combine flags as needed; run `vspcli --help` for details.

Both the script and CLI accept `--group` filters (`AI`, `CMake`, `Java`, `Rust`, `General`). Example: `vspcli --install java-profile-crisp --group AI --group Java` installs only those stacks, while `vspcli --install all` covers every profile.

Install shell completions (optional):
```bash
vspcli --completion bash  >> ~/.config/shell/completions/vspcli.bash
vspcli --completion zsh   >> ~/.config/zsh/completions/_vspcli
vspcli --completion fish  >> ~/.config/fish/completions/vspcli.fish
```
Adjust the target paths to whatever your shell sources so the CLI works even on machines where the helper functions are absent.

## Adding a Profile
1. Create `profiles/<name>/extensions.json` describing required extensions.
2. Add `_overrides/<name>.jsonc` (strict JSON). Use `"@extends"` to reuse existing fragments if possible.
3. Run the compose + export scripts and smoke tests (steps above).
4. Optionally generate `exports/<name>.code-profile` via the export script for easy sharing.

For more detail, see `agents/project.md` (architecture) and `agents/instructions.md` (maintenance workflow). Host-wide (non-editor-specific) policies remain in `~/.config/agents/project.md` and `~/.config/agents/instructions.md`; update those whenever a change should apply beyond VS Code.
## Custom toolchains (C/C++)
By default the C/C++ profiles rely on whatever toolchain is on `$PATH` (e.g., `/usr/bin/clang`). For custom setups:

1. Copy `profiles/<profile>/cmake-kits.example.json` to `.vscode/cmake-kits.json`. Use `scripts/setup-cpp-toolchain.sh <profile>` to automate this copy (also populates `cmake/toolchains/`).
2. Edit the kits with your compiler paths (Homebrew LLVM, MSVC). Optional: supply a toolchain file under `cmake/toolchains/` and reference it from the kit.
3. Copy `profiles/<profile>/cmake/tasks.example.json` to `.vscode/tasks.json` if you want ready-made configure/build/test commands on the command palette.
4. Use `CMake: Select Kit` in VS Code to pick the kit locally; avoid committing machine-specific paths to `_overrides`.

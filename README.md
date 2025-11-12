# VS Code Profiles (XDG Managed)

This repository tracks all Visual Studio Code profiles, shared settings, and helper scripts under `~/.config/vscode`. It replaces the stock `User/` folder with a reproducible, profile-aware layout and scripting toolkit. Clone it anywhere (e.g., `~/src/vscode-profiles`) if you prefer—profile symlinks stay relative, so the layout remains portable even when teammates do not follow the same XDG conventions.

## About

[![OS macOS](https://img.shields.io/badge/os-macOS-000000?logo=apple&logoColor=white)](#)
[![OS Linux](https://img.shields.io/badge/os-Linux-0b7261?logo=linux&logoColor=white)](#)
[![OS Windows](https://img.shields.io/badge/os-Windows-0078D4?logo=windows&logoColor=white)](#)
[![AI Copilot Only](https://img.shields.io/badge/AI-Copilot%20only-1f6feb?logo=githubcopilot&logoColor=white)](#)
[![Topics](https://img.shields.io/badge/profiles-astro%2C%20java%2C%20rust%2C%20c%2B%2B-4a7bbb)](#)

- Portable, profile‑based VS Code setup with crisp/retina shared baselines and per‑profile overrides.
- Web/Astro (TypeScript/JS + HTML/CSS), Java via jenv, Rust, and C/C++ stacks pre‑configured.
- AI policy: GitHub Copilot + Copilot Chat only (no other assistants).
- Scripts for validate/compose/export/open/install; exports ready for “Profiles: Import Profile”.

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

## Profile Matrix

| Profile | Stack | Key Extensions (subset) | Notable Settings / Notes |
|---|---|---|---|
| `web-astro-(crisp|retina)` | Astro · Node · TS/JS · HTML/CSS | `astro-build.astro-vscode`, `dbaeumer.vscode-eslint`, `esbenp.prettier-vscode`, `bradlc.vscode-tailwindcss`, `ecmel.vscode-html-css` | Prettier on save for web files; Astro formatter; Emmet in `.astro`; Tailwind mapped for Astro; TOML formatting enabled |
| `java-profile-(crisp|retina)` | Java (general) | Java Debug/Test/Dependency, Maven, Gradle, Lombok, Checkstyle, PMD, XML/YAML, SonarLint, GitLens | JDK resolved via `${command:jenv.javaHome}`; Gradle & Maven both enabled |
| `java-gradle-(crisp|retina)` | Java (Gradle‑first) | Gradle language/completion, Java tooling | Maven import disabled; jenv for JDK |
| `java-maven-(crisp|retina)` | Java (Maven‑first) | VS Code Maven + dependency explorer, Java tooling | Gradle import disabled; jenv for JDK |
| `java-spring-(crisp|retina)` | Spring Boot | Spring Boot/Initializr/Cloud, Java tooling | `spring-boot.ls.java.home` via jenv; YAML schema for Spring; Java formatter defaults |
| `rust-profile-(crisp|retina)` | Rust | `rust-lang.rust-analyzer`, `vadimcn.vscode-lldb`, `tamasfe.even-better-toml`, `panicbit.cargo`, `fill-labs.dependi` | clippy on save; watcher excludes `**/target`; TOML formatting |
| `cpp-clangd-(crisp|retina)` | C/C++ (clangd) | `llvm-vs-code-extensions.vscode-clangd`, `ms-vscode.cmake-tools`, `ms-vscode.cpptools` | Microsoft IntelliSense disabled; clang‑tidy enabled; CMake Ninja; watcher excludes build dirs |
| `cpp-intellisense-(crisp|retina)` | C/C++ (cpptools) | `ms-vscode.cpptools`, `ms-vscode.cmake-tools` | cpptools provider; `compile_commands.json` path; CMake Ninja |
| `ai-profile-(crisp|retina)` | Utilities / AI | `github.copilot`, `github.copilot-chat` (+ common utilities) | Copilot‑only policy; Chat command center enabled |

## Guides (Step‑by‑Step)

Below are copy‑pasteable shell snippets for each stack. Each block shows: compose → open (register) → install → a minimal smoke test. Replace `*-crisp` with `*-retina` if you prefer the HiDPI baseline.

### Web (Astro/Node)
Install Node LTS, compose/open the profile, install extensions, and scaffold an Astro app.

```bash
# 1) Node.js LTS (pick your favorite version manager)
#    macOS example with nvm:
export NVM_DIR="$HOME/.nvm" && [ -s "/opt/homebrew/opt/nvm/nvm.sh" ] && . "/opt/homebrew/opt/nvm/nvm.sh"
nvm install --lts && nvm use --lts
node -v && npm -v

# 2) Compose + open (register profile without installing extensions)
bash scripts/compose-settings.sh web-astro-crisp
vspcli --open-profiles --skip-install web-astro-crisp

# 3) Install extensions declared by the profile
bash scripts/install-extensions.sh web-astro-crisp

# 4) Create and run an Astro project
npm create astro@latest my-astro-app -- --template starter
cd my-astro-app
npm install
npm run dev

# Notes: Prettier/ESLint format on save for JS/TS/HTML/CSS/JSON.
# Astro files use the Astro formatter; Emmet works in .astro.
# Tailwind IntelliSense mapped for Astro.
```

### Java (jenv; Gradle/Maven/Spring)
Use jenv so profiles can resolve the active JDK automatically.

```bash
# 1) Install jenv and register JDKs (macOS example)
brew install jenv
echo 'export PATH="$HOME/.jenv/bin:$PATH"' >> ~/.zshrc
echo 'eval "$(jenv init -)"'          >> ~/.zshrc
source ~/.zshrc

jenv add /Library/Java/JavaVirtualMachines/<jdk>/Contents/Home
jenv global <version>

# 2) Compose + open (register), then install extensions
bash scripts/compose-settings.sh java-profile-crisp
vspcli --open-profiles --skip-install java-profile-crisp
bash scripts/install-extensions.sh java-profile-crisp

# Tips: use java-gradle-* / java-maven-* / java-spring-* for focused workflows.
# Profiles read the JDK via ${command:jenv.javaHome} — no manual JAVA_HOME.
```

### Rust
Install the toolchain and compose/open/install the profile.

```bash
# 1) Toolchain + components
curl https://sh.rustup.rs -sSf | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rust-src rustfmt clippy

# 2) Compose + open (register), then install extensions
bash scripts/compose-settings.sh rust-profile-crisp
vspcli --open-profiles --skip-install rust-profile-crisp
bash scripts/install-extensions.sh rust-profile-crisp

# Notes: rust-analyzer + clippy on save; CodeLLDB for debugging; Dependi + Even Better TOML.
```

### C/C++ (clangd or Microsoft cpptools)
Pick either clangd (fast/precise LSP) or cpptools (Microsoft IntelliSense). Install build tools, then compose/open/install.

```bash
# 1) Build tools (macOS examples)
brew install cmake ninja llvm   # clangd in LLVM for the clangd profile

# 2) Compose + open (clangd example) and install extensions
bash scripts/compose-settings.sh cpp-clangd-crisp
vspcli --open-profiles --skip-install cpp-clangd-crisp
bash scripts/install-extensions.sh cpp-clangd-crisp

# Alternative: Microsoft C/C++ IntelliSense profile
# bash scripts/compose-settings.sh cpp-intellisense-crisp
# vspcli --open-profiles --skip-install cpp-intellisense-crisp
# bash scripts/install-extensions.sh cpp-intellisense-crisp

# Tips: build dir defaults to ${workspaceFolder}/build (watcher excludes set).
# clangd profile disables Microsoft IntelliSense; cpptools uses CMake Tools provider
# and compile_commands.json for accurate navigation/completion.
```

### AI (Copilot)
Open the AI profile, install Copilot + Chat, then sign in when prompted.

```bash
vspcli --open-profiles --skip-install ai-profile-crisp
bash scripts/install-extensions.sh ai-profile-crisp

# Sign in to GitHub Copilot and Copilot Chat when prompted.
# Policy: other AI assistants are intentionally excluded.
```

## Layout Overview

Directory tree (top‑level):

```
.
├─ _shared/           # editor baselines (crisp/retina)
├─ _overrides/        # per‑profile settings (strict JSON; supports "@extends")
├─ _merged/           # merged settings (generated)
├─ profiles/          # per‑profile extensions.json + settings symlink
├─ scripts/           # helpers (compose/export/validate/open/install, CLI)
├─ exports/           # .code-profile bundles (for VS Code import)
├─ agents/            # maintainer/LLM docs
├─ branding/          # social preview assets
└─ .github/           # issue/PR templates, CODEOWNERS, funding
```

Paths and purpose:

| Path | Purpose | Notes |
|---|---|---|
| `_shared/` | Base editor/terminal settings | JSONC files: `editor-crisp.jsonc`, `editor-retina.jsonc` |
| `_overrides/` | Per‑profile overrides | Strict JSON; supports `"@extends"` chains |
| `_merged/` | Generated merged settings | Do not edit; created by composer |
| `profiles/` | Profile manifests | `extensions.json` + `settings.json` symlink into `_merged/` |
| `scripts/` | Tooling scripts + CLI | compose/export/validate/open/install, git hooks, tests, `vspcli` |
| `exports/` | Portable bundles | Import via VS Code “Profiles: Import Profile” |
| `agents/` | Docs for maintainers/LLMs | Architecture + maintenance workflow |
| `branding/` | Social preview assets | `social-preview.svg` (export PNG, upload in repo settings) |
| `.github/` | Repo metadata | Issue/PR templates, CODEOWNERS, FUNDING |

Scripts quick reference:

| Script | Action | Example |
|---|---|---|
| `scripts/compose-settings.sh` | Merge shared + overrides → `_merged/` and refresh symlinks | `bash scripts/compose-settings.sh java-spring-*` |
| `scripts/export-profiles.sh` | Build `.code-profile` bundles | `bash scripts/export-profiles.sh web-astro-crisp` |
| `scripts/validate-json.sh` | Validate JSON/JSONC fragments and extensions lists | `bash scripts/validate-json.sh` |
| `scripts/open-profiles.sh` | Open/register profiles (optionally skip installs) | `scripts/open-profiles.sh --skip-install rust-profile-crisp` |
| `scripts/install-extensions.sh` | Install declared extensions (group filters supported) | `bash scripts/install-extensions.sh <profile> --group General` |
| `scripts/vspcli` | Unified CLI for list/open/install/compose/export | `vspcli --open-profiles --skip-install <profiles...>` |
| `scripts/tests/run.sh` | Smoke test: validate→compose→export→open (mock) | `bash scripts/tests/run.sh` |

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

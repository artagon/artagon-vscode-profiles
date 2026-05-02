# VS Code under XDG

This folder manages Visual Studio Code configuration in an XDG‑friendly, portable way.

## Canonical Layout
- Profiles live in `~/.config/vscode/profiles/<profile-name>` (settings symlink + extensions.json).
- Shared fragments stay at the repo root: `_shared/`, `_overrides/`, `_merged/`.
- Helper scripts (composer, profile opener) stay in `~/.config/vscode/scripts/`.
- LLM-facing docs stay in `~/.config/vscode/agents/` (this folder).

## Runtime Relocation (State/Cache)
To keep `~/.config` free of runtime data, these are moved and symlinked:
- State: `~/.local/state/vscode/User/{sync,History,globalStorage,Backups,logs}`
- Cache: `~/.cache/vscode/User/workspaceStorage`

## Version Control
- A `.gitignore` here excludes volatile content:
  - `User/globalStorage/`, `User/workspaceStorage/`, `User/logs/`, `User/Backups/`, `User/sync/`, `User/*.db`
- Stable config tracked: `_shared/*.jsonc`, `_overrides/*.jsonc`, `profiles/*/extensions.json`, `scripts/*.sh`, etc.

## Layered Composition
Compose settings from fragments using `compose-settings.sh`:
- Fragments:
  - `_shared/editor-crisp.jsonc` or `_shared/editor-retina.jsonc` (base, chosen by profile name).
  - `_overrides/<profile>.jsonc` (per-profile JSON overrides, strict JSON).
- Usage:
  - `bash ~/.config/vscode/scripts/compose-settings.sh` (all profiles) or pass profile names.
  - Requires `jq`.
  - Outputs merged files `_merged/<profile>.json` and symlinks `profiles/<profile>/settings.json`.

## Profiles
- List profiles: `vspl` (stable), `vspli` (Insiders)
- Open with profile: `vsp "Profile Name"` or `vspi "Profile Name"`
- Backed by `scripts/open-profiles.sh` (iterates over `profiles/*` and uses `code`/`code-insiders`; falls back to macOS `open -a`).

### C++ Variants
- clangd: `cpp-clangd-crisp`, `cpp-clangd-retina`
- cpptools (IntelliSense): `cpp-intellisense-crisp`, `cpp-intellisense-retina`

### Web (Astro/Node)
- `web-astro-crisp`, `web-astro-retina`

### CLI Tips
- Open profiles without installing extensions:
  - `vspcli --open-profiles --skip-install <profile> [more profiles]`
  - Or: `scripts/open-profiles.sh --skip-install <profile> [more profiles]`
- Compose and export all profiles:
  - `scripts/compose-settings.sh && scripts/export-profiles.sh`

## Rendering Defaults
- Fonts: JetBrains Mono (primary) with professional fallback chain: JetBrainsMono Nerd Font, Monaspace Neon (GitHub), Geist Mono (Vercel), Berkeley Mono, Fira Code, Cascadia Code (Microsoft), Intel One Mono, IBM Plex Mono, SFMono-Regular (macOS), monospace. Ligatures + variable axes enabled.
- To customize fonts: edit `editor.fontFamily` and `terminal.integrated.fontFamily` in `_shared/editor-crisp.jsonc` and/or `_shared/editor-retina.jsonc`, then recompose with `bash scripts/compose-settings.sh`.
- GPU: hardware acceleration forced for terminals/editors with contrast ratio safeguards.
- Font antialiasing: crisp profiles force `antialiased`; retina leaves `auto` for HiDPI.
- Inline docs & IntelliSense: hover previews, inline suggestions, code lens, parameter hints, linked editing, and IntelliSense detail panes enabled by default.
- Notifications suppressed: VS Code update popups, extension update popups, welcome pages, walkthroughs, and tips are all disabled globally.

## Java JDK via jenv
- Java profiles now use `${command:jenv.javaHome}` to resolve the active JDK.
- Prerequisite: install and initialize jenv, add your JDKs, and enable the VS Code `jenv.javaHome` command (provided by the jenv extension).
- Typical setup:
  - brew install jenv; echo 'export PATH="$HOME/.jenv/bin:$PATH"' >> ~/.zshrc; echo 'eval "$(jenv init -)"' >> ~/.zshrc
  - jenv add /Library/Java/JavaVirtualMachines/<jdk>/Contents/Home
  - jenv global <version>
  - Install the jenv VS Code extension if prompted.

## Agents Docs (LLM Context)
- `project.md` and `instructions.md` here contain VS Code–specific guidance (plan/test workflow, hooks, etc.).
- Canonical machine-wide docs still live at `~/.config/agents/`; review them when changes should apply globally.
- Convenience symlinks: `~/.config/vscode/.agents`, `.claude`, `.gemini` -> `~/.config/vscode/agents`

## AI Assistants
- AI-only profiles (ai-profile): GitHub Copilot + Copilot Chat only.
- AI-plus profiles (ai-plus): Copilot + Cody (Sourcegraph) + Continue.
- Language profiles (Java, Rust, C++, Web): include Copilot, Claude Code (`anthropic.claude-code`), Codex (`openai.chatgpt`), and Continue (`Continue.continue`).
- AI fragment configs in `_overrides/ai/`: copilot.jsonc, claude-code.jsonc, codex.jsonc, continue.jsonc, cody.jsonc.

## Maintenance
- Migrate/ensure links: `xdg-migrate vscode`
- Health check: `xdg-status`
- Keys status: `xdg-keys`
- Edit shell helpers in `~/.config/shell/xdg.sh` (fish: `~/.config/fish/conf.d/xdg-vscode.fish`).

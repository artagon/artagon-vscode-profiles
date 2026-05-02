# File Locations Cheat Sheet

Authoritative path table for VS Code configuration, per OS and per variant. Verified against official VS Code docs as of April 2026.

## Path placeholders

- macOS `$HOME` → `~` → `/Users/<username>`
- Linux `$HOME` → `~` → `/home/<username>`
- Windows `%APPDATA%` → `C:\Users\<username>\AppData\Roaming`
- Windows `%USERPROFILE%` → `C:\Users\<username>`
- Windows `%LOCALAPPDATA%` → `C:\Users\<username>\AppData\Local`

On Linux, VS Code's User dir lives under `~/.config/Code/` by default. Electron's `userData` resolution *will* consult `$XDG_CONFIG_HOME` if it is set in the process environment when VS Code starts, but in practice most distro launchers (deb, rpm, snap, flatpak, AUR, tarball wrappers) do not preserve a custom `$XDG_CONFIG_HOME` from the user's interactive shell — they read environment from `/etc/environment` or systemd user units instead. The result: most users who set `XDG_CONFIG_HOME=/my/path` and restart VS Code still find their User dir at `~/.config/Code/`. Multi-year upstream issue history (microsoft/vscode#21428, #76722) confirms this is launcher-dependent, not a clean "yes, XDG works" answer. **Recommendation**: don't promise the user that relocating `XDG_CONFIG_HOME` will move the VS Code User dir. Instead, have them confirm via `Preferences: Open User Settings (JSON)` and inspect the status-bar path. The `argv.json` and extensions directories are a different story: they're hardcoded to `~/.vscode/` regardless of XDG settings — this is a long-standing feature request, not a bug fix that's coming.

## Stable VS Code (Microsoft build)

Parent folder name on every OS: **`Code`**

| File | macOS | Linux | Windows |
| --- | --- | --- | --- |
| User `settings.json` | `~/Library/Application Support/Code/User/settings.json` | `~/.config/Code/User/settings.json` | `%APPDATA%\Code\User\settings.json` |
| User `keybindings.json` | `~/Library/Application Support/Code/User/keybindings.json` | `~/.config/Code/User/keybindings.json` | `%APPDATA%\Code\User\keybindings.json` |
| User snippets | `~/Library/Application Support/Code/User/snippets/` | `~/.config/Code/User/snippets/` | `%APPDATA%\Code\User\snippets\` |
| User tasks | `~/Library/Application Support/Code/User/tasks.json` | `~/.config/Code/User/tasks.json` | `%APPDATA%\Code\User\tasks.json` |
| Profile `settings.json` | `~/Library/Application Support/Code/User/profiles/<id>/settings.json` | `~/.config/Code/User/profiles/<id>/settings.json` | `%APPDATA%\Code\User\profiles\<id>\settings.json` |
| Profiles index | `~/Library/Application Support/Code/User/profiles/profiles.json` | `~/.config/Code/User/profiles/profiles.json` | `%APPDATA%\Code\User\profiles\profiles.json` |
| User MCP config | `~/Library/Application Support/Code/User/mcp.json` | `~/.config/Code/User/mcp.json` | `%APPDATA%\Code\User\mcp.json` |
| Extensions | `~/.vscode/extensions/` | `~/.vscode/extensions/` | `%USERPROFILE%\.vscode\extensions\` |
| `argv.json` (runtime args) | `~/.vscode/argv.json` | `~/.vscode/argv.json` | `%USERPROFILE%\.vscode\argv.json` |
| Logs | `~/Library/Application Support/Code/logs/` | `~/.config/Code/logs/` | `%APPDATA%\Code\logs\` |
| Workspace storage | `~/Library/Application Support/Code/User/workspaceStorage/` | `~/.config/Code/User/workspaceStorage/` | `%APPDATA%\Code\User\workspaceStorage\` |

**The two-folder split is the part people miss:** `User/` (under `Code/`) holds settings, keybindings, snippets, profiles. `~/.vscode/` holds extensions and `argv.json`. They are not the same folder.

## Linux: Snap and Flatpak sandboxes

Snap and Flatpak builds redirect VS Code's HOME-relative paths into sandbox roots. Users on Ubuntu (Snap by default) and Fedora (Flatpak common) will *not* find their settings under `~/.config/Code/`. Always check both sandbox locations before declaring a config "missing":

| Install method | User dir | Extensions / argv.json |
| --- | --- | --- |
| Native deb/rpm/tarball | `~/.config/Code/User/` | `~/.vscode/` |
| Snap (`snap install code`) | `~/snap/code/current/.config/Code/User/` | `~/snap/code/current/.vscode/` |
| Flatpak (`com.visualstudio.code`) | `~/.var/app/com.visualstudio.code/config/Code/User/` | `~/.var/app/com.visualstudio.code/data/vscode/` |

VSCodium and Code-OSS Snap/Flatpak builds follow the same pattern, swapping the application id (`codium`, `com.vscodium.codium`, etc.). When a user reports "I edited settings.json but nothing changed," confirm install method first — many editing the unsandboxed path while VS Code reads the sandbox copy.

## VS Code Insiders

Parent folder name: **`Code - Insiders`** (with the spaces and hyphen).

Substitute everywhere in the table above. Extensions and `argv.json` live in `~/.vscode-insiders/` instead of `~/.vscode/`.

## VSCodium

Parent folder name: **`VSCodium`**. Extensions and runtime args live in `~/.vscode-oss/` (note the `-oss` suffix — VSCodium reuses Code-OSS's extension dir).

## Code-OSS (the open-source upstream)

Parent folder name: **`Code - OSS`**. Extensions in `~/.vscode-oss/`.

## Cursor

Parent folder name: **`Cursor`**. Extensions in `~/.cursor/extensions/`. Cursor adds its own keys (e.g., `cursor.cpp.disabledLanguages`, `cursor.general.*`) to `settings.json` alongside standard `editor.*` keys.

## Windsurf (Codeium)

Parent folder name: **`Windsurf`**. Extensions in `~/.windsurf/extensions/`. Like Cursor, adds vendor-specific keys to a standard `settings.json`.

## Portable mode

If VS Code is run in portable mode, *everything* — settings, extensions, state — relocates to a `data/` folder next to the executable. Per-OS:

- macOS: `code-portable-data/` next to `Visual Studio Code.app`
- Linux: `data/` next to the `code` binary
- Windows: `data/` next to `Code.exe`

Inside `data/`, the structure is `data/user-data/User/` (settings, keybindings) and `data/extensions/`. To switch a normal install to portable mode, create the `data/` (or `code-portable-data/` on macOS) folder and restart VS Code.

## Remote (SSH/WSL/Container/Codespaces)

When connected to a remote, the remote settings file lives **on the remote**, not locally. The path is the same as a regular User `settings.json` but on the remote filesystem. For VS Code Server specifically:

- Linux remote (most common): `~/.vscode-server/data/User/settings.json` for stable, `~/.vscode-server-insiders/data/User/settings.json` for Insiders
- WSL: same as Linux remote, inside the WSL distro
- Codespaces: `/home/<user>/.vscode-remote/data/User/settings.json` typically, but Codespaces may layer additional defaults via `devcontainer.json`

Open the remote settings UI with `Preferences: Open Remote Settings (JSON)` while connected. Don't try to find it by guessing paths if the user is connected — use the command.

## Workspace files (per-project)

These don't vary by OS or variant. They live in the workspace itself:

| File | Location | Purpose |
| --- | --- | --- |
| `settings.json` | `<workspace>/.vscode/settings.json` | Workspace settings |
| `launch.json` | `<workspace>/.vscode/launch.json` | Debug configurations |
| `tasks.json` | `<workspace>/.vscode/tasks.json` | Tasks |
| `extensions.json` | `<workspace>/.vscode/extensions.json` | Recommendations |
| `mcp.json` | `<workspace>/.vscode/mcp.json` | Workspace MCP servers |
| Snippets | `<workspace>/.vscode/*.code-snippets` | Project snippets |
| Multi-root | `<anywhere>/<name>.code-workspace` | Multi-root workspace file |

For multi-root workspaces, the `.code-workspace` file can sit anywhere (commonly the parent of all the included folders) and contains the `folders` list plus inline `settings`, `launch`, `tasks`, `extensions` blocks.

## How to verify a path on someone's machine

If unsure about a user's actual paths, ask them to run in the Command Palette:

- `Preferences: Open User Settings (JSON)` — opens the file, status bar shows the absolute path.
- `Developer: Open Extensions Folder` — opens the extensions dir.
- `Developer: Open Logs Folder` — opens the logs dir.

Don't guess when they can confirm directly in two seconds.

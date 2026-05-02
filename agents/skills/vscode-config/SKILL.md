---
name: vscode-config
description: Authoritative knowledge of VS Code (and Cursor/Windsurf/VSCodium/Code-OSS/Insiders) configuration — settings, keybindings, launch, tasks, extensions, mcp, argv, .code-workspace, profiles — including scope precedence, per-OS paths, Settings Sync, and remote/devcontainer/Codespaces layers. Use whenever the user asks where a config lives, why a setting isn't taking effect, or for any task involving reading, writing, validating, or migrating a VS Code-family JSONC config (including multi-root workspaces, cross-variant moves, and portable mode).
---

# VS Code Configuration

This skill encodes how VS Code (and its variants) stores configuration: which files exist, where they live per OS, how scopes interact, and what tends to surprise people. Verified against current VS Code documentation as of April 2026.

## When this skill applies

- The user mentions any of: `settings.json`, `keybindings.json`, `launch.json`, `tasks.json`, `extensions.json`, `mcp.json`, `argv.json`, `.code-workspace`, `.vscode/`, "user settings", "workspace settings", "profile", "Settings Sync".
- The user asks where something is stored, why a setting is ignored, or how to share a config with a team.
- The user wants to migrate or merge configurations between machines, OSes, or VS Code variants (Insiders, VSCodium, Code-OSS, Cursor, Windsurf).
- A `.json` file is shown that uses VS Code-style keys like `editor.*`, `workbench.*`, `terminal.integrated.*`, `[language]`, or `version: "2.0.0"` task schema.

## Core mental model

VS Code resolves a setting by walking a stack of scopes. **Each setting also has its own scope declaration** that limits which layers it can be set in — that is the source of most confusion. There are two related but distinct concepts:

1. **Configuration scopes** (where you put the value): Default → User → Remote → Workspace → Workspace Folder. Later scopes win for primitive/array values; objects merge. Language-specific overrides (`"[python]": { ... }`) form a parallel stack on top.

2. **Setting scopes** (declared by the setting itself): `application`, `machine`, `machine-overridable`, `window`, `resource`, `language-overridable`. These constrain which configuration scopes are *allowed* to set a value. A `machine`-scoped setting placed in workspace settings is silently ignored with a warning the first time the workspace is opened.

If a workspace setting "isn't taking effect," the answer is almost always one of:
- The setting has `application` or `machine` scope (workspace can't override it — security).
- A language-specific override outranks it.
- The user is connected to a remote (SSH/WSL/Container/Codespaces) and the *remote* user settings layer is winning.
- The file is invalid JSONC and the parser bailed (look for trailing commas in strict-JSON tools, or duplicate keys).
- **Enterprise policy is locking it** (lock icon and "Managed by your organization" in the Settings editor) — see `references/precedence.md` for how Windows GPO, macOS configuration profiles, and Linux `/etc/vscode/policy.json` push values that override even user settings.

For the full precedence walkthrough and the list of `application`/`machine`-scoped settings that workspaces can't override, read `references/precedence.md`.

## File locations

VS Code's config layout is **OS-specific** and **variant-specific**. The Insiders build, VSCodium, Code-OSS, Cursor, and Windsurf all use different parent folders to avoid colliding with stable VS Code.

For the canonical per-OS, per-variant cheat sheet — including profile sub-paths and what each file is for — read `references/file-locations.md`. Always consult that file before telling the user a path; OS conventions and variant names are precisely the kind of detail that gets remembered wrong.

Quick reference for stable VS Code, default profile only:

| File | macOS | Linux | Windows |
| --- | --- | --- | --- |
| User `settings.json` | `~/Library/Application Support/Code/User/settings.json` | `~/.config/Code/User/settings.json` | `%APPDATA%\Code\User\settings.json` |
| User `keybindings.json` | same dir | same dir | same dir |
| `argv.json` (runtime args) | `~/.vscode/argv.json` | `~/.vscode/argv.json` | `%USERPROFILE%\.vscode\argv.json` |

Note that **`argv.json` lives in `~/.vscode/`, not in the User dir** — this trips people up. It's a small file for runtime flags like `--disable-hardware-acceleration` or `password-store`.

## Profiles

Profiles let users keep separate setting bundles (e.g., one for web dev, one for Python). Non-default profiles are stored under `User/profiles/<profile-id>/`. Critically:

- The **default profile** is the special case: it uses the regular `User/settings.json`, *not* a profile subfolder. Don't go looking for it under `profiles/`.
- A non-default profile's `settings.json` is **only created when the user actually changes a setting** in that profile — absence of the file ≠ broken profile.
- When inside a non-default profile, the command `Preferences: Open Application Settings (JSON)` opens the *default* profile's settings, not the current one.
- Settings Sync preserves profiles and downloads their extensions on demand.

The `<profile-id>` is a short hash-like string that VS Code generates; it's not the human-readable profile name. To find the active profile's ID, look at `User/profiles/profiles.json`.

## The `.vscode/` folder (per-project)

Project-scoped configuration lives in a `.vscode/` folder at the workspace root. The file inventory is fixed and well-defined; for each file's schema and gotchas, read `references/per-project-files.md`. The files are:

- `settings.json` — workspace settings (also `[language]` overrides allowed)
- `launch.json` — debug configurations
- `tasks.json` — build/test/run tasks (must declare `"version": "2.0.0"`)
- `extensions.json` — `recommendations` and `unwantedRecommendations` arrays; triggers a prompt when the workspace opens
- `mcp.json` — Model Context Protocol server configurations (workspace-scoped MCP servers)
- `c_cpp_properties.json`, `cmake-kits.json`, etc. — language- and extension-specific files

For multi-root workspaces, prefer a single `.code-workspace` file at the repository's parent directory. It can contain `folders`, `settings`, `launch`, `tasks`, `extensions` blocks all in one place. Workspace-folder-level settings (per individual folder in a multi-root workspace) still go in each folder's own `.vscode/settings.json` and override the multi-root file for that folder only.

## Variant editors

Cursor, Windsurf, VSCodium, Code-OSS, and VS Code Insiders are all VS Code forks or rebuilds. They share the file *formats* but use different *folder names* to keep their configurations separate. When the user asks about any of these, do not silently assume the stable VS Code paths apply — read `references/variants.md` for the complete folder-name table and the cross-variant migration guidance.

## JSONC, not JSON

All VS Code config files are **JSONC** (JSON with Comments). This is non-obvious and matters whenever you reach for a parser:

- `//` line comments and `/* */` block comments are allowed.
- Trailing commas are allowed in objects and arrays.
- Duplicate keys are tolerated by the parser but the *last* value wins (no error reported in many cases — silently shadow-bugs configs).
- Python's stdlib `json` module **will fail** on these files. Use `jsonc-parser` (Node), `json5`/`pyjson5`, or strip comments before parsing. A robust strip is non-trivial because of `//` inside strings — prefer a real JSONC parser.

When generating a settings file for the user, comments are encouraged for clarity. When *editing* an existing file, preserve their comments — losing a user's comments is a nasty surprise.

## Settings Sync

Settings Sync uploads to GitHub or Microsoft accounts and covers:

- User settings (default profile + named profiles)
- Keybindings (per-platform by default — change with `settingsSync.keybindingsPerPlatform`)
- User snippets, tasks, UI state
- Installed extensions and their global enablement
- Profiles (full set, with their own settings/keybindings/extensions/snippets)

It does **not** cover:

- Workspace settings (`.vscode/settings.json`)
- `argv.json`
- Settings declared with `machine` or `machine-overridable` scope (full list managed via `settingsSync.ignoredSettings`)
- Workspace trust decisions

If a user reports "my setting isn't syncing," check the scope of the setting first.

## Remote settings (SSH, WSL, Containers, Codespaces)

When connected to a remote, a fourth layer slides into the precedence chain between User and Workspace: **Remote settings**. This is the user-level settings file that lives *on the remote machine*. It has its own JSON file and its own UI in the Settings editor (use the "Remote" tab, or `Preferences: Open Remote Settings (JSON)`).

For Dev Containers specifically, `devcontainer.json` can include a `customizations.vscode.settings` block that bakes default settings into the container — those become the remote user settings inside the container. `customizations.vscode.extensions` works the same way for extensions. This is where most of the "but it works on my machine" config drift comes from in container-based dev workflows.

## Common workflows

When the user asks for help, locate the workflow first:

1. **"Why isn't my setting working?"** → Walk the precedence chain, check setting scope, check for language overrides, check for remote.
2. **"Where is X stored?"** → Consult `references/file-locations.md`. Confirm the OS and variant before answering.
3. **"Share this config with my team"** → Move it to `.vscode/settings.json` (and check for `machine`-scoped settings that won't survive the move).
4. **"Migrate from VSCodium to Cursor"** (or any cross-variant) → Read `references/variants.md`. Copy `User/` contents to the new variant's User dir; warn about extension compatibility.
5. **"Audit extensions.json"** → Read `extensions.json`, compare `recommendations` against an allowed list; flag entries in `unwantedRecommendations` that contradict.
6. **"Validate a settings.json"** → Use a JSONC parser. Common fail modes: trailing comma the user forgot, duplicate key shadowing, unclosed string.

## Bundled scripts

Three shell tools live under `scripts/`. Use them when the task can be automated rather than answered in prose; they're more reliable than eyeballing a config:

- **`scripts/vscode-jsonc-validate`** — validate a VS Code JSONC file. Detects syntax errors, duplicate top-level keys, and per-kind schema problems (wrong `tasks.json` version, malformed extension IDs, MCP servers missing `command`/`url`, etc.). Auto-detects file kind from basename or accepts `--kind`. Use this whenever a user pastes a config file or asks "is this valid?" — running the script is faster and catches issues a manual scan misses.

- **`scripts/vscode-extensions-audit`** — compare a `.vscode/extensions.json` against an allowed list of extension IDs (file or inline CSV). Reports rogue recommendations, items in both `recommendations` and `unwantedRecommendations`, and allowed items explicitly marked unwanted. Use this for workflow #5 above.

- **`scripts/vscode-profile-diff`** — JSONC-aware diff between two settings files. Ignores comments, key order, and trailing-comma variations. Outputs added/removed/changed keys, optionally as `--json` for further processing. Use this for workflow #4 (cross-variant or cross-profile migration) and for "what's different between my laptop's config and my desktop's" questions.

All three depend on `jq` and `awk`. They share a JSONC stripper (`scripts/jsonc-strip.awk`) — when writing your own shell that needs to parse a VS Code config, pipe through that stripper before handing to `jq`. The `scripts/tests/` directory has `bats` tests covering syntax, schema, and edge cases for each tool; run `scripts/run_tests.sh` to verify the tools work in the user's environment before relying on them.

## What this skill does NOT cover

- Default values for individual settings (use the Settings editor or the official docs — there are tens of thousands).
- Extension authoring (contributing settings via `package.json` `contributes.configuration`).
- Specific extension behaviors (Python interpreter selection, ESLint rules, etc.).
- VS Code's own source code or build configuration.

For those, search the official docs at https://code.visualstudio.com/docs or the extension's own README.

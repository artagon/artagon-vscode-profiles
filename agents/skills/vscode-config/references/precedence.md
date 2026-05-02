# Settings Precedence and Scopes

How VS Code decides which value wins when the same setting is defined in multiple places. Verified against current VS Code documentation (April 2026).

## The two scope concepts

The word "scope" gets used for two different things in the VS Code docs. They interact, which is what makes this confusing.

1. **Configuration scope** — the *layer* you put a value in (User, Workspace, Remote, etc.). This is what determines precedence.
2. **Setting scope** — a property of the *setting itself*, declared by the extension or core, that limits which configuration scopes are *allowed* to set it.

Most "why isn't my setting working" cases come down to: the user put a value in a configuration scope that the setting's declared scope doesn't permit, and VS Code silently discarded it.

## Configuration scopes (precedence chain)

Lowest to highest priority. Higher entries override lower entries.

1. **Default settings** — built-in defaults from VS Code core and from extensions.
2. **User settings** — `User/settings.json`. Global to all instances of VS Code on this machine.
3. **Remote settings** — applies only when connected to a remote (SSH, WSL, Container, Codespaces). Lives on the remote machine.
4. **Workspace settings** — `.vscode/settings.json` in the workspace, or the `settings` block of a `.code-workspace` file.
5. **Workspace Folder settings** — for multi-root workspaces, each folder's own `.vscode/settings.json` overrides the multi-root file *for that folder*.
6. **Language-specific default settings** — defaults contributed for a specific language (e.g., `[python]` defaults from the Python extension).
7. **Language-specific user settings** — `"[python]": { ... }` in user settings.
8. **Language-specific remote settings** — same, in remote settings.
9. **Language-specific workspace settings** — same, in workspace settings.
10. **Language-specific workspace folder settings** — same, in workspace folder settings.
11. **Enterprise policy** — applied via OS-level device management; wins over everything above. See "Enterprise policies" below.

**Key rule about language-specific settings:** they always beat non-language-specific settings, *even if the non-language-specific setting is in a higher-priority configuration scope* (excluding enterprise policy). A language-specific user setting beats a non-language-specific workspace setting. This is unusual and surprises people often.

**Merge vs. override:**
- **Primitives and arrays** — higher scope replaces lower scope entirely. An array doesn't get appended; the new array wins outright.
- **Objects** — higher scope merges with lower scope key-by-key. `workbench.colorCustomizations` is the canonical example. To delete a key from a lower scope, you can't — you can only override it with a different value.

## Enterprise policies (the silent override)

Since VS Code 1.69 (Windows) and later versions on macOS/Linux, organizations can push policy values that **override every other layer including user settings**. When a setting is policy-managed, the Settings editor shows it with a lock icon and the message "Managed by your organization." Editing user settings won't help — the policy re-applies on every launch.

Where policies live per OS:

| OS | Mechanism | Path |
| --- | --- | --- |
| Windows | Registry-based Group Policy (ADMX templates ship with VS Code) | `HKLM\SOFTWARE\Policies\Microsoft\VSCode` and `HKCU\SOFTWARE\Policies\Microsoft\VSCode` |
| macOS | Configuration profile (`.mobileconfig`), deployed via MDM or installed manually | Sample at `Visual Studio Code.app/Contents/Resources/app/policies/*.mobileconfig` |
| Linux | JSON file readable by all users | `/etc/vscode/policy.json` |

Policy-controllable settings are a curated subset, not the entire setting registry — examples include update mode, telemetry level, allowed extensions, MCP server access, chat features. The full list ships with each release in the `policies/` directory of the install. Settings that aren't covered by a policy continue to follow normal precedence.

When debugging "why won't this setting change," the lock icon is the giveaway. If the user is on a corporate machine and a setting is locked, the answer is policy and the fix is "talk to IT," not "edit a different file."

## Setting scopes (declared by the setting)

Each setting carries a scope that limits which configuration layers can set it. When a layer tries to set a value the scope forbids, VS Code ignores the value and (for workspace-level violations) shows a warning the first time the workspace is opened.

| Setting scope | Where it can be set | Synced by Settings Sync? | Typical use |
| --- | --- | --- | --- |
| `application` | User only (truly global) | Yes | Cross-instance behavior, e.g. update channel |
| `machine` | User or Remote (per-machine) | **No** | Paths, hardware-specific options |
| `machine-overridable` | User, Remote, Workspace, Folder | **No** | Path defaults that workspaces may override |
| `window` | User, Remote, Workspace (instance-wide) | Yes | UI options that apply to a window |
| `resource` | User, Remote, Workspace, Folder | Yes | Per-file/folder behavior |
| `language-overridable` | All of `resource`, plus per-language overrides | Yes | Editor options that vary per language |

**Workspace settings cannot override:**
- Anything `application`-scoped.
- Anything `machine`-scoped (security: a workspace shouldn't be able to dictate machine paths).
- A specific list of executable-path settings even if not formally `machine`-scoped, because letting an opened workspace point shells/runtimes at arbitrary binaries would be a code-execution vulnerability. The historical list includes things like `git.path`, `terminal.integrated.shell.*`, `php.validate.executablePath`, `python.pythonPath` (deprecated in favor of interpreter selection), etc. The first time a workspace tries to set one, VS Code warns the user and then ignores it permanently for that workspace.

If you're unsure of a setting's declared scope, the Settings editor shows it: open the setting's gear menu, the scope appears in the metadata. Or check `package.json` `contributes.configuration` for extension-contributed settings.

## Per-platform overrides

A setting can be platform-specific via dedicated keys. The convention is `<setting>.<platform>` where `<platform>` is `windows`, `linux`, or `osx` (note: `osx`, not `macos` — historical artifact). Examples:

```jsonc
{
  "terminal.integrated.env.osx": { "FOO": "bar" },
  "terminal.integrated.env.linux": { "FOO": "bar" },
  "terminal.integrated.env.windows": { "FOO": "bar" }
}
```

These are independent keys, not a special syntax — VS Code just happens to read the right one based on the OS. There's no fallback chain; if you set `osx` only, Linux gets nothing.

## Keybindings precedence

Keybindings work on a different model — there is no scope chain, just two layers:

1. Default keybindings (built-in + extension-contributed)
2. User `keybindings.json` (overrides defaults)

There is no workspace-level keybindings file. If you need workspace-specific shortcuts, use the `when` clause on a user-level binding to scope it to a workspace condition, or use `tasks.json` with `keybindings` in user settings pointing at task labels.

`keybindings.json` can have entries with a leading `-` on the `command` field — that *removes* a binding rather than adding one. This is the correct way to disable a default keybinding without rebinding it.

## Settings Sync exclusions in detail

By default, Settings Sync ignores:
- Anything `machine` or `machine-overridable` scoped.
- A small built-in ignore list (e.g., `window.zoomLevel` historically).
- Anything the user adds to `settingsSync.ignoredSettings`.

Keybindings are synced **per-platform** by default — your macOS keybindings don't sync to your Windows machine. Override with `settingsSync.keybindingsPerPlatform: false` if your bindings are platform-agnostic.

Profiles sync as full bundles. Each profile carries its own settings, keybindings, snippets, tasks, extensions, and UI state.

## Debugging "my setting isn't working"

A checklist, in the order to apply:

1. **Is the JSONC valid?** Look for trailing commas, unclosed strings, duplicate keys (last one wins silently). Use the Problems panel or the Settings editor's "Open Settings (JSON)" — it'll highlight syntax errors.
2. **What's the setting's declared scope?** If `machine` or `application`, workspace can't set it.
3. **Is there a language-specific override?** A `"[python]": { ... }` block in any layer beats non-language-specific in higher layers.
4. **Connected to a remote?** Remote user settings sit between user and workspace; they may be winning.
5. **Multi-root workspace?** Workspace folder settings beat the multi-root `.code-workspace` settings for files in that folder.
6. **Profile?** The active profile's settings file is what's loaded — not the default profile's.
7. **Settings editor confirmation:** open the Settings editor, search the setting, click the gear → "Copy Setting as JSON" — the value shown is the *resolved* value after precedence.

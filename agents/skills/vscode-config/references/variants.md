# VS Code Variants

VS Code is forked, rebuilt, and rebranded under several names. They share the file *formats* but not the *folder names*, and they have varying levels of additional features that introduce non-standard settings keys.

## Variant overview

| Variant | Origin | Folder name | Extension dir | Notes |
| --- | --- | --- | --- | --- |
| **VS Code (Stable)** | Microsoft | `Code` | `~/.vscode/extensions/` | The reference implementation |
| **VS Code Insiders** | Microsoft | `Code - Insiders` | `~/.vscode-insiders/extensions/` | Daily build, can run alongside Stable |
| **Code-OSS** | Open-source upstream of VS Code (the MIT-licensed source) | `Code - OSS` | `~/.vscode-oss/extensions/` | Built from `microsoft/vscode` repo without proprietary additions |
| **VSCodium** | Telemetry-free rebuild of Code-OSS | `VSCodium` | `~/.vscode-oss/extensions/` | Same binaries-from-source as Code-OSS, no Microsoft branding |
| **Cursor** | Closed-source fork by Anysphere | `Cursor` | `~/.cursor/extensions/` | Adds AI features; maintains its own marketplace (Anysphere-curated, with Open VSX as one upstream source) plus Anysphere-published shims for popular extensions |
| **Windsurf** | Closed-source fork by Codeium | `Windsurf` | `~/.windsurf/extensions/` | AI-focused fork; own marketplace |

Other AI-focused forks exist (Trae, Kiro, Void, etc.) and follow the same general pattern — their own parent folder name, their own extensions dir — but the exact paths vary by version, so check `--user-data-dir` defaults rather than guessing.

The *parent folder name* (third column) is what replaces `Code` in all paths from `file-locations.md`. The *extension directory* (fourth column) replaces `~/.vscode/extensions/`. The `argv.json` file follows the extension directory's parent (`~/.cursor/argv.json`, `~/.windsurf/argv.json`, etc.).

## Why the differences matter

Two reasons configs don't trivially port between variants:

1. **Marketplace differences** — Microsoft restricts the official VS Code Marketplace to its own builds. Cursor maintains its own catalog (Anysphere-curated, with [Open VSX](https://open-vsx.org/) as one upstream source) and publishes Anysphere-maintained replacements for popular extensions. VSCodium and Code-OSS have always used Open VSX. Windsurf maintains its own catalog. Practical consequence: an extension recommendation that works in VS Code Stable may not be installable in any of these forks without a manual VSIX install — and the VSIX may resolve to a different (forked or shimmed) build than the official Marketplace would have served.

2. **Vendor-specific keys** — Cursor and Windsurf add their own settings keys to a regular `settings.json`. Examples:
   - Cursor: `cursor.cpp.disabledLanguages`, `cursor.general.enableAutoUpdate`, `cursor.aiPreview`
   - Windsurf: `windsurf.cascade.*`, `windsurf.autocomplete.*`
   - VS Code Stable: would just ignore these keys silently — they don't break anything.

   Going the other direction (stable VS Code keys in Cursor's settings) generally works because Cursor inherits VS Code's setting registry.

## Migrating between variants

The reliable migration recipe:

1. **Copy the User folder.** The whole `User/` subdirectory of the old variant's parent folder maps directly onto the new variant's parent folder. This brings settings, keybindings, snippets, profiles, tasks.

   ```bash
   # Example: VS Code → VSCodium on macOS
   cp -R "~/Library/Application Support/Code/User/" "~/Library/Application Support/VSCodium/User/"
   ```

2. **Don't copy the extensions directory.** Extensions are tied to the variant's marketplace and binary. Re-install via the recommendations flow or by listing them and running `<variant-cli> --install-extension <id>` for each.

3. **Strip vendor-specific keys** (only relevant going Cursor → VS Code or Windsurf → VS Code). They won't break anything but they pollute the file.

4. **Review `keybindings.json`** — the platform-specific note from Settings Sync also applies to manual migration. If you're moving across OSes at the same time, expect modifier mismatches.

5. **`argv.json` flags** — variant-dependent. Cursor and Windsurf may not accept all the flags VS Code Stable does.

## Identifying which variant a config came from

If someone hands you a config without context:

- **Vendor-specific keys** in `settings.json` are the strongest signal. `cursor.*` → Cursor; `windsurf.*` → Windsurf; `editor.inlineSuggest.suppressSuggestions` plus a `github.copilot.*` block → likely VS Code with Copilot.
- **Extension IDs in `extensions.json`** that aren't on Open VSX are a hint that the file came from VS Code Stable or Insiders rather than VSCodium / Code-OSS.
- **Profile structure** under `User/profiles/` is the same across all VS Code-derived variants.

## Code Server / Browser builds

Several browser-based VS Code derivatives exist (code-server by Coder, github.dev, vscode.dev, GitPod, etc.). Storage models vary widely — some persist to disk like a normal install, others use IndexedDB in the browser, others sync via account. Don't assume a path; ask the user how they're running it before answering location questions.

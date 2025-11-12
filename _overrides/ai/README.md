# AI Config Fragments

Reusable settings fragments for AI assistants. Include them in a profile override via `@extends`.

## Usage

In `_overrides/<profile>.jsonc`:

```json
{
  "$schema": "https://json.schemastore.org/vscode-settings",
  "@extends": [
    "ai/copilot.jsonc"
  ]
}
```

## Fragments

- `ai/copilot.jsonc` — Sensible defaults for GitHub Copilot + Chat:
  - Enable Copilot for code (disable in plaintext/SCM input)
  - Turn on inline suggestions (via `editor.inlineSuggest.enabled`)
  - Enable Chat + command center + chatView on startup
  - Don’t auto‑open the Copilot panel

> Note: Advanced/undocumented keys (e.g., `github.copilot.advanced.*`) aren’t included. Prefer official settings to avoid breakage across updates.

## Continue/Cody

Continue and Cody expose many provider‑specific options (local LLMs, enterprise endpoints). These vary by setup, so we recommend configuring them in VS Code’s Settings UI or in workspace `.vscode/settings.json` and tool‑specific files (e.g., `.continue/config`). Skeleton fragments are included here as placeholders you can extend locally:

- `ai/continue.jsonc` — empty shell; add your Continue settings here if you want to reuse via ``.
- `ai/cody.jsonc` — empty shell; add Cody settings (autocomplete, chat toggles) as needed.

Helpful links:
- Continue: https://marketplace.visualstudio.com/items?itemName=continue.continue
- Cody: https://marketplace.visualstudio.com/items?itemName=sourcegraph.cody-ai


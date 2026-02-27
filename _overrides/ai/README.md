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
  - Don't auto-open the Copilot panel

- `ai/claude-code.jsonc` — Placeholder for Anthropic Claude Code settings.
  - Extension: `anthropic.claude-code` (official Anthropic extension, 5M+ installs)
  - Config is primarily managed through `~/.claude/settings.json` and env vars (`ANTHROPIC_API_KEY`)
  - Supports direct API, Amazon Bedrock, and Google Vertex AI providers
  - https://marketplace.visualstudio.com/items?itemName=anthropic.claude-code

- `ai/codex.jsonc` — Placeholder for OpenAI Codex settings.
  - Extension: `openai.chatgpt` (official OpenAI extension, 4.9M+ installs)
  - Auth via ChatGPT account sign-in or API key
  - Agent mode (default), full-access mode, and chat-only mode available
  - https://marketplace.visualstudio.com/items?itemName=openai.chatgpt

- `ai/continue.jsonc` — Placeholder for Continue settings.
  - Extension: `Continue.continue` (open source, Apache 2.0, 2.2M+ installs)
  - Config via `~/.continue/config.yaml` or `config.json`
  - Supports any model provider: OpenAI, Anthropic, Ollama (local), Mistral, DeepSeek, Azure, etc.
  - https://marketplace.visualstudio.com/items?itemName=Continue.continue

- `ai/cody.jsonc` — Placeholder for Sourcegraph Cody settings.
  - Extension: `sourcegraph.cody-ai` (Enterprise only since July 2025)
  - For non-enterprise users, Sourcegraph now offers Amp (`sourcegraph.amp`)
  - https://marketplace.visualstudio.com/items?itemName=sourcegraph.cody-ai

> Note: Advanced/undocumented keys (e.g., `github.copilot.advanced.*`) aren't included. Prefer official settings to avoid breakage across updates. Claude Code and Codex manage their configuration outside VS Code settings (via `~/.claude/` and ChatGPT sign-in respectively), so the fragment files are empty shells.

## Extending

Continue and Cody expose many provider-specific options (local LLMs, enterprise endpoints). These vary by setup, so we recommend configuring them in VS Code's Settings UI or in workspace `.vscode/settings.json` and tool-specific files (e.g., `.continue/config`). The fragment files are placeholders you can extend locally.

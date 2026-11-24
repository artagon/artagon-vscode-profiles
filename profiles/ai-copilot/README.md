# AI Copilot-Only Profile (`ai-copilot`)

Strict GitHub Copilot-only VS Code profile (`github.copilot-chat`).

- Excludes `google.google-antigravity`, `anthropic.claude-code`, and `openai.chatgpt` (`modules/capabilities/ai-copilot` declares `conflicts: ["ai-agents"]`).
- Target of legacy `ai-profile-crisp` and `ai-profile-retina` aliases and export symlinks so existing Copilot-only consumers are never silently upgraded to additional AI vendors.

## Usage

```bash
# Import the Copilot-only bundle
bash scripts/import-profile.sh ai-copilot exports/ai-copilot.code-profile

# Migrate an existing profile to strict Copilot-only (removing Claude/Codex + openai.codex-audio)
bash scripts/vspcli reconcile --profile ai-copilot --as ai-copilot --prune-unmanaged
```

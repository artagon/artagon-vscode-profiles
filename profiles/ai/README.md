# AI Profile (`ai`)

Compiled VS Code profile featuring the default AI agent suite:
- **Google Antigravity** (`google.google-antigravity`) — default primary AI agent
- **Claude Code** (`anthropic.claude-code`)
- **OpenAI Codex** (`openai.chatgpt`)
- **GitHub Copilot Chat** (`github.copilot-chat`) — inline ghost-text completions and Next Edit Suggestions

For a strict **Copilot-only** profile (preserving the legacy `ai-profile-{crisp,retina}` policy), use [`profiles/ai-copilot/`](../ai-copilot/README.md).
For a zero-AI baseline (`"chat.disableAIFeatures": true`), use [`profiles/general-minimal/`](../general-minimal/README.md).

## Usage

```bash
# Import the compiled bundle (installs all extensions + verifies SHA256SUMS)
bash scripts/import-profile.sh ai exports/ai.code-profile

# Or sync extensions into an existing profile
bash scripts/vspcli reconcile --profile ai

# Apply personal UX preset (crisp or retina) to VS Code User settings
bash scripts/vspcli apply --ux=crisp --ux-target=user .
```

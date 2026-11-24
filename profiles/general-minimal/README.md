# General Minimal Profile (`general-minimal`)

Zero-AI, lightweight general editor profile.

- Excludes `ai-agents` and sets `"chat.disableAIFeatures": true` (disabling VS Code's built-in Copilot Chat integration as well as external AI extensions).
- Includes `core-editor`, `markdown`, and `shell` capabilities.

## Usage

```bash
# Import the zero-AI profile bundle
bash scripts/import-profile.sh general-minimal exports/general-minimal.code-profile

# Import any other bundle while stripping AI extensions and disabling built-in chat
bash scripts/import-profile.sh --no-ai rust-no-ai exports/rust.code-profile
```

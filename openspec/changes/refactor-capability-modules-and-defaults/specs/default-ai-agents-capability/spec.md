# Specification: Default AI Agents Capability

## Purpose

Defines the default `ai-agents` capability bundling Claude Code, OpenAI Codex, and unified GitHub Copilot Chat with verified VS Code `1.147.0` coexistence settings, a parsed-config autonomy deny-list, explicit opt-outs (build, workspace, and import), and strict `ai-copilot`.

## ADDED Requirements

### Requirement: Default AI agent trio in profile scope with AI-free and import opt-outs

`modules/capabilities/ai-agents/module.json` SHALL declare `requires: ["core-editor"]`, `conflicts: ["ai-copilot"]`, `extensions.workspace: []`, `extensions.profile: ["anthropic.claude-code", "openai.chatgpt", "github.copilot-chat"]`, and `extensions.unwanted: ["github.copilot"]`. It SHALL be in `catalog/defaults.json`, `profiles/general-minimal/` SHALL set `"chat.disableAIFeatures": true`, and `import-profile.sh --no-ai` SHALL skip `AI` extensions.

#### Scenario: Standard toolchain profile includes all three default AI agents
- **WHEN** `vspcli build rust` compiles `profiles/rust/profile.json`
- **THEN** `profiles/rust/extensions.json` and `exports/rust.code-profile` contain `anthropic.claude-code`, `openai.chatgpt`, and `github.copilot-chat`, and omit `github.copilot`.

#### Scenario: Workspace apply does not leak AI trio or unwanted entries into .vscode/extensions.json
- **WHEN** `vspcli apply --target=workspace /path/to/repo` runs with defaults
- **THEN** `/path/to/repo/.vscode/extensions.json` omits `anthropic.claude-code`, `openai.chatgpt`, `github.copilot-chat`, and `github.copilot`.

#### Scenario: AI-free profile disables built-in AI features and import --no-ai skips AI trio
- **WHEN** `profiles/general-minimal/` is built OR a user runs `scripts/import-profile.sh rust-clean exports/rust.code-profile --no-ai`
- **THEN** `general-minimal` omits AI trio extensions and sets `"chat.disableAIFeatures": true`, and `import-profile.sh --no-ai` installs zero `AI`-group extensions.

### Requirement: Anti-conflict coexistence settings and parsed-config autonomy deny-list

`modules/capabilities/ai-agents/settings.profile.jsonc` SHALL designate Copilot Chat as sole inline-completion provider using verified VS Code `1.147.0` keys. No parsed JSON/JSONC config under `catalog/`, `modules/`, `profiles/`, `_merged/`, `exports/`, `.vscode/`, `.agents/`, or `.claude/` SHALL set `^chat\.tools\..*autoApprove$`, `claudeCode.allowDangerouslySkipPermissions: true`, `claudeCode.claudeProcessWrapper`, `claudeCode.environmentVariables`, or `"bypassPermissions"`.

#### Scenario: Coexistence settings prevent ghost-text and startup collisions
- **WHEN** `ai-agents` settings are compiled into a profile (including `rust`)
- **THEN** merged settings include `"editor.inlineSuggest.enabled": true`, `"editor.quickSuggestions": { "other": "offWhenInlineCompletions", "comments": "off", "strings": "off" }`, `"github.copilot.nextEditSuggestions.enabled": true`, `"chat.agent.enabled": true`, `"chat.useAgentsMdFile": true`, `"chat.useNestedAgentsMdFiles": true`, `"chat.useClaudeMdFile": true`, `"chat.useAgentSkills": true`, `"chatgpt.openOnStartup": false`, `"chatgpt.commentCodeLensEnabled": false`, and `"claudeCode.lockEditorGroups": false`, and `[rust]` does NOT override `editor.quickSuggestions`.

### Requirement: Strict Copilot-only alternative and authoritative legacy ai-profile mapping

`modules/capabilities/ai-copilot/module.json` SHALL declare `"requires": ["core-editor"]`, `"conflicts": ["ai-agents"]`, and install only `github.copilot-chat`. One authoritative helper (`list_legacy_bundle_links()` in `scripts/lib/legacy-profile-name.sh`) shared by `export-profiles.sh` and `migrate-catalog.sh --refresh-symlinks` SHALL map `ai-profile-{crisp,retina}.code-profile` to `ai-copilot.code-profile`.

#### Scenario: Legacy ai-profile alias and --refresh-symlinks both resolve to ai-copilot
- **WHEN** `resolve_legacy_profile_name ai-profile-crisp` runs or `migrate-catalog.sh --refresh-symlinks` runs after `vspcli build`
- **THEN** the alias returns `ai-copilot crisp` and `exports/ai-profile-{crisp,retina}.code-profile` both continue to symlink to `ai-copilot.code-profile`.

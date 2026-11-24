# ACPX Planning Prompt: Extension Registry Modernization & Version Upgrade

## Objective
Analyze the current extension registry and profiles in `artagon-vscode-profiles`, and create a comprehensive modernization plan to integrate better plugins, latest extension versions, and new toolchain stacks (specifically Shell + BATS + bat integration, and Python + uv + pytest + modern LSP).

## Context & Files to Inspect
- Repository Root: `/Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles`
- Extension definitions: `_shared/extensions/*.json`, `profiles/*/extensions.json`
- Extension pins & catalog: `_catalog/extension-pins.json`
- Publisher allowlist & ID validation: `scripts/lib/extension-id.sh`
- Compatibility checker: `scripts/check-extension-compatibility.sh`
- Toolchain detection: `scripts/detect-toolchain.sh`
- Profile composition: `scripts/compose-settings.sh`

## Areas of Focus for Planning
1. **Current Registry Audit**:
   - Review all extensions in `_shared/extensions/base.json` and profile-specific extension JSONs.
   - Identify deprecated, unmaintained, or superseded extensions.
2. **Shell Scripting Toolchain (New/Enhanced)**:
   - Language Server & IntelliSense: `mads-hartmann.bash-ide-vscode` (bash-language-server) vs alternative modern shell LSPs.
   - Linting & Formatting: `timonwong.shellcheck`, `foxundermoon.shell-format` (shfmt), fish language support.
   - BATS Test Runner: `eamodio.bats` or modern BATS test explorer extensions for running and debugging test suites.
   - Syntax & highlighting: `jeff-hykin.better-shell-syntax` / bat preview.
3. **Python Toolchain (New/Enhanced)**:
   - Package manager & envs: `charliermarsh.ruff` (Astral Ruff for formatting & linting), uv integration patterns.
   - IntelliSense & type checking: `ms-python.python`, `ms-python.vscode-pylance` / `ms-pyright.pyright`, `ms-python.debugpy`.
   - Testing: `ms-python.pytest-adapter` / native pytest Test Explorer integration.
4. **Supply Chain & Compatibility**:
   - Adding required publishers to `ALLOWED_PUBLISHERS` in `scripts/lib/extension-id.sh`.
   - Updating `_catalog/extension-pins.json` schema and entries.
   - Updating `scripts/check-extension-compatibility.sh` and tests.
5. **Action Plan**:
   - Provide concrete, phased steps to implement the updates, update delta specs under `openspec/changes/`, and verify against the test suite.

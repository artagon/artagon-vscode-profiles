# Adversarial Review: Agy (Google DeepMind)

**Verdict**: REJECTED (Requires Revision Before Implementation)

## Critical Findings & Blockers
1. **Hallucinated Extension Identifiers**:
   - `eamodio.bats` -> Correct ID is `jetmartin.bats`.
   - `jeff-hykin.better-shell-syntax` -> Correct ID is `jeff-hykin.better-shellscript-syntax`.
2. **Layering Duplicates**: `foxundermoon.shell-format` is in `base.json`; clarify modular layering.
3. **Multi-Platform VSIX Pinning**: `charliermarsh.ruff` and `ms-python.vscode-python-envs` distribute platform-specific VSIXs (`darwin-arm64`, `linux-x64`, etc.).

## Major Concerns & Edge Cases
1. **Security & Privacy**: Public ExplainShell WAN endpoint (`https://explainshell.com`) leaks code/tokens on hover. Must be disabled by default.
2. **LSP Collision**: Set `"python.languageServer": "None"` to make `ms-pyright.pyright` the sole LSP authority.
3. **CLI Synchronization**: Update `VALID_FLAVORS`, `ORDERED_GROUPS`, and `vspcli` completions.

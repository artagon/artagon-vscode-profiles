# Adversarial Review: Codex (OpenAI)

**Verdict**: REJECTED (Revise findings before implementation)

## Critical Findings & Blockers
1. **F1 — Pyright/Pylance architecture**:
   - Combining `ms-python.python` and `ms-pyright.pyright` requires handling Pylance auto-installation behavior.
   - Recommended: Explicitly enforce `"python.languageServer": "None"` and verify compatibility checker rules.
2. **F2 — Shell diagnostics and formatting collision**:
   - `bash-language-server` invokes `shellcheck` and `shfmt` by default if on PATH, while dedicated extensions also invoke them.
   - Mitigation: Configure `"bashIde.shellcheckPath": ""` and `"bashIde.shfmt.path": ""` in `_overrides/shell.jsonc` so `timonwong.shellcheck` and `foxundermoon.shell-format` are sole owners.
3. **F3 — Detection capability boundary**:
   - Re-align detection spec with `detect-workspace-toolchain` contract (polyglot precedence, `.editorconfig`, generic `test/**/*.bats`, and Python markers).
4. **F4 — ExplainShell privacy boundary**:
   - Set `"bashIde.explainshellEndpoint": ""` (empty string) as the strict normative default to ensure no WAN requests are made on hover.
5. **F5 — Supply chain least privilege**:
   - Verify publisher allowlist entries strictly correspond to installed layers.

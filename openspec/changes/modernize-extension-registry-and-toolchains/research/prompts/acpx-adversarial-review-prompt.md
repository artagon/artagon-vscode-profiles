# Prompt: ACPX 4-CLI Adversarial Review

- **Target Tool**: `art-acpx.sh run --mode review --tier flagship` (Claude, Codex, Agy, Kimi)
- **Date**: 2026-08-16
- **Sessions**:
  - Claude: `local-review-claude-92308-1342`
  - Agy: `local-review-agy-93920-19280`
  - Kimi: `local-review-kimi-94410-23200`
  - Codex: `local-review-codex-93095-26746`
- **Output Artifacts**: `reviews/review-claude.md`, `reviews/review-agy.md`, `reviews/review-kimi.md`, `reviews/review-codex.md`

## Prompt Text

```markdown
# Adversarial Review: OpenSpec Change `modernize-extension-registry-and-toolchains`

## Review Objective
Perform a rigorous, adversarial review of the OpenSpec proposal, technical design, specifications, and tasks for the change `modernize-extension-registry-and-toolchains` in `artagon-vscode-profiles`.

## Change Location & Key Files
- Change Directory: `/Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/modernize-extension-registry-and-toolchains`
- Proposal: `openspec/changes/modernize-extension-registry-and-toolchains/proposal.md`
- Design: `openspec/changes/modernize-extension-registry-and-toolchains/design.md`
- Tasks: `openspec/changes/modernize-extension-registry-and-toolchains/tasks.md`
- Specs: `openspec/changes/modernize-extension-registry-and-toolchains/specs/**/*.md`
- Research Evidence: `openspec/changes/modernize-extension-registry-and-toolchains/research/`
- Existing codebase: `_shared/extensions/*.json`, `scripts/detect-toolchain.sh`, `scripts/lib/extension-id.sh`, `scripts/compose-settings.sh`, `scripts/tests/bats/`

## Review Checklist
1. **Spec & Design Coherence**:
   - Are the proposed extension layers (`shell.json`, `python.json`) and setting overrides sound, complete, and free of conflicting settings or colliding diagnostics (e.g. Pyright vs Ruff, ShellCheck vs bash-language-server)?
   - Are BATS, bat syntax, ExplainShell, uv, pytest, and Ruff configurations following modern 2026 best practices?
2. **Supply Chain & Security**:
   - Are all publisher IDs correct and allowlisted?
   - Are there any typo-squatting, deprecated, or malicious extension risks?
3. **Detection & CLI Integration**:
   - Are the heuristic signals in `detect-toolchain.sh` robust against false positives/negatives in polyglot workspaces?
4. **Adversarial Risks & Edge Cases**:
   - What could break in CI, offline environments, Codespaces, or Windows/Linux/macOS platforms?
   - Are backward compatibility and legacy profile symlinks maintained?

## Expected Output
Provide a structured adversarial review report:
- **Verdict**: (APPROVED / APPROVED WITH COMMENTS / REJECTED)
- **Critical Findings & Blockers** (if any)
- **Major Concerns & Edge Cases**
- **Actionable Recommendations for Improvement**
```

# Contributing to VS Code Profiles

Thanks for helping maintain the VS Code profile repository. This guide covers setup, conventions, and the checks used by automation.

## Prerequisites
- VS Code CLI on PATH (`code --version`)
- `jq`

## Quick Start

```bash
bash scripts/validate-json.sh
bash scripts/compose-settings.sh
bash scripts/export-profiles.sh
```

## Local Checks

```bash
# Validate JSON and JSONC fragments
bash scripts/validate-json.sh

# Compose merged settings (all profiles)
bash scripts/compose-settings.sh

# Compose only selected profiles
bash scripts/compose-settings.sh java-profile-crisp rust-profile-retina

# Export .code-profile bundles
bash scripts/export-profiles.sh

# Smoke test scripts (validate + compose + export + mocked open-profiles)
bash scripts/tests/run.sh
```

## Conventions
- Do not edit `_merged/`; it is generated output.
- `_shared/*.jsonc` and `_overrides/*.jsonc` must be strict JSON (no comments).
- Use `"@extends"` in overrides to reuse base settings in parent-first order.
- Extensions live in `profiles/<name>/extensions.json`; settings live in `_overrides/<name>.jsonc`.
- Crisp vs retina is determined by the profile name containing `retina`.
- Regenerate exports after changing settings or extensions.

## Workflow
- For non-trivial changes, outline the steps before editing.
- After each edit: run `scripts/validate-json.sh`, then recompose affected profiles, then export bundles.
- If extension lists change, run `scripts/install-extensions.sh <profile>` or `scripts/open-profiles.sh --skip-install <profile>` to refresh local installs.
- The pre-commit hook enforces validate, compose, and export. Enable it with:
  `git config core.hooksPath ~/.config/vscode/scripts/git-hooks`

## Branch and Commit Conventions
- Branch format: `<type>/<change-id>` (examples: `feat/add-new-profile`, `fix/java-gradle-extensions`).
- Commit messages follow conventional commits: `<type>(<scope>): <subject>`.
- Common scopes: `scripts`, `profiles`, `overrides`, `shared`, `agents`, `openspec`, `docs`.

## OpenSpec Workflow
- Read `openspec/AGENTS.md` before creating or modifying OpenSpec changes.
- Use change proposals for new capabilities or behavior changes.
- Validate changes with `openspec validate --strict` when specs are updated.

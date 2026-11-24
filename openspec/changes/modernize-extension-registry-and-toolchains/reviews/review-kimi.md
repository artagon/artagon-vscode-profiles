# Adversarial Review: Kimi (Moonshot)

**Verdict**: REJECTED (Resubmit after fixing blockers)

## Critical Findings & Blockers
1. **`jeff-hykin.better-shell-syntax` does not exist**: Canonical Marketplace ID is `jeff-hykin.better-shellscript-syntax`.
2. **`eamodio.bats` 404s**: Maintained extension is `jetmartin.bats`.
3. **Pyright will disable itself if Pylance is present**: `ms-python.python` bundles/prompts Pylance. Explicitly set `"python.languageServer": "None"`.

## Major Concerns & Edge Cases
1. **Duplicate ShellCheck diagnostics**: `bash-language-server` runs ShellCheck if on PATH, while `timonwong.shellcheck` also lints. Set `"bashIde.shellcheckPath": ""` or let `timonwong.shellcheck` manage diagnostics.
2. **`ruff.nativeServer` deprecation**: Native server is default since Ruff 0.5.3; omit setting or use `"on"`.
3. **`VALID_FLAVORS` update**: Expand `detect-toolchain.sh` and completion scripts to include `shell` and `python`.
4. **BATS testing claim**: `jetmartin.bats` is grammar/snippets; clarify scope or add test explorer runner.

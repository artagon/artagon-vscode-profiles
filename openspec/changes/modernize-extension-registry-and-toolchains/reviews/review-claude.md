# Adversarial Review: Claude (Anthropic)

**Verdict**: REJECTED (Revise and Resubmit)

## Critical Findings (Blockers)
1. **`eamodio.bats` does not exist (404)**: Real BATS extension is `jetmartin.bats`.
2. **`jeff-hykin.better-shell-syntax` is the wrong ID (404)**: Real extension is `jeff-hykin.better-shellscript-syntax`.
3. **Supply-chain baseline**: The 8 modern publishers were already allowlisted in previous commit.
4. **Snapshot sync**: Layered composition requires updating `profiles/*/extensions.json` snapshots.
5. **Test assertions**: `cli-flags.bats` and `detect-toolchain.bats` need updates for 13 flavors.

## Major Concerns & Edge Cases
1. **Pyright ⟂ Pylance**: Configure `"python.languageServer": "None"`.
2. **ShellCheck placement**: Keep `timonwong.shellcheck` in `base.json` so all languages retain shell linting.
3. **BATS/Zsh formatting**: Let `jetmartin.bats` own the `bats` language, scope shfmt to shellscript, use `**/.zshrc` glob.
4. **Detection heuristics**: Replace `.shfmt` with generic signals (`.editorconfig`, `test/*.bats`, `*.sh`).
5. **ExplainShell endpoint**: Ship disabled by default to prevent data exfiltration.

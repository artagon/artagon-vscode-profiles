## ADDED Requirements

### Requirement: Shell Toolchain Extension Layer
The system SHALL define a composable `shell` extension layer in `_shared/extensions/shell.json` containing `mads-hartmann.bash-ide-vscode`, `timonwong.shellcheck`, `foxundermoon.shell-format`, `jeff-hykin.better-shellscript-syntax`, `jetmartin.bats`, and `bmalehorn.vscode-fish`.

#### Scenario: Shell layer composition
- **WHEN** the `shell` toolchain layer is composed
- **THEN** it includes the full suite of Shell language server, linting, formatting, syntax, and BATS testing extensions.

### Requirement: Shell Toolchain Settings Overrides
The system SHALL provide `_overrides/shell.jsonc` defining:
1. Google Shell Style formatting via `foxundermoon.shell-format` (`-i 2 -ci -bn`) as default formatter for `shellscript`.
2. Disabled internal bashIde linters (`"bashIde.shellcheckPath": ""`, `"bashIde.shfmt.path": ""`) so `timonwong.shellcheck` and `foxundermoon.shell-format` are sole authorities without duplicate diagnostics.
3. Disabled WAN ExplainShell endpoint (`"bashIde.explainshellEndpoint": ""`) to eliminate accidental command token egress.
4. Language associations preserving `jetmartin.bats` ownership over the `bats` language and scoping ShellCheck exclusions (`**/.zshrc`, `**/.zprofile`, `**/.zshenv`).

#### Scenario: Settings generation
- **WHEN** settings are composed for the `shell` profile
- **THEN** `settings.json` contains single-owner formatting, isolated ShellCheck linting, and disabled WAN egress.

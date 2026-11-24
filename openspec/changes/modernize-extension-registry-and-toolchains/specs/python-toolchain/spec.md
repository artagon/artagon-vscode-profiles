## ADDED Requirements

### Requirement: Python Toolchain Extension Layer
The system SHALL define a composable `python` extension layer in `_shared/extensions/python.json` containing `ms-python.python`, `ms-python.vscode-python-envs`, `ms-python.debugpy`, `charliermarsh.ruff`, `ms-pyright.pyright`, and `fill-labs.dependi`.

#### Scenario: Python layer composition
- **WHEN** the `python` toolchain layer is composed
- **THEN** it includes the decoupled modern Python environment, native Ruff server, Pyright, and debugpy adapters.

### Requirement: Python Toolchain Settings Overrides
The system SHALL provide `_overrides/python.jsonc` configuring:
1. `charliermarsh.ruff` as default formatter with synchronous on-save actions (`source.fixAll.ruff`, `source.organizeImports.ruff`).
2. Explicit LSP authority delegation (`"python.languageServer": "None"`) to ensure `ms-pyright.pyright` operates without Pylance fallback/disablement conflicts.
3. Native Test Explorer integration (`python.testing.pytestEnabled: true`) with scoped test discovery paths.
4. Pyright diagnostic severity overrides (`reportUnusedImport: "none"`, `reportUnusedVariable: "none"`) to eliminate duplicate linting squiggles.

#### Scenario: Settings generation
- **WHEN** settings are composed for the `python` profile
- **THEN** `settings.json` contains Ruff formatting on save, pytest Test Explorer configuration, and diagnostic severity overrides.

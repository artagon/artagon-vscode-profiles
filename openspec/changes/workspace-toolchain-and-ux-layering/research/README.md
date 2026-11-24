# Extension Registry Modernization & Toolchain Research

This directory contains research prompts, deep research investigation reports from Google's Gemini Deep Research API, and ACPX planning agent outputs for modernizing the VS Code Extension Registry and toolchain stacks in `artagon-vscode-profiles`.

---

## 1. Research Prompts

All executed prompts are archived under [`prompts/`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/prompts):

- [`prompts/deep-research-shell-bats-prompt.md`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/prompts/deep-research-shell-bats-prompt.md): Prompt for investigating Shell script LSP, BATS test execution, bat syntax highlighting, ShellCheck static analysis, and shfmt AST-based formatting.
- [`prompts/deep-research-python-pytest-uv-prompt.md`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/prompts/deep-research-python-pytest-uv-prompt.md): Prompt for investigating Python development with Astral `uv`, `pytest` via native Test Explorer, native `Ruff` language server, and `Pylance` vs `ty` LSP performance.
- [`prompts/acpx-extension-registry-plan-prompt.md`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/prompts/acpx-extension-registry-plan-prompt.md): Prompt executed by ACPX planning agents (Codex and Agy) for repository-wide extension audit, deprecation pruning, supply chain validation, and phased implementation plans.

---

## 2. Deep Research Reports

- [`research-shell-bats-intellisense.md`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/research-shell-bats-intellisense.md):
  - **LSP / IntelliSense**: `mads-hartmann.bash-ide-vscode` powered by `bash-language-server` and Tree-sitter AST, with interactive ExplainShell flag inspection.
  - **Static Analysis**: `timonwong.shellcheck` with multi-arch bundled binaries, shebang-based POSIX vs Bash validation, and Zsh pattern exclusions.
  - **AST-Based Formatting**: `foxundermoon.shell-format` / `mkhl.shfmt` with Google Shell Style parameters (`-i 2 -ci -bn`).
  - **Testing & BATS**: `eamodio.bats` and `jetmartin.bats` for TAP-compliant unit test execution, isolated `$TEST_TEMP` environments, and snippet scaffolding.
  - **Enhanced Syntax & Fish**: `jeff-hykin.better-shell-syntax` and `bmalehorn.vscode-fish`.

- [`research-python-pytest-uv-intellisense.md`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/openspec/changes/workspace-toolchain-and-ux-layering/research/research-python-pytest-uv-intellisense.md):
  - **Decoupled Python Architecture**: Decoupling monolithic extension into `ms-python.python`, `ms-python.vscode-python-envs` (PET Rust scanner for `.venv`), and `ms-python.debugpy`.
  - **uv Environment Integration**: PEP 405 compliance, automated interpreter binding, `uv run`, `uv sync`, and `uvx` transient tooling.
  - **Astral Ruff**: Deprecation of Python-based `ruff-lsp` in favor of compiled native `ruff server` (`charliermarsh.ruff`), replacing Black, Flake8, isort, and pydocstyle with sub-second execution and synchronized on-save actions (`source.fixAll.ruff`, `source.organizeImports.ruff`).
  - **LSP & Type Checking**: Pylance (`ms-python.vscode-pylance`) / Pyright (`ms-pyright.pyright`) with diagnostic suppression overrides to prevent collision with Ruff, and next-gen Rust checker `ty` (`astral-sh.ty`) utilizing Salsa incremental recomputation.
  - **Testing with pytest**: Native Test Explorer binding via `python.testing.pytestEnabled: true`, scoped discovery paths (`tests`), and `purpose: ["debug-test"]` in `launch.json`.

---

## 3. Recommended New Toolchain Layers

### 3.1 Shell Toolchain Layer (`_shared/extensions/shell.json`)
```json
[
  { "identifier": { "id": "mads-hartmann.bash-ide-vscode" } },
  { "identifier": { "id": "timonwong.shellcheck" } },
  { "identifier": { "id": "foxundermoon.shell-format" } },
  { "identifier": { "id": "jeff-hykin.better-shell-syntax" } },
  { "identifier": { "id": "eamodio.bats" } },
  { "identifier": { "id": "bmalehorn.vscode-fish" } }
]
```

### 3.2 Python Toolchain Layer (`_shared/extensions/python.json`)
```json
[
  { "identifier": { "id": "ms-python.python" } },
  { "identifier": { "id": "ms-python.vscode-python-envs" } },
  { "identifier": { "id": "ms-python.debugpy" } },
  { "identifier": { "id": "charliermarsh.ruff" } },
  { "identifier": { "id": "ms-pyright.pyright" } },
  { "identifier": { "id": "fill-labs.dependi" } }
]
```

---

## 4. Supply Chain & Publisher Allowlist Updates

In [`scripts/lib/extension-id.sh`](file:///Users/gtrump001c@cable.comcast.com/Projects/Artagon/artagon-vscode-profiles/scripts/lib/extension-id.sh), add the following validated publishers to `ALLOWED_PUBLISHERS`:
- `mads-hartmann` (Bash IDE / LSP)
- `charliermarsh` (Astral Ruff)
- `ms-pyright` (Pyright LSP)
- `astral-sh` (Astral Ty / Tooling)
- `bmalehorn` (Fish shell support)
- `jeff-hykin` (Enhanced shell syntax)
- `jetmartin` (BATS syntax)
- `fill-labs` (Dependi package management)

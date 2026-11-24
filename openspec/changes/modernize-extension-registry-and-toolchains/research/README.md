# Extension Registry Modernization, Toolchains & UX Research

This directory contains research prompts, deep research investigation reports from Google's Gemini Deep Research API and OpenAI Deep Research API, ACPX planning outputs, and 4-CLI adversarial reviews for modernizing the VS Code Extension Registry, toolchain layers, IntelliSense/documentation integrations, and UX customizations in `artagon-vscode-profiles`.

---

## 1. Research Prompts

All executed prompts are archived under [`prompts/`](./prompts):

- [`prompts/deep-research-shell-bats-prompt.md`](./prompts/deep-research-shell-bats-prompt.md): Prompt for investigating Shell script LSP, BATS test execution, bat syntax highlighting, ShellCheck static analysis, and shfmt AST-based formatting.
- [`prompts/deep-research-python-pytest-uv-prompt.md`](./prompts/deep-research-python-pytest-uv-prompt.md): Prompt for investigating Python development with Astral `uv`, `pytest` via native Test Explorer, native `Ruff` language server, and `Pylance` vs `ty` LSP performance.
- [`prompts/deep-research-vscode-ux-prompt.md`](./prompts/deep-research-vscode-ux-prompt.md): Prompt for investigating VS Code editor tweaks, typography, cursor animations, sticky scroll, breadcrumbs, tree indent guides, and high-DPI retina vs crisp tuning.
- [`prompts/deep-research-intellisense-docs-prompt.md`](./prompts/deep-research-intellisense-docs-prompt.md): Prompt for investigating advanced IntelliSense, memory layout hovers in Rust, `fill-labs.dependi`, Python Intersphinx scraping with `KiidxAtlas.python-hover`, C++ `clangd` IntelliSense decoupling, `doxdocgen`, CppReference lookups, and offline Dash/Zeal integration.
- [`prompts/acpx-extension-registry-plan-prompt.md`](./prompts/acpx-extension-registry-plan-prompt.md): Prompt executed by ACPX planning agents (Codex and Agy) for repository-wide extension audit, deprecation pruning, supply chain validation, and phased implementation plans.
- [`prompts/acpx-adversarial-review-prompt.md`](./prompts/acpx-adversarial-review-prompt.md): Prompt executed for the 4-CLI adversarial review across Claude, Codex, Agy, and Kimi.
- [`prompts/openai-deepresearch-java-jdk28.md`](./prompts/openai-deepresearch-java-jdk28.md): OpenAI Deep Research prompt for JDK 28 EA runtimes, JDT LS parallel GC acceleration, and Gradle Build Server integration.

---

## 2. Deep Research Reports

1. [`research-shell-bats-intellisense.md`](./research-shell-bats-intellisense.md) (45KB):
   - **LSP / IntelliSense**: `mads-hartmann.bash-ide-vscode` powered by `bash-language-server` and Tree-sitter AST.
   - **Static Analysis**: `timonwong.shellcheck` with multi-arch bundled binaries and Zsh pattern exclusions.
   - **AST-Based Formatting**: `foxundermoon.shell-format` / `mkhl.shfmt` with Google Shell Style parameters (`-i 2 -ci -bn`).
   - **Testing & BATS**: `jetmartin.bats` for TAP-compliant unit test execution and snippet scaffolding.
   - **Enhanced Syntax & Fish**: `jeff-hykin.better-shellscript-syntax` and `bmalehorn.vscode-fish`.

2. [`research-python-pytest-uv-intellisense.md`](./research-python-pytest-uv-intellisense.md) (45KB):
   - **Decoupled Python Architecture**: Monolith decoupled into `ms-python.python`, `ms-python.vscode-python-envs` (PET Rust scanner for `.venv`), and `ms-python.debugpy`.
   - **uv Environment Integration**: PEP 405 compliance, automated interpreter binding, `uv run`, `uv sync`, and `uvx`.
   - **Astral Ruff**: Native `ruff server` (`charliermarsh.ruff`), replacing Black, Flake8, isort, and pydocstyle with sub-second execution and synchronized on-save actions (`source.fixAll.ruff`, `source.organizeImports.ruff`).
   - **LSP & Type Checking**: Pyright (`ms-pyright.pyright`) with diagnostic suppression overrides and `python.languageServer: "None"`.
   - **Testing with pytest**: Native Test Explorer binding via `python.testing.pytestEnabled: true` and `purpose: ["debug-test"]` in `launch.json`.

3. [`research-vscode-ux-customizations.md`](./research-vscode-ux-customizations.md) (54KB):
   - **Typography & Font Ligatures**: JetBrains Mono, Monaspace Neon, Fira Code with `fontVariations`, `fontLigatures`, and customized `lineHeight`.
   - **Workbench Layout**: Tree indent guides (`workbench.tree.renderIndentGuides: "always"`, `workbench.tree.indent: 16`), compact explorer folders (`explorer.compactFolders: true`), smooth scrolling, and sticky scroll (`editor.stickyScroll.enabled: true`).
   - **Diagnostics & Visual Polish**: Bracket pair colorization, ErrorLens inline diagnostics, and terminal GPU acceleration.

4. [`research-intellisense-docs-ecosystem.md`](./research-intellisense-docs-ecosystem.md) (55KB):
   - **Rust**: Memory layout hovers (`rust-analyzer.hover.memoryLayout.enable`), drop glue inspection, `fill-labs.dependi`, and `JScearcy.rust-doc-viewer`.
   - **Python**: Automated Google docstring generation (`njpwerner.autodocstring`), live Intersphinx documentation scraping (`KiidxAtlas.python-hover`), and CVE dependency remediation (`mspythondeprem.python-dependency-remediation`).
   - **C/C++ & Universal Docs**: `clangd` IntelliSense decoupling with `C_Cpp.intelliSenseEngine: "disabled"`, `zatkins-dev.doxdocgen-with-direction`, `Guyutongxue.cpp-reference` in-editor lookups, and offline Dash/Zeal integration (`deerawan.vscode-dash`).

5. [`research-java-jdk28-gradle-fastbuild.md`](./research-java-jdk28-gradle-fastbuild.md) (16KB, OpenAI Deep Research):
   - **JDK 28 Early Access & Preview Syntax**: `java.jdt.ls.javac.enabled: "on"` with `java.completion.engine: "dom"` for Javac-backed experimental language parsing; conservative ECJ engine for standard builds.
   - **Compiler & GC Acceleration**: Low-latency parallel GC tuning (`-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Xms512m -Xmx4G`), AppCDS shared indexing (`java.jdt.ls.appcds.enabled: "on"`), and non-blocking autobuilds (`java.maxConcurrentBuilds: 1`).
   - **Official Gradle Build Server**: `vscjava.vscode-gradle` with `"java.gradle.buildServer.enabled": "on"`, daemon toolchain separation, build caching (`--build-cache`), and parallel execution (`--parallel`).

---

## 3. Recommended Toolchain Layers

### 3.1 Shell Toolchain Layer (`_shared/extensions/shell.json`)
```json
[
  { "identifier": { "id": "mads-hartmann.bash-ide-vscode" } },
  { "identifier": { "id": "timonwong.shellcheck" } },
  { "identifier": { "id": "foxundermoon.shell-format" } },
  { "identifier": { "id": "jeff-hykin.better-shellscript-syntax" } },
  { "identifier": { "id": "jetmartin.bats" } },
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

## 4. Adversarial Review Reports (4 CLIs)

Synthesized reports located under [`reviews/`](../reviews):
- [`../reviews/review-claude.md`](../reviews/review-claude.md)
- [`../reviews/review-codex.md`](../reviews/review-codex.md)
- [`../reviews/review-agy.md`](../reviews/review-agy.md)
- [`../reviews/review-kimi.md`](../reviews/review-kimi.md)

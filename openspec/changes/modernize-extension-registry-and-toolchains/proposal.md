> [!WARNING]
> **SUPERSEDED by `refactor-capability-modules-and-defaults`** — archive ONLY with `openspec archive <change-id> --skip-specs` so retired `_shared/`, `_overrides/`, `_catalog/`, and legacy Decision-15 deltas do not re-enter `openspec/specs/`.

## Why

The current VS Code extension registry in `artagon-vscode-profiles` has accumulated superseded and conflicting extensions (e.g. `richardwillis.vscode-gradle` duplicating official Gradle tooling, `BarbossHack.crates-io` superseded by `fill-labs.dependi`, and duplicate `timonwong.shellcheck` entries scattered across multiple language layers). Furthermore, developers lack dedicated, modern toolchain layers for **Shell script development** (Bash, Zsh, POSIX sh, and Fish with BATS test framework integration) and **modern Python development** (built around Astral's fast `uv` package manager, native `Ruff` server, `pytest` via native Test Explorer, and `Pyright`/`Pylance` LSP).

Adding first-class Shell and Python toolchains, pruning legacy dependencies, and updating the publisher allowlist and extension pins will modernize developer workflows across the organization while maintaining strict supply-chain and configuration integrity.

## What Changes

- **Add Shell Toolchain (`shell`)**:
  - Introduce `_shared/extensions/shell.json` containing `mads-hartmann.bash-ide-vscode` (Tree-sitter LSP), `timonwong.shellcheck`, `foxundermoon.shell-format` (shfmt), `jeff-hykin.better-shellscript-syntax`, `jetmartin.bats`, and `bmalehorn.vscode-fish`.
  - Add `_overrides/shell.jsonc` configuring Google Shell Style formatting (`-i 2 -ci -bn`), on-type ShellCheck analysis, and BATS file associations.
  - Update `scripts/detect-toolchain.sh` to detect Shell script projects (`.shellcheckrc`, `test/**/*.bats`, `*.bats`).
- **Add Python Toolchain (`python`)**:
  - Introduce `_shared/extensions/python.json` containing decoupled modern tools: `ms-python.python`, `ms-python.vscode-python-envs` (Rust PET scanner for `.venv`), `charliermarsh.ruff` (native Rust server replacing Black/Flake8/isort), `ms-pyright.pyright`, `ms-python.debugpy`, and `fill-labs.dependi`.
  - Add `_overrides/python.jsonc` configuring synchronous on-save Ruff formatting/imports, diagnostic overrides to prevent collision with LSP, and native `pytest` Test Explorer binding.
  - Update `scripts/detect-toolchain.sh` to detect Python projects (`pyproject.toml`, `requirements.txt`, `setup.py`, `uv.lock`, `.python-version`).
- **Modernize Java Toolchain & Gradle (`java-toolchain`)**:
  - Configure `java.configuration.runtimes` for supported LTS releases (`JavaSE-25`, `JavaSE-21`, `JavaSE-17`) with version-gated Javac-backed compilation contract for experimental JDK 28 EA preview code.
  - Optimize Eclipse JDT LS VM memory arguments (`-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable`), AppCDS, and shared indexing.
  - Enable official `vscjava.vscode-gradle` with Gradle Build Server (`"java.gradle.buildServer.enabled": "on"`), toolchain separation, and build caching.
- **Prune Deprecated & Conflicting Extensions**:
  - Remove `richardwillis.vscode-gradle` from `java-gradle`, `java-spring`, and `java-profile` in favor of official `vscjava.vscode-gradle`.
  - Remove `BarbossHack.crates-io` in favor of `fill-labs.dependi` and `rust-analyzer`.
  - Remove `eg2.vscode-npm-script` from `astro.json` (obsolete; built into VS Code core).
  - Remove `notskm.clang-tidy` from `cpp-clangd.json` (redundant with `llvm-vs-code-extensions.vscode-clangd`).
  - Centralize `timonwong.shellcheck` from disparate toolchain manifests into the shared `shell` layer.
- **Update Base Extensions & Workspace UX**:
  - Add Markdown & Mermaid rendering tools (`bierner.markdown-mermaid`, `shd101wyy.markdown-preview-enhanced`, `yzhang.markdown-all-in-one`, `bierner.markdown-preview-github-styles`, `davidanson.vscode-markdownlint`) to `_shared/extensions/base.json`.
  - Enable `"explorer.compactFolders": true` in `_shared/editor-base.jsonc` to flatten single-child source directory trees.
  - Update `ALLOWED_PUBLISHERS` in `scripts/lib/extension-id.sh` with verified publishers (`bierner`, `yzhang`, `shd101wyy`, `davidanson`).
  - Regenerate and synchronize all 11 profile `extensions.json` snapshots.

## Capabilities

### New Capabilities
- `shell-toolchain`: Layered extension definitions, configuration overrides, and detection for Shell scripting (Bash/Zsh/Fish/POSIX sh) with BATS testing, Tree-sitter LSP, and AST formatting.
- `python-toolchain`: Layered extension definitions, configuration overrides, and detection for modern Python development with Astral `uv`, native `Ruff` linter/formatter, `Pyright`, and `pytest` Test Explorer integration.

### Modified Capabilities
- `compose-profile-settings`: Updates Java profile overrides (`_overrides/java-*.jsonc`) with JDK 28 Early Access (EA) runtimes, high-throughput JDT LS VM arguments, AppCDS, and official Gradle Build Server integration with configuration caching.
- `install-profile-extensions`: Updates publisher allowlists, cleans up duplicate/deprecated extensions (`richardwillis.vscode-gradle`, `BarbossHack.crates-io`), and adds Markdown/Mermaid tooling to base layers.
- `manage-profile-cli`: Updates `vspcli` and `scripts/detect-toolchain.sh` to auto-detect and configure `shell` and `python` workspace stacks.

## Impact

- **Layer Definitions**: Adds `_shared/extensions/{shell,python}.json`, updates `_shared/extensions/*.json`.
- **Overrides & Profiles**: Adds `_overrides/{shell,python}.jsonc`, generates canonical profiles under `profiles/` and `_merged/`.
- **Scripts & CLI**: Modifies `scripts/detect-toolchain.sh`, `scripts/lib/extension-id.sh`, and `scripts/check-extension-compatibility.sh`.
- **Tests**: Adds BATS tests in `scripts/tests/bats/` for shell and python detection, layer composition, and compatibility checks.

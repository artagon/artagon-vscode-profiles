## Context

`artagon-vscode-profiles` provides reproducible, layered VS Code configurations. While stacks for Rust, Astro, Java, C++, and AI are configured, developers regularly build shell scripts (Bash, Zsh, POSIX sh, Fish) and modern Python applications.

Recent audits revealed:
1. Fragmented and superseded extensions in existing profiles (`richardwillis.vscode-gradle`, `BarbossHack.crates-io`, `eg2.vscode-npm-script`, `notskm.clang-tidy`).
2. Duplicated `timonwong.shellcheck` across 8 different language profiles instead of a standalone composable layer.
3. Lack of first-class toolchain layers for Shell scripting (BATS testing, Tree-sitter LSP, AST formatting) and modern Python (Astral `uv`, `charliermarsh.ruff` native server, `Pyright`, native `pytest` Test Explorer binding).

## Goals / Non-Goals

**Goals:**
- Create `_shared/extensions/shell.json` and `_overrides/shell.jsonc` with ExplainShell, ShellCheck, Google Style shfmt formatting (`-i 2 -ci -bn`), and BATS testing support.
- Create `_shared/extensions/python.json` and `_overrides/python.jsonc` with Astral `uv` environment integration, native `ruff server`, `pytest` Test Explorer binding, and `Pyright` type checking.
- Clean up deprecated/conflicting extensions across all profiles.
- Expand `scripts/lib/extension-id.sh` publisher allowlists with verified publishers.
- Extend `scripts/detect-toolchain.sh` and `scripts/vspcli` to auto-detect and configure `shell` and `python` workspace stacks.
- Ensure 100% test coverage with BATS regression suites.

**Non-Goals:**
- Forcing a specific Python version or installing external binaries (uv, ruff, shfmt, bats) via VS Code extension installers; users install CLI tools via system packages or package managers.
- Supporting legacy Python linters/formatters (Black, Flake8, isort, autopep8, pylint) in the primary stack; Ruff completely replaces them.

## Decisions

### Decision 1: Tree-sitter LSP (`mads-hartmann.bash-ide-vscode`) for Shell Scripting
- **Choice**: Use `mads-hartmann.bash-ide-vscode` powered by `bash-language-server` and `jetmartin.bats`.
- **Rationale**: Uses Tree-sitter for incremental concrete syntax tree parsing, enabling reliable multi-file symbol resolution, function outlines, and local documentation hover. To protect against sensitive data exfiltration over WAN, no public `explainshell.com` endpoint is configured by default.
- **Alternatives Considered**: Regex-only grammar (`vscode.shellscript` built-in) which lacks symbol indexing and semantic analysis; `rail5.bashpp` which requires specialized external binaries.

### Decision 2: AST-Based Shell Formatting with shfmt (`foxundermoon.shell-format`)
- **Choice**: Enforce `foxundermoon.shell-format` / `shfmt` with flags `-i 2 -ci -bn`.
- **Rationale**: `shfmt` reconstructs code from an Abstract Syntax Tree, guaranteeing semantic safety. Standardized Google Shell Style prevents stylistic disputes in CI/CD.
- **Alternatives Considered**: Ad-hoc regex formatters that risk introducing syntax errors into complex pipelines.

### Decision 3: Decoupled Python Architecture with Native Ruff Server & Pyright
- **Choice**: Combine `ms-python.python`, `ms-python.vscode-python-envs`, `charliermarsh.ruff` (using native compiled server), and `ms-pyright.pyright` (with `"python.languageServer": "None"`).
- **Rationale**: Decouples virtualenv discovery (powered by Rust PET scanner for instant `.venv` binding) from formatting and type-checking. Setting `python.languageServer: "None"` delegates all LSP responsibilities directly to `ms-pyright.pyright` without Pylance fallback prompts or Jedi conflicts. Ruff native server executes in compiled Rust without Python subprocess serialization, replacing Black, Flake8, and isort. Diagnostic severity overrides suppress duplicate Pyright linting squiggles.
- **Alternatives Considered**: Legacy `ruff-lsp` (deprecated by Astral); Pylance (closed source, restricted to Microsoft builds; Pyright provides portable spec-compliant typing).

### Decision 4: Native Test Explorer pytest Binding
- **Choice**: Configure `python.testing.pytestEnabled: true` with scoped discovery `["tests"]` and `purpose: ["debug-test"]` in `launch.json`.
- **Rationale**: Avoids monolithic discovery timeouts and provides direct gutter test running and debugging with breakpoint execution into dependencies (`justMyCode: false`).
- **Alternatives Considered**: Third-party test runners that conflict with native VS Code Testing API.

### Decision 5: Modern Java Runtimes, Fast Building, and Gradle Build Server Integration
- **Choice**: Configure `java.configuration.runtimes` for supported LTS releases (`JavaSE-25`, `JavaSE-21`, `JavaSE-17`) with version-gated Javac-backed compilation for JDK 28 EA preview features (`"java.jdt.ls.javac.enabled": "on"`, `"java.completion.engine": "dom"`); tune Eclipse JDT LS VM arguments for low-latency parallel compilation (`-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable`); enable official `vscjava.vscode-gradle` with the Gradle Build Server (`"java.gradle.buildServer.enabled": "on"`), parallel execution (`--parallel`), and build caching (`--build-cache`).
- **Rationale**: Accelerates Java incremental compilation cycles, prevents memory exhaustion during heavy multi-module builds, enforces toolchain separation (running Gradle on supported JVM while compiling preview features), and avoids globally forcing experimental configuration caching on incompatible plugins.
- **Alternatives Considered**: Default serial compilation with legacy single-threaded Gradle daemons.

## Risks / Trade-offs

- **[Risk]**: Conflict between Pyright and Ruff diagnostic warnings.
  - **Mitigation**: Configure `python.analysis.diagnosticSeverityOverrides` in `_overrides/python.jsonc` to set `reportUnusedImport` and `reportUnusedVariable` to `"none"`, allowing Ruff to manage lint diagnostics exclusively.
- **[Risk]**: ShellCheck false positives on Zsh profile files.
  - **Mitigation**: Add `shellcheck.ignorePatterns` for `**/*.zshrc`, `**/*.zprofile`, and `**/*.zshenv`.
- **[Risk]**: Supply chain rejection of new extensions during automated install.
  - **Mitigation**: Proactively update `ALLOWED_PUBLISHERS` in `scripts/lib/extension-id.sh` with all new publisher IDs and pin hashes in `_catalog/extension-pins.json`.

## Migration Plan

1. Prune deprecated extension entries from `_shared/extensions/*.json`.
2. Introduce new layers `_shared/extensions/shell.json` and `_shared/extensions/python.json`.
3. Add `_overrides/shell.jsonc` and `_overrides/python.jsonc`.
4. Update `scripts/lib/extension-id.sh` publisher allowlist.
5. Update `scripts/detect-toolchain.sh` heuristic detection.
6. Re-run `scripts/compose-settings.sh` and `scripts/export-profiles.sh`.
7. Execute BATS test suites to verify detection, layer composition, and compatibility.

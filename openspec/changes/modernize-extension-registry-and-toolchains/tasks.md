## 1. Supply Chain & Publisher Allowlist Updates

- [x] 1.1 Add new approved publishers (`mads-hartmann`, `charliermarsh`, `ms-pyright`, `astral-sh`, `bmalehorn`, `jeff-hykin`, `jetmartin`, `fill-labs`) to `ALLOWED_PUBLISHERS` in `scripts/lib/extension-id.sh`.
- [x] 1.2 Update extension grouping in `scripts/install-extensions.sh` to recognize `Shell` and `Python` groups.
- [x] 1.3 Update `_catalog/extension-pins.json` with modern toolchain extension pin definitions.

## 2. Registry Pruning & Deprecation Cleanup

- [x] 2.1 Remove `richardwillis.vscode-gradle` from `_shared/extensions/java-gradle.json`, `_shared/extensions/java-spring.json`, and `_shared/extensions/java-profile.json`.
- [x] 2.2 Remove `BarbossHack.crates-io` from `_shared/extensions/rust.json` in favor of `fill-labs.dependi`.
- [x] 2.3 Remove `eg2.vscode-npm-script` from `_shared/extensions/astro.json`.
- [x] 2.4 Remove `notskm.clang-tidy` from `_shared/extensions/cpp-clangd.json`.
- [x] 2.5 Remove duplicated `timonwong.shellcheck` entries across individual language layers.
- [x] 2.6 Remove pruned `richardwillis` publisher from `scripts/lib/extension-id.sh`.

## 3. Shell Toolchain Implementation

- [x] 3.1 Create `_shared/extensions/shell.json` containing `mads-hartmann.bash-ide-vscode`, `timonwong.shellcheck`, `foxundermoon.shell-format`, `jeff-hykin.better-shellscript-syntax`, `jetmartin.bats`, and `bmalehorn.vscode-fish`.
- [x] 3.2 Create `_overrides/shell.jsonc` defining shfmt formatting, ShellCheck on-type analysis, local-only ExplainShell configuration, and BATS file associations.
- [x] 3.3 Create `profiles/shell/` and generate symlinks for `settings.json` and `extensions.json`.

## 4. Python Toolchain Implementation

- [x] 4.1 Create `_shared/extensions/python.json` containing `ms-python.python`, `ms-python.vscode-python-envs`, `charliermarsh.ruff`, `ms-pyright.pyright`, `ms-python.debugpy`, and `fill-labs.dependi`.
- [x] 4.2 Create `_overrides/python.jsonc` defining native Ruff server, on-save formatting/imports, pytest Test Explorer configuration, and diagnostic severity overrides (`python.languageServer: "None"`).
- [x] 4.3 Create `profiles/python/` and generate symlinks for `settings.json` and `extensions.json`.

## 5. Java Toolchain Modernization & Fast Building

- [x] 5.1 Replace `richardwillis.vscode-gradle` with `vscjava.vscode-gradle` across Java layer definitions.
- [x] 5.2 Configure `java.configuration.runtimes` supporting LTS releases (`JavaSE-25`, `JavaSE-21`, `JavaSE-17`) and version-gated Javac-backed compilation for JDK 28 EA in `_overrides/java-*.jsonc`.
- [x] 5.3 Configure low-latency parallel GC JDT LS VM arguments (`-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable`), AppCDS, and incremental autobuilds.
- [x] 5.4 Configure Gradle Build Server (`"java.gradle.buildServer.enabled": "on"`), toolchain separation, and build acceleration (`"java.import.gradle.arguments": ["--parallel", "--build-cache"]`) in `_overrides/java-gradle.jsonc` and `_overrides/java-spring.jsonc`.

## 6. Toolchain Detection & CLI Integration

- [x] 6.1 Update `scripts/detect-toolchain.sh` (`VALID_FLAVORS`) to detect `shell` and `python` with proper precedence rules.
- [x] 6.2 Update `scripts/install-extensions.sh` (`ORDERED_GROUPS`, `normalize_group`) to handle `Shell` and `Python`.
- [x] 6.3 Update `scripts/vspcli` list and completion scripts (`.vspcli-completion.*`) to include `shell` and `python` flavors.

## 7. Composition, Verification & Testing

- [x] 7.1 Run `scripts/compose-settings.sh` to generate merged settings for new profiles.
- [x] 7.2 Run `scripts/export-profiles.sh` to produce `.code-profile` bundles and update `exports/SHA256SUMS`.
- [x] 7.3 Add BATS test cases in `scripts/tests/bats/detect-toolchain.bats` for shell and python detection.
- [x] 7.4 Add BATS test cases in `scripts/tests/bats/install-extensions-layered.bats` for new toolchain composition.
- [x] 7.5 Run full test suite via `bats scripts/tests/bats/` and ensure all tests pass.

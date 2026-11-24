# Design: Vertical Capability-Module Architecture, Default AI Trio & Visual/Extension Refresh (v3.1)

## Context

`artagon-vscode-profiles` manages reusable Visual Studio Code profiles, workspace `.vscode/` stacks, and portable `.code-profile` bundles. Currently, toolchain knowledge is fragmented horizontally across `_shared/extensions/*.json`, `_overrides/*.jsonc`, `profiles/*/extensions.json`, and 8+ hardcoded Bash tables.

This v3.1 design incorporates ALL findings from 3 rounds of adversarial NTM swarm review (Opus `PASS`, Astra, Sol) and internal `bash-pro`/`research` reviews, plus direct verification against local VS Code `1.147.0`, Apple `jq` 1.7.1, macOS `/bin/bash` 3.2.57, BSD `find`, and OpenSpec 2.1.0 in-memory spec merge. It implements **Option 2: Vertical Capability-Module Architecture with Unified Target Compiler**, strictly in **Bash 3.2+ + `jq`** (zero Python, Node, or Rust runtime dependencies).

## Goals / Non-Goals

**Goals:**
1. **Vertical module SSOT** (`modules/{capabilities,toolchains,ux}/<id>/`) and **unified catalog SSOT** (`catalog/{extensions,publishers,extension-pins,defaults,setting-scopes}.json`) separating both settings (`workspace` vs `profile`) and extensions (`workspace[]` vs `profile[]` vs `unwanted[]`).
2. **Unified Bash 3.2 + `jq` compiler** driving profile builds, workspace generation, native profile application (with consistent `--user-data-dir`), and global installs.
3. **Default AI Agent Trio (`ai-agents`)**: `anthropic.claude-code`, `openai.chatgpt`, and unified `github.copilot-chat` in `catalog/defaults.json` with verified VS Code `1.147.0` coexistence settings, parsed-config autonomy deny-list (enforced in both `validate-json.sh` AND `import-profile.sh`), `--without ai-agents`, AI-free `general-minimal` (`chat.disableAIFeatures: true`), `import-profile.sh --no-ai`, strict `ai-copilot`, and ONE authoritative legacy link table keeping `ai-profile-*` pointed at `ai-copilot`.
4. **Live-verified extension refresh & supply-chain remediation**: Compromised-first removal of GlassWorm `jscearcy.rust-doc-viewer`, removal of 5 `NOT_FOUND` Marketplace extensions, real symmetric conflict pairs (including UX presets), Spring + build-tool layering, additive `github-workflows`, `defaultWith` Astro/TS formatter selection, and 6 install-time SHA-256 VSIX pins.
5. **VS Code `1.147.0` schema & scope accuracy**: Verified key/type/enum/scope compliance (`catalog/setting-scopes.json`), 14 symmetric UX keys (`catppuccin-mocha`), explicit `--ux-target` routing, workspace symlink containment, bounded timestamped `.bak` backups, and fixed `fish`/`bash`/`zsh` XDG RTK wrappers.
6. **Archive-safe OpenSpec disposition & full macOS Bash 3.2 + Ubuntu CI parity** with hermetic offline test harnesses and PATH-pinned `/bin/bash` 3.2 subprocesses.

**Non-Goals:**
- Runtime Python, Node.js, or Rust dependencies in production shell scripts.
- Symlinking `~/.vscode/extensions` or mutating VS Code's SQLite `state.vscdb` (`microsoft/vscode#208195`).
- Pinning multi-platform native VSIXs under a single universal SHA-256 hash.
- Removing checked-in `exports/*.code-profile` or the 22 legacy `exports/*-{crisp,retina}.code-profile` symlinks.

---

## Decisions

### Decision 1: Directory Layout, Module Taxonomy & Recipe Schema

```text
catalog/
  extensions.json          # Lowercase canonical IDs: publisher, group, status, replacedBy, packMembers, themeLabels, universal
  publishers.json          # Exact LC_ALL=C sorted unique lowercase publishers used by active modules
  extension-pins.json      # Lowercase unique VSIX SHA-256 pins (6 universal core/UX extensions; install-time verified)
  defaults.json            # Default capability list + default UX preset
  setting-scopes.json      # Versioned VS Code 1.147.0 + extension key/type/enum/scope registry
modules/
  capabilities/
    core-editor/           # workspace[]: editorconfig, toml, dotenv, yaml; profile[]: errorlens, cspell + 4 pinned crisp/retina theme/icon packs
    ai-agents/             # requires: ["core-editor"], conflicts: ["ai-copilot"], profile[]: claude-code, chatgpt, copilot-chat; workspace[]: []
    ai-copilot/            # requires: ["core-editor"], conflicts: ["ai-agents"], profile[]: copilot-chat; workspace[]: []
    markdown/              # requires: ["core-editor"], settings.workspace.jsonc
    shell/                 # requires: ["core-editor"], workspace[]: bash-ide, shellcheck, mkhl.shfmt, vscode-fish, better-shellscript-syntax, kherring.bats-test-runner
    github-workflows/      # requires: ["core-editor"], workspace[]: github-actions, additive detect (fallback: false, priority: 80)
    themes-pack/           # requires: ["core-editor"], profile[]: 12 optional extra themes/icons
    containers/            # requires: ["core-editor"], workspace[]: ms-azuretools.vscode-containers
    quality/               # requires: ["core-editor"], profile[]: sonarsource.sonarlint-vscode
    git-pro/               # requires: ["core-editor"], profile[]: eamodio.gitlens, github.vscode-pull-request-github
    collaboration/         # requires: ["core-editor"], profile[]: ms-vsliveshare.vsliveshare
    rtk-terminal/          # requires: ["core-editor"], scripts/rtk-init.{fish,bash,zsh} + zsh-zdotdir shims
  toolchains/
    rust/                  # requires: ["core-editor"], settings.workspace.jsonc, tasks.json (plain cargo doc default)
    python-base/           # requires: ["core-editor"], workspace[]: ms-python.python, ms-python.debugpy, ms-python.vscode-python-envs, charliermarsh.ruff; settings.{workspace,profile}.jsonc; detect: null
    python/                # requires: ["python-base"], conflicts: ["python-ty"], workspace[]: detachhead.basedpyright; detects Python
    python-ty/             # requires: ["python-base"], conflicts: ["python"], workspace[]: astral-sh.ty; detect: null
    ts-base/               # requires: ["core-editor"], defaultWith: ["ts-eslint-prettier"], detects tsconfig.json (superseded by astro)
    ts-eslint-prettier/    # requires: ["ts-base"], conflicts: ["biome"], ESLint + Prettier settings & extensions, detect: null
    biome/                 # requires: ["ts-base"], conflicts: ["ts-eslint-prettier"], supersedes: ["ts-eslint-prettier"], Biome settings & extension, detects biome.json*
    astro/                 # requires: ["ts-base"], defaultWith: ["ts-eslint-prettier"], supersedes: ["ts-base"], detects package.json + astro.config.*
    java-base/             # requires: ["core-editor"], settings.workspace.jsonc, detect: null
    java-gradle/           # requires: ["java-base"], conflicts: ["java-maven"], supersedes: ["java-maven"], settings.{workspace,profile}.jsonc
    java-maven/            # requires: ["java-base"], conflicts: ["java-gradle"], settings.workspace.jsonc
    java-spring/           # requires: ["java-base"], conflicts: [], detects @SpringBootApplication (layers WITH java-gradle or java-maven!)
    cpp-base/              # requires: ["core-editor"], settings.workspace.jsonc, detect: null
    cpp-clangd/            # requires: ["cpp-base"], conflicts: ["cpp-intellisense"]
    cpp-intellisense/      # requires: ["cpp-base"], conflicts: ["cpp-clangd"]
  ux/                      # Mutually exclusive one-of presets (symmetric conflicts[] among crisp, retina, default!)
    crisp/                 # conflicts: ["retina", "default"]; 14 exact symmetric keys (Tokyo Night, material-icon-theme)
    retina/                # conflicts: ["crisp", "default"]; 14 exact symmetric keys (Catppuccin Mocha, catppuccin-mocha)
    default/               # conflicts: ["crisp", "retina"]; {} (explicit apply deletes all 14 symmetric keys)
profiles/                  # 15 canonical profile recipes:
  general/                 # defaults (core-editor, ai-agents, markdown, shell)
  general-minimal/         # excludeModules: ["ai-agents"], overrides.profile: { "chat.disableAIFeatures": true }
  ai/                      # defaults + chat focus
  ai-copilot/              # excludeModules: ["ai-agents"], modules: ["ai-copilot"] (authoritative target of ai-profile-*)
  ai-plus/                 # defaults + git-pro, containers, quality, themes-pack
  rust/, python/, astro/   # defaults + toolchain (astro auto-pulls ts-eslint-prettier via defaultWith unless --with biome!)
  java-gradle/, java-maven/, java-spring/ # java-spring includes ["java-gradle", "java-spring"] (Maven Spring: java-maven --with java-spring, or java-spring --without java-gradle --with java-maven)
  cpp-clangd/, cpp-intellisense/, shell/, github-workflows/
```

#### Profile Recipe Schema (`profiles/<name>/profile.json`)
- Fields: `version`, `name`, `description`, `useDefaults` (default `true`), `modules[]`, `excludeModules[]`, `ux` (`"default"` for canonical builds), `overrides` (`{ "workspace": {...}, "profile": {...} }`).
- **Effective Exclusion & Selection Precedence:**
  1. Build ONE effective `EXCLUDED` set = `recipe.excludeModules[]` + CLI `--without`, minus any ID explicitly passed via CLI `--with` (with an info note when CLI `--with` overrides a recipe `excludeModules` entry; specifying the SAME ID in both CLI `--with` and CLI `--without` fails exit `4`).
  2. Expand transitive `requires[]` and conditional `defaultWith[]` against that unified `EXCLUDED` set so a recipe exclusion is NEVER reintroduced by a companion!
  3. **Profile extension projection (`profiles/<name>/extensions.json`):** Ordered, case-insensitively deduplicated (first wins) union of `extensions.workspace[]` + `extensions.profile[]` in VS Code `[ { "identifier": { "id": "<canonical>" } } ]` shape.
  4. **Workspace-active module predicate:** A module is workspace-active iff its `extensions.workspace[]` is non-empty OR its `settings.workspace` is non-null. Only workspace-active modules contribute `extensions.unwanted[]` to committed `.vscode/extensions.json`.

---

### Decision 2: Data Schemas, Pre-v2 Catalog Completeness & Data-Driven Detection

#### `catalog/extensions.json` & `catalog/publishers.json`
- Lowercase canonical IDs; 5 statuses: `active`, `superseded` (incl. `jetmartin.bats` -> `kherring.bats-test-runner`, `github.copilot` -> `github.copilot-chat`, etc.), `removed` (5 Marketplace 404s), `compromised` (`jscearcy.rust-doc-viewer`), `retired` (stale/redundant drops).
- **Pre-v2 completeness invariant:** Every extension ID shipped in pre-v2 `exports/*.code-profile` bundles MUST exist as a key in `catalog/extensions.json` with one of the 5 statuses so pre-v2 bundles never hard-fail on import.
- `packMembers[]` (e.g., `openai.chatgpt` -> `["openai.codex-audio"]`) qualifies for uninstall membership alongside top-level catalog keys.
- `catalog/publishers.json` equals the exact `LC_ALL=C` sorted unique lowercase publisher set across all active module extensions, and every catalog entry's `.publisher` matches its ID prefix.

#### Data-Driven Detection (`scripts/detect-toolchain.sh`)
- Scans all `modules/*/*/module.json` with `.detect != null` in ascending `detect.priority` order (`rust` 10, `astro` 20, `biome` 22, `ts-base` 26, `java-gradle` 30, `java-maven` 32, `java-spring` 35, `cpp-clangd` 40, `cpp-intellisense` 42, `python` 50, `shell` 60, `github-workflows` 80 additive `fallback: false`).
- **BSD-safe `github-workflows` rules:** Uses TWO explicit glob rules (`{ "type": "glob", "dir": ".github/workflows", "pattern": "*.yml", "max_depth": 1 }` and `{ "type": "glob", "dir": ".github/workflows", "pattern": "*.yaml", "max_depth": 1 }`, `match: "any"`) instead of non-BSD `*.{yml,yaml}`.
- **BSD `find` & ZERO-SIGPIPE expression:** Canonicalize workspace root first; use separate prune vs regular-file branches:
  `find "${search_root}" -mindepth 1 -maxdepth "${depth}" \( -type d \( -name .git -o -name node_modules -o -name target -o -name build -o -name .venv \) -prune \) -o \( -type f -name "${pat}" -print \)`
  piped into draining `awk 'NR==1{print; found=1} END{exit !found}'` while capturing `PIPESTATUS[0]` explicitly so genuine `find` errors propagate and SIGPIPE `141` never occurs!
- Preserves existing public CLI contracts (`--toolchain <id>`, `--no-detect`, `--json`, and exit codes `0` detected, `2` missing path, `3` none detected, `4` usage/unknown toolchain).

---

### Decision 3: Pure Bash 3.2 DAG, `jq` 1.7.1 Collision Check & Workspace Provenance

- **`scripts/lib/module-resolver.sh`** (NO `set -e` mutation, ALWAYS `return 4`, NEVER `exit`):
  - Enforces ID regex `^[a-z0-9][a-z0-9-]*$`, global uniqueness, unified `EXCLUDED` set, transitive `requires[]` DFS, conditional `defaultWith[]` (suppressed if in `EXCLUDED` or conflicting with selected modules), symmetric `conflicts[]` (including UX presets!), and deterministic total order.
- **Apple `jq` 1.7.1-Safe Collision Checker (`validate-json.sh`):**
  - Compares every pair `(A, B)` of **jointly selectable** modules (neither transitively requires the other, neither lists the other in `conflicts[]`, and excluding pairwise `kind == "ux"` comparisons since UX presets are mutually exclusive) within the same output scope (`workspace` vs `profile`).
  - Checks BOTH object-vs-leaf type conflicts at common prefixes BEFORE flattening AND differing typed scalar/non-union-array leaves (via `scalar_leaves($union_keys)`, skipping declared union array keys `shellcheck.exclude`, `java.import.exclusions`, `java.project.resourceFilters`, `eslint.validate`, `recommendations`, `unwantedRecommendations`).
- **Hardened `jsonc-strip.awk` & `jq -e -s` Single-Document Validation:**
  - `LC_ALL=C awk -f scripts/lib/jsonc-strip.awk` replaces comments with a single space and strips trailing commas only after valid value end tokens.
  - Settings/manifest/bundle JSON checks ALWAYS use `jq -e -s 'length == 1 and (.[0] | type == "object")'` (and `jq -e -s 'length == 1 and (.[0] | type == "array")'` for `profiles/*/extensions.json` arrays) so empty streams and `{}{}` fail with exit non-zero! Strict `.json` files bypass `jsonc-strip.awk`.
- **Workspace Provenance (`XDG_STATE_HOME`):**
  - Stores managed workspace recommendations AND managed workspace settings keys (`managedRecommendations[]`, `managedSettingsKeys[]`, `workspaceFingerprint` = canonical path + git root/baseline hash) in `${XDG_STATE_HOME:-$HOME/.local/state}/artagon-vscode/workspaces/<sha256-of-canonical-repo>.json`.
  - If state is missing or `workspaceFingerprint` mismatches (e.g., checkout deleted and recreated at the same path!), conservatively adopts existing user entries (does NOT subtract entries it did not prove it wrote!).
  - When switching formatter stacks on an existing workspace (e.g., Astro+ESLint/Prettier -> Astro+Biome) or switching from `general-minimal` back to an AI recipe (`chat.disableAIFeatures`), retires previously managed keys no longer emitted by the new module set while preserving user overrides!
  - Writes `.vscode/{settings,extensions,tasks}.json` before updating the state file atomically.

---

### Decision 4: UX Boundary, Symlink Containment, Backups & VS Code `1.147.0` Scopes

- **Explicit `--ux-target`:** `--target=workspace` with `--ux`/`--font`/`--font-size`/`--theme`/`--icon-theme` requires `--ux-target=workspace` (`--ux-in-workspace`), `--ux-target=profile:<name>` (`--profile`), or `--ux-target=user` (exit `4`, zero writes if omitted).
- **Workspace symlink containment & mode preservation:** Canonicalize repo root; refuse (`exit 4`) if `.vscode` or any target resolves outside canonical repo root. Preserve existing file modes via portable `file_mode()` (`stat -f %Lp` BSD / `stat -c %a` GNU) — never widen permissions!
- **`<CODE_USER_DIR>` safety:** Refuse symlinks pointing outside `<CODE_USER_DIR>` (exit `4` with hint; never replace symlink). Create timestamped `0600` `.bak.<ts>.<pid>` backups before any `<CODE_USER_DIR>` write and retain the last 5 backups per file (`prune_backups`). Guard comment loss on user `settings.json` (fail exit `4` with `--allow-comment-loss` hint).
- **14 Symmetric UX Keys (`crisp` & `retina` declare symmetric `conflicts[]`):** Disjoint from `core-editor`, verified `catppuccin-mocha` icon theme, `1.6`/`1.65` line heights, Nerd Font terminal stack, deleted cleanly by `--ux=default`, stripped dynamically by `export-profiles.sh`.
- **VS Code `1.147.0` Setting Scope Policy (`catalog/setting-scopes.json`):**
  - Every managed setting key has an entry recording `type`, optional `enum`, and `scope` (`workspace`, `machine`, `profile`, `application`).
  - `validate-json.sh` rejects machine/profile/application keys in `settings.workspace.jsonc` and rejects wrong types/enums everywhere.
  - Application-scoped keys (`telemetry.telemetryLevel: "off"`, `security.workspace.trust.untrustedFiles: "prompt"`, `window.titleBarStyle: "custom"`, `window.nativeTabs: false`) are marked `"allowInCompiledProfile": true, "effectiveTarget": "default-profile"` — emitted in profile bundles for portability AND audited against the user's Default profile `<CODE_USER_DIR>/settings.json` by `vspcli doctor` (`WARN` / `--strict` `FAIL`)!
  - `extensions.autoUpdate` & `extensions.autoCheckUpdates` are omitted from `core-editor` (pins are install-time verified; `doctor` warns on pinned version drift).
  - `python-base`: `extensions.workspace[]` = `["ms-python.python", "ms-python.debugpy", "ms-python.vscode-python-envs", "charliermarsh.ruff"]`; workspace keeps portable `python-envs.defaultEnvManager: "ms-python.python:venv"` (pip fallback when `uv` absent); `settings.profile.jsonc` holds machine-scoped `python-envs.alwaysUseUv: true`.
  - `java-gradle`: `settings.profile.jsonc` holds machine-scoped `java.import.gradle.arguments: "--parallel --build-cache"`.
  - `shell`: `extensions.workspace[]` = `["mads-hartmann.bash-ide-vscode", "timonwong.shellcheck", "mkhl.shfmt", "bmalehorn.vscode-fish", "jeff-hykin.better-shellscript-syntax", "kherring.bats-test-runner"]`; preserves built-in `shellscript` highlighting, sets `"[shellscript]": { "editor.defaultFormatter": "mkhl.shfmt", "editor.formatOnSave": false }`, `"shfmt.formatIgnored": false` (so `.editorconfig` `ignore = true` for Zsh blocks BOTH save and manual `Format Document`!), `"shfmt.executableArgs": ["-i", "2", "-ci", "-bn"]`, and `shellcheck.ignorePatterns` covering `**/*.zsh`, `**/.zshrc`, `**/.zshenv`, `**/.zprofile`, `**/.zlogin`, `**/.zlogout`, `**/*.zsh-theme`.
  - `astro`, `ts-eslint-prettier`, `biome`: `ts-eslint-prettier` owns Prettier defaultFormatter + ESLint `source.fixAll.eslint` for TS/JS/JSON/CSS/Astro; `biome` owns `biomejs.biome` defaultFormatter + `source.fixAll.biome` / `source.organizeImports.biome` (so switching stacks swaps formatter & save actions cleanly!).
  - `rust/tasks.json`: Plain `cargo doc` default in portable workspace; `rtk` variants only under `--rtk-in-workspace` or profile scope.

---

### Decision 5: Portable XDG RTK Terminal Wrappers (`rtk-terminal`)

- Installed via `--no-follow` atomic write into validated absolute `${XDG_CONFIG_HOME:-$HOME/.config}/artagon-vscode/rtk/`.
- **`rtk-init.bash`:** `_ARTAGON_RTK_RC_LOADED=1` set BEFORE login-only `~/.bash_profile` OR `~/.bashrc` (never both); `ARTAGON_RTK_WRAPPED=1` set AFTER `command -v rtk`; `type -P "${cmd}"` + `unalias` + `eval "function ..."`; final `if [[ $- == *i* ]]; then ...; fi` returns `0`.
- **`rtk-init.fish`:** `set -g ARTAGON_RTK_WRAPPED 1` AFTER `command -q rtk` (stays unset without `rtk`); `return 0`; interactive-only banner; launch args quote spaced XDG paths.
- **`rtk-init.zsh` + `ZDOTDIR` proxy:** Launch env captures BOTH `ARTAGON_ORIG_ZDOTDIR_SET=0|1` AND `ARTAGON_ORIG_ZDOTDIR` BEFORE overriding `ZDOTDIR` (with self-loop guard). Non-login interactive Zsh restores/unsets `ZDOTDIR` at the end of `.zshrc`; login Zsh keeps proxy `ZDOTDIR` through `.zshrc` and restores/unsets `ZDOTDIR` at the end of `.zlogin` (cooperating with `USER_ZDOTDIR` / `VSCODE_SHELL_INTEGRATION`).

---

### Decision 6: Locking, Store Identity, Import Security, Reconcile & Compatibility

1. **ABA-Free Locking (`common.sh`):**
   - `acquire_lock` uses atomic `mkdir` writing `pid`, `host`, and `started_at`.
   - **Never silently auto-evicts stale locks** (eliminates the two-reclaimer ABA race!). If a lock exists and exceeds timeout, fails closed printing owner PID/host/path and actionable hint (`--break-stale-lock` when PID is confirmed dead). `release_lock` verifies PID ownership before `rm -rf`.
   - `vspcli build` holds ONE shared generation lock across compose + export so `exports/*.code-profile` and `exports/SHA256SUMS` NEVER observe mixed generations!
2. **Single Consistent VS Code Store Identity (`resolve_code_store()`):**
   - Supports `VSCODE_USER_DATA_DIR` (preferred root containing `User/`) and legacy `VSCODE_USER_DIR` (the `User/` dir).
   - Verifies that `canonical_path("${CODE_USER_DATA_ROOT}/User") == canonical_path("${CODE_USER_DIR}")` BEFORE invoking `code --user-data-dir "${CODE_USER_DATA_ROOT}"`! If a legacy caller passes an arbitrary non-`User` basename (e.g., `/tmp/custom-dir`) AND runs a mutating `code` command, refuses with a clear actionable error (requiring `.../User` or `VSCODE_USER_DATA_DIR`) so file lookup and `code` CLI can NEVER target two different stores!
3. **`import-profile.sh` Hardening & Parsed-Config Deny-List:**
   - Canonical-first single private `0600` staging copy; `SHA256SUMS` check (mismatch fails pre-mutation; managed `exports/` requires unique match; third-party warns on missing/unlisted).
   - Exact `jq -e -s` schema validation: `length == 1 and (.[0] | type == "object") and (.[0].settings == null or (.[0].settings | type == "object")) and (.[0].extensions == null or ((.[0].extensions | type == "object") and (.[0].extensions.enabled == null or (.[0].extensions.enabled | type == "array"))))`.
   - Runs the SAME parsed-config autonomy/exec deny-list (`^chat\.tools\..*autoApprove$`, `claudeCode.allowDangerouslySkipPermissions: true`, `claudeCode.claudeProcessWrapper`, `claudeCode.environmentVariables`, `"bypassPermissions"`) over bundle `.settings` BEFORE mutation (fail closed unless `--allow-unsafe-settings` is passed!).
   - Validates `EXTENSION_ID_REGEX` on every item, skips ALL 4 non-active catalog statuses (`removed`/`compromised`/`superseded`/`retired`), plus skips `AI`-group IDs AND strips `ai-agents` coexistence settings when `--no-ai` / `--without-group AI` is passed, and verifies `catalog/publishers.json` on remaining active IDs BEFORE touching `storage.json`.
   - **Exclusive `storage.json` registration:** Checks running VS Code BEFORE lock AND re-checks AFTER acquiring lock (failing closed unless `--force`, which explicitly accepts lost-update risk); records `storage.json` pre-read SHA-256 and re-verifies SHA-256 right before atomic rename (CAS guard against any independent writer starting mid-flight!); validates JSON, rejects duplicate names or invalid locations (`^[A-Za-z0-9_-]+$`), verifies unused `rand_hex_8` ID dir, writes bounded timestamped `.bak` (`0600`), publishes `0600`.
4. **`vspcli reconcile` & `vspcli doctor`:**
   - `reconcile` computes the full ordered plan (Step 0 compromised, Step 1 missing active, Step 2 removed/superseded/retired/unmanaged+packMembers) FIRST; under `--dry-run`, prints the complete plan and returns `0` with ZERO mutations (including ZERO Step 0 uninstalls!).
   - `reconcile --compromised-only` (or `doctor` printing exact `code [--user-data-dir] --uninstall-extension` commands) remediates compromised extensions even in Default or unmanaged profiles.
   - `install-extensions.sh` and `open-profiles.sh` also support `--without-group AI` and overridable `VSCODE_EXT_CACHE_DIR`!
5. **`check-extension-compatibility.sh` & `--check-compat=block`:**
   - Preserves exit-code contract (`0` all compatible, `1` findings, `2` misuse/error) and accepts `--extensions-file <path>`.
   - `install-workspace.sh --check-compat=block` verifies every requested ID from `--extensions-file` appears cleanly in the parsed JSON output (rejecting missing IDs, mismatched IDs, duplicates, unknowns, missing `code`/checker, or non-JSON) with zero writes to `.vscode/`, XDG, or `VSCODE_USER_DIR`.
6. **Hermetic Offline Test Harness:**
   - Both `scripts/tests/run.sh` AND `scripts/tests/bats/test_helper.bash` isolate `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `VSCODE_USER_DIR="${TEST_TMP}/vscode/User"`, `VSCODE_EXT_CACHE_DIR="${TEST_TMP}/ext-cache"`, `ARTAGON_VSCODE_PINS_FILE` (empty), preserve `CARGO_HOME`/`RUSTUP_HOME`, AND install a fail-closed `curl` stub in `TEST_TMP/bin` for offline tests so live Marketplace fallback is physically impossible!
   - Migrates ALL existing tests (`run.sh`, `check-ux-symmetry.sh`, `cli-flags.bats`, `profile-catalog-migration.bats`, `install-extensions-layered.bats`, `workspace-vscode-recs.bats`, `rust-docs-settings.bats`, `rust-docs-strict-gate.bats`) in the same phase that retires `_shared/`/`_overrides/`/`_catalog/`.

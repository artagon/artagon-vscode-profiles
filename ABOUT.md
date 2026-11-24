# About — Artagon VS Code Profiles

## Overview

`artagon-vscode-profiles` is a modular profile compiler, workspace toolchain configurator, and supply-chain-hardened extension manager for Visual Studio Code (`1.147+`). It ships **15** compiled profile recipes and **32** composable vertical modules (**14** capabilities, **15** toolchains, **3** UX presets) backed by an authoritative catalog of **103** active Marketplace-verified extensions and **8** install-time SHA-256 VSIX pins.

The entire compiler, validator, importer, reconciler, and doctor CLI (`scripts/vspcli`) runs on **portable Bash + `jq`** across macOS and Linux with zero Python, Node, or Rust runtime dependencies. Live profile recipes, module manifests, catalog entries, and compiled bundles live in [`catalog/`](catalog), [`modules/`](modules), [`profiles/`](profiles), [`exports/`](exports), and [`README.md`](README.md).

## Design Guarantees

- **Strict workspace-vs-personal separation**: `vspcli apply --target=workspace` writes only team-shareable `extensions.workspace[]`, `settings.workspace.jsonc`, and portable plain `tasks.json` into `.vscode/`. Personal AI agents, UX presets (`crisp`, `retina`), and RTK terminal wiring never leak into team workspaces unless explicitly opted in (`--ux-in-workspace`, `--rtk-in-workspace`).
- **Hardened local-first & non-autonomous defaults**: Shipped profiles default to `security.workspace.trust.untrustedFiles: "prompt"`, disable telemetry (`telemetry.telemetryLevel: "off"`), preserve explicit AI opt-outs (`ai-copilot`, `general-minimal`, `--no-ai`, `--without ai-agents`), and block auto-approve / CLI-wrapper bypass settings via the trusted bundle autonomy deny-list.
- **Supply-chain verification & GlassWorm remediation**: Universal core/UX extensions install from SHA-256-pinned official `https://` Marketplace VSIX packages through a zero-TOCTOU content-addressed local cache (`0700`/`0600` private permissions); publisher allowlists (`catalog/publishers.json`) gate every install; and compromised `jscearcy.rust-doc-viewer` is uninstalled unconditionally as Step 0 in `vspcli reconcile` and flagged `FAIL` across all profiles (including Default) by `vspcli doctor`.
- **Portable Bash + `jq` deterministic builds & atomic writes**: Rebuilding with `vspcli build` deterministically emits `_merged/*.json`, `profiles/*/{extensions,settings}.json`, `exports/*.code-profile`, 22 legacy v1 symlinks, and `exports/SHA256SUMS` using atomic temp-file swaps (`0644`/`0600`) and graceful theme fallback on partial extension installs.

## Capability Domains

- **Core Editor, IntelliJ-Grade DX & Universal Data Formats** — 24+ native IDE ergonomics settings (inlay hints, sticky scroll, semantic highlighting, bracket pairs, file nesting, Terminal IntelliSense, Kitty image support, SCM Graph, inline diffs), universal JSON/JSONC, YAML, TOML, XML, CSV (`rainbow-csv`), DotEnv, INI/Properties, Error Lens, spell checking, plus opt-in `dx-pro` (Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io) and `intellij-keybindings`
- **Multi-Agent AI Suite & Clean Completion Coexistence** — Google Antigravity (`google.google-antigravity`) as default primary agent alongside Claude Code (`anthropic.claude-code`), OpenAI Codex (`openai.chatgpt`), and unified GitHub Copilot Chat (`github.copilot-chat`) owning inline completions and Next Edit Suggestions, with `Monaspace Krypton NF` ghost-text typography, safe VS Code `1.120`–`1.147` chat tool ergonomics, and first-class Copilot-only (`ai-copilot`) and zero-AI (`general-minimal`, `--no-ai`) opt-outs
- **Systems, JVM, Python, Web, Shell & CI/CD Toolchains** — auto-detected composable toolchains for Rust (`rust-analyzer`, CodeLLDB, Dependi, Coverage Gutters, strict rustdoc policy), Java (`java-base` with native Lombok + `java-gradle` / `java-maven` / `java-spring`), Python (`python-base` with Ruff + `uv`/venv + pytest + `python` BasedPyright or `python-ty` Astral `ty`), TypeScript/Astro (`ts-base` + `astro` + `ts-eslint-prettier` or `biome`), C/C++ (`cpp-base` + `cpp-clangd` or `cpp-intellisense`), Shell (Bash IDE, ShellCheck, `shfmt`, Fish, BATS across all 15 profiles), Markdown, Containers, Git Pro, SonarQube for IDE, and GitHub Actions (`github-workflows`)
- **Curated Typography, Symmetric UX Presets & Themes** — 15-key exact-symmetry presets for HiDPI (`retina`: Maple Mono NF cursive italics + Catppuccin Mocha + Catppuccin Icons + Fluent Product Icons + `'calt', 'liga', 'zero', 'ss03', 'ss07', 'ss08'`) and standard-DPI monitors (`crisp`: JetBrains Mono + Tokyo Night + Material Icons + Material Product Icons + `'calt', 'liga', 'zero'`), UX-agnostic `default`, plus 30 curated theme/icon packs in `themes-pack`
- **Portable RTK Terminal Wrappers & Lifecycle Tooling** — XDG-installed Fish, Bash, and Zsh (`ZDOTDIR` proxy) RTK terminal wrappers, batched `--install-extension` with per-extension retry fallback, `vspcli reconcile` drift management, `vspcli doctor --strict` host/Default-profile auditing, and live Marketplace compatibility verification

## Profile & Module Inventory

This repository compiles **15** canonical profiles from **32** vertical modules and **103** active catalog extensions (**134** total tracked catalog entries including retired/superseded/compromised records, plus **8** SHA-256 VSIX pins).

| Layer | Count | Location | Role |
| :--- | ---: | :--- | :--- |
| Compiled Profiles | **15** | [`profiles/`](profiles) · [`exports/`](exports) | Thin `profile.json` recipes compiled to UX-agnostic `_merged/*.json`, `.code-profile` bundles, 22 legacy v1 symlinks, and `exports/SHA256SUMS` |
| Capabilities | **14** | [`modules/capabilities/`](modules/capabilities) | Vertical editor, AI, DX, data-format, Shell, Markdown, Git, Containers, Quality, GitHub Actions, RTK terminal, and theme modules |
| Toolchains | **15** | [`modules/toolchains/`](modules/toolchains) | Auto-detected language stacks (`rust`, `java-*`, `python*`, `ts-*`, `astro`, `biome`, `cpp-*`) with explicit `requires`/`conflicts` DAG rules |
| UX Presets | **3** | [`modules/ux/`](modules/ux) | 15-key symmetric `crisp` and `retina` presets plus empty `default` |
| Catalog & Pins | **103** active / **8** pins | [`catalog/`](catalog) | Lowercase extension registry, exact publisher allowlist, setting-scope & forbidden-key registry, and live SHA-256 VSIX pins |

See [`README.md#profile--module-matrix`](README.md#profile--module-matrix) for the auto-generated full table of all 15 profiles and 32 modules.

## Pipeline & Verification

- **Dual-OS CI**: [`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every push and pull request across `ubuntu-latest` and `macos-latest` using each platform's Bash environment without pinning to a specific shell version.
- **Static & Structural Gates**: `shellcheck` across all production/test Bash scripts, `zsh -n` across all Zsh `ZDOTDIR` RTK scripts, `fish -n` on Fish RTK scripts, `vspcli build` artifact-sync gate, `validate-json.sh` (JSON/JSONC syntax, `catalog/` referential integrity, `GROUP_ORDER` completeness, module DAG & collision checks, application-scope & forbidden-key guards), `check-ux-symmetry.sh`, Rust docs policy sync (`apply-rust-policy.sh --check`), and `generate-readme-matrix.sh --check`.
- **Hermetic Offline Tests & Live Audits**: `scripts/tests/run.sh` runs **128** hermetic offline `bats-core` tests and smoke checks in an isolated checkout copy; live Marketplace compatibility (`scripts/check-extension-compatibility.sh --all --marketplace-only --json`), pin refresh (`scripts/refresh-pins.sh`), and host health (`vspcli doctor --strict`) verify runtime state end to end.

## Quick Links

- [`README.md`](README.md) — quick start, security contract, capability highlights, auto-generated profile & module matrix, CLI reference, and troubleshooting
- [`CHANGELOG.md`](CHANGELOG.md) — release and architecture change history
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — contributor workflow, git hooks (`scripts/install-hooks.sh`), and portable Bash + `jq` guidelines
- [`SECURITY.md`](SECURITY.md) — vulnerability disclosure policy and GitHub Security Advisories channel
- [`docs/rust.md`](docs/rust.md) — Rust documentation policy, `rust-analyzer` hover/signature settings, and `cargo doc` workflow
- [`catalog/`](catalog) — authoritative `extensions.json`, `publishers.json`, `extension-pins.json`, `setting-scopes.json`, and `defaults.json`
- [`scripts/vspcli`](scripts/vspcli) & [`scripts/tests/run.sh`](scripts/tests/run.sh) — unified CLI entrypoint and hermetic offline test runner
- [`openspec/`](openspec) — OpenSpec design proposals, specifications, research notes, and adversarial review ledgers

## Topics

`vscode` `vscode-profiles` `profiles` `developer-experience` `ai-agents` `antigravity` `claude-code` `codex` `copilot` `rust` `java` `python` `typescript` `astro` `cpp` `shell` `bash` `supply-chain-security` `dotfiles` `devtools`

## Authors & License

Maintained by Artagon & Giedrius Trumpickas. Repository license is not yet declared in a root `LICENSE` file; referenced VS Code extensions and fonts ship under their respective upstream Marketplace and vendor licenses.

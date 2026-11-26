# About — Artagon VS Code Profiles

## Overview

`vsp` manages Visual Studio Code profiles, workspace toolchains, and extension installation on macOS and Linux. The v3.0.0 cutover makes the compiled Rust CLI the primary interface. One engine resolves packs, previews changes, applies settings, audits installed extensions, and exports portable bundles.

## Architecture

Canonical v3 source definitions live in the paths below. Legacy `modules/`, `profiles/`, and `catalog/*.json` files remain in the repository strictly for legacy BATS compatibility suites.

| Source | Role |
| --- | --- |
| [`packs/`](packs) | Canonical JSONC capability and toolchain packs with dependencies, conflicts, and workspace detection signals. |
| [`presets.jsonc`](presets.jsonc) | 15 canonical profile recipes, display presets, and 22 legacy profile aliases. |
| [`catalog/extensions.jsonc`](catalog/extensions.jsonc) | Extension metadata, defaults, lifecycle status, and publisher policy. |
| [`catalog/policy.jsonc`](catalog/policy.jsonc) | Setting scopes, forbidden keys, autonomy rules, and Default-profile expectations. |
| [`vsp.lock.json`](vsp.lock.json) | Per-platform VSIX hashes, version locks, and verification state. |
| [`crates/vsp/`](crates/vsp) | Rust CLI, embedded schema, resolver, export pipeline, planner, and target synchronization. |

The compiled binary embeds the canonical source catalog and packs. Use `--catalog DIR` to point it at a local checkout. The v3 design keeps personal defaults and custom user recipes in `~/.config/vsp/config.jsonc` or its XDG equivalent, so users can tailor profiles without forking this repository. Teams can share an explicit config file with `vsp --config FILE`. For security, `vsp` never loads repo-local `.vsp.toml` or `.vsp.jsonc` automatically.

## Design Guarantees

- **Workspace scope.** Workspace sync writes recommendations, workspace-scoped settings, and portable tasks under `.vscode/`. Profile or user sync can apply personal UX and XDG RTK terminal wrappers.
- **Autonomy policy.** The deny-list scans flattened and nested settings paths for auto-approval and execution-wrapper overrides. Relevant key and string checks also catch case variations and boolean strings.
- **Pre-write atomicity.** `vsp sync` validates policies, host-path containment, and VSIX lock preflight before modifying `storage.json`, settings, extensions, or provenance. Pre-write safety refusals exit `5` with zero mutations; mid-apply `code` failures after mutations begin exit `6` with an applied-vs-failed summary.
- **GlassWorm remediation.** Profile sync removes compromised and removed extensions in Step 0 before installing extensions. Use `vsp sync --all --compromised-only` to sweep registered profiles and Default. `vsp doctor` flags compromised extensions, passes cleanly without opt-in `rtk-terminal` wrappers unless enabled, and returns exit `7` on incomplete observation.
- **Four-tier VSIX policy.** Tier 0 tracks compromised and removed IDs in `catalog/extensions.jsonc` for uninstall. Tier 1 uses per-platform SHA-256 hashes, Tier 2 records executable extension versions, and Tier 3 permits unlocked declarative extensions. Unverified entries remain marked `pending_refresh`. Selective `--allow-unlocked` relaxes only pending or missing Tier 1 entries while continuing to enforce verified SHA-256 digests, approved HTTPS Marketplace URLs, and Tier 0 blocks.
- **Lossless JSONC edits.** Concrete syntax tree merges retain untouched comments and user-owned keys. Advisory locks, compare-and-swap checks, and atomic replacement protect VS Code profile registration. Local provenance records track what `vsp` manages for each workspace and profile.

## File Footprint

The checkout no longer tracks 83 build outputs. These comprised 15 `_merged/` settings files, 30 generated settings and extension files under `profiles/`, and 38 files under `exports/`. The engine compiles profiles in memory. `vsp export` writes selected bundles on request, and `vsp dev dist` writes 15 canonical bundles, 22 legacy aliases, and `SHA256SUMS`. The release workflow publishes profile and binary archives with checksums and signed build provenance. [GitHub Releases](https://github.com/Artagon/artagon-vscode-profiles/releases)

## Capability Domains

- **Core Editor, IntelliJ-Grade DX & Universal Data Formats** — 24+ native IDE ergonomics settings (inlay hints, sticky scroll, semantic highlighting, bracket pairs, file nesting, Terminal IntelliSense, Kitty image support, SCM Graph, inline diffs), universal JSON/JSONC, YAML, TOML, XML, CSV (`rainbow-csv`), DotEnv, INI/Properties, Error Lens, spell checking, plus opt-in `dx-pro` (Bookmarks, Project Manager, Scratchpads, Better Comments, Regex Previewer, Peacock, Bruno HTTP, Draw.io) and `intellij-keybindings`
- **Multi-Agent AI Suite & Clean Completion Coexistence** — Google Antigravity (`google.google-antigravity`) as default primary agent alongside Claude Code (`anthropic.claude-code`), OpenAI Codex (`openai.chatgpt`), and unified GitHub Copilot Chat (`github.copilot-chat`) owning inline completions and Next Edit Suggestions, with `Monaspace Krypton NF` ghost-text typography, safe VS Code `1.120`–`1.147` chat tool ergonomics, and first-class Copilot-only (`ai-copilot`) and zero-AI (`general-minimal`, `--ai off`) opt-outs
- **Systems, JVM, Python, Web, Shell & CI/CD Toolchains** — auto-detected composable toolchains for Rust (`rust-analyzer`, CodeLLDB, Dependi, Coverage Gutters, strict rustdoc policy), Java (`java-base` with native Lombok + `java-gradle` / `java-maven` / `java-spring`), Python (`python-base` with Ruff + `uv`/venv + pytest + `python` BasedPyright or `python-ty` Astral `ty`), TypeScript/Astro (`ts-base` + `astro` + `ts-eslint-prettier` or `biome`), C/C++ (`cpp-base` + `cpp-clangd` or `cpp-intellisense`), Shell (Bash IDE, ShellCheck, `shfmt`, Fish, BATS across all 15 profiles), Markdown, Containers, Git Pro, SonarQube for IDE, and GitHub Actions (`github-workflows`)
- **Curated Typography, Symmetric UX Presets & Themes** — 15-key exact-symmetry presets for HiDPI (`retina`: Maple Mono NF cursive italics + Catppuccin Mocha + Catppuccin Icons + Fluent Product Icons + `'calt', 'liga', 'zero', 'ss03', 'ss07', 'ss08'`) and standard-DPI monitors (`crisp`: JetBrains Mono + Tokyo Night + Material Icons + Material Product Icons + `'calt', 'liga', 'zero'`), UX-agnostic `default`, plus 30 curated theme/icon packs in `themes-pack`
- **Portable RTK terminal wrappers.** The `rtk-terminal` pack includes Bash, Fish, and Zsh wrappers under `packs/rtk-terminal/scripts/`.

## Verification

CI runs locked Rust tests, Clippy, rustdoc with warnings treated as errors, locked release builds, catalog validation, and two distribution builds to compare output bytes. It also checks the `README.md` matrix natively with `vsp --catalog . dev docs --check`, validates `Formula/vsp.rb` syntax, checks the Rust documentation policy, and tests shell wrappers on macOS and Linux. The golden tests compare canonical settings, extensions, and bundles against the v2.1 checksums. [CI workflow](.github/workflows/ci.yml)

See [README](README.md) for installation and CLI examples, [Contributing](CONTRIBUTING.md) for local gates, and [Security](SECURITY.md) for vulnerability reporting.

## Authors & License

Maintained by Artagon & Giedrius Trumpickas. Repository license is not yet declared in a root `LICENSE` file. Referenced extensions and fonts use their respective upstream licenses.

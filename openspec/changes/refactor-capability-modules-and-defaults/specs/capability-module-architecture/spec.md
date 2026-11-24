# Specification: Capability Module Architecture

## Purpose

Defines the declarative vertical capability, toolchain, and UX module layout, pure Bash 3.2 + `jq` topological DAG (`requires[]`, `defaultWith[]`, `conflicts[]`), dual settings/extension scope separation with `XDG_STATE_HOME` workspace provenance, and `jq` 1.7.1-safe collision checking.

## ADDED Requirements

### Requirement: Vertical module manifest schema and auto-discovery

The system SHALL organize modular capabilities under `modules/capabilities/<id>/`, toolchains under `modules/toolchains/<id>/`, and UX presets under `modules/ux/<id>/`, each containing a `module.json` whose `id` matches `^[a-z0-9][a-z0-9-]*$`, equals its directory basename, and is unique across all `modules/*/`. Tooling SHALL auto-discover modules and `profiles/<name>/profile.json` recipes.

#### Scenario: Auto-discovering a newly added toolchain module
- **WHEN** a contributor adds `modules/toolchains/go/module.json` and registers its extensions in `catalog/extensions.json` and `catalog/publishers.json`
- **THEN** `vspcli detect`, `vspcli build`, `vspcli apply`, `vspcli validate`, and `vspcli completions` recognize `go` automatically without editing shell script `case` tables.

#### Scenario: Invalid or duplicate module manifest is rejected
- **WHEN** a `module.json` has an invalid `id`, omits `kind`, duplicates an ID in another category, or references a nonexistent sibling file
- **THEN** `vspcli validate` exits non-zero citing the offending module path and violation.

### Requirement: Topological DAG resolution with defaultWith, cycle, and conflict checks

`scripts/lib/module-resolver.sh` SHALL expand transitive `requires[]` and conditional `defaultWith[]` (suppressed when excluded or conflicting with another selected module) in deterministic parent-first order using pure Bash 3.2 logic, returning status `4` (never calling `exit`) on invalid IDs, cycles, pairwise conflicts, excluded required dependencies, or contradictory CLI `--with`/`--without` on the same ID.

#### Scenario: Transitive dependency ordering
- **WHEN** module `astro` declares `"requires": ["ts-base"]` and `ts-base` declares `"requires": ["core-editor"]`
- **THEN** resolving `astro` emits `core-editor`, `ts-base`, `astro` in deterministic order with no duplicates.

#### Scenario: Default companion auto-selects ESLint/Prettier and yields to Biome
- **WHEN** `astro` declares `"defaultWith": ["ts-eslint-prettier"]`
- **THEN** resolving `astro` alone includes `ts-eslint-prettier`, while resolving `astro` + `biome` omits `ts-eslint-prettier` cleanly without conflict.

#### Scenario: Dependency cycle, conflict, or excluded requirement returns status 4
- **WHEN** resolution encounters a dependency cycle, conflicting pair (`cpp-clangd`+`cpp-intellisense`, `java-gradle`+`java-maven`, `python`+`python-ty`, `ts-eslint-prettier`+`biome`, `ai-agents`+`ai-copilot`), excluded required dependency, or `--with X --without X`
- **THEN** `resolve_modules` returns status `4` with an error on stderr and writes no files.

### Requirement: Dual scope separation and XDG_STATE_HOME workspace provenance

Each module SHALL separate workspace vs profile settings and `extensions.workspace[]` vs `extensions.profile[]` vs `extensions.unwanted[]`. Workspace applies SHALL emit only workspace settings, recommendations, and workspace-module `unwanted[]`, track `vspcli`-inserted IDs in `XDG_STATE_HOME` provenance, subtract only previously generated `recommendations`, normalize IDs case-insensitively, and exclude unwanted IDs from `recommendations`.

#### Scenario: Workspace apply composes toolchain settings and extensions without AI or UX leakage
- **WHEN** a user runs `vspcli apply --target=workspace /path/to/python-repo`
- **THEN** `.vscode/settings.json` includes `python`'s workspace settings and omits personal UX and machine-scoped keys, and `.vscode/extensions.json` omits `anthropic.claude-code`, `openai.chatgpt`, `github.copilot-chat`, `github.copilot`, and UX themes.

#### Scenario: Workspace provenance preserves user-owned IDs and unwantedRecommendations
- **WHEN** `.vscode/extensions.json` already contains user-added `timonwong.shellcheck` and `unwanted.bar`, `vspcli` applies Rust+Shell and then reruns with `--without shell`
- **THEN** `timonwong.shellcheck`, `unwanted.bar`, `jscearcy.rust-doc-viewer` in `unwantedRecommendations`, and unrelated JSONC fields are preserved, only `vspcli`-inserted shell-only IDs are removed from `recommendations`, and zero `code --install-extension` calls run.

#### Scenario: Profile build projects both workspace and profile extensions in VS Code format
- **WHEN** `vspcli build rust` generates `profiles/rust/extensions.json`
- **THEN** it emits the ordered, case-insensitively deduplicated union of `extensions.workspace[]` and `extensions.profile[]` as `[ { "identifier": { "id": "..." } } ]`.

### Requirement: Hardened JSONC stripping, single-object parsing, and jq 1.7.1 collision checks

`scripts/lib/jsonc-strip.awk` (`LC_ALL=C`) SHALL replace comments with a single space and strip trailing commas only after valid JSON value tokens. `scripts/lib/compiler.sh` SHALL enforce `jq -e -s` single-object parsing, strip `"$schema"` and `"@extends"`, deep-merge objects, set-union declared array keys, merge `tasks.json` by `.label`, and reject object-vs-leaf and typed `scalar_leaves` collisions among jointly selectable modules (excluding mutually exclusive UX presets).

#### Scenario: Malformed JSONC tokens and multi-document inputs are rejected
- **WHEN** an input contains `{"limit":1/*x*/2}`, `{"list":[,]}`, an empty file, or two adjacent objects `{}{}`
- **THEN** validation and compilation reject it non-zero without altering existing output.

#### Scenario: Recursive collision walker handles Apple jq 1.7.1, nested keys, diamonds, and union arrays
- **WHEN** `vspcli validate` runs under Apple `jq` 1.7.1 across non-conflicting sibling toolchains (including `astro` + `ts-eslint-prettier` sharing `eslint.validate`)
- **THEN** declared union arrays do not false-fail, diamond requirements pass, and differing nested typed leaves (including `false` vs `null`) fail with exit 1.

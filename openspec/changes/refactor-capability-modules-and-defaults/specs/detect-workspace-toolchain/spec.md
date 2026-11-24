# Specification: Detect Workspace Toolchain

## Purpose

Defines data-driven workspace toolchain and capability detection from module manifests, manual override, structured JSON output, and the CLI exit-code contract.

## ADDED Requirements

### Requirement: Data-driven detection with layering, additive workflows, and zero-SIGPIPE BSD find

`scripts/detect-toolchain.sh` SHALL scan all `modules/*/*/module.json` with non-null `.detect` in ascending priority order, evaluate `match` across all 6 rule types using canonical roots, separate `-type d ... -prune` vs `-type f -name` branches, and draining `awk` matching (zero SIGPIPE, propagating `find` errors), apply `detect.supersedes[]`, and keep `github-workflows` additive (`fallback: false`).

#### Scenario: Spring Boot layers over detected Gradle or Maven build tool
- **WHEN** `scripts/detect-toolchain.sh` inspects (a) `build.gradle.kts` + `@SpringBootApplication`, (b) `pom.xml` + `@SpringBootApplication`, (c) both manifests WITH `@SpringBootApplication`, and (d) both manifests WITHOUT `@SpringBootApplication`
- **THEN** it emits (a) `java-gradle java-spring`, (b) `java-maven java-spring`, (c) `java-gradle java-spring` with a stderr supersession note, and (d) `java-gradle` (no `java-spring`) with a stderr supersession note.

#### Scenario: Mixed Rust plus GitHub Workflows retains additive Actions tooling
- **WHEN** a repository contains both `Cargo.toml` and `.github/workflows/ci.yml`
- **THEN** `detect-toolchain.sh` emits `rust github-workflows`, while a repo with only `Cargo.toml` emits `rust` and a repo with only `.github/workflows/ci.yml` emits `github-workflows`.

#### Scenario: Symlinked root with spaces and large match sets never fail with SIGPIPE
- **WHEN** `scripts/detect-toolchain.sh` inspects a symlinked directory path containing spaces and >100 matching `.bats` files
- **THEN** it resolves the canonical root, drains `find` cleanly without SIGPIPE `141`, and emits `shell` with exit `0`.

### Requirement: Manual toolchain override, JSON mode, and exit code contract

`scripts/detect-toolchain.sh` SHALL support `--toolchain <id>` (bypassing auto-detection), `--no-detect` (requiring `--toolchain`), and `--json` (emitting `{ "toolchains": [...], "signals": {...}, "override": true|false }`), with exit codes `0` (detected), `2` (missing path), `3` (none detected), and `4` (unknown toolchain or invalid flag combination).

#### Scenario: Manual override and structured JSON output work on empty workspaces
- **WHEN** `detect-toolchain.sh --json --toolchain rust <empty-dir>` or `detect-toolchain.sh --json <empty-dir>` runs
- **THEN** `--toolchain rust` exits `0` with `"override": true` and `["rust"]`, while an empty workspace without override exits `3` with `"toolchains": []`.

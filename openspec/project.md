# Project Context

## Purpose
This repository manages Visual Studio Code profile configurations under `~/.config/vscode`. It keeps shared editor baselines, per-profile overrides, and automation scripts in one place so teams can compose, validate, export, and share profiles consistently.

## Scope and Goals
- Reproducible VS Code profile definitions for multiple stacks and workflows.
- Shared base settings for crisp and retina variants with per-profile overrides.
- Automation for validation, composition, export, installation, and profile registration.
- Portable `.code-profile` bundles for import and sharing.

## Tech Stack
- Bash shell scripts
- `jq`
- VS Code CLI (`code`)
- JSON and JSONC configuration files

## Project Structure
- `_shared/` - shared editor baselines (`editor-crisp.jsonc`, `editor-retina.jsonc`)
- `_overrides/` - per-profile overrides (strict JSON, supports `@extends`)
- `_merged/` - generated merged settings (do not edit)
- `profiles/` - profile manifests and settings symlinks
- `scripts/` - compose, validate, export, install, open, import, and CLI helpers
- `exports/` - generated `.code-profile` bundles
- `openspec/` - OpenSpec documentation and specs
- `agents/` - maintainer and LLM documentation

## Core Workflows
- `scripts/validate-json.sh` validates JSON/JSONC fragments and extensions lists.
- `scripts/compose-settings.sh` merges shared bases and overrides into `_merged/` and refreshes profile symlinks.
- `scripts/export-profiles.sh` builds `exports/<profile>.code-profile` bundles from merged settings and extensions.
- `scripts/install-extensions.sh`, `scripts/open-profiles.sh`, and `scripts/import-profile.sh` manage extensions and profile registration.
- `scripts/vspcli` provides a unified CLI for listing, composing, exporting, opening, installing, and importing.

## Constraints and Notes
- `_merged/` is generated output; do not edit files there.
- `_shared/*.jsonc` and `_overrides/*.jsonc` must be strict JSON (no comments).
- Overrides may declare `"@extends"` to reuse base settings in parent-first order. The composer accepts only bare filenames or single-level subpaths under `_overrides/` and rejects `..`, leading `/`, leading `~`, backslashes, and NUL. Cycles are detected by resolved real path, so symlinked overrides cannot trick the detector.
- Profile names containing `retina` use `editor-retina.jsonc`; all others use `editor-crisp.jsonc`.
- `profiles/<name>/settings.json` is a repo-relative symlink to `_merged/<name>.json`.
- Java Spring leaf overrides use a symlink invariant: `_overrides/java-spring-{crisp,retina}.jsonc` are symlinks pointing at `java-spring-base.jsonc`. The composer's filename-keyed base selection means each leaf inherits the correct shared-DPI base while sharing every other key. Tools that "convert symlinks to files" (some IDE save modes, some archivers) will silently break propagation. `scripts/tests/run.sh` asserts the symlink shape on every run.

## External Dependencies
- VS Code CLI (`code`)
- `jq`
- Optional toolchains per profile (for example, jenv for Java, rustup for Rust, and CMake for C/C++)

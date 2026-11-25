//! `vsp` — Typed domain model, legacy-layout offline catalog loader, DAG module
//! resolver, security policy validator, unified `vsp-plan/v1` planner, lossless JSONC
//! CST merger, OS-locked `storage.json` CAS manager, SHA-256 VSIX verifier, mutating
//! `sync` engine, `open` launcher, and deterministic `.code-profile` bundle compiler.
//!
//! # Architecture
//!
//! - [`exit_codes`]: Canonical exit codes (`0..=7`) and structured [`VspError`](exit_codes::VspError).
//! - [`model`]: Typed catalog, module, profile recipe, extension, and bundle structs.
//! - [`legacy_loader`]: Offline reader for `catalog/`, `modules/`, and `profiles/` using strict JSONC parsing.
//! - [`resolver`]: Topological DAG module resolver, deterministic settings/extensions/tasks compiler, and `dev dist`.
//! - [`policy`]: Nested-safe autonomy deny-list, Marketplace VSIX URL/pin checks, scope checks, collision detection, and UX symmetry.
//! - [`detect`]: Strictly read-only data-driven workspace toolchain detector.
//! - [`vscode`]: VS Code store resolver, `storage.json` reader, and `code --list-extensions` observer.
//! - [`cst_merge`]: Lossless JSONC Concrete Syntax Tree (CST) settings, extensions, and tasks merger.
//! - [`provenance`]: XDG state provenance manifests, bounded `0600` backups, atomic file writes, and path containment guards.
//! - [`storage`]: OS advisory file locking (`fs4`), SHA-256 CAS `storage.json` writer, and macOS/Linux running-VS-Code guard.
//! - [`vsix`]: Fail-closed VSIX lock verifier, 4-worker bounded downloader with per-hop redirect validation, and `--repair-pins`.
//! - [`plan`]: Unified `vsp-plan/v1` read-only planner shared by `vsp diff` and `vsp sync --dry-run`.
//! - [`sync`]: Unified mutating `vsp sync` engine for profiles, workspaces, Default user settings, bundle imports, and emergency sweeps.
//! - [`open`]: Single-window `--user-data-dir`-aware `vsp open` (alias `use`) launcher with `--sync` convergence.
//! - [`doctor`]: Read-only environment, catalog, Default-profile security, provenance, and live VS Code auditor.
//! - [`dev_ops`]: Maintainer workflows (`dev pins`, `dev compat`, `dev docs`, `dev hooks`).
//! - [`cli`]: `clap` Derive CLI definitions and command dispatch.

#![warn(missing_docs)]

pub mod cli;
pub mod cst_merge;
pub mod detect;
pub mod dev_ops;
pub mod doctor;
pub mod exit_codes;
pub mod legacy_loader;
pub mod model;
pub mod open;
pub mod plan;
pub mod policy;
pub mod provenance;
pub mod resolver;
pub mod storage;
pub mod sync;
pub mod vscode;
pub mod vsix;

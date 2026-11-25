//! `vsp` — Typed domain model, legacy-layout offline catalog loader, DAG module
//! resolver, security policy validator, unified `vsp-plan/v1` planner, and
//! deterministic `.code-profile` bundle compiler for Artagon VS Code profiles.
//!
//! # Architecture
//!
//! - [`exit_codes`]: Canonical exit codes (`0..=7`) and structured [`VspError`](exit_codes::VspError).
//! - [`model`]: Typed catalog, module, profile recipe, extension, and bundle structs.
//! - [`legacy_loader`]: Offline reader for `catalog/`, `modules/`, and `profiles/` using strict JSONC parsing.
//! - [`schema_loader`]: Consolidated v2.2 schema loader supporting embedded build snapshots and explicit catalog directories.
//! - [`resolver`]: Topological DAG module resolver, deterministic settings/extensions/tasks compiler, and `dev dist`.
//! - [`policy`]: Nested-safe autonomy deny-list, Marketplace VSIX URL/pin checks, scope checks, collision detection, and UX symmetry.
//! - [`detect`]: Strictly read-only data-driven workspace toolchain detector.
//! - [`vscode`]: Read-only VS Code store resolver, `storage.json` reader, and `code --list-extensions` observer.
//! - [`plan`]: Unified `vsp-plan/v1` read-only planner shared by `vsp diff` and `vsp sync --dry-run`.
//! - [`doctor`]: Read-only environment, catalog, Default-profile security, and live VS Code auditor.
//! - [`cli`]: `clap` Derive CLI definitions and command dispatch.

#![warn(missing_docs)]

pub mod cli;
pub mod detect;
pub mod doctor;
pub mod exit_codes;
pub mod legacy_loader;
pub mod model;
pub mod plan;
pub mod policy;
pub mod resolver;
pub mod schema_loader;
pub mod vscode;

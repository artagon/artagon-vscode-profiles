# Tasks: `unify-vsp-cli-and-composition`

## Phase 0: Security Hotfix Baseline (`v2.0.1`)

- [x] 0.1 Harden `check_autonomy_denylist_json` (nested paths, case-insensitive exec keys, string booleans), fail-closed VSIX pin loading, atomic publish error propagation, spaced macOS VS Code detection, and contested lock reclamation (`scripts/lib/{catalog,vsix-pin,common}.sh`, `scripts/validate-json.sh`).
- [x] 0.2 Harden `scripts/vspcli` (`doctor` fail-closed on listing error, `extensions.autoUpdate` boolean `false` check), `scripts/detect-toolchain.sh` error propagation, and compatibility audit checks.
- [x] 0.3 Verify full Phase 0 CI gate (ShellCheck, `zsh -n`, `fish -n`, `vspcli build`, validators, and full `scripts/tests/run.sh`: 149/149 BATS + smoke).

## Phase 1: ADR, OpenSpec, Thin `bin/vsp` Facade, Release Pipeline & Rust Core (`v2.1.0`)

### 1.1 Architecture, Facade & Release Pipeline

- [x] 1.1.1 Write STE-aligned ADR [`docs/adr/0001-vsp-cli-and-composition.md`](../../../docs/adr/0001-vsp-cli-and-composition.md) documenting CLI taxonomy, exit codes `0`–`7`, locked decisions `D1`–`D5`, 4-tier `vsp.lock.json`, Phase 2 untracking of 83 generated files, and the 4-phase Bash-to-Rust plan.
- [x] 1.1.2 Author OpenSpec change artifacts under `openspec/changes/unify-vsp-cli-and-composition/{proposal,design,tasks}.md`.
- [x] 1.1.3 Implement executable `bin/vsp` (`chmod 0755`, strict `<= 400` LOC, pure Bash 3.2+, zero new parser/planner/lock code) with 40-hop symlink resolution, `VSP_BIN` / compiled binary delegation, explicit selector guards, Step 0 `sync -p` composition, read-only `detect` and `diff`, `dev`, and `self` routing.
- [x] 1.1.4 Create SHA-pinned `.github/workflows/release.yml` for cross-platform `vsp` tarballs (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`), double-run deterministic `vsp dev dist --out dist/` verification, `vsp-profiles-<tag>.tar.gz` + `SHA256SUMS`, build provenance attestation, and GitHub Release upload.
- [x] 1.1.5 Add fast BATS suite `scripts/tests/bats/vsp-facade.bats` (<15s) verifying LOC budget, relative symlink resolution, explicit selector exit `4`, read-only `detect`, `list`, `dev docs --check`, Step 0 ordering in `sync -p`, `self`, and `VSP_BIN` delegation.

### 1.2 Rust Read-Only Core & Golden Parity (`crates/vsp`)

- [x] 1.2.1 Bootstrap workspace `Cargo.toml` and `crates/vsp` with `clap`, `clap_complete`, `serde`, `serde_json`, `jsonc-parser`, `sha2`, `semver`, and `thiserror`.
- [x] 1.2.2 Implement CLI exit codes `0`–`7` (mapping clap parse errors to `4`, `--help`/`--version` to `0`, incomplete observation to `7`) and read-only commands (`list`, `show`, `detect`, `diff`, `doctor`, `export`, `completions`, `dev validate`, `dev dist`, `self path`).
- [x] 1.2.3 Implement legacy-layout loader (`catalog/`, `modules/`, `profiles/`), DAG resolver, nested-safe autonomy policy validator, and deterministic `.code-profile` + 22 legacy alias generator (preserving `D5` `chat.editor.fontSize=14` and `ai-profile-*` -> `ai-copilot`).
- [x] 1.2.4 Pass 100% golden parity tests against the frozen v2.1 checksums for all 15 profile settings and extension lists, 15 canonical bundles, and 22 legacy aliases, double-run `dev dist` determinism, all 5 N1 security payloads, `cargo clippy -- -D warnings`, and `RUSTDOCFLAGS="-D warnings" cargo doc`.

## Phase 2: Rust Mutating `sync`, `fs4`/CAS Locking, `vsp.lock.json` & Untrack Generated Files (`v2.2.0`)

- [x] 2.1 Implement Rust mutating `sync` and `open` with `fs4` advisory locking and SHA-256 CAS guards on VS Code `storage.json` (`D3`).
  - Profile registration and workspace/profile sync are covered by `crates/vsp/tests/sync_cst_integration.rs`. User configuration and binary installation are tracked in Phase 3.1.
- [x] 2.2 Implement JSONC comment-preserving writes, workspace symlink containment, XDG target provenance, and four-tier VSIX policy (`vsp.lock.json` plus catalog Tier 0).
- [x] 2.3 Untrack the 83 generated files (`15` `_merged/*.json`, `30` `profiles/*/{settings,extensions}.json`, `38` `exports/*`) from git (`6972eeb`). Reject staged generated files in pre-commit and run `vsp dev validate` for relevant changes. CI validates and compares two `vsp dev dist` outputs.

## Phase 3: Legacy Retirement, Homebrew Launch & Final Cutover (`v3.0.0`)

- [x] 3.1 Deliver the native v3 CLI for packaged installs, add opt-in native delegation to the checkout's `scripts/vspcli`, and retire obsolete shell entry points (`718183e`, `4c8f807`).
  - [x] Implement `vsp init` and load user-only composition defaults from `~/.config/vsp/config.jsonc` (`D1`).
  - [x] Implement and verify native `vsp self install` and `vsp self uninstall`.
  - [x] Implement and test native `vspcli` compatibility dispatch and legacy exit mapping.
  - [x] Remove `scripts/.vspcli-completion.zsh`, `scripts/.vspcli-completion.fish`, `scripts/setup-rust-toolchain.sh`, `scripts/setup-cpp-toolchain.sh`, and `profiles/rust/tasks.example.json`.
- [x] 3.2 Complete release preparation and Round 2 adversarial remediation (`4c8f807`, `35f4915`, `a9b9fe7`).
  - [x] Update README, project overview, changelog, contributor workflow, and OpenSpec context for the v3.0.0 cutover.
  - [x] Document the implemented configuration and compatibility contracts, and verify the CLI interface, composition, supply-chain locking, and user-config deltas with `openspec validate unify-vsp-cli-and-composition --strict`.
  - [x] Validate staged index bytes in pre-commit and reject staged generated artifacts. Run Rust checks for relevant staged edits and deletions when Cargo is available.
  - [x] Prepare the Homebrew formula and remove `--clobber` from GitHub Release uploads to preserve existing assets.
  - [x] Land Round 2 Rust and shell remediations with regression coverage for validation, path containment, extension verification, sync, and CLI compatibility. Update the README example tests for the v3 CLI.
- [ ] 3.3 Run final release verification and close out OpenSpec.
  - [ ] Verify the tag release on all target platforms, check the published tarballs and `SHA256SUMS` attestations, and run the Homebrew install and audit gates.
  - [ ] Sync `openspec/specs/` and archive the completed change after the final gate passes.

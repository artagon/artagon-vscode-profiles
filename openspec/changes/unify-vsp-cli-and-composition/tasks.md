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

- [ ] 1.2.1 Bootstrap workspace `Cargo.toml` and `crates/vsp` with `clap`, `clap_complete`, `serde`, `serde_json`, `jsonc-parser`, `sha2`, `semver`, and `thiserror`.
- [ ] 1.2.2 Implement CLI exit codes `0`–`7` (mapping clap parse errors to `4`, `--help`/`--version` to `0`, incomplete observation to `7`) and read-only commands (`list`, `show`, `detect`, `diff`, `doctor`, `export`, `completions`, `dev validate`, `dev dist`, `self path`).
- [ ] 1.2.3 Implement legacy-layout loader (`catalog/`, `modules/`, `profiles/`), DAG resolver, nested-safe autonomy policy validator, and deterministic `.code-profile` + 22 legacy alias generator (preserving `D5` `chat.editor.fontSize=14` and `ai-profile-*` -> `ai-copilot`).
- [ ] 1.2.4 Pass 100% golden parity tests against all 15 `_merged/*.json`, 15 `profiles/*/extensions.json`, 22 legacy aliases, double-run `dev dist` determinism, all 5 N1 security payloads, `cargo clippy -- -D warnings`, and `RUSTDOCFLAGS="-D warnings" cargo doc`.

## Phase 2: Rust Mutating `sync`, `fs4`/CAS Locking, `vsp.lock.json` & Untrack Generated Files (`v2.2.0`)

- [ ] 2.1 Implement Rust mutating `sync`, `open`, `init` (`~/.config/vsp/config.jsonc` per `D1`), and `self {install,uninstall}` with `fs4` advisory locking and SHA-256 CAS guards on VS Code `storage.json` (`D3`).
- [ ] 2.2 Implement JSONC comment-preserving writes, workspace symlink containment, and 4-tier `vsp.lock.json` provenance state.
- [ ] 2.3 Untrack the 83 generated files (`15` `_merged/*.json`, `30` `profiles/*/{settings,extensions}.json`, `38` `exports/*`) from git and update CI/pre-commit hooks to validate via `vsp dev validate` and `vsp dev dist`.

## Phase 3: Legacy Retirement, Homebrew Launch & Final Cutover (`v3.0.0`)

- [ ] 3.1 Retire legacy Bash compiler internals (`scripts/lib/*.sh`) and finalize `scripts/vspcli` deprecation shim per deprecation policy.
- [ ] 3.2 Publish signed, SLSA-attested `vsp` tag release via `.github/workflows/release.yml` and launch the Homebrew formula (`D4`).
- [ ] 3.3 Archive superseded OpenSpec changes and sync `openspec/specs/` to reflect the unified `vsp` architecture.

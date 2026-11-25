# Proposal: Unify `vsp` CLI, Composition Engine & Zero-Double-Work Bash-to-Rust Migration

## Why

Following the `refactor-capability-modules-and-defaults` vertical module architecture and the Phase 0 (`v2.0.1`) security hotfix, four operational gaps remain:

1. **Fragmented CLI surface.** Everyday workflows span multiple verbs (`apply`, `reconcile`, `import`, `open`) and top-level scripts (`export-profiles.sh`, `refresh-pins.sh`, `check-extension-compatibility.sh`, `generate-readme-matrix.sh`). Users have no single `sync` / `diff` pair backed by one plan contract or a uniform explicit-selector safety rule.
2. **Ambiguous error signaling.** Verification and audit failures (for example, `code --list-extensions` failing in `doctor` or unreachable compatibility checks) share exit codes with normal operational failures instead of signaling incomplete observation (`exit 7`).
3. **Shell maintenance and speed ceiling.** Pure Bash 3.2 + `jq` composition forks hundreds of subprocesses per run, lacks OS-level advisory file locking (`fs4`), and makes full test runs take over 40 minutes.
4. **83 tracked generated files and no compiled release pipeline.** Git tracks 15 `_merged/*.json`, 30 `profiles/*/{settings,extensions}.json`, and 38 `exports/*` files, generating diff noise on every module change while lacking cross-platform compiled binaries and SLSA-attested releases.

This change introduces the unified `vsp` CLI, documents locked architecture decisions (`D1`–`D5`) in ADR 0001, adds a thin `<= 400` LOC Bash 3.2 facade (`bin/vsp`) with automatic compiled-binary handoff, establishes a SHA-pinned release workflow, and delivers a 4-phase zero-double-work migration to a compiled Rust engine (`crates/vsp`).

## What Changes

### 1. Unified `vsp` CLI & Exit Code Contract (`0`–`7`)

- Introduce `bin/vsp` (executable, `<= 400` LOC, pure Bash 3.2+, zero new JSONC parser/planner/lock logic) while keeping `scripts/vspcli` intact as a compatibility shim.
- Standardize command taxonomy:
  - Everyday verbs: `sync`, `diff`, `open` (hidden alias `use`), `doctor`.
  - Inspection & setup verbs: `list`, `show`, `detect` (strictly read-only), `export`, `init`, `completions`.
  - Grouped maintainer verbs: `dev {validate, dist, pins, compat, docs, hooks, test, release}` (plus `build` in Phase 1).
  - Grouped self-management verbs: `self {path, install, uninstall, update}`.
- Enforce explicit target selectors for `sync` and `diff` (missing selector exits `4` and never mutates `cwd`).
- Compose `vsp sync -p <name>` with unconditional Step 0 (`reconcile --compromised-only`) before settings writes (`apply --target=profile`) and full extension reconciliation (`reconcile --profile <name>`).
- Standardize exit codes `0` through `7` across facade and Rust binary, including exit `7` for incomplete observation / unavailable verification (`D2`).

### 2. Locked Architecture Decisions (`D1`–`D5`) & ADR 0001

- Add `docs/adr/0001-vsp-cli-and-composition.md` recording:
  - **D1:** JSONC-only user config (`~/.config/vsp/config.jsonc`); zero repo `.vsp.toml`.
  - **D2:** Exit `7` for incomplete observation or unavailable verification.
  - **D3:** Fail-closed Bash lock hardening in `v2.0.1` followed by OS-level `fs4` + SHA-256 CAS locking in Rust.
  - **D4:** Homebrew formula launches with the compiled Rust `vsp` binary.
  - **D5:** Preserve `profiles/ai/profile.json` `chat.editor.fontSize=14` override for 100% golden settings parity across all 15 profiles.
  - 4-tier `vsp.lock.json` provenance model and the Phase 2 untracking of 83 generated files.

### 3. Rust Engine Bootstrap (`crates/vsp`) & Deterministic Release Pipeline

- Phase 1 (`v2.1.0`): Bootstrap `crates/vsp` read-only engine (legacy-layout loader, DAG resolver, nested-safe autonomy deny-list, unified `vsp-plan/v1` planner, read-only CLI verbs, and deterministic `vsp dev dist`) verified at 100% golden parity against all 15 tracked profiles and 22 legacy export aliases.
- Add `.github/workflows/release.yml` with SHA-pinned GitHub Actions to build cross-platform `vsp` binaries (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`), run `vsp dev dist --out dist/` twice to verify byte-identical `SHA256SUMS`, package `vsp-profiles-<tag>.tar.gz`, attest build provenance, and publish GitHub Release assets.
- Phase 2 (`v2.2.0`): Implement Rust mutating `sync`/`open`/`init`/`self` with `fs4`/CAS locking and `vsp.lock.json`, then untrack the 83 generated files from git.
- Phase 3 (`v3.0.0`): Retire legacy Bash compiler internals and publish the Homebrew formula (`D4`).

## Capabilities

### New Capabilities

- `vsp-unified-cli`: `vsp` front-door taxonomy (`sync`, `diff`, `open`/`use`, `doctor`, `list`, `show`, `detect`, `export`, `init`, `completions`, `dev`, `self`), explicit-selector contract, binary auto-delegation, and exit codes `0`–`7`.
- `vsp-rust-engine-and-release`: Compiled Rust composition engine (`crates/vsp`), `vsp-plan/v1` planning, 4-tier `vsp.lock.json` provenance, and SHA-pinned deterministic GitHub Release pipeline with build provenance attestations.

### Modified Capabilities

- `manage-profile-cli`: Adds `bin/vsp` facade and exit code `7` while preserving `scripts/vspcli` backwards compatibility during transition.
- `export-profile-bundles`: Prepares transition from 83 git-tracked generated files to deterministic `vsp dev dist` release bundles (`vsp-profiles-<tag>.tar.gz` + `SHA256SUMS`).
- `extension-catalog-and-reconcile`: Formalizes Step 0 compromised-first execution inside `vsp sync -p <name>` and exit `7` audit failure semantics.

## Impact

- **Code:** Adds `docs/adr/0001-vsp-cli-and-composition.md`, `bin/vsp`, `.github/workflows/release.yml`, `scripts/tests/bats/vsp-facade.bats`, and (in parallel Phase 1.2) `Cargo.toml` + `crates/vsp/**`. Existing `scripts/vspcli` and `scripts/*.sh` remain untouched in Phase 1.
- **APIs:** Introduces the `vsp` CLI and exit code `7`; existing `scripts/vspcli` flags and scripts keep full compatibility through Phase 2.
- **Dependencies:** Phase 1 `bin/vsp` requires only Bash 3.2+ and existing script dependencies. Compiled `crates/vsp` requires Rust (`rustc`/`cargo`) at build time and produces zero-runtime-dependency native binaries.
- **Backwards compatibility:** Legacy `vspcli` invocations, 15 canonical profiles, and 22 legacy `.code-profile` aliases (including `ai-profile-{crisp,retina}` -> `ai-copilot`) remain 100% preserved.
- **Risk:** Low in Phase 1 because `bin/vsp` is strictly a thin router (`<= 400` LOC) and the 83 tracked generated files stay committed as the parity oracle until Phase 2 gates pass.

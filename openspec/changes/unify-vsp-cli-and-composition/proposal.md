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

- Introduce a thin `bin/vsp` facade for the transition. The v3 release uses the native binary and provides compatibility for supported `vspcli` commands.
- Standardize command taxonomy:
  - Everyday verbs: `sync`, `diff`, `open` (hidden alias `use`), `doctor`.
  - Inspection & setup verbs: `list`, `show`, `detect` (strictly read-only), `export`, `init`, `completions`.
  - Grouped maintainer verbs: `dev {validate, dist, pins, compat, docs, hooks, test, release}` (plus `build` in Phase 1).
  - Grouped self-management verbs: `self {path, install, uninstall, update}`.
- Enforce explicit target selectors for `sync` and `diff` (missing selector exits `4` and never mutates `cwd`).
- Run Step 0 compromised-extension removal before named-profile settings writes or extension installation.
- Standardize native exit codes `0` through `7`, including exit `7` for incomplete observation (`D2`). Legacy `doctor`, `reconcile`, and `install-ext` map `5`, `6`, and `7` to `1`.

### 2. Locked Architecture Decisions (`D1`–`D5`) & ADR 0001

- Add `docs/adr/0001-vsp-cli-and-composition.md` recording:
  - **D1:** JSONC-only user config (`~/.config/vsp/config.jsonc`); zero repo `.vsp.toml`.
  - **D2:** Exit `7` for incomplete observation or unavailable verification.
  - **D3:** Fail-closed Bash lock hardening in `v2.0.1` followed by OS-level `fs4` + SHA-256 CAS locking in Rust.
  - **D4:** Homebrew formula launches with the compiled Rust `vsp` binary.
  - **D5:** Preserve `profiles/ai/profile.json` `chat.editor.fontSize=14` override for 100% golden settings parity across all 15 profiles.
  - Four-tier VSIX locking, local target provenance, and the Phase 2 untracking of 83 generated files.

### 3. Rust Engine Bootstrap (`crates/vsp`) & Deterministic Release Pipeline

- Phase 1 (`v2.1.0`): Bootstrap `crates/vsp` read-only engine (legacy-layout loader, DAG resolver, nested-safe autonomy deny-list, unified `vsp-plan/v1` planner, read-only CLI verbs, and deterministic `vsp dev dist`) verified at 100% golden parity against all 15 tracked profiles and 22 legacy export aliases.
- Add `.github/workflows/release.yml` with SHA-pinned GitHub Actions to build cross-platform `vsp` binaries (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`), run `vsp dev dist --out dist/` twice to verify byte-identical `SHA256SUMS`, package `vsp-profiles-<tag>.tar.gz`, attest build provenance, and publish GitHub Release assets.
- Phase 2 (`v2.2.0`): Implement native `sync` and `open` with advisory locking, compare-and-swap guards, and JSONC comment preservation. Consolidate the catalog and lockfile, then untrack the 83 generated files.
- Phase 3 (`v3.0.0`): Add personal config, `init`, native self-management, and compatibility mode. Replace obsolete shell entry points, prepare the Homebrew formula (`D4`), and verify the tag release at the final gate.

## Capabilities

### New Capabilities

- [`cli-interface`](specs/cli-interface/spec.md) — Explicit selectors, read-only detection, shared previews, native exit codes, and legacy parser selection and exit mapping.
- [`profile-composition`](specs/profile-composition/spec.md) — Built-in profile recipes, pack dependency and conflict handling, preservation of unrelated JSONC settings, and on-demand bundles.
- [`supply-chain-locking`](specs/supply-chain-locking/spec.md) — Four lock tiers, compromised-first remediation, VSIX digest verification, and audit reporting for exact-ID `user-unlocked` extensions.
- [`user-config`](specs/user-config/spec.md) — User-only JSONC configuration, versioned schema, personal defaults and overrides, validated extension IDs, and `vsp init`.

## Impact

- **Code:** Adds `docs/adr/0001-vsp-cli-and-composition.md`, `bin/vsp`, `.github/workflows/release.yml`, `scripts/tests/bats/vsp-facade.bats`, and (in parallel Phase 1.2) `Cargo.toml` + `crates/vsp/**`. Existing `scripts/vspcli` and `scripts/*.sh` remain untouched in Phase 1.
- **APIs:** Introduces the `vsp` CLI and exit code `7`; existing `scripts/vspcli` flags and scripts keep full compatibility through Phase 2.
- **Dependencies:** Phase 1 `bin/vsp` requires only Bash 3.2+ and existing script dependencies. Compiled `crates/vsp` requires Rust (`rustc`/`cargo`) at build time and produces zero-runtime-dependency native binaries.
- **Backwards compatibility:** Native compatibility mode supports the v2 commands that have a v3 equivalent. The 15 canonical profiles and 22 legacy `.code-profile` aliases remain available. Both `ai-profile-{crisp,retina}` aliases resolve to `ai-copilot`.
- **Risk:** Low in Phase 1 because `bin/vsp` is strictly a thin router (`<= 400` LOC) and the 83 tracked generated files stay committed as the parity oracle until Phase 2 gates pass.

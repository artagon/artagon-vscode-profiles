# Design: Unified `vsp` CLI, Composition Architecture & 4-Phase Rust Migration

## Context

`artagon-vscode-profiles` compiles vertical capability, toolchain, and UX modules (`modules/`) against a lowercase-keyed registry (`catalog/`) into 15 canonical profiles (`profiles/`) and workspace `.vscode/` stacks. Phase 0 (`v2.0.1`) closed security and audit hotfixes in the Bash 3.2 codebase (`scripts/`).

This design defines the `vsp` CLI contract, the Phase 1 thin Bash facade (`bin/vsp`), the Rust engine (`crates/vsp`) boundary, the 4-tier `vsp.lock.json` provenance model, and the deterministic release workflow (`.github/workflows/release.yml`). See [`docs/adr/0001-vsp-cli-and-composition.md`](../../../docs/adr/0001-vsp-cli-and-composition.md) for the authoritative architectural record.

## Goals / Non-Goals

**Goals:**

1. Expose one cohesive CLI (`vsp`) with explicit target selectors, read-only `detect` and `diff`, Step 0 compromised-first `sync -p`, grouped `dev` / `self` commands, and exit codes `0`–`7`.
2. Achieve zero double work: keep `bin/vsp` strictly `<= 400` LOC of pure Bash 3.2 routing code with zero new JSONC parser, DAG planner, or lock logic; build all new engine capabilities once in `crates/vsp`.
3. Guarantee 100% golden parity between `crates/vsp` and the 15 tracked `_merged/*.json`, 15 `profiles/*/extensions.json`, and 22 legacy export aliases before untracking the 83 generated files in Phase 2.
4. Provide deterministic, SLSA-attested cross-platform release builds (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`) and profile bundle archives (`vsp-profiles-<tag>.tar.gz` + `SHA256SUMS`).

**Non-Goals:**

- Introducing a repository-level `.vsp.toml` configuration file (`D1`).
- Shipping a Homebrew formula backed by Bash scripts (`D4`).
- Modifying existing `scripts/vspcli` or untracking the 83 generated files during Phase 1.
- Weakening any Phase 0 (`v2.0.1`) security invariants (nested/case-insensitive/string-bool autonomy deny-list, fail-closed VSIX pins, `ai-profile-*` -> `ai-copilot` mapping).

## Decisions

### Decision 1: CLI Surface, Explicit Selectors & Step 0 Sync Composition

```text
vsp [global-flags] <command> [args...]
  Everyday:    sync | diff | open (alias: use) | doctor
  Inspection:  list | show | detect | export | init | completions
  Maintainer:  dev {validate, build, dist, pins, compat, docs, hooks, test, release}
  Self:        self {path, install, uninstall, update}
```

- **Explicit selector guard:** `vsp sync` and `vsp diff` reject missing target selectors with exit `4` before reading or mutating the current working directory. Valid selectors are positional `PATH` or `--workspace PATH`, `-p/--profile NAME`, `--user`, `--from BUNDLE` (with `-p NAME`), or `--all [--compromised-only]`.
- **Profile sync ordering (`vsp sync -p <name>`):**
  1. **Step 0 (unconditional first):** Run `reconcile --profile <name> --compromised-only` so compromised extensions are removed before any settings write or extension install.
  2. **Step 1 (settings/profile apply):** Run `apply --target=profile --profile <name>` with requested UX, AI, and module flags.
  3. **Step 2 (extension reconcile):** Run `reconcile --profile <name>` (with `--as <recipe>` when `--preset` or `--as` is given).
- **Bundle import (`vsp sync --from <bundle> -p <name>`):** Routes to `scripts/import-profile.sh <name> <bundle>` (forwarding `--no-ai`, `--force`, `--no-batch`, `--without-group`, `--allow-unsafe-settings`, `--break-stale-lock`), followed by optional `--ux` application via `apply --target=profile`.
- **Read-only guarantees:** `vsp detect` routes only to `scripts/detect-toolchain.sh` (never legacy mutating `vspcli --detect`). `vsp diff` always executes read-only preview (`--dry-run`) and exits `3` under `--check` when drift is confirmed.

### Decision 2: Canonical Exit Codes (`0`–`7`)

- `0`: Ok, clean check, dry-run preview, `--help`, `--version`.
- `1`: Operational, audit, or validation failure.
- `2`: Target missing (path, profile, preset, module, or bundle).
- `3`: Complete negative check (`detect` no match, `diff --check` verified drift).
- `4`: Usage, flag parsing, config, selector, or composition conflict error.
- `5`: Safety guard refusal before write (deny-list, running editor without `--force`, symlink escape, refusing to overwrite a non-symlink in `self install`/`uninstall`).
- `6`: Retryable lock contention, CAS preimage mismatch, or partial apply.
- `7`: Incomplete observation or verification unavailable (`D2` — e.g., failed `code --list-extensions` in `doctor`).

### Decision 3: Thin `bin/vsp` Facade & Binary Handoff

`bin/vsp` resolves its own physical path across relative symlink chains (with a 40-hop loop guard and `cd -P --`) to compute `VSP_SELF_PATH` and `REPO_ROOT`.

Delegation order:

1. If `VSP_BIN` is set, executable, not a directory, and not `VSP_SELF_PATH`, `exec "${VSP_BIN}" "${@+"${@}"}"`.
2. Else if `VSP_FORCE_BASH != 1`, `exec` `${REPO_ROOT}/target/release/vsp` if executable, else `${REPO_ROOT}/target/debug/vsp` if executable.
3. Otherwise execute the Bash 3.2 facade router delegating to `scripts/vspcli` and sibling scripts under `${REPO_ROOT}/scripts/`.

Constraints enforced by automated BATS tests:

- Total line count `wc -l bin/vsp <= 400`.
- Strict Bash 3.2+ syntax (`/bin/bash -n bin/vsp`) and zero ShellCheck warnings (`shellcheck --exclude=SC1091 bin/vsp`).
- Zero JSONC parsing, DAG resolution, or lock acquisition code inside `bin/vsp`.

### Decision 4: Locked Architecture Decisions (`D1`–`D5`)

| ID | Locked Decision | Enforcement Point |
| --- | --- | --- |
| `D1` | JSONC-only user config at `${XDG_CONFIG_HOME:-~/.config}/vsp/config.jsonc`; zero repo `.vsp.toml` | Rust config loader (`crates/vsp`), CLI help & docs |
| `D2` | Exit `7` for incomplete observation / unavailable verification | `crates/vsp` `doctor` & compat checks; documented exit table |
| `D3` | Fail-closed Bash locks in `v2.0.1`; OS `fs4` advisory lock + SHA-256 CAS in Rust | Phase 0 (`scripts/lib/common.sh`), Phase 2 (`crates/vsp`) |
| `D4` | Homebrew formula launches with compiled Rust binary | Phase 3 release tap backed by `.github/workflows/release.yml` |
| `D5` | Preserve `profiles/ai/profile.json` `chat.editor.fontSize=14` override | `crates/vsp` golden parity test suite (15/15 profiles) |

### Decision 5: Four-Tier `vsp.lock.json` & Deterministic Distribution

1. **Tier 1 (Inputs):** Canonical digests of `catalog/*.json` and active module definitions.
2. **Tier 2 (Supply-chain pins):** Install-time VSIX SHA-256 pins (`catalog/extension-pins.json`) and publisher allowlist (`catalog/publishers.json`).
3. **Tier 3 (Dist bundles):** `LC_ALL=C`-sorted `SHA256SUMS` over 15 canonical `.code-profile` bundles and 22 materialized legacy alias files (`ai-profile-{crisp,retina}.code-profile` byte-identical to `ai-copilot.code-profile`).
4. **Tier 4 (Target provenance):** Managed settings keys, managed recommendations, and workspace/profile fingerprints under `XDG_STATE_HOME` / `vsp.lock.json`.

`.github/workflows/release.yml` triggers on `v*` tags and `workflow_dispatch`, pins every GitHub Action to a verified 40-character commit SHA, builds `vsp` for `aarch64-apple-darwin`, `x86_64-apple-darwin`, and `x86_64-unknown-linux-gnu`, executes `vsp dev dist --out dist/` twice to verify byte-identical `SHA256SUMS`, packages `vsp-profiles-${TAG}.tar.gz`, generates build provenance attestations (`actions/attest-build-provenance`), and publishes assets on tag pushes.

## Risks / Trade-offs

- **[Dual runtime drift during Phase 1]** Bash facade and Rust read-only CLI coexist in `v2.1.0`. Mitigation: `bin/vsp` automatically hands off to compiled `vsp` when present; `VSP_FORCE_BASH=1` keeps facade BATS tests deterministic; Rust golden tests lock 100% parity against all 15 tracked profiles and 22 aliases.
- **[Cross-platform macOS Intel build on ARM runners]** `x86_64-apple-darwin` cross-compiles on `macos-latest` via `rustup target add x86_64-apple-darwin` without relying on retired Intel runners. Mitigation: release workflow validates binary linkage and packages artifacts per target.
- **[Untracking 83 generated files in Phase 2]** Bookmarks pointing at raw git branch `exports/*.code-profile` paths will move to GitHub Releases. Mitigation: keep all 83 files tracked through Phase 1, document release URLs before Phase 2 cutover, and materialize all 22 legacy filenames inside `vsp-profiles-<tag>.tar.gz`.

## Migration Plan

1. **Phase 0 (`v2.0.1` — Done):** Security and audit hotfixes merged and verified (149/149 BATS + smoke).
2. **Phase 1 (`v2.1.0`):** Land ADR 0001, OpenSpec change, `bin/vsp`, `.github/workflows/release.yml`, `vsp-facade.bats`, and `crates/vsp` read-only core with 100% golden parity. Keep 83 generated files tracked.
3. **Phase 2 (`v2.2.0`):** Implement Rust mutating `sync`/`open`/`init`/`self`, `fs4` + CAS locking, and 4-tier `vsp.lock.json`. Untrack the 83 generated files once parity and mutation tests pass.
4. **Phase 3 (`v3.0.0`):** Retire legacy Bash compiler scripts, launch Homebrew formula (`D4`), and archive superseded OpenSpec changes.

**Rollback:** In Phase 1, removing `bin/vsp` and `crates/vsp` leaves `scripts/vspcli` and all 83 tracked generated files completely intact with zero user impact.

## Open Questions

All major architectural questions are locked via `D1`–`D5`. Detailed JSONC comment-preservation edge cases for Rust mutating `sync` are scoped to Phase 2 design verification.

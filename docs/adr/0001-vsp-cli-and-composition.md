# ADR 0001: Unified `vsp` CLI, Composition Model & Zero-Double-Work Bash-to-Rust Plan

- **Status:** Implemented
- **Date:** 2026-11-24
- **Release Target:** Phase 0 (`v2.0.1`) through Phase 3 (`v3.0.0`)
- **OpenSpec Change:** [`openspec/changes/unify-vsp-cli-and-composition/`](../../openspec/changes/unify-vsp-cli-and-composition/proposal.md)

## Context

At the start of this migration, the project managed 15 profile recipes, vertical modules, and a unified catalog from Bash. Four structural limits motivated the Rust cutover.

1. **Fragmented CLI surface.** Users had to coordinate `vspcli apply`, `vspcli reconcile`, `vspcli import`, `vspcli open`, and sibling scripts (`scripts/export-profiles.sh`, `scripts/refresh-pins.sh`, `scripts/generate-readme-matrix.sh`). Mutating verbs lack a single explicit-selector contract and a shared machine-readable plan format.
2. **Shell runtime ceiling.** Pure Bash 3.2 + `jq` scripts required subshell forks for every JSONC merge, DAG resolution, collision check, and BATS test. Full regression verification takes over 40 minutes, and shell-level `mkdir` locks cannot provide OS-level advisory lock guarantees across crashes.
3. **83 tracked generated files.** Git tracked 83 derived artifacts: 15 `_merged/*.json`, 30 `profiles/*/{settings.json,extensions.json}`, and 38 `exports/*` files (15 canonical `.code-profile` bundles, 22 legacy alias symlinks, and `SHA256SUMS`). Every module edit produces noisy diffs and merge conflicts.
4. **No compiled distribution channel.** Users had to clone the repository and install `jq`, `awk`, and `curl` to manage profiles.

A big-bang rewrite would risk behavior drift and double maintenance. This ADR defines a unified `vsp` CLI, a strict exit code contract, five locked architecture decisions (`D1`–`D5`), a 4-tier provenance model, and a 4-phase zero-double-work migration from Bash to Rust.

## Decisions

### 1. CLI Name & Command Taxonomy

The primary command is `vsp`. Phase 3 provides native compatibility for supported v2.x `vspcli` commands.

| Group | Verbs | Role |
| --- | --- | --- |
| Everyday | `sync`, `diff`, `open` (hidden alias `use`), `doctor` | Apply state, preview changes, launch profiles, audit health |
| Inspection & Setup | `list`, `show`, `detect`, `export`, `init`, `completions` | Read-only discovery, bundle export, user config bootstrap, shell completion |
| Maintainer (`dev`) | `dev {validate, dist, pins, compat, docs, hooks, test, release}` (`build` in Phase 1 facade) | Catalog validation, deterministic bundle builds, pin refresh, CI checks |
| Self (`self`) | `self {path, install, uninstall, update}` | Binary path resolution, symlink management in `${XDG_BIN_HOME:-~/.local/bin}/vsp` |

Core command invariants:

- **Explicit selector required.** `vsp sync` and `vsp diff` accept `PATH` or `--workspace PATH`, `-p/--profile NAME`, `--preset NAME`, or `--user`. `sync` also accepts bundle import and all-profile selectors. Missing selectors exit `4` before mutation.
- **Read-only detection.** `vsp detect` is strictly read-only and never creates `.vscode/`.
- **Unified plan.** `vsp diff` and `vsp sync --dry-run` compute the same `vsp-plan/v1` plan for standard workspace, profile, preset, and user targets.
- **Unconditional Step 0.** `vsp sync -p <name>` runs compromised-extension removal before writing settings or installing extensions.

### 2. Canonical Exit Code Contract (`0`–`7`)

All `vsp` commands share one exit code table across both the Phase 1 Bash facade and the compiled Rust binary:

| Code | Meaning | Examples |
| ---: | --- | --- |
| `0` | Ok / clean / preview / info | Successful `sync`, clean `diff --check`, `--help`, `--version` |
| `1` | Operational, audit, or validation failure | Compromised extension found in `doctor`, `dev validate` failure, failed install |
| `2` | Target missing | Missing workspace path, unknown profile/preset/module in `show`, missing bundle |
| `3` | Complete negative check | `detect` found no matching toolchain; `diff --check` confirmed drift |
| `4` | Usage, config, or composition conflict | Unknown flag, missing selector, clap parse error, conflicting modules |
| `5` | Safety guard refusal before write | Autonomy deny-list violation, running VS Code without `--force`, symlink escape, non-symlink clobber |
| `6` | Retryable lock, preimage, or partial apply | Lock contention, `storage.json` CAS preimage mismatch, partial apply |
| `7` | Incomplete observation / verification unavailable | Failed `code --list-extensions` during `doctor`, unreachable compatibility check |

The native binary selects legacy parsing if the basename of `argv[0]` is `vspcli` (or `vspcli.exe`, ignoring case), if `VSP_COMPAT_VSPCLI=1`, or if the first argument is `compat-vspcli` as in `vsp compat-vspcli …`. Only legacy `doctor`, `reconcile`, and `install-ext` map `5`, `6`, and `7` to `1`. All other exit codes pass through.

### 3. Locked Architecture Decisions (`D1`–`D5`)

- **D1 — User-only JSONC configuration.** `vsp init` writes `${XDG_CONFIG_HOME:-~/.config}/vsp/config.jsonc`. An explicit `--config PATH` selects a single alternate source. `vsp` ignores repo-local `.vsp.toml` and `.vsp.jsonc`. Workspace intent stays in standard project markers, `.vscode/` files, and explicit CLI selectors.
- **D2 — Exit `7` for incomplete observation.** Audit and verification commands must fail closed with exit `7` whenever required observation fails (for example, `code --list-extensions` fails or `storage.json` cannot be enumerated). Incomplete checks never downgrade to warnings or exit `0`.
- **D3 — Fail-closed lock hardening in `v2.0.1` + `fs4`/CAS in Rust.** Phase 0 (`v2.0.1`) hardened shell lock reclamation and spaced macOS VS Code process detection. Phase 2 replaces shell locks with OS-level advisory locks (`fs4`) paired with pre-publish SHA-256 compare-and-swap (CAS) checks on VS Code `storage.json` and atomic renames.
- **D4 — Homebrew formula launches with the compiled Rust binary.** A Homebrew formula ships only when the cross-platform Rust `vsp` binary and SLSA-attested release tarballs are ready (Phase 3), avoiding a temporary Bash formula that users would need to migrate.
- **D5 — Preserve `ai` `chat.editor.fontSize=14` override during schema consolidation.** `profiles/ai/profile.json` sets `overrides.profile["chat.editor.fontSize"] = 14`. Schema consolidation keeps this override intact so Rust achieves 100% golden settings parity across all 15 canonical profiles.

The user config requires integer `version: 1`. The optional `defaults` object accepts `ux` (`crisp`, `retina`, `default`), `ai` (`full`, `copilot`, `off`), and boolean `rtkTerminal`. The optional `profiles` map accepts `description`, `extends` (catalog preset or pack), `with` and `without` (pack ID arrays), `overrides` (profile settings), and `extraExtensions` (exact extension ID array) per profile name. The parser permits top-level `$schema` metadata. Defaults apply to personal targets. Explicit CLI selections take precedence, and profile overrides apply after UX settings.

Override keys must be dotted VS Code setting IDs or `[language]` blocks. Bare setting keys exit `4`. The autonomy deny-list rejects overrides with exit `5`, including within language blocks. This config check cannot be bypassed with `--allow-unsafe-settings`. Publisher trust fields (`trust`, `publishers`, `trustedPublishers`) and wildcard extension IDs exit `4`. Compromised or removed extension IDs exit `5`.

### 4. Four-Tier VSIX Policy and Target Provenance

Tier 0 is declared in `catalog/extensions.jsonc`. `vsp.lock.json` records tiers 1–3 and supports platform-specific VSIX metadata.

1. **Tier 0** covers compromised and removed extensions that Step 0 uninstalls.
2. **Tier 1** covers extensions with per-platform SHA-256 VSIX locks.
3. **Tier 2** covers executable extensions with version locks.
4. **Tier 3** covers unlocked declarative themes, grammars, and keymaps.

An exact catalog-external ID in `extraExtensions` is also Tier 3, labeled `user-unlocked`. It has no VSIX pin. `vsp doctor` records a `WARN` and includes the ID in the JSON `user_unlocked_extensions` array. With complete observation, `vsp doctor --strict` fails the audit with exit `1` while retaining the `WARN` finding.

`pending_refresh` records indicate incomplete VSIX verification. Target provenance is stored separately under `${XDG_STATE_HOME:-~/.local/state}/vsp/` and tracks managed settings, recommendations, and workspace fingerprints. Distribution checksums live in `SHA256SUMS`.

### 5. Untracking 83 Generated Files in Phase 2

Phase 1 retained the 83 tracked generated files (`15` `_merged/*.json`, `30` `profiles/*/{settings,extensions}.json`, `38` `exports/*`) as the golden parity oracle for `crates/vsp`. Phase 2 removed all 83 files from git tracking and saved frozen checksums as the regression oracle. `vsp export` and `vsp dev dist` generate bundles locally. The tag workflow builds `vsp-profiles-<tag>.tar.gz` and publishes checksums and signed provenance through GitHub Releases.

### 6. Four-Phase Zero-Double-Work Bash-to-Rust Plan

No new parser, planner, or lock code is written in Bash after Phase 0.

1. **Phase 0 (`v2.0.1` — Complete):** Close critical security and audit gaps in Bash (`catalog.sh` nested/case-insensitive/string-boolean autonomy deny-list, fail-closed VSIX pins, `common.sh` lock reclamation & macOS process detection, `doctor` audit completeness, `detect` error propagation).
2. **Phase 1 (`v2.1.0` — Complete):**
   - Land ADR 0001, OpenSpec change artifacts, `.github/workflows/release.yml`, and thin `bin/vsp` facade (`<= 400` LOC, pure Bash 3.2+, zero new parser/planner/lock code; automatically `exec`s `VSP_BIN` or compiled `target/{release,debug}/vsp` when present).
   - Bootstrap `crates/vsp` with the typed domain model, legacy-layout loader, DAG resolver, policy validator, unified `Plan`, read-only CLI verbs (`list`, `show`, `detect`, `diff`, `doctor`, `export`, `completions`, `dev validate`, `dev dist`), and 100% golden parity tests against all 15 tracked profiles and 22 legacy aliases.
3. **Phase 2 (`v2.2.0`):** Implement Rust mutating `sync` and `open`, `fs4` and CAS locking, comment-preserving JSONC edits, target provenance, and the four-tier VSIX policy. Untrack the 83 generated files from git and switch CI/release generation to `vsp dev dist`.
4. **Phase 3 (`v3.0.0`):** Retire the legacy Bash compiler from the v3 workflow and provide native `vspcli` compatibility. Deliver `vsp init`, local composition defaults, and native binary installation. Publish signed cross-platform Rust binaries (`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`) and launch the Homebrew formula (`D4`).

The v3 workflow retires `scripts/.vspcli-completion.{zsh,fish}`, `scripts/setup-rust-toolchain.sh`, `scripts/setup-cpp-toolchain.sh`, and `profiles/rust/tasks.example.json` as entry points. Generate completions with `vsp completions`, install language toolchains separately, and use workspace sync for generated settings and Rust documentation tasks.

## Implementation Evidence

- `7b2f532` introduced the Rust planner and legacy loader. The golden tests now compare all canonical settings and extensions and 22 aliases against frozen v2.1 checksums.
- `52aeaf9` introduced the consolidated schema and build-time snapshot. `cd7aa47` added Rust synchronization, JSONC CST edits, VSIX verification, and storage CAS. `6972eeb` switched to the embedded catalog and untracked the 83 generated files.
- `718183e` added user configuration, `init`, native `self` commands, live completions, and `vspcli` compatibility. `crates/vsp/tests/v3_user_config_and_compat.rs` covers the configuration gates, strict doctor exit, compatibility dispatch, and install lifecycle.
- The architecture is implemented in the Rust engine. The [release checklist](../../openspec/changes/unify-vsp-cli-and-composition/tasks.md#phase-3-legacy-retirement-homebrew-launch--final-cutover-v300) tracks the remaining cutover and publication gates.

## Consequences

### Positive

- Users gain one consistent CLI (`vsp`) immediately in `v2.1.0` without waiting for the full Rust rewrite.
- Zero double work: Bash receives only the `<= 400` LOC facade router, while every new engine feature lands once in `crates/vsp`.
- 100% golden parity tests against tracked `_merged/`, `profiles/*/extensions.json`, and `exports/` eliminate silent regressions before untracking generated files.
- Cross-platform release builds verify deterministic `SHA256SUMS` twice and attach SLSA build provenance attestations.

### Negative and Trade-offs

- Two runtimes (Bash facade + Rust core) coexist during `v2.1.0`; `bin/vsp` mitigates drift by delegating directly to the compiled Rust binary when available (`VSP_FORCE_BASH=1` isolates facade tests).
- Untracking `exports/*.code-profile` in Phase 2 breaks raw-branch download URLs that do not use GitHub Releases; release tarballs preserve all 15 canonical and 22 legacy bundle filenames with verified `SHA256SUMS`.

## Verification Gates

| Phase | Required Gates |
| --- | --- |
| Phase 0 (`v2.0.1`) | `shellcheck`, `zsh -n`, `fish -n`, `validate-json.sh`, `check-ux-symmetry.sh`, `apply-rust-policy.sh --check`, `generate-readme-matrix.sh --check`, full `scripts/tests/run.sh` (verified: 149/149 BATS + smoke) |
| Phase 1 (`v2.1.0`) | `wc -l bin/vsp <= 400`, `shellcheck --exclude=SC1091 bin/vsp`, `/bin/bash -n bin/vsp`, `bats scripts/tests/bats/vsp-facade.bats`, YAML validation of `.github/workflows/release.yml`, `cargo test --workspace` (15-profile + 22-alias golden parity + N1 + exit codes `0`–`7`), `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps` |
| Phase 2 (`v2.2.0`) | Rust mutating `sync`/`open`/`fs4`+CAS integration tests, double-run `vsp dev dist` byte-identical `SHA256SUMS`, git tree verified clean of the 83 generated files |
| Phase 3 (`v3.0.0`) | Tag release workflow green on all 3 targets, `gh attestation verify` passing on tarballs and `SHA256SUMS`, Homebrew formula audit green |

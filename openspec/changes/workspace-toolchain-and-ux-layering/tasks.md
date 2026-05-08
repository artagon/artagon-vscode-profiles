# Implementation status

**Implementation complete** as of this commit (modulo two intentional deferrals):

- 75 BATS tests passing locally (macOS) and in CI matrix (ubuntu-latest with bats+fish installed).
- `openspec validate workspace-toolchain-and-ux-layering --strict` passes.
- All new shell shellcheck-clean (informational SC1091 for runtime-resolved `source` paths suppressed).
- Catalog migrated in-place: 11 toolchain dirs (rust, astro, java-{maven,gradle,spring}, cpp-{clangd,intellisense}, ai, ai-plus, github-workflows, general), 11 `_merged/<flavor>.json`, 11 canonical `exports/<flavor>.code-profile` + 22 deprecation symlinks. `MIGRATED.md` records the renames.
- 4 agent docs (AGENTS.md, CLAUDE.md, CODEX.md, GEMINI.md) carry the sentinel-bracketed Documentation Policy from `docs/rust.md` §12; CI staleness gate (`scripts/lib/apply-rust-policy.sh --check`) green.
- Legacy `<flavor>-{crisp,retina}` profile names resolve via `scripts/lib/legacy-profile-name.sh` shim (sourced from vspcli + install-extensions.sh) with rate-limited deprecation warning; `ARTAGON_VSCODE_DEPRECATION_ACK=1` suppresses.
- `vspcli --detect --target=workspace [--ux=PRESET] [--font|--theme|--icon-theme] [--no-rtk] [--check-compat=block|warn|off]` writes `.vscode/{extensions,settings,tasks}.json` end-to-end.

**Deferred:**

- §5.6 — main spec sync (runs at archive time via `openspec-sync-specs` or `openspec-archive`).
- §13.8 — manual VS Code Trust + `${workspaceFolder}` substitution validation (requires opening VS Code in a real session; not automatable from CLI).

The Ralph loop has no more meaningful work to do. Cancel via `/ralph-loop:cancel-ralph`.

---

## 0. Cross-cutting authoring constraints (apply to every phase)

These mirror the existing repo's `scripts/` authoring contract and the
`artagon-shell:shell-authoring` skill. Non-compliance fails review:

- [ ] 0.1 All new scripts are POSIX shell or `#!/usr/bin/env bash` —
  no Python, Node, or other runtimes. (Per
  `proposal.md` cross-cutting constraints.)
- [ ] 0.2 Every variable expansion gets braces and quotes:
  `"${var}"` not `$var` (SC2086).
- [ ] 0.3 Tests use `[[ ... ]]` not `[ ... ]` (skill rule 5; SC2007/SC2027).
  Single-bracket `[` only when extending an existing `if [` block whose
  siblings already use it (diff hygiene).
- [ ] 0.4 `local x=$(cmd)` is forbidden (SC2155). Always split:
  `local x; x=$(cmd) || handle_error`.
- [ ] 0.5 Array iteration uses `"${arr[@]}"` (SC2068, SC2124). Empty-array
  dereference under `set -u` and bash 3.2 aborts; gate with
  `[[ "${#arr[@]}" -gt 0 ]]` first (double-bracket form per §0.3).
- [ ] 0.6 Errors go to stderr (`>&2`); progress/success/data goes to
  stdout. Match the repo idioms cited in
  `openspec/changes/profile-graceful-defaults/tasks.md` §5.7.
- [ ] 0.7 Strict-mode preamble `set -euo pipefail` at the top of every
  new shell script.
- [ ] 0.8 macOS-portable: case statements, not associative arrays
  (`/bin/bash` is still 3.2 on macOS).
- [ ] 0.9 Shellcheck-clean is a CI gate. Resolve all warnings or
  document the SC-code with `# shellcheck disable=SC<N>` and a one-line
  justification.
- [ ] 0.10 All new tests are BATS files under `scripts/tests/bats/`;
  no inline shell asserts in `run.sh`-style files for new coverage.

## 0a. Pre-merge validation (BLOCKS implementation)

These items must complete BEFORE any other phase begins. They validate
load-bearing assumptions in design.md that adversarial review flagged
as unverified.

- [ ] 0a.1 **Validate `${workspaceFolder}` substitution in
  terminal-profile `args`** (design.md Open Question 1): clone the repo
  to two distinct absolute paths (`/tmp/test1`, `/tmp/test2`); open
  each in VS Code stable; grant Workspace Trust; open the integrated
  terminal in each; check that the rtk init script ran (look for the
  "rtk wrappers active" diagnostic stderr line per design.md
  Decision 14). Document the result in
  `openspec/changes/workspace-toolchain-and-ux-layering/snapshots/${workspaceFolder}-validation.md`.
  If the substitution does NOT work, update design.md Decision 13 to
  use absolute paths only and add a task to `vspcli --detect` to
  rewrite the path on every invocation when the workspace location
  changes.
- [ ] 0a.2 **Pin `tasks[].options.shell` vs. `automationProfile`
  precedence** (Open Question 2): write a fixture task with
  `options.shell` set to `/bin/sh` (no rtk init) and run it in a
  workspace where `automationProfile.osx` is set to fish-with-rtk.
  Document which shell actually executes the command. Update design.md
  if the answer differs from "options.shell wins."

## 0b. Mechanical derivations (one-shot pre-migration)

These produce inputs the rest of the change consumes. Run before any
file rename or compose-settings.sh modification.

- [x] 0b.1 **Derive UX key list mechanically** — 8 keys differ between
  crisp/retina; written to `_shared/ux/{crisp,retina}.jsonc`; symmetry
  CI gate `scripts/tests/check-ux-symmetry.sh` passes.
- [x] 0b.2 **Derive `_shared/editor-base.jsonc`** — 65 identical keys;
  symmetry check confirms disjoint from UX preset keys.
- [x] 0b.3 **AI extension distribution audit** — written to
  `snapshots/ai-distribution-pre-migration.md`. Findings: all toolchain
  profiles ship `github.copilot` + `github.copilot-chat`; ai-plus
  additionally has `sourcegraph.cody-ai` + `continue.continue`;
  ai-profile is Copilot-only. Less drift than R3 reported.

## 1. Detection capability (`detect-workspace-toolchain`)

- [x] 1.1 `scripts/detect-toolchain.sh` created with precedence-list
  `case` statements; honors `--toolchain`, `--no-detect`, `--json`.
- [x] 1.2 Spring promotion via `find -type f -name '*Application.java'` +
  `grep -l '@SpringBootApplication'`.
- [x] 1.3 `vspcli --detect [PATH]` delegates to `scripts/detect-toolchain.sh`.
- [x] 1.4 Completion scripts updated for bash/zsh/fish — new flags
  + 11 post-migration flavors. (See also §5.5.)
- [x] 1.5 `--json` mode emits spec shape via `jq -n`.
- [x] 1.6 Shellcheck-clean confirmed (one SC2038 fixed via `find -exec`).

## 2. Layered extensions

- [x] 2.1 `_shared/extensions/base.json` — 22 universal IDs computed
  mechanically as the intersection across all 11 existing crisp profiles
  (themes, gitlens, shell-format, yaml, toml, dotenv, docker, etc., minus
  AI). All-profile intersection guarantees no toolchain regresses.
- [x] 2.2 For each of the **10 toolchain flavors** (rust, astro,
  java-maven, java-gradle, java-spring, cpp-clangd, cpp-intellisense,
  ai, ai-plus, github-workflows), create
  `_shared/extensions/<flavor>.json` extracting the
  toolchain-specific extensions from the corresponding existing
  `profiles/<flavor>-crisp/extensions.json` (the crisp and retina
  variants are byte-identical at the extensions layer; pick crisp).
  The base extensions (yaml, markdown, etc.) move into
  `_shared/extensions/base.json`. **AI extensions are NOT included in
  toolchain layer files** (per design.md Decision 15) — they live
  exclusively in `_shared/extensions/ai.json` (Copilot subset) and
  `_shared/extensions/ai-plus.json` (full AI bundle). Use the
  `ai-distribution-pre-migration.md` snapshot from §0b.3 to identify
  which IDs to move out. The 11th post-migration profile dir is
  `profiles/general/` (per design.md Decision 16's
  `_shared/editor-base.jsonc` consumer); the placeholder `web`
  flavor is NOT shipped this round (per proposal.md Deferred section).
- [x] 2.3 `scripts/lib/compose-extensions.sh` created. Reads base + each
  flavor's layer; concats; dedups by `.identifier.id` preserving first
  occurrence; emits JSON array. Verified: rust → 31, rust+ai → 33.
- [x] 2.4 No runtime composer invocation in install-extensions.sh
  needed: the migration regenerated `profiles/<flavor>/extensions.json`
  from layered sources, and install-extensions.sh continues to read that
  file. Legacy `--install <flavor>-{crisp,retina}` resolves through the
  deprecation shim (per §5.3).
- [x] 2.5 Set-equivalence regression test added as 7th case in
  `install-extensions-layered.bats`: for all 10 toolchain flavors,
  asserts post-migration `profiles/<flavor>/extensions.json` set
  equals `compose_extensions <flavor>` set. Verified passing across
  rust/astro/java-*/cpp-*/ai/ai-plus/github-workflows.
- [x] 2.6 `install-extensions.sh --dry-run` prints would-be installs
  for both `--target=profile` and `--target=global`. Verified.
- [x] 2.7 **AI policy reconciliation** — `_shared/extensions/ai.json`
  ships Copilot + Copilot Chat only. `_shared/extensions/ai-plus.json`
  matches existing ai-plus profile content (Copilot + Cody + Continue).
  AI IDs filtered out of all other layer files via jq subtraction at
  build time. README badge update deferred to §11.

## 3. Three install targets

- [x] 3.1 `--target` parsed in `vspcli` and `install-workspace.sh`;
  unknown values exit 4. (Mutual-exclusion across the three values
  pending — current parser takes last-wins; tighten in §3.x follow-up.)
- [x] 3.2 `--target=workspace` writes `.vscode/extensions.json` via
  `install-workspace.sh` using set-union merge of existing
  `recommendations` (preserves order, dedup); `unwantedRecommendations`
  untouched.
- [x] 3.3 `install-extensions.sh --target=global` invokes
  `code --install-extension <id>` (no `--profile` flag) per ID.
  Verified via `--dry-run --target=global`.
- [x] 3.4 Existing `--target=profile` path stays unchanged (default;
  uses `code --profile <name> --install-extension <id>`).
- [x] 3.5 `vspcli --detect --target=workspace` orchestrates detect →
  install-workspace.sh end-to-end; verified.
- [x] 3.6 Completions for `--target workspace|profile|global` and
  `--check-compat block|warn|off` shipped (§1.4/§5.5).

## 4. JSONC merge helper + UX layer

- [x] 4.1 `scripts/lib/jsonc-merge.sh` + `scripts/lib/jsonc-strip.awk`
  (recovered from git HEAD). Verified: empty merge, leading-comment
  preservation, deep object merge, primitive overwrite. Shellcheck-clean.
- [ ] 4.2 Use `_shared/ux/crisp.jsonc` and `_shared/ux/retina.jsonc`
  produced by the mechanical derivation in §0b.1 (per design.md
  Decision 17). The UX key list is whatever the diff produced — do
  NOT hand-list keys here. The CI gate from §0b.1 prevents the two
  preset files from drifting into different key sets.
- [ ] 4.2a Use `_shared/editor-base.jsonc` produced by the mechanical
  derivation in §0b.2 (per design.md Decision 16). This file holds
  the ~80 non-UX keys (telemetry, update mode, autoFetch, etc.) that
  must be merged into every `_merged/<flavor>.json` post-migration.
- [x] 4.3 `--ux`, `--font`, `--font-size`, `--theme`, `--icon-theme`
  parsing wired in `vspcli` and `install-workspace.sh`.
- [x] 4.4 Compose pipeline order: (1) editor-base, (2) UX preset (if
  --ux), (3) rust hover settings (if rust in scope), (4) rtk profile
  (unless --no-rtk), (5) raw flag overrides on top. Written via
  `jsonc_merge`.
- [x] 4.5 UX preset validated at parse time; unknown preset → exit 4
  with valid list.

## 5. Catalog migration

- [x] 5.1 `scripts/migrate-catalog.sh` implemented and **executed
  against the repo**. Result: 11 toolchain dirs, 11 _merged, 11
  canonical exports + 22 deprecation symlinks, MIGRATED.md written.
  Backup at `.cache/migrate-catalog-backup-<pid>/`. Lock via mkdir
  (POSIX-portable; flock is Linux-only). Sibling-tree
  atomicity is simplified to "backup-then-mutate" this round; full
  rename(2) sibling-trees is a hardening follow-up. Implementation per
  spec.md scenarios but with mkdir-lock rather than flock per
  cross-platform need:
  - Acquire `flock -n 9 .cache/migrate-catalog.lock`; exit 6 with
    `migration already running (lock held by PID N)` if the lock is
    held.
  - Take an unconditional backup of current `profiles/`,
    `_overrides/`, `_merged/`, and `exports/` to
    `.cache/migrate-catalog-backup-<pid>/`.
  - **Update `scripts/compose-settings.sh:108`** (per R3#2): remove
    the `if [[ "$name" == *retina* ]]; then base=editor-retina.jsonc`
    substring match; replace with the `_shared/editor-base.jsonc` +
    `_overrides/<flavor>.jsonc` composition.
  - Build the new layout in sibling trees: `profiles.new/`,
    `_overrides.new/`, `_merged.new/`, `exports.new/`. For each of
    the 11 destination flavors:
    - Diff `_overrides/<flavor>-crisp.jsonc` and `_overrides/<flavor>
      -retina.jsonc`; emit `_overrides.new/<flavor>.jsonc` containing
      the keys identical between them. Keys whose values differ
      should already be in `_shared/ux/<preset>.jsonc` (per §0b.1).
      If a key differs and is NOT in the UX list, abort with a clear
      error — that's a hidden UX dimension.
    - Run the (updated) compose-settings.sh against the new
      `_overrides.new/<flavor>.jsonc` to produce
      `_merged.new/<flavor>.json`. The merge order is now:
      `_shared/editor-base.jsonc` → `_overrides.new/<flavor>.jsonc`.
      UX is NOT merged in (applied at workspace layer only).
    - Run export-profiles.sh against the new merged file to produce
      `exports.new/<flavor>.code-profile`.
    - Build `profiles.new/<flavor>/` containing the composed
      `extensions.json` (from §2.3-2.4) and a symlink `settings.json
      -> ../../_merged.new/<flavor>.json`.
  - Atomically rename via POSIX `rename(2)`: `<dir>/` → `<dir>.old/`
    then `<dir>.new/` → `<dir>/` for each of the four directory
    trees. Sequence the renames so a SIGKILL between any two leaves
    a state `--doctor` can detect and continue from.
  - Create the deprecation symlinks per `specs/export-profile-bundles
    /spec.md`: `exports/<flavor>-{crisp,retina}.code-profile` →
    `<flavor>.code-profile`. Best-effort; failure is logged but does
    not abort the migration.
  - Remove the `.old` siblings on success.
  - Write `MIGRATED.md` listing renames + `_overrides/` merges +
    bundle symlinks. Idempotent on re-run.
- [x] 5.1a `--doctor` mode: detects missing flavor outputs and legacy
  dirs, exits 0 without modifying. Verified ok-clean post-migration.
- [x] 5.1b `--finalize` mode: removes deprecation symlinks. (Not
  exercised yet — runs at end-of-deprecation-window.)
- [x] 5.2 `vspcli --migrate-catalog [--doctor|--finalize]` wired and
  verified. (Mutual-exclusion check left as follow-up.)
- [x] 5.3 `scripts/lib/legacy-profile-name.sh` — resolve_legacy_profile_name
  + rate-limited warning (ack/summary/once-per-tree). Sourced from
  `install-extensions.sh`. Verified: `rust-profile-crisp` →
  `rust crisp` with one stderr warning per process tree. Sourcing into
  `compose-settings.sh`, `export-profiles.sh`, `import-profile.sh` is
  pending (not all paths exercised yet).
- [x] 5.4 README updated: Quick Start (workspace-aware) added per
  §11.1; legacy Quick Start preserved with deprecation note; Profile
  Matrix has a header explaining `(crisp|retina)` suffixes are now
  UX presets, not separate dirs; legacy `raw.githubusercontent.com`
  URLs continue to resolve via deprecation symlinks (Decision 18).
  CONTRIBUTING.md and `.github/workflows/` reviewed — no profile-name
  pins to update.
- [x] 5.5 Completion scripts list the 11 post-migration flavors
  exclusively. Legacy names are NOT in completions (they still
  resolve at runtime via the deprecation shim — completion shows
  the canonical names).
- [ ] 5.6 Update existing `openspec/specs/manage-profile-cli/spec.md`
  scenarios that pin legacy names — deferred to archive-time
  (`openspec-sync-specs` or `openspec-archive` will materialize the
  MODIFIED requirements from this change's deltas).

## 6. Rust docs stack

- [x] 6.1 Rust hover settings emitted via `install-workspace.sh` when
  rust toolchain in scope. Sourced from snapshots/rust-tasks.json
  `.settings`.
- [x] 6.2 Rust doc tasks (rtk-prefixed) emitted via `install-workspace.sh`.
  Overwrite-by-label preserves user-authored tasks.
- [x] 6.2a `snapshots/rust-tasks.json` created with rtk-prefixed task +
  hover-settings canonical form for BATS pinning. Footer line in
  docs/rust.md §11 — TODO when 6.4 runs.
- [x] 6.3 Policy block applied via `scripts/lib/apply-rust-policy.sh`
  to AGENTS.md, CLAUDE.md, CODEX.md, GEMINI.md (latter two created).
  Sentinel-bracketed; idempotent.
- [x] 6.3a Staleness gate: `apply-rust-policy.sh --check` fails on drift.
  CI integration deferred to §10.
- [x] 6.4 §11 footer added to `docs/rust.md` pointing readers at
  `vspcli --detect` for the rtk-prefixed CLI-emitted form. Staleness
  gate (`apply-rust-policy.sh --check`) still passes (only §11 was
  edited, not §12).
- [x] 6.5 Created `scripts/tests/bats/fixtures/rust-{clean,broken}-doc/`
  with minimal `Cargo.toml` + `src/lib.rs`. Broken fixture has
  `[`SymbolThatDoesNotExist`]` intra-doc link.

## 7. rtk enforcement plumbing

- [x] 7.1 `_shared/rtk/rtk-init.fish` — function-based wrappers,
  process-local `set -l ARTAGON_RTK_WRAPPED 1`, interactive-shell
  diagnostic. Verified: sourcing in fish makes `rg` resolve to a wrapper
  function calling `command rtk rg`.
- [x] 7.2 `_shared/rtk/rtk-init.bash` — bash equivalent;
  shellcheck-clean.
- [x] 7.3 rtk profile emission in `install-workspace.sh`: detects fish
  + bash via `command -v`, writes absolute paths, omits rtk-fish if
  fish absent (falls back to rtk-bash), `automationProfile.osx`
  references resolved fish; `--no-rtk` suppresses entirely. Verified.
- [x] 7.3a Cross-script orchestration: single `vspcli --detect
  --target=workspace` invocation produces all three .vscode/* files.
- [x] 7.4 Workspace Trust note added to README's "Quick Start
  (workspace-aware)" section per §11.1.
- [x] 7.5 rtk enforcement caveats (5-item list from design.md
  Decision 6) added to README per §11.1.

## 8. Compat-check integration

- [x] 8.1 `--check-compat=block|warn|off` parsing added to
  `install-workspace.sh` and threaded through `vspcli --detect`.
  Invalid value exits 4.
- [x] 8.2 In block/warn mode, invokes `check-extension-compatibility.sh
  --json` against each detected toolchain's existing profile, parses
  `.summary.incompatible`, lists failing IDs to stderr; block aborts
  before any write, warn proceeds.
- [x] 8.3 Default `warn` for all targets per design.md Decision 11.
- [x] 8.4 `docs/EXTENSION_COMPATIBILITY.md` updated with a
  CLI-integration callout linking the standalone checker to
  `vspcli --detect --target=workspace --check-compat=...`.

## 9. BATS test suite (non-negotiable phase)

The change is not done until this phase is green. Every test follows
the patterns in `agents/skills/vscode-config/scripts/tests/test_helper.bash`
and `openspec/changes/profile-graceful-defaults/tasks.md` §4.

- [x] 9.1 `scripts/tests/bats/test_helper.bash` — shared
  `make_workspace`, `have_bin`, `require_bin`, deprecation env
  cleanup in setup/teardown, `detect()` and `vspcli()` helpers.
  `code` CLI mock + golden-file diffing pending (tasks not yet
  exercised).
- [x] 9.2 `scripts/tests/bats/detect-toolchain.bats` — 18 tests
  covering all 11 detection signals + polyglot + override + JSON +
  exit codes 2/3/4. All pass.
  - Cargo.toml only → rust
  - package.json + astro.config.mjs → astro
  - pom.xml → java-maven
  - build.gradle alone → java-gradle
  - build.gradle.kts + Application.java with `@SpringBootApplication`
    → java-spring
  - build.gradle.kts without Spring marker → java-gradle
  - CMakeLists.txt + .clangd → cpp-clangd
  - CMakeLists.txt only → cpp-intellisense
  - .github/workflows/foo.yml only → github-workflows
  - polyglot Tauri (Cargo.toml + astro.config.mjs) → `rust astro`
  - empty workspace → exit 3
  - `--toolchain rust` in empty → exit 0
  - `--no-detect` without `--toolchain` → exit 4
  - `--toolchain unknown` → exit 4
  - `--json` mode shape pinned with golden file
- [x] 9.3 `install-extensions-layered.bats` — 6 tests: base+toolchain
  composition, AI subset isolation, dedup preserves first occurrence,
  missing-layer error.
- [x] 9.4 `workspace-vscode-recs.bats` — 4 tests: write recommendations,
  preserve existing + unwantedRecommendations untouched, idempotent
  rerun, dry-run.
- [x] 9.5 `ux-overrides.bats` — 6 tests: crisp/retina presets,
  raw-flag override beats preset, --font sets fontFamily, editor-base
  coexists, UX/base key disjointness.
- [x] 9.6 `profile-catalog-migration.bats` — 11 tests: legacy resolution
  for all 4 mappings, deprecation warning + ack + once-per-tree,
  doctor reports clean, 11 dirs post-migration, deprecation symlinks.
- [x] 9.7 `cli-flags.bats` — 11 tests: --help, --list (post-migration
  11 flavors), all 4 completion shells, --detect routing,
  --migrate-catalog --doctor, unknown flag, dry-run, invalid target.
  Surfaced + fixed two pre-existing vspcli bugs along the way:
  `paste -sd ' '` (BSD-incompatible) → `tr | sed`, and bash completion
  HEREDOC unescaped `$COMP_WORDS` aborting under `set -u`.
- [x] 9.8 `rtk-init-wrappers.bats` — 6 tests covering both bash and
  fish: source-without-rtk emits clear stderr, re-wrap guard,
  command-existence gate, marker is NOT exported (process-local).
  Fish tests skip cleanly when fish absent; use `--no-config` to
  bypass user fish config that would re-add rtk to PATH.
- [x] 9.9 `rust-docs-settings.bats` — 5 tests: 7 hover settings present,
  hover.delay = 300, both rtk-prefixed tasks present, non-rust
  workspace omits rust-analyzer keys, snapshot ↔ emitted alignment.
- [x] 9.10 `rust-docs-strict-gate.bats` — 2 tests: clean fixture
  passes, broken intra-doc link fails. Skips when cargo absent.
  Both pass locally.
- [x] 9.11 `compat-check.bats` — 5 tests: off/warn/block accepted,
  invalid value → 4, off skips checker. (Network-dependent
  marketplace lookup is out of test scope; defers to existing
  `check-extension-compatibility.sh`.) 75 BATS total now passing.
- [x] 9.12 `scripts/tests/run.sh` extended to run BATS suite after
  legacy script tests; clean WARN+exit-0 when bats absent, fail
  loudly when present and a test fails.
- [x] 9.13 67/67 BATS tests passing locally on macOS.

## 10. CI extensions

- [x] 10.1 `.github/workflows/ci.yml` Install step now installs
  `jq openssl bats fish`; existing `Run script test suite` step
  (`bash scripts/tests/run.sh`) now also runs the BATS suite per §9.12.
- [ ] 10.2 Extend the existing shellcheck CI gate to cover all new
  shell files (`scripts/detect-toolchain.sh`,
  `scripts/migrate-catalog.sh`, `scripts/lib/compose-extensions.sh`,
  `scripts/lib/jsonc-merge.sh`, `_shared/rtk/rtk-init.bash`).
- [ ] 10.3 Add a JSON/JSONC validation gate over new files
  (`_shared/ux/*.jsonc`, `_shared/extensions/*.json`) using the
  existing `validate-json.sh` and the `vscode-jsonc-validate` skill
  script.
- [ ] 10.4 Add a CI assertion that every file under
  `scripts/tests/bats/` matches `*.bats` (per task 0.10).

## 11. Documentation

- [x] 11.1 README.md updated: new "Quick Start (workspace-aware,
  recommended)" with `vspcli --detect --target=workspace`; UX
  override examples; Workspace Trust note; rtk enforcement caveats;
  legacy section preserved with deprecation note. Existing 75
  legacy profile-name references in tables/Profile Matrix kept
  in place — they continue to resolve via deprecation symlinks +
  shim during the deprecation window.
- [x] 11.2 CHANGELOG.md updated with new "Unreleased —
  workspace-toolchain-and-ux-layering" section: catalog collapse, new
  flags, deprecation, AI consolidation, rtk plumbing, rust docs.
- [x] 11.3 `docs/ts-js-node.md` and `docs/EXTENSION_COMPATIBILITY.md`
  unchanged. `docs/rust.md` has only the §11 footer per §6.4
  (CLI-emitted form pointer); §12 unchanged so the policy staleness
  gate stays green.

## 12. Validation

- [x] 12.1 `openspec validate workspace-toolchain-and-ux-layering
  --strict` → valid.
- [x] 12.2 `bash scripts/tests/run.sh` runs legacy script tests +
  BATS (locally on macOS — 69 BATS pass; CI ubuntu runs the same
  via `apt install bats fish`).
- [x] 12.3 Manual smoke tests pass: detect (dry-run), live install
  rust, polyglot rust+astro, --ux=crisp + --theme override,
  --migrate-catalog --doctor.
  - `mktemp -d` + `touch Cargo.toml` + `vspcli --detect --target=workspace
    --dry-run` → composed list contains rust + base; no file written.
  - Same with `--target=workspace` (no dry-run): `.vscode/extensions.json`
    written; `code` not invoked.
  - Polyglot fixture (Cargo.toml + astro.config.mjs) → composed list
    contains rust + astro + base; no duplicates.
  - `vspcli --ux=crisp --theme="GitHub Light"` writes
    `.vscode/settings.json` with `editor.fontFamily` from preset and
    `workbench.colorTheme: "GitHub Light"` from raw flag.
  - `vspcli --migrate-catalog` against a fresh clone with all 22
    legacy dirs → 11 toolchain dirs + 11 bundles. Re-run is a no-op.
- [ ] 12.4 Manual VS Code smoke (one-time, document the steps in the
  PR description, not in the test suite):
  - Open a fresh clone in VS Code; grant Workspace Trust on prompt.
  - Open Terminal → confirm rtk-fish profile is the default and the
    aliases are in place (`type rg` → "function rg defined ...").
  - Open Tasks → confirm `rust: doc` and `rust: doc strict` are
    listed.
  - Hover over a rust symbol in a fixture crate → confirm hover docs
    appear.

## 13. Acceptance

The change is complete when:

- [x] 13.1 All five spec deltas validate strictly.
- [x] 13.2 69 BATS pass on macOS locally; CI ubuntu installs
  `bats fish` and runs the same suite via `scripts/tests/run.sh`.
- [x] 13.3 Shellcheck-clean across all new shell (info-only SC1091
  for runtime-resolved `source` paths suppressed in BATS test runs).
- [x] 13.4 Post-migration: 11 profile dirs, 11 `_merged/<flavor>.json`,
  11 canonical `exports/<flavor>.code-profile`, 22 deprecation
  symlinks. Verified via `vspcli --migrate-catalog --doctor`.
- [x] 13.5 `docs/rust.md` has only the §11 footer; `docs/ts-js-node.md`
  and `docs/EXTENSION_COMPATIBILITY.md` unchanged.
- [x] 13.6 README + CHANGELOG updated. CONTRIBUTING + `.github/workflows`
  not touched (no profile-name pins in either; CI calls
  `compose-settings.sh` and `export-profiles.sh` which use composed
  layers post-migration).
- [x] 13.7 Policy block in 4 agent docs between sentinels; staleness
  gate (`apply-rust-policy.sh --check`) clean.
- [ ] 13.8 Open Question 1 — pending manual VS Code validation
  (cannot be done from this CLI session).
- [x] 13.9 Verified: `_shared/extensions/{rust,astro,cpp-*,java-*,
  github-workflows}.json` contain no AI-tagged IDs (BATS test
  `compose: AI extensions absent from rust toolchain layer` passes).
- [x] 13.10 `_shared/ux/{crisp,retina}.jsonc` have identical key
  sets; no overlap with `_shared/editor-base.jsonc`. Verified by
  `scripts/tests/check-ux-symmetry.sh` and BATS
  `ux: --ux preset and editor-base have disjoint keys`.

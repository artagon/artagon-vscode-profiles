## ADDED Requirements

### Requirement: Workspace detection subcommand

The CLI SHALL accept `--detect [PATH]` (defaulting to `$PWD` when PATH is
omitted) that delegates to `scripts/detect-toolchain.sh`. When detection
succeeds, the CLI continues into the install + compose pipeline using the
detected toolchain(s); when detection fails, the CLI emits the same exit
codes as the underlying script (per the `detect-workspace-toolchain`
capability).

#### Scenario: Detect in current directory

- **WHEN** the user runs `vspcli --detect` from a rust workspace root
- **THEN** the CLI emits `rust` to stdout (or `rust <flavor>...` for
  polyglot) and proceeds to install + compose for the detected
  toolchain(s).

#### Scenario: Detect with explicit path

- **WHEN** the user runs `vspcli --detect /path/to/project`
- **THEN** detection inspects `/path/to/project` rather than `$PWD`.

#### Scenario: Detect path does not exist

- **WHEN** the user runs `vspcli --detect /no/such/path`
- **THEN** the CLI exits with status 2 and writes
  `error: workspace path /no/such/path does not exist` to stderr; no
  toolchain output is written to stdout.

#### Scenario: Detect path is not a directory

- **WHEN** the user runs `vspcli --detect /etc/hosts`
- **THEN** the CLI exits with status 2 and writes
  `error: workspace path /etc/hosts is not a directory` to stderr.

### Requirement: Install target flag

The CLI SHALL accept `--target=workspace|profile|global` that selects
the install dispatch path per the `install-profile-extensions` capability.
The flag SHALL be parsed by `vspcli` directly (not just passed through to
the install script) so error messages on invalid values surface from the
top-level CLI.

#### Scenario: Workspace target

- **WHEN** the user runs `vspcli --detect --target=workspace`
- **THEN** the CLI writes `.vscode/extensions.json` and does not invoke
  `code --install-extension`.

#### Scenario: Global target

- **WHEN** the user runs `vspcli --install rust --target=global`
- **THEN** the CLI invokes `code --install-extension <id>` (no
  `--profile` flag) for each extension in the composed list.

#### Scenario: Invalid target

- **WHEN** the user passes `--target=zzz`
- **THEN** the CLI exits 4 and writes
  `--target accepts: workspace, profile, global` to stderr.

### Requirement: UX flags

The CLI SHALL accept `--ux=<preset>`, `--font <family>`, `--font-size <n>`,
`--theme <id>`, `--icon-theme <id>`, all of which are passed through to
the compose pipeline per the `compose-profile-settings` capability.

#### Scenario: UX flag composition

- **WHEN** the user runs `vspcli --ux=crisp --font="JetBrains Mono"`
- **THEN** the compose pipeline overlays the crisp preset, then sets
  `editor.fontFamily: "JetBrains Mono"` on top.

#### Scenario: Unknown UX preset

- **WHEN** the user passes `--ux=zzz`
- **THEN** the CLI exits 4 and writes `unknown UX preset 'zzz'; valid:
  crisp, retina, default` to stderr.

### Requirement: Catalog migration command

The CLI SHALL accept `--migrate-catalog` that renames legacy profile
directories from `profiles/<flavor>-{crisp,retina}/` to
`profiles/<flavor>/`, regenerates `exports/*.code-profile`, and writes a
`MIGRATED.md` note recording each rename. The command SHALL be
idempotent: running it twice is a no-op on the second run.

#### Scenario: First migration run

- **WHEN** the user runs `vspcli --migrate-catalog` against a repo with
  the legacy 22-profile layout
- **THEN** `profiles/` contains 11 toolchain dirs (no `-crisp`/`-retina`
  suffix); `_overrides/` is renamed in-place from
  `<flavor>-{crisp,retina}.jsonc` to `<flavor>.jsonc` (post-merging
  the two if they differ — UX keys hoisted to `_shared/ux/<preset>.jsonc`,
  toolchain keys retained); `_merged/<flavor>.json` is regenerated;
  `exports/<flavor>.code-profile` is generated; legacy `exports/
  <flavor>-{crisp,retina}.code-profile` files are replaced with
  symlinks to `exports/<flavor>.code-profile` per design.md
  Decision 18; and `MIGRATED.md` lists 22 source dirs and 11
  destinations.

#### Scenario: Migration is concurrency-safe

- **WHEN** two `vspcli --migrate-catalog` invocations run simultaneously
  on the same checkout
- **THEN** the second invocation exits 6 with `migration already
  running (lock held by PID N)` to stderr; the first completes
  normally; no half-state results.

#### Scenario: --migrate-catalog mutually exclusive with other flags

- **WHEN** the user passes `--migrate-catalog` together with `--detect`,
  `--install`, `--target`, `--ux`, `--font`, `--theme`, `--icon-theme`,
  `--toolchain`, `--no-detect`, `--check-compat`, or `--dry-run`
- **THEN** the CLI exits 4 with `--migrate-catalog cannot be combined
  with: <flag-list>` to stderr.

#### Scenario: --doctor detects half-migrated state

- **WHEN** a previous `vspcli --migrate-catalog` was killed mid-run AND
  the user runs `vspcli --migrate-catalog --doctor`
- **THEN** the CLI prints the remaining work to stdout (e.g., "5
  legacy dirs not yet renamed: cpp-clangd-crisp, cpp-clangd-retina,
  ...") and exits 0 without modifying anything.

#### Scenario: Second migration run

- **WHEN** the user runs `vspcli --migrate-catalog` after the first run
  succeeded
- **THEN** the command writes `catalog already migrated; no changes`
  to stdout and exits 0.

#### Scenario: Partially-migrated repo

- **WHEN** the user runs `vspcli --migrate-catalog` against a repo
  where some legacy dirs were renamed by hand and some were not
- **THEN** the command renames the remaining legacy dirs, regenerates
  `exports/`, and exits 0 with a stdout note listing what it did.

### Requirement: Toolchain override flag

The CLI SHALL accept `--toolchain <flavor>` and `--no-detect` per the
`detect-workspace-toolchain` capability.

#### Scenario: Toolchain override

- **WHEN** the user runs `vspcli --toolchain rust --target=workspace` in
  an empty directory
- **THEN** the CLI uses the rust layer (no detection), writes
  `.vscode/extensions.json` with the rust+base composed list, and exits 0.

### Requirement: Compatibility check flag

The CLI SHALL accept `--check-compat=block|warn|off` per the
`install-profile-extensions` capability.

#### Scenario: Compat check forwarded

- **WHEN** the user runs `vspcli --detect --check-compat=warn`
- **THEN** the compat check runs, prints any incompatibilities to
  stderr, and the install proceeds.

### Requirement: Dry-run flag

The CLI SHALL accept `--dry-run` that prints would-be writes and
invocations without modifying files or running `code`.

#### Scenario: Dry run for detect+install

- **WHEN** the user runs `vspcli --detect --target=workspace --dry-run`
- **THEN** stdout lists the would-be `.vscode/extensions.json` path and
  composed extension IDs; the file is not created.

## MODIFIED Requirements

### Requirement: Profile discovery and listing

The CLI SHALL list profiles by enumerating directories under `profiles/`
in sorted order. After the catalog migration (per "Catalog migration
command"), the listing SHALL contain only toolchain-flavor names
(no `-crisp`/`-retina` suffix). During the deprecation window for legacy
names, the CLI SHALL also accept legacy names on input and resolve them
to `<flavor>` plus an implied `--ux=<look>`, with a deprecation warning
written to stderr on every legacy-name invocation.

#### Scenario: List profiles after migration

- **WHEN** the user runs `vspcli --list` post-migration
- **THEN** the output contains 11 toolchain-flavor names (one per
  directory under `profiles/`), in sorted order.

#### Scenario: Legacy name resolves with deprecation warning (rate-limited)

- **WHEN** the user runs `vspcli --install rust-profile-crisp` during
  the deprecation window in a fresh shell
- **THEN** the install resolves to flavor `rust` plus implied
  `--ux=crisp`; stderr contains
  `deprecation: 'rust-profile-crisp' resolves to 'rust' with
  --ux=crisp; use --toolchain rust --ux=crisp directly`.
- **AND** subsequent invocations within the same shell process tree
  (where `ARTAGON_VSCODE_DEPRECATION_SEEN=1` is inherited from the
  first invocation) suppress the warning per design.md Decision 20.

#### Scenario: Deprecation warning ack via env var

- **WHEN** the user has `ARTAGON_VSCODE_DEPRECATION_ACK=1` exported in
  the environment AND runs `vspcli --install rust-profile-crisp`
- **THEN** no deprecation warning is printed to stderr on any
  invocation; the resolution to `rust + --ux=crisp` still happens.

#### Scenario: Deprecation summary opt-in

- **WHEN** the user has `ARTAGON_VSCODE_DEPRECATION_SUMMARY=1`
  exported AND runs five legacy-name invocations in a CI script
- **THEN** the warning is suppressed per-invocation; an end-of-run
  stderr summary `5 legacy invocations; deprecation window closes
  <date>` is printed by the script that owns the process tree.

#### Scenario: Legacy name resolves via direct script call

- **WHEN** the user runs `bash scripts/install-extensions.sh
  rust-profile-crisp` (bypassing `vspcli`)
- **THEN** the script also resolves the legacy name to `rust` plus
  implied `--ux=crisp` and emits the same rate-limited deprecation
  warning. The deprecation alias is shared via
  `scripts/lib/legacy-profile-name.sh` (sourced by `vspcli`,
  `install-extensions.sh`, `compose-settings.sh`, `export-profiles.sh`,
  and `import-profile.sh` per design.md Decision 21).

#### Scenario: Legacy name fails post-deprecation

- **WHEN** the user runs `vspcli --install rust-profile-crisp` after
  the deprecation window has closed (legacy support removed)
- **THEN** the CLI exits 4 with
  `unknown profile 'rust-profile-crisp'; legacy names removed in
  release X; use --toolchain rust --ux=crisp` to stderr.

### Requirement: Shell completion output

The CLI SHALL output completion scripts for bash, zsh, or fish that
include:

- The current toolchain-flavor list (from `profiles/` post-migration).
- The list of toolchain flavors known to detection (per
  `detect-workspace-toolchain`).
- The new flags: `--detect`, `--target`, `--ux`, `--font`,
  `--font-size`, `--theme`, `--icon-theme`, `--migrate-catalog`,
  `--toolchain`, `--no-detect`, `--check-compat`, `--dry-run`.

#### Scenario: Bash completion includes new flags

- **WHEN** `vspcli --completion bash` runs
- **THEN** the emitted snippet's flag list contains every flag added
  by this change.

#### Scenario: Zsh completion includes new flags

- **WHEN** `vspcli --completion zsh` runs
- **THEN** the emitted snippet's `_arguments` list contains every
  flag added by this change, including `--detect`, `--target`,
  `--ux`, `--font`, `--theme`, `--icon-theme`, `--migrate-catalog`,
  `--toolchain`, `--no-detect`, `--no-rtk`, `--check-compat`,
  `--dry-run`. (Preserves the original spec's "Zsh completion
  output" coverage.)

#### Scenario: Fish completion includes UX presets

- **WHEN** `vspcli --completion fish` runs
- **THEN** `--ux` is offered with completions for `crisp` and `retina`.

#### Scenario: Unsupported shell

- **WHEN** `vspcli --completion powershell` runs
- **THEN** it exits non-zero with a message about supported shells.

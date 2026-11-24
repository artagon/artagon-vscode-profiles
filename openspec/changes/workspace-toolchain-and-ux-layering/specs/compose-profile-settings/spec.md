## ADDED Requirements

### Requirement: Workspace UX overrides

The system SHALL accept a `--ux=crisp|retina` flag and four raw-override
flags (`--font <family>`, `--font-size <n>`, `--theme <id>`,
`--icon-theme <id>`) that compose UX settings into the workspace's
`.vscode/settings.json` without trampling unrelated user-authored keys.

The composition order SHALL be:

1. Read existing `.vscode/settings.json` (JSONC-tolerant: comments,
   trailing commas, duplicate keys handled by last-wins dedup).
2. If `--ux=<preset>` is supplied, overlay
   `_shared/ux/<preset>.jsonc` on top of the existing keys at
   key-level (objects merge, primitives/arrays replace).
3. If any raw override flag is supplied, overlay that single key on
   top.
4. Write the result back as canonical JSON. The leading file-comment
   block (everything before the first non-whitespace, non-comment
   character of the original file) is preserved as a string prefix on
   write-back. Interior comments (between keys) are NOT preserved per
   design.md Decision 12. On first write to a previously-comment-bearing
   file, the system SHALL also write a `<file>.bak` sibling so users can
   recover lost interior comments on a one-off basis.

The system SHALL NOT remove or rewrite any key not named in the named
preset or the supplied raw flags.

#### Scenario: Backup written for first comment-bearing merge

- **WHEN** existing `.vscode/settings.json` contains interior comments
  AND the user runs `vspcli --ux=crisp` (any merge invocation)
- **THEN** the system writes `.vscode/settings.json.bak` containing the
  pre-merge contents (comments intact) before writing the new merged
  file.

#### Scenario: Leading comment block preserved

- **WHEN** existing `.vscode/settings.json` starts with `// Workspace
  settings — owner: SRE\n` followed by JSON
- **THEN** the post-merge file's first line is still `// Workspace
  settings — owner: SRE`.

#### Scenario: Named preset writes UX keys

- **WHEN** the user runs `vspcli --ux=crisp` in a workspace with no
  `.vscode/settings.json`
- **THEN** the file is created containing the keys from
  `_shared/ux/crisp.jsonc` only.

#### Scenario: Raw flag overrides preset

- **WHEN** the user runs `vspcli --ux=crisp --theme="GitHub Light"` and
  `_shared/ux/crisp.jsonc` sets `"workbench.colorTheme": "Tokyo Night"`
- **THEN** the resulting file contains `"workbench.colorTheme": "GitHub Light"`,
  with all other crisp.jsonc keys untouched.

#### Scenario: Existing user keys preserved

- **WHEN** `.vscode/settings.json` already contains
  `{"editor.fontSize": 16, "files.autoSave": "onFocusChange"}` and the
  user runs `vspcli --ux=retina` (which sets `editor.fontSize: 14` and
  `editor.fontFamily: "..."`)
- **THEN** the resulting file has `editor.fontSize: 14` (preset wins,
  per Decision 1 of design.md), `editor.fontFamily` set, and
  `files.autoSave` unchanged.

#### Scenario: Raw flag without preset

- **WHEN** the user runs `vspcli --font="JetBrains Mono"` without
  `--ux`
- **THEN** only `editor.fontFamily` is set; no other keys are touched.

### Requirement: Rust toolchain settings emission

The system SHALL write the seven rust-analyzer hover and signature settings from `docs/rust.md` §2 to the workspace's `.vscode/settings.json` whenever the rust toolchain is in scope (detected via signals or supplied via `--toolchain rust`):

- `rust-analyzer.hover.actions.enable: true`
- `rust-analyzer.hover.documentation.enable: true`
- `rust-analyzer.hover.links.enable: true`
- `rust-analyzer.signatureInfo.documentation.enable: true`
- `editor.hover.enabled: true`
- `editor.hover.delay: 300`
- `editor.parameterHints.enabled: true`

The settings SHALL be emitted with the exact key names and values from
`docs/rust.md`. If `docs/rust.md` evolves and the BATS rust-docs-settings
test detects drift between the doc and the emitted settings, the test
fails until both are realigned.

#### Scenario: Rust workspace gets hover settings

- **WHEN** the user runs `vspcli --detect` in a rust workspace
- **THEN** `.vscode/settings.json` contains all seven keys from
  `docs/rust.md` §2 with the values defined in that file.

#### Scenario: Non-rust workspace omits hover settings

- **WHEN** the user runs `vspcli --detect` in an astro-only workspace
- **THEN** `.vscode/settings.json` does NOT contain any
  `rust-analyzer.*` keys.

#### Scenario: UX preset does not strip rust hover settings

- **WHEN** a rust workspace has the seven hover keys set, and the user
  runs `vspcli --ux=crisp`
- **THEN** the seven hover keys remain unchanged after the UX overlay.

### Requirement: Rust toolchain task emission

When the rust toolchain is in scope, the system SHALL write two tasks
to the workspace's `.vscode/tasks.json`:

```jsonc
{ "label": "rust: doc",
  "type": "shell",
  "command": "rtk cargo doc --workspace --all-features --no-deps",
  "problemMatcher": "$rustc" }

{ "label": "rust: doc strict",
  "type": "shell",
  "command": "rtk env RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps",
  "problemMatcher": "$rustc" }
```

The `rtk` and `rtk env` prefixes are required (not the bare-`cargo`
form quoted in `docs/rust.md` §11) because tasks bypass the rtk
shell-init aliases. Existing tasks with the same `label` SHALL be
overwritten in place; tasks with other labels SHALL be preserved
verbatim.

#### Scenario: Rust workspace gets doc tasks

- **WHEN** the user runs `vspcli --detect` in a rust workspace
- **THEN** `.vscode/tasks.json` contains both tasks above with the
  `rtk`-prefixed `command` fields.

#### Scenario: Existing user tasks preserved

- **WHEN** `.vscode/tasks.json` already contains a task with
  `"label": "my-build"` and the user runs `vspcli --detect` in a rust
  workspace
- **THEN** the resulting tasks.json contains the my-build task
  unchanged plus the two rust doc tasks.

#### Scenario: Doc task labels are stable

- **WHEN** `.vscode/tasks.json` contains an existing `rust: doc` task
  with a different `command`, and the user runs `vspcli --detect`
- **THEN** the existing `rust: doc` task is replaced with the canonical
  `rtk cargo doc ...` form (overwrite-by-label).

### Requirement: rtk terminal profile emission

The system SHALL write the rtk-fish and rtk-bash terminal profiles plus the matching `defaultProfile.{osx,linux}` and `automationProfile.osx` keys to `.vscode/settings.json` whenever `--target=workspace` is in effect (the default for `vspcli --detect`). The system SHALL detect fish and bash binary paths via `command -v` at compose time per design.md Decision 13 and write the resolved absolute paths into the profile entries; hardcoded `/opt/homebrew/bin/fish` is not permitted. The exact JSON shape is defined in design.md "Reference: rtk enforcement architecture".

The system SHALL also write the workspace-relative init scripts to
`_shared/rtk/rtk-init.fish` and `_shared/rtk/rtk-init.bash` if they do
not already exist; if they exist, they are NOT modified.

The system SHALL accept a `--no-rtk` flag (per design.md Decision 14)
that suppresses rtk profile emission entirely (no
`terminal.integrated.profiles.*` keys written, no `_shared/rtk/` files
created or referenced).

#### Scenario: Workspace gets rtk profile (default)

- **WHEN** the user runs `vspcli --detect --target=workspace` AND
  `command -v fish` resolves to `/opt/homebrew/bin/fish` AND
  `command -v bash` resolves to `/bin/bash`
- **THEN** `.vscode/settings.json`'s
  `terminal.integrated.profiles.osx.rtk-fish.path` is
  `"/opt/homebrew/bin/fish"`, `.rtk-bash.path` is `"/bin/bash"`, and
  `terminal.integrated.defaultProfile.osx` is `"rtk-fish"`.

#### Scenario: Linux fish path detected

- **WHEN** the user runs the same command on Linux where `command -v
  fish` resolves to `/usr/bin/fish`
- **THEN** `.vscode/settings.json` contains
  `terminal.integrated.profiles.linux.rtk-fish.path: "/usr/bin/fish"`
  and `terminal.integrated.defaultProfile.linux: "rtk-fish"`.

#### Scenario: Fish absent — fall back to bash

- **WHEN** `command -v fish` returns nothing (fish not installed) AND
  `command -v bash` resolves
- **THEN** `terminal.integrated.profiles.{osx,linux}.rtk-fish` is
  omitted entirely; `defaultProfile.{osx,linux}` is set to
  `"rtk-bash"`; a stderr notice explains the omission.

#### Scenario: --no-rtk suppresses entirely

- **WHEN** the user runs `vspcli --detect --target=workspace --no-rtk`
- **THEN** `.vscode/settings.json` contains no
  `terminal.integrated.profiles.*` keys and no
  `defaultProfile.*` keys related to rtk; `_shared/rtk/rtk-init.{fish,
  bash}` files are not created if they don't already exist.

#### Scenario: Existing init script not overwritten

- **WHEN** `_shared/rtk/rtk-init.fish` already exists and the user runs
  `vspcli --detect`
- **THEN** the file is not modified.

#### Scenario: User-authored rtk-fish key preserved on conflict

- **WHEN** the user has manually authored
  `terminal.integrated.profiles.osx.rtk-fish.args` with custom values
  AND runs `vspcli --detect --target=workspace`
- **THEN** the CLI overwrites only the `.path` key (with the resolved
  binary) and any keys it manages explicitly (`--init-command` source
  argument); it does NOT overwrite user-authored keys it doesn't
  manage. (Implementation note: the merge is at the level of the
  `rtk-fish` object's known managed keys, not a wholesale replacement.)

## MODIFIED Requirements

### Requirement: Base selection by profile name

The system SHALL select the shared base settings file based on whether a
UX preset is in scope:

- For the legacy compose path (invoked via `vspcli --compose <name>` with
  a legacy `*-crisp` or `*-retina` profile name during the deprecation
  window), base selection retains its existing behavior:
  `_shared/editor-retina.jsonc` when the name contains `retina`,
  otherwise `_shared/editor-crisp.jsonc`.
- For the workspace path (invoked via `vspcli --detect` or
  `vspcli --ux=<preset>`), base selection uses
  `_shared/ux/<preset>.jsonc` when `--ux=<preset>` is supplied; when no
  UX flag is supplied, no editor-base file is composed (the workspace
  inherits whatever the user's User-scope settings provide).

#### Scenario: Retina profile uses retina base (legacy)

- **WHEN** compose runs for `rust-profile-retina` via the legacy path
- **THEN** it uses `_shared/editor-retina.jsonc` as the base.

#### Scenario: Crisp profile uses crisp base (legacy)

- **WHEN** compose runs for `rust-profile-crisp` via the legacy path
- **THEN** it uses `_shared/editor-crisp.jsonc` as the base.

#### Scenario: Workspace path with --ux=crisp

- **WHEN** the user runs `vspcli --detect --ux=crisp`
- **THEN** `_shared/ux/crisp.jsonc` is overlaid on
  `.vscode/settings.json`.

#### Scenario: Workspace path without --ux

- **WHEN** the user runs `vspcli --detect` with no `--ux` flag
- **THEN** no UX preset is overlaid; the resulting `.vscode/settings.json`
  contains only the toolchain-derived keys (e.g., rust-analyzer.\* for a
  rust workspace) plus existing user-authored keys.

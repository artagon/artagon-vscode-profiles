## ADDED Requirements

### Requirement: Layered extension composition

The system SHALL compose the installable extension list for a workspace
or profile by concatenating, in order:

1. The base layer at `_shared/extensions/base.json` — extensions every
   workspace gets (shell, yaml, markdown, json, toml, .editorconfig
   support).
2. One toolchain layer per detected (or supplied) flavor at
   `_shared/extensions/<toolchain>.json`. When multiple toolchains stack,
   layers are concatenated in the precedence order defined by the
   `detect-workspace-toolchain` capability.

The composed list SHALL be deduplicated by extension ID (preserving the
first occurrence). The composed list SHALL be deterministic — running the
composition twice with the same inputs produces a byte-equivalent output.

#### Scenario: Base layer always installed

- **WHEN** the user installs extensions for any toolchain
- **THEN** every extension ID in `_shared/extensions/base.json` is in the
  composed list.

#### Scenario: Toolchain layer appended after base

- **WHEN** the user installs extensions for the rust toolchain
- **THEN** the composed list is `_shared/extensions/base.json` followed by
  `_shared/extensions/rust.json`, deduplicated.

#### Scenario: Polyglot stacking

- **WHEN** detection emits `rust astro` (in that order)
- **THEN** the composed list is base + rust.json + astro.json,
  deduplicated, with rust IDs winning on conflict.

#### Scenario: Explicit toolchain override forces single layer

- **WHEN** the user runs `vspcli --install --toolchain ai
  --target=profile <name>` (regardless of workspace contents)
- **THEN** the composed list is base + ai.json (only), even if the
  workspace has detection signals for other flavors. The `ai` and
  `ai-plus` flavors are NEVER auto-detected (per
  `detect-workspace-toolchain` precedence) but ARE installable when
  explicitly requested.

#### Scenario: Stacked explicit toolchains

- **WHEN** the user runs `vspcli --install --toolchain rust --toolchain ai`
- **THEN** the composed list is base + rust.json + ai.json,
  deduplicated, in that order.

#### Scenario: Missing layer file

- **WHEN** `_shared/extensions/<flavor>.json` does not exist for a
  requested flavor
- **THEN** the system exits non-zero with the missing path and the flavor
  named, before invoking `code`.

#### Scenario: AI extensions absent from non-AI toolchain layers

- **WHEN** the rust toolchain layer (`_shared/extensions/rust.json`) is
  composed
- **THEN** the resulting list does NOT contain `github.copilot`,
  `github.copilot-chat`, `anthropic.claude-code`, `openai.chatgpt`,
  `Continue.continue`, or any other AI-tagged extension. AI extensions
  live exclusively in `_shared/extensions/ai.json` and
  `_shared/extensions/ai-plus.json` (per design.md Decision 15).

### Requirement: Three install targets

The system SHALL accept a `--target` flag selecting one of three install
modes:

- `--target=workspace` (default for `vspcli --detect`): the system writes
  the composed extension list to `.vscode/extensions.json` under the
  `recommendations` key. The system SHALL NOT invoke `code --install-extension`
  in this mode. Existing entries in `recommendations` SHALL be preserved
  (deduplicated set union); existing `unwantedRecommendations` SHALL be
  preserved untouched.
- `--target=profile <name>`: the system invokes
  `code --profile <name> --install-extension <id>` for each extension in
  the composed list. This is the existing default for `vspcli --install <name>`.
- `--target=global`: the system invokes
  `code --install-extension <id>` (no `--profile` flag) for each
  extension, installing into the user's default VS Code profile.

`--target=workspace` and `--target=global` are mutually exclusive with
`--target=profile`; passing more than one of these is an error (exit 4).

#### Scenario: Workspace target writes recommendations

- **WHEN** the user runs `vspcli --detect --target=workspace` in a rust
  workspace
- **THEN** `.vscode/extensions.json` exists at the workspace root with
  `recommendations` containing the composed rust+base list, and `code` was
  not invoked.

#### Scenario: Workspace target preserves existing recommendations

- **WHEN** `.vscode/extensions.json` already contains
  `{"recommendations":["foo.bar"],"unwantedRecommendations":["baz.qux"]}`
  and the user runs `vspcli --detect --target=workspace`
- **THEN** the resulting file's `recommendations` is the deduplicated
  union of the existing list and the composed list (order: existing
  first, new appended); `unwantedRecommendations` is unchanged.

#### Scenario: Global target installs without --profile

- **WHEN** the user runs `vspcli --install rust --target=global`
- **THEN** `code --install-extension <id>` is invoked once per extension
  in the composed list, with no `--profile` flag.

#### Scenario: Mutually exclusive targets

- **WHEN** the user passes both `--target=workspace` and `--target=global`
- **THEN** the CLI exits 4 and writes
  `--target accepts only one of: workspace, profile, global` to stderr.

### Requirement: Optional pre-install compatibility check

The system SHALL accept a `--check-compat=block|warn|off` flag that runs
`scripts/check-extension-compatibility.sh` over the composed extension
list before invoking `code`. The flag SHALL accept three values:

- `--check-compat=block`: incompatible extensions cause exit code 2 with
  the failing IDs on stderr; no extensions are installed and no
  `.vscode/extensions.json` is written. Opt-in only; never the default.
- `--check-compat=warn` (DEFAULT for all three install targets):
  incompatibilities print to stderr but the install proceeds.
- `--check-compat=off`: no compat check runs.

#### Scenario: Block mode rejects incompatible extension

- **WHEN** the composed list contains an extension whose
  `engines.vscode` requires a newer VS Code than the installed one, and
  the user runs with `--check-compat=block`
- **THEN** the CLI exits 2, writes the failing IDs to stderr, and does
  not write `.vscode/extensions.json` or invoke `code`.

#### Scenario: Warn mode proceeds with notice (default)

- **WHEN** the same condition holds and the user does not pass
  `--check-compat` (default `warn`) OR explicitly passes
  `--check-compat=warn`
- **THEN** the CLI writes the failing IDs to stderr but the install
  proceeds (file written for `--target=workspace`, `code` invoked for
  the other targets).

#### Scenario: Off mode skips check entirely

- **WHEN** the user passes `--check-compat=off`
- **THEN** the CLI does not invoke
  `scripts/check-extension-compatibility.sh` at all and writes no
  compat-related output.

### Requirement: Dry-run mode

The system SHALL accept a `--dry-run` flag that prints the composed list
and the would-be invocations to stdout without modifying any file or
invoking `code`.

#### Scenario: Dry-run for workspace target

- **WHEN** the user runs `vspcli --detect --target=workspace --dry-run`
- **THEN** stdout contains the composed extension IDs and the would-be
  path of `.vscode/extensions.json`; the file is not created; `code` is
  not invoked.

## MODIFIED Requirements

### Requirement: Profile selection

The system SHALL install extensions from a composed source: either the
layered model (`_shared/extensions/{base,<toolchain>}.json`, see
"Layered extension composition") OR, for backwards compatibility, from
`profiles/<profile>/extensions.json` when invoked via the legacy
`vspcli --install <profile-name>` path. When both are available (e.g.,
the user names a profile that has been collapsed to a flavor), the
layered composition takes precedence and the legacy file is ignored.

#### Scenario: Legacy profile selection

- **WHEN** the user runs `vspcli --install rust-profile-crisp --target=profile rust-profile-crisp`
  during the deprecation window
- **THEN** the legacy name resolves to flavor `rust` plus implied UX
  preset `crisp`, and the composed list (base + rust) is installed into
  the named profile. A deprecation warning is written to stderr.

#### Scenario: All profiles
- **WHEN** the profile name is `all`
- **THEN** the script installs extensions for every profile under `profiles/` and exits non-zero if any profile fails.

#### Scenario: Extensions file missing under legacy name

- **WHEN** the user invokes a legacy profile name that no longer
  resolves (post-deprecation)
- **THEN** the script exits 4 with `unknown profile 'X'; legacy names
  removed in release Y; use --toolchain <flavor> instead` to stderr.

#### Scenario: Extensions file missing for current profile

- **WHEN** the user runs `vspcli --install <flavor>` (current,
  non-legacy name) AND `_shared/extensions/<flavor>.json` is missing
  AND no `profiles/<flavor>/extensions.json` fallback exists
- **THEN** the script exits non-zero, reports the missing layer path,
  and does not invoke `code`. (Preserves the original spec's
  "Extensions file missing" scenario semantics for the layered model.)

### Requirement: Extension grouping

The system SHALL preserve the existing extension group categories
(`AI`, `CMake`, `Java`, `Rust`, `General`) and the `--group <name>`
filter flag for backwards compatibility. Group categorization runs over
the composed list (after layer composition + dedup), not over a
per-profile frozen list.

#### Scenario: Group filter on composed list

- **WHEN** the user runs `vspcli --install --target=profile rust-profile-crisp --group Rust`
- **THEN** only extensions in the `Rust` group from the composed list
  are installed.

#### Scenario: Unknown group warns and skips

- **WHEN** the user passes `--group SomeUnknownGroup`
- **THEN** the script warns to stderr (`Warning: unknown group
  'SomeUnknownGroup' (valid: AI, CMake, Java, Rust, General). Skipping.`)
  and continues with any other group filters. Preserves the original
  "Unknown group" scenario semantics.

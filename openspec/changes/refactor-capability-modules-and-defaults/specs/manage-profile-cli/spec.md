# Delta Specification: Manage Profile CLI

## MODIFIED Requirements

### Requirement: Open profiles in VS Code

The CLI SHALL open a workspace using `code [--user-data-dir <root>] --profile <name> -- <path>` (preventing flag injection from paths beginning with `-` and targeting the resolved VS Code data root) and SHALL require the `code` CLI on PATH.

#### Scenario: Open workspace
- **WHEN** `vspcli --open astro /tmp` or `vspcli --open astro -weird-dir` runs
- **THEN** it invokes `code --profile astro -- /tmp` (or `code --profile astro -- -weird-dir`).

#### Scenario: Code CLI missing
- **WHEN** `code` is not available on PATH
- **THEN** the CLI exits non-zero with a clear error.

### Requirement: Install extensions for a profile

The CLI SHALL parse and validate all CLI flags (`--target`, `--dry-run`, `--group`) before dispatching via `"${BASH}"` to `scripts/install-extensions.sh`, and `vspcli --install-ext` (and `vspcli install-ext`) SHALL enforce `catalog/publishers.json` and `scripts/lib/vsix-pin.sh` before invoking `code`.

#### Scenario: Install group filters
- **WHEN** `vspcli --install java-maven --group Java --group AI` runs
- **THEN** it validates all flags and passes the group arguments to `scripts/install-extensions.sh` for that profile.

#### Scenario: Install single extension
- **WHEN** `vspcli --install-ext rust github.copilot-chat` runs
- **THEN** it validates publisher allowlist and VSIX pins before invoking `code --profile rust --install-extension`, and rejects unallowlisted publishers non-zero.

## ADDED Requirements

### Requirement: Unified CLI verbs and composable flags

`scripts/vspcli` SHALL provide unified verbs (`build`, `apply`, `detect`, `reconcile`, `doctor`, `validate`, `export`, `import`, `completions`, `list`, `open`, `install-ext`) with `--with`/`--without` capability flags while preserving backwards-compatible flags, guarded argument shifts, `"${BASH}"` sub-script dispatch, and dynamic shell completions for bash, zsh, and fish.

#### Scenario: Composable capability flags work across targets
- **WHEN** a user runs `vspcli apply --target=profile my-polyglot --with rust,ts-base --without markdown --ux=retina --dry-run`
- **THEN** `vspcli` resolves the DAG, prints the planned settings and extensions without mutating local VS Code state, and exits 0.

#### Scenario: Missing option value fails cleanly under set -u
- **WHEN** a user invokes `vspcli --ux` or `vspcli --font` with no value
- **THEN** `vspcli` exits with status 4 and a clear error message rather than an unbound-variable Bash crash.

### Requirement: Explicit UX target routing, containment, and user backups

When `--ux` (or `--font`/`--font-size`/`--theme`/`--icon-theme`) is passed with `--target=workspace`, `vspcli` SHALL require `--ux-target=workspace` (`--ux-in-workspace`), `--ux-target=profile:<name>` (`--profile`), or `--ux-target=user` (failing exit `4` if omitted). Workspace writes SHALL refuse symlinks escaping repo root and preserve modes; `<CODE_USER_DIR>` writes SHALL create timestamped `0600` `.bak` backups and guard comment loss.

#### Scenario: Workspace apply with --ux and no --ux-target or escaping symlink fails closed
- **WHEN** a user passes `--target=workspace --ux=crisp` without `--ux-target`, OR `<repo>/.vscode/settings.json` symlinks outside `<repo>`
- **THEN** `vspcli` exits with status `4` and writes zero files.

#### Scenario: User UX apply backs up commented settings and guards comment loss
- **WHEN** `vspcli apply --target=workspace --ux=retina --ux-target=user` targets a `<CODE_USER_DIR>/settings.json` containing comments
- **THEN** it creates a timestamped `0600` `.bak` backup and either preserves comments or exits `4` with an `--allow-comment-loss` hint.

### Requirement: Fail-closed pre-write compatibility gate on workspace installs

`scripts/install-workspace.sh --check-compat` SHALL pass the exact resolved extension list (`--extensions-file`) to `check-extension-compatibility.sh` BEFORE creating `<repo>/.vscode/` or writing XDG/VS Code files. Under `block`, it SHALL exit non-zero with zero writes on incompatible, unknown, missing `code`/version error, missing checker, non-JSON, or partial result sets; under `warn`, it SHALL warn and continue.

#### Scenario: Block mode leaves workspace, XDG, and VS Code untouched on unverifiable or incompatible check
- **WHEN** `check-extension-compatibility.sh` reports an incompatible or unknown extension, fails version lookup, or outputs malformed JSON under `--check-compat=block`
- **THEN** `install-workspace.sh` exits non-zero with a stderr diagnostic and makes zero writes to `.vscode/`, XDG, or `VSCODE_USER_DIR`.

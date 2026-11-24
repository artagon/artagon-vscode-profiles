# Delta Specification: Install Profile Extensions

## MODIFIED Requirements

### Requirement: Profile selection

The system SHALL install extensions from `profiles/<profile>/extensions.json` using consistent `--user-data-dir` when `VSCODE_USER_DIR` is set and exit non-zero when missing; when `<profile>` is `all`, it SHALL propagate `--target`, `--dry-run`, and `--group` flags to every profile under `profiles/` and exit non-zero if any profile fails.

#### Scenario: Extensions file missing
- **WHEN** the profile extensions file does not exist
- **THEN** the script exits non-zero and reports the missing path.

#### Scenario: All profiles
- **WHEN** `scripts/install-extensions.sh all --target=global --dry-run` runs
- **THEN** every profile under `profiles/` runs with `--target=global --dry-run`, zero `code --install-extension` calls execute, and any failing profile causes a non-zero exit.

### Requirement: Extension grouping

The system SHALL resolve each extension's group dynamically from `catalog/extensions.json` (case-insensitively) and default to `General` when unmapped, replacing hardcoded shell case tables.

#### Scenario: Group filter
- **WHEN** `--group AI --group Java` is provided
- **THEN** only extensions mapped to `AI` or `Java` in `catalog/extensions.json` (including `anthropic.claude-code`, `openai.chatgpt`, and `github.copilot-chat`) are installed.

#### Scenario: Unknown group
- **WHEN** an unknown group is provided
- **THEN** the script warns and skips that group.

### Requirement: Installation order and delay

The system SHALL install extensions in deterministic group order (`AI`, `Themes`, `CMake`, `Java`, `Rust`, `Python`, `Web`, `Shell`, `Markdown`, `Quality`, `Git`, `Containers`, `Collaboration`, `General`), enforcing `catalog/publishers.json` and `scripts/lib/vsix-pin.sh`, pausing by `VSCODE_EXTENSION_INSTALL_DELAY`.

#### Scenario: Group ordering
- **WHEN** multiple groups are installed
- **THEN** extensions are installed in the defined catalog group order with VSIX pin verification for pinned universal extensions.

## ADDED Requirements

### Requirement: Safe uninstall mode for catalog entries and pack members

`scripts/install-extensions.sh` SHALL support `--uninstall <id>` gated by `EXTENSION_ID_REGEX` and membership in either top-level `catalog/extensions.json` keys OR validated nested `.packMembers[]` arrays (passing consistent `--user-data-dir` to `code`), so `vspcli reconcile` can remove `compromised`, `removed`, `superseded`, `retired`, and unmanaged extensions including pack children safely.

#### Scenario: Uninstall removes compromised extension and nested pack member
- **WHEN** `scripts/install-extensions.sh rust --uninstall jscearcy.rust-doc-viewer` or `--uninstall openai.codex-audio` runs
- **THEN** both pass catalog+packMembers validation and invoke `code` with `--uninstall-extension` against the resolved `--user-data-dir`.

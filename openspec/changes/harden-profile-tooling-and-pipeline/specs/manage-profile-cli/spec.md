## ADDED Requirements

### Requirement: CLI extension ID validation
The CLI SHALL validate single extension identifiers passed to `--install-ext` against the marketplace ID pattern `^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$` and SHALL exit non-zero without invoking the VS Code CLI when the value fails the pattern.

#### Scenario: Reject malformed identifier
- **WHEN** `vspcli --install-ext rust-profile-retina "evil; rm -rf ~"` runs
- **THEN** the CLI reports the offending value and exits non-zero without invoking `code`

#### Scenario: Accept well-formed identifier
- **WHEN** `vspcli --install-ext rust-profile-retina github.copilot` runs
- **THEN** the CLI invokes `code --profile rust-profile-retina --install-extension github.copilot`

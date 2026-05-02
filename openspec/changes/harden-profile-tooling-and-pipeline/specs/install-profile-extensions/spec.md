## ADDED Requirements

### Requirement: Extension ID validation
The installer SHALL validate every extension identifier extracted from `profiles/<profile>/extensions.json` against the marketplace ID pattern `^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$` before invoking `code --install-extension`, and SHALL exit non-zero when any value fails the pattern.

#### Scenario: Identifier contains shell metacharacters
- **WHEN** an extensions list contains an identifier such as `evil; rm -rf ~`
- **THEN** the installer rejects the identifier, reports the offending value, and exits non-zero without invoking the VS Code CLI

#### Scenario: Well-formed identifier passes
- **WHEN** an identifier matches the publisher.extension pattern (for example `rust-lang.rust-analyzer`)
- **THEN** the installer proceeds with installation

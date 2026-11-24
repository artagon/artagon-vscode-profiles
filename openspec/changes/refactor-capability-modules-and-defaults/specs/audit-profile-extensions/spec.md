# Specification: Audit Profile Extensions

## Purpose

Defines Marketplace and local VS Code compatibility auditing for profile and workspace extension sets, pre-release handling, structured JSON reporting, and exit-code semantics.

## ADDED Requirements

### Requirement: Stable-first Marketplace compatibility verification with pre-release fallback

`scripts/check-extension-compatibility.sh` SHALL accept `--extensions-file <path>`, query Marketplace via `curl -fsS --max-time 30`, select the latest stable release (falling back to pre-release with `"prerelease": true` for pre-release-only extensions), evaluate full semver ranges, emit JSON via `jq --arg` + `jq -s`, and stay Bash 3.2 safe.

#### Scenario: Pre-release Marketplace version does not trigger false incompatibility
- **WHEN** an extension publishes a pre-release requiring VS Code `^1.148.0` alongside a stable release compatible with local VS Code `1.147.0`
- **THEN** `check-extension-compatibility.sh --marketplace-only --json` evaluates the latest stable release and reports `compatible`.

#### Scenario: Pre-release-only extension falls back cleanly and --extensions-file audits arbitrary sets
- **WHEN** `check-extension-compatibility.sh --extensions-file <list> --marketplace-only --json` inspects a pre-release-only extension (e.g., `astral-sh.ty`) compatible with local VS Code `1.147.0`
- **THEN** it audits every ID in `<list>`, evaluates the latest pre-release with `"prerelease": true`, and reports `compatible`.

### Requirement: Compatibility audit exit code contract

`scripts/check-extension-compatibility.sh` SHALL exit `0` when all audited extensions are compatible, `1` when one or more extensions are incompatible or `NOT_FOUND`, and `2` on usage errors, missing tools, or failed VS Code version discovery.

#### Scenario: Usage errors and findings return distinct non-zero codes
- **WHEN** `check-extension-compatibility.sh` receives an invalid flag vs an incompatible extension response
- **THEN** invalid usage exits `2` while compatibility findings exit `1` with valid JSON on stdout under `--json`.

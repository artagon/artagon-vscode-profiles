# Delta Specification: Validate Profile JSON

## RENAMED Requirements

- FROM: `### Requirement: Validate shared, override, and extension files`
- TO: `### Requirement: Validate catalog, module, profile, merged, and export files`

## MODIFIED Requirements

### Requirement: Validate catalog, module, profile, merged, and export files

The system SHALL validate strict `.json` and `.code-profile` files directly with `jq -e -s` and `.jsonc` files via `LC_ALL=C awk -f scripts/lib/jsonc-strip.awk` + `jq -e -s` across `catalog/`, `modules/`, `profiles/`, `_merged/`, and `exports/*.code-profile`, enforcing single-document JSON validity with the expected root type per artifact (object for settings/manifests/bundles, array for `profiles/*/extensions.json`).

#### Scenario: Validation succeeds
- **WHEN** all discovered files under `catalog/`, `modules/`, `profiles/`, `_merged/`, and `exports/` parse cleanly as single valid JSON documents of their expected root type
- **THEN** the validator exits successfully and reports the number of files checked.

#### Scenario: Validation fails
- **WHEN** any file fails to parse (including malformed JSONC `{"limit":1/*x*/2}` or `{"list":[,]}` or multi-document `{}{}` or comments inside strict `.json`)
- **THEN** the validator reports the failing file path and exits with a non-zero status.

## ADDED Requirements

### Requirement: Catalog, module, schema-scope, and parsed-config security validation

`scripts/validate-json.sh` (and `vspcli validate`) SHALL enforce active-module publisher parity, catalog prefix and pre-v2 bundle completeness, DAG acyclicity and valid conflicts, jointly-selectable `jq` 1.7.1 `scalar_leaves` + object-vs-leaf collision checks, `catalog/setting-scopes.json` type/enum/scope checks, tracked forbidden paths, parsed-config autonomy deny-list, `exports/SHA256SUMS`, broken symlinks, and `apply-rust-policy.sh --check`.

#### Scenario: Parsed-config autonomy deny-list rejects unsafe settings without tripping on docs
- **WHEN** an explanatory Markdown spec mentions `bypassPermissions` while a module or recipe JSON sets `chat.tools.terminal.autoApprove: true` or `defaultMode: "bypassPermissions"`
- **THEN** `scripts/validate-json.sh` allows the explanatory prose and fails exit `1` on the offending JSON config key/value.

#### Scenario: Schema-scope fixture and baseline layout invariants are enforced
- **WHEN** `scripts/validate-json.sh` validates modules, `_merged/*.json`, and `exports/*.code-profile`
- **THEN** wrong setting types/enums or non-workspace scopes in `settings.workspace.jsonc` fail exit `1`, mutually exclusive UX presets validate cleanly, and every compiled profile carries `workbench.sideBar.location == "left"`, `security.workspace.trust.untrustedFiles == "prompt"`, `telemetry.telemetryLevel == "off"`, `editor.hover.enabled == "on"`, and zero `"$schema"` or `"@extends"` keys.

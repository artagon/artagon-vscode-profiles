# Specification: Extension Catalog, Supply Chain & Lifecycle Reconciliation

## Purpose

Establishes `catalog/` as the single source of truth for extension lifecycle metadata, least-privilege publisher allowlists, install-time universal VSIX SHA-256 pins, default capabilities, compromised-first `vspcli reconcile`, and `vspcli doctor` diagnostics.

## ADDED Requirements

### Requirement: Unified extension catalog with five lifecycle statuses

`catalog/extensions.json` SHALL store lowercase canonical IDs with `publisher` matching the ID prefix, `group`, `universal`, optional `themeLabels` and `packMembers`, and `status` in `active`, `superseded` (with `replacedBy`), `removed`, `compromised`, or `retired`. Every active module extension MUST be `active`, every `active` row MUST be referenced by a module, and `catalog/publishers.json` MUST equal the exact `LC_ALL=C` sorted active publisher set.

#### Scenario: Compromised and removed extensions are excluded from active modules
- **WHEN** `catalog/extensions.json` is validated
- **THEN** `jscearcy.rust-doc-viewer` has `status == "compromised"`, the five Marketplace 404 IDs have `status` in `["removed", "superseded"]`, none appear in active module install lists, and `jscearcy` is absent from `catalog/publishers.json`.

#### Scenario: Publisher drift or orphan active catalog row fails validation
- **WHEN** `catalog/publishers.json` omits `openai` or contains an unused publisher, or `catalog/extensions.json` has an unreferenced `active` row or publisher prefix mismatch
- **THEN** `vspcli validate` exits non-zero reporting the exact discrepancy.

### Requirement: Case-insensitive fail-closed install-time VSIX pinning

`scripts/lib/vsix-pin.sh` SHALL store lowercase unique pin keys in `catalog/extension-pins.json`, match IDs case-insensitively, require `https://` official Marketplace hosts and 64-hex `sha256`, fetch via `curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --max-time 60` (relaxing `--proto` only when `ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1` for offline tests) into a private `mktemp`, support `pinRequired`, and fail closed across all install paths.

#### Scenario: Six universal core and UX extensions ship with verified install-time pins
- **WHEN** `catalog/extension-pins.json` is validated
- **THEN** it contains lowercase pins with 64-hex SHA-256 digests for `catppuccin.catppuccin-vsc`, `catppuccin.catppuccin-vsc-icons`, `enkia.tokyo-night`, `pkief.material-icon-theme`, `editorconfig.editorconfig`, and `tamasfe.even-better-toml`.

#### Scenario: Mixed-case extension ID resolves pin and mismatch aborts closed
- **WHEN** a manifest contains `Catppuccin.catppuccin-vsc` and the fetched VSIX digest mismatches the pinned `sha256`
- **THEN** `vsix-pin.sh` matches the pin case-insensitively and aborts non-zero without invoking `code --install-extension` on the live Marketplace.

### Requirement: Compromised-first profile reconciliation (`vspcli reconcile`)

`vspcli reconcile --profile <target> [--as <recipe>] [--dry-run] [--compromised-only] [--prune-retired] [--prune-unmanaged]` SHALL target the resolved `--user-data-dir`, abort on list error, compute the full ordered plan first (`--dry-run` mutates nothing), uninstall `compromised` entries unconditionally first, install missing active entries, then uninstall `removed`, verified `superseded`, opt-in `retired`, and opt-in unmanaged plus non-retained `packMembers[]`.

#### Scenario: Dry-run emits ordered plan with zero mutations including compromised extensions
- **WHEN** profile `rust` has `jscearcy.rust-doc-viewer` installed and is missing `openai.chatgpt` and runs `vspcli reconcile --profile rust --dry-run`
- **THEN** it prints the planned Step 0 uninstall and Step 1 install and makes zero `code --uninstall-extension` or `--install-extension` calls.

#### Scenario: Compromised extension is removed even if subsequent install fails
- **WHEN** profile `rust` has `jscearcy.rust-doc-viewer` installed and installing a missing extension fails due to network error
- **THEN** `vspcli reconcile --profile rust` uninstalls `jscearcy.rust-doc-viewer` first before the install step fails, leaving GlassWorm absent.

#### Scenario: Reconciling a seeded profile against ai-copilot removes Claude, Codex, and codex-audio
- **WHEN** a profile `dev` seeded with `anthropic.claude-code`, `openai.chatgpt`, `openai.codex-audio`, and `github.copilot-chat` runs `vspcli reconcile --profile dev --as ai-copilot --prune-unmanaged`
- **THEN** `anthropic.claude-code`, `openai.chatgpt`, and `openai.codex-audio` are uninstalled (skipping any pack member still required by a retained parent and staying idempotent if already removed) while `github.copilot-chat` remains installed.

#### Scenario: Reconcile --all operates only on profiles present in both profiles/ and storage.json
- **WHEN** `vspcli reconcile --all` runs against a specific `VSCODE_USER_DIR` where only `rust` and `python` are registered
- **THEN** only `rust` and `python` are reconciled using matching `--user-data-dir` invocations.

### Requirement: Read-only environment, compromised, and application-scope diagnostics (`vspcli doctor`)

`vspcli doctor [--strict]` SHALL perform zero mutations, exit `1` on `FAIL` (missing `bash`, `jq`, `shasum` OR `sha256sum`, or `curl`; invalid catalog/modules; publisher drift; broken symlinks; forbidden tracked paths; parsed-config autonomy deny-list hits; or ANY installed `compromised` extension in ANY profile including Default), and exit `0` (`1` under `--strict`) on `WARN` (missing `code`, profile drift, missing RTK wrappers, pinned version drift, or Default-profile application-scope drift).

#### Scenario: Doctor fails on compromised extension in Default profile and warns on application-scope drift
- **WHEN** the Default profile has `jscearcy.rust-doc-viewer` installed or sets `telemetry.telemetryLevel` to `"all"`
- **THEN** `vspcli doctor` emits `FAIL` (exit `1`) for `jscearcy.rust-doc-viewer` and `WARN` (exit `0`, or `1` with `--strict`) for application-scope telemetry/trust/autoUpdate drift without modifying any file.

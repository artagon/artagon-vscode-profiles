## Why

The repo's existing extension-install supply-chain story stops at
`EXTENSION_ID_REGEX` (rejects shell metacharacters in IDs) and a
marketplace pre-flight check. Three gaps remain:

1. **Bundle integrity.** `exports/*.code-profile` files ship as plain
   JSON. A bundle modified in-flight or in storage (CDN, mirror, repo
   fork) would be parsed and installed without warning.
2. **Publisher provenance.** `validate_extension_id` accepts any
   `<publisher>.<name>` shape. A typosquatted publisher
   (`microsfot.python`, `gitub.copilot`) passes the gate today.
3. **Extension version drift.** `code --install-extension <id>` always
   resolves the latest marketplace build. A compromised or rolled-back
   marketplace publish lands silently. There is no version pinning and
   no per-version content hash.

This change closes all three with the cheapest mitigation that
actually defends against the threat.

## What Changes

### Bundle SHA256SUMS

- `scripts/export-profiles.sh` writes `exports/SHA256SUMS` after each
  full export run: one line per `.code-profile`, GNU coreutils format
  (`<sha256>  <basename>`), sorted by basename, no symlink entries.
- `scripts/import-profile.sh` looks for `<bundle-dir>/SHA256SUMS` (the
  directory the user's bundle path lives in). When present and the
  bundle is listed: verify; refuse to import on mismatch with a clear
  error. When absent or the bundle isn't listed: proceed with a
  one-line stderr warning so the unverified path remains usable in
  ad-hoc scenarios but visibly degraded.

### Publisher allowlist

- `scripts/lib/extension-id.sh` gains `ALLOWED_PUBLISHERS` (a `case`
  pattern, bash 3.2 safe — see profile-graceful-defaults precedent)
  covering every publisher referenced by this repo's
  `profiles/*/extensions.json`. `validate_extension_id` now also
  rejects extensions whose publisher segment is not in the list.
- `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` lets a developer override
  per-process to add a new extension before updating the allowlist.
  The override emits a one-time stderr notice naming the rejected
  publisher and the env var.

### VSIX version + sha256 pinning

- New `_catalog/extension-pins.json` schema:
  `{ "<publisher>.<name>": { "version": "1.2.3", "sha256": "<hex>", "vsix_url": "<https-url>" } }`.
  Initial population covers the project's theme/icon-theme extensions
  (the most likely targets of a marketplace incident, given they
  ship in every bundle).
- New `scripts/lib/vsix-pin.sh`: `lookup_pin <id>` /
  `fetch_and_verify_vsix <id> <out_dir>`. Uses `curl` (already a
  dependency on every shipped CI worker) and `shasum -a 256`
  (BSD/GNU portable).
- `scripts/import-profile.sh` and `scripts/install-extensions.sh`
  consult the pin manifest before each `code --install-extension`:
  - **Pin present**: download VSIX → verify sha256 → install offline
    via `code --install-extension <vsix-path>`. On mismatch, refuse
    install (NOT fall back to live marketplace) and add the ID to
    the failure list (so the graceful-defaults rewrite still kicks
    in for theme failures). On HTTP/network failure, also refuse —
    network errors aren't safe to silently downgrade.
  - **No pin**: install via the existing live-marketplace path.
- `ARTAGON_VSCODE_BYPASS_PINS=1` skips the pin path entirely (live
  marketplace for everything). Emits one-time stderr notice.

### Composer-side guards

- `scripts/check-extension-compatibility.sh` and `scripts/install-extensions.sh`
  call `validate_extension_id` on every ID they hand to `code` — the
  publisher allowlist closes silently on existing call sites without
  schema drift.

## Capabilities

### New Capabilities

(none — extends existing extension-install and import paths)

### Modified Capabilities

- `extension-install-supply-chain`: bundle integrity verification,
  publisher allowlist enforcement, and per-extension VSIX pinning.

## Impact

- **Code**: `scripts/export-profiles.sh`, `scripts/import-profile.sh`,
  `scripts/install-extensions.sh`, `scripts/lib/extension-id.sh`,
  `scripts/lib/vsix-pin.sh` (new), `_catalog/extension-pins.json` (new).
- **APIs**: existing CLI surfaces unchanged. Two new env-var escape
  hatches (`ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER`,
  `ARTAGON_VSCODE_BYPASS_PINS`).
- **Dependencies**: adds `curl` (typically present; documented
  failure if absent) and `shasum -a 256` / `sha256sum` (one or the
  other present on macOS/Linux). No new install-time deps for the
  user.
- **Backwards compatibility**:
  - Bundle SHA256SUMS: importer warns-and-proceeds when SHA256SUMS is
    absent → fully backward compatible. New bundles ship with the
    file; existing bundles in the wild import unchanged.
  - Publisher allowlist: gates installs on a curated list. Any
    extension whose publisher isn't in the list (today: none in this
    repo's profiles, by construction) is rejected. Developers adding
    a NEW publisher must update `ALLOWED_PUBLISHERS` in the same PR
    that adds it to `profiles/*/extensions.json`. Override env-var
    available for the install-once-then-add-to-list workflow.
  - VSIX pinning: opt-in per extension. Pin manifest starts with the
    9 theme/icon-theme extensions; rest of the catalog uses the live
    marketplace path. No behavior change for unpinned IDs.
- **Risk**: medium. The pinning path introduces a new install
  modality. Mitigated by a strict allow-or-refuse failure mode (no
  silent fallback to live marketplace on hash mismatch — that would
  defeat the point) and an explicit bypass env var.

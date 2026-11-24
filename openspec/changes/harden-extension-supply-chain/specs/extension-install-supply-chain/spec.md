## ADDED Requirements

### Requirement: Bundle integrity manifest

The system SHALL emit `exports/SHA256SUMS` at the end of every full
`export-profiles.sh` run. The manifest SHALL list the SHA-256 of every
non-symlink `.code-profile` in `exports/`, in GNU coreutils format
(`<sha256>  <basename>`, two spaces, sorted by basename), and SHALL
NOT list symlink entries.

#### Scenario: SHA256SUMS produced after export

- **WHEN** `export-profiles.sh` finishes a full export pass
- **THEN** `exports/SHA256SUMS` exists and lists every non-symlink
  `*.code-profile` in `exports/`, sorted, in GNU coreutils format.

#### Scenario: SHA256SUMS atomic write

- **WHEN** the SHA256SUMS file is being rewritten
- **THEN** writes go to a sibling `SHA256SUMS.tmp.<pid>` first
- **AND** are atomically `mv`d into place on success
- **AND** on failure the temp file is removed and the previous
  SHA256SUMS (if any) is left untouched.

### Requirement: Bundle integrity verification

The system SHALL look for `<bundle-dir>/SHA256SUMS` when
`import-profile.sh` runs, where `<bundle-dir>` is the directory
containing the bundle path the user passed. When the file exists and
the bundle's basename appears in it, the system SHALL verify the
sha256 of the bundle and refuse to import on mismatch. When the file
is absent or the bundle is not listed, the system SHALL emit a
one-line stderr warning and proceed.

#### Scenario: Bundle hash matches manifest

- **WHEN** `<bundle-dir>/SHA256SUMS` exists and the bundle's hash
  matches the listed value
- **THEN** import proceeds normally with no extra output.

#### Scenario: Bundle hash mismatches manifest

- **WHEN** `<bundle-dir>/SHA256SUMS` exists, the bundle's basename is
  listed, and the computed hash does not match the listed value
- **THEN** the script writes an error to stderr including the
  expected and computed hashes
- **AND** exits non-zero before any write to the profile cache.

#### Scenario: SHA256SUMS absent

- **WHEN** `<bundle-dir>/SHA256SUMS` does not exist
- **THEN** the script emits a one-line stderr warning
  (`import-profile: SHA256SUMS not found; bundle integrity unverified`)
- **AND** import proceeds.

#### Scenario: Bundle not listed in SHA256SUMS

- **WHEN** SHA256SUMS exists but the bundle's basename is not listed
- **THEN** the script emits a one-line stderr warning
  (`import-profile: <basename> not listed in SHA256SUMS; integrity unverified`)
- **AND** import proceeds.

### Requirement: Publisher allowlist

The system SHALL refuse to install an extension whose publisher
segment (`<publisher>.<name>` → `<publisher>`) is not in
`scripts/lib/extension-id.sh`'s `ALLOWED_PUBLISHERS` allow-pattern.
The allowlist SHALL cover every publisher referenced by this repo's
`profiles/*/extensions.json` files.

#### Scenario: Known publisher accepted

- **WHEN** an extension ID's publisher matches the allowlist
- **THEN** `validate_extension_id` returns success.

#### Scenario: Unknown publisher rejected

- **WHEN** an extension ID's publisher does not match the allowlist
  and `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER` is unset or empty
- **THEN** `validate_extension_id` writes an error to stderr naming
  the publisher and the override env var, and returns non-zero
- **AND** the install loop adds the rejected ID to the failure list
  rather than passing it to `code --install-extension`.

#### Scenario: Override env var bypasses allowlist

- **WHEN** an extension ID's publisher does not match the allowlist
  and `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` is set
- **THEN** `validate_extension_id` emits a one-time stderr notice
  (`accepting unknown publisher <publisher> (ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1)`)
- **AND** returns success.

### Requirement: VSIX version + sha256 pinning

The system SHALL consult `_catalog/extension-pins.json` before each
`code --install-extension` call. When a pin entry exists for the ID,
the system SHALL fetch the VSIX from the pin's `vsix_url`, verify
its SHA-256 matches the pin's recorded `sha256`, and install via
`code --install-extension <vsix-path>`. On mismatch or fetch failure,
the system SHALL refuse the install and add the ID to the failure
list.

#### Scenario: Pin entry present and hash matches

- **WHEN** `_catalog/extension-pins.json` lists the extension ID and
  the fetched VSIX's sha256 matches the pin
- **THEN** the install proceeds via `code --install-extension <vsix-path>`
- **AND** the live marketplace is not contacted.

#### Scenario: Pin entry hash mismatches

- **WHEN** the fetched VSIX's sha256 does not match the pin
- **THEN** the script writes an error to stderr including the
  expected and computed hashes and the URL
- **AND** the ID is added to the failure list
- **AND** the live marketplace is NOT contacted as a fallback.

#### Scenario: VSIX fetch fails

- **WHEN** `curl` returns non-zero (network error, 404, redirect
  limit, etc.) for a pinned extension
- **THEN** the script writes an error to stderr including the URL
  and exit code
- **AND** the ID is added to the failure list
- **AND** the live marketplace is NOT contacted as a fallback.

#### Scenario: No pin entry

- **WHEN** an extension ID is not listed in `extension-pins.json`
- **THEN** the install proceeds via the existing live-marketplace
  path (`code --install-extension <id>`).

#### Scenario: Bypass env var

- **WHEN** `ARTAGON_VSCODE_BYPASS_PINS=1` is set
- **THEN** the script emits a one-time stderr notice
  (`bypassing pin manifest (ARTAGON_VSCODE_BYPASS_PINS=1)`)
- **AND** every install uses the live-marketplace path regardless
  of pin manifest entries.

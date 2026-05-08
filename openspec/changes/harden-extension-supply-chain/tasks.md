## 1. Bundle SHA256SUMS

- [x] 1.1 In `scripts/export-profiles.sh`, after the export loop, build
  `exports/SHA256SUMS` listing every non-symlink `*.code-profile` in
  GNU coreutils format. Detect the available hasher: `sha256sum`
  (Linux) or `shasum -a 256` (macOS). Sort by basename. Atomic write
  via `SHA256SUMS.tmp.$$` then `mv -f`. Skip symlink bundles (they
  point at the canonical entry; only the target needs a hash).
- [x] 1.2 In `scripts/import-profile.sh`, before parsing the bundle,
  look up `<dirname-of-bundle>/SHA256SUMS`. If absent, warn-and-proceed.
  If present and the bundle's basename appears in it, verify; refuse
  on mismatch. If present but unlisted, warn-and-proceed (likely a
  user-built bundle outside the export pipeline).
- [ ] 1.3 Document the warn-vs-fail behavior in a top-of-script
  comment. Add the same hasher detection used in §1.1; abort with a
  clear error if neither is available.
- [x] 1.4 Shellcheck: `shellcheck -x scripts/export-profiles.sh
  scripts/import-profile.sh`. Resolve all findings.
- [ ] 1.5 Smoke test: run `bash scripts/export-profiles.sh`, verify
  `exports/SHA256SUMS` exists, has entries for every non-symlink
  bundle, sorted, in GNU coreutils format. Run `sha256sum -c
  exports/SHA256SUMS` (or `shasum -a 256 -c`) and confirm it
  succeeds. Tamper one byte in a bundle and re-verify; confirm the
  importer refuses.

## 2. Publisher allowlist

- [x] 2.1 In `scripts/lib/extension-id.sh`, add an
  `is_allowed_publisher(publisher)` function backed by a `case`
  statement. Cover every publisher in `profiles/*/extensions.json`:
  audit by `jq -r '[.[].identifier.id | split(".")[0]] | unique' profiles/*/extensions.json | sort -u`.
- [x] 2.2 Update `validate_extension_id` to also call
  `is_allowed_publisher` after the regex check. On rejection, write
  a stderr message naming the publisher and pointing at
  `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1`.
- [x] 2.3 Implement the override: when
  `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` is set, accept unknown
  publishers but emit a one-time-per-process stderr notice. Use a
  shell variable guard (e.g., `_artagon_unknown_pub_warned`) to
  enforce one-time semantics.
- [x] 2.4 Shellcheck: `shellcheck -x scripts/lib/extension-id.sh`.
- [ ] 2.5 Run `bash scripts/tests/run.sh` and confirm the existing
  extension-id allowlist tests still pass. Add a new assertion
  covering rejection of `microsfot.python` (typosquat) and
  acceptance of `microsoft.python`.

## 3. VSIX version + sha256 pinning

- [ ] 3.1 Create `_catalog/extension-pins.json` with initial entries
  for the 9 theme/icon-theme extensions from spec §53-58: the IDs,
  the latest stable version available at change-merge time, the
  marketplace VSIX URL, and the sha256 of the fetched VSIX.
  Resolve URLs and hashes manually (one-time bootstrap; CI workflow
  is a follow-up).
- [x] 3.2 Create `scripts/lib/vsix-pin.sh`:
  - `lookup_pin <id>` — prints the pin record's `version`, `sha256`,
    `vsix_url` (newline-separated) to stdout if the ID is pinned;
    nothing if not. Uses `jq` (already a hard dep).
  - `fetch_and_verify_vsix <id> <out_dir>` — fetches via `curl
    -fsSL` to a sibling tempfile in `<out_dir>`, computes sha256 via
    the §1.1 hasher, compares to the pin, atomically renames to
    `<out_dir>/<id>.vsix` on success. On any failure, removes the
    tempfile and returns non-zero.
  - Both functions check for `curl` and the hasher up-front.
- [x] 3.3 In `scripts/import-profile.sh`'s install loop, after
  `validate_extension_id` and before `code --install-extension <id>`:
  call `lookup_pin "$ext"`. If pinned: `fetch_and_verify_vsix`; if
  that succeeds, `code --install-extension <vsix-path>`; if it
  fails, add `$ext` to `FAILED_EXT` and continue (do NOT fall back
  to live marketplace).
- [x] 3.4 Same wiring in `scripts/install-extensions.sh`'s install
  loop.
- [x] 3.5 Implement `ARTAGON_VSCODE_BYPASS_PINS=1`: when set, skip
  `lookup_pin` entirely. One-time stderr notice.
- [x] 3.6 Shellcheck on every modified file.
- [ ] 3.7 Smoke test: run import against a pinned theme extension;
  confirm the .vsix is downloaded and sha256-verified before
  install. Tamper the pin's sha256; re-run; confirm the install
  fails closed (no live-marketplace fallback).

## 4. Documentation

- [ ] 4.1 Append a "Supply chain" subsection to `README.md`
  documenting the three mechanisms and the bypass env vars.
  Two paragraphs, no more.
- [ ] 4.2 Add a one-line comment to `_catalog/extension-pins.json`'s
  intro (or a sibling `_catalog/README.md`) describing the schema
  and how to refresh entries.

## 5. Validate

- [ ] 5.1 `openspec validate harden-extension-supply-chain --strict`.
- [ ] 5.2 `bash scripts/tests/run.sh` passes including new assertions.
- [ ] 5.3 Manual end-to-end: export → tamper bundle → import refuses;
  rebuild → import succeeds. Pin a theme → install via pinned path;
  set `ARTAGON_VSCODE_BYPASS_PINS=1` → live marketplace path used.

## 6. Future enhancements (not in scope)

- CI workflow that detects when a marketplace publish updates a
  pinned extension's version and opens a PR with the new sha256.
- GPG/cosign signing of `SHA256SUMS`.
- Mirror VSIXs to GitHub Release assets to remove the marketplace
  dependency entirely.

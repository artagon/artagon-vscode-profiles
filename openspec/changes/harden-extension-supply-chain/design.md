## Context

This repo's profile bundles are downloaded from raw.githubusercontent
and imported via a shell script that calls `code --install-extension <id>`.
Three known-bad failure modes exist:

1. Bundle tampering between export and import.
2. Typosquatted publishers (`microsfot.python`).
3. Marketplace publish-chain compromise (a real, observed industry
   incident class — extension hijacks via stolen publisher tokens).

The mitigations below trade implementation cost for blast-radius
reduction. Each is independently shippable; they compose.

## Goals / Non-Goals

**Goals:**

- Detect bundle tampering between `export-profiles.sh` and
  `import-profile.sh` runs.
- Refuse extensions whose publisher segment isn't on a curated list.
- For high-risk extensions (themes ship in every bundle, large blast
  radius if compromised), pin to a specific version and verify
  sha256 before install.
- Keep the existing happy-path scripts working unchanged when the
  new files are absent (forward compatibility for users on older
  bundles).

**Non-Goals:**

- Rewriting `code --install-extension` semantics. We layer on top.
- Auditing every transitive dependency of every extension. The pin
  manifest covers the extensions this repo ships, period.
- Verifying bundle JSON schema beyond what `import-profile.sh`
  already does.
- A self-updating pin manifest (CI workflow) — that's a follow-up.

## Decisions

### Decision 1: GNU coreutils SHA256SUMS format

**Choice**: `<sha256>  <basename>` per line, two spaces, sorted by
basename. Match `sha256sum --binary` (Linux) / `shasum -a 256 -b`
(macOS). One file per export run, named `SHA256SUMS` in `exports/`.

**Rationale**: portable, parseable by `sha256sum -c` / `shasum -a 256 -c`
without custom tooling. Sort order eliminates spurious diffs.

**Alternatives considered**:

- *Per-bundle `<bundle>.sha256` sidecar files*: more files, same
  semantic info. Harder to verify in one pass.
- *Embed sha256 in a JSON manifest*: nicer for tooling but loses
  the "drop into any sha256sum-aware verifier" property.

### Decision 2: Importer warns-and-proceeds on missing SHA256SUMS

**Choice**: when `<bundle-dir>/SHA256SUMS` is absent or the bundle's
basename isn't listed, emit a one-line stderr warning
(`import-profile: SHA256SUMS not found; bundle integrity unverified`)
and proceed.

**Rationale**: existing bundles in the wild don't ship a SHA256SUMS
file. Hard-failing would break every install today. Warn-and-proceed
preserves backward compat while making the unverified path visibly
degraded.

**Alternatives considered**:

- *Hard-fail*: cleanest security stance but breaks every existing
  user. Reject.
- *Silent bypass*: defeats the audit trail. Reject.
- *Opt-in via env var*: `ARTAGON_VSCODE_REQUIRE_SHASUM=1` — possible
  follow-up, but the warning + opt-in fits a wider class of users.

### Decision 3: Publisher allowlist as a case statement

**Choice**: `case "${publisher}" in <patterns>) return 0;; *) return 1;; esac`
in `lib/extension-id.sh`. Same bash 3.2 idiom used by
profile-graceful-defaults' theme-label map.

**Rationale**: bash 3.2 (macOS /bin/bash) lacks associative arrays.
`case` is the project's documented portable pattern.

**Alternatives considered**:

- *Associative array*: bash 4+ only.
- *External `_catalog/publishers.json`*: more files; worse for diff
  hygiene; harder to grep for "what publishers are trusted today."
- *Generate from `profiles/*/extensions.json`*: tempting, but the
  allowlist should be reviewed by humans, not autoderived from
  whatever we last shipped.

### Decision 4: Override env var for unknown publisher

**Choice**: `ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1` lets a
developer trying out a new extension proceed without first updating
the allowlist. Emits a one-time-per-process stderr notice naming the
publisher.

**Rationale**: hard-failing on an unknown publisher breaks the
exploratory developer workflow. The env var preserves the audit
trail (the warning is loud) without forcing a documentation edit
for every "let me try this new linter" moment.

**Alternatives considered**:

- *No override, force allowlist edit per try*: too friction-y; users
  will hack around the gate by editing `lib/extension-id.sh`
  temporarily and forgetting to revert.

### Decision 5: VSIX pinning is opt-in per extension

**Choice**: pin manifest is sparse. Only listed extensions go through
the fetch-verify-install path. Unlisted extensions install via the
live marketplace path (today's behavior).

**Rationale**: the operational cost of pinning is non-trivial — a
new minor extension version requires a manifest update or every
install fails. Pinning everything would multiply that cost by the
number of extensions in the catalog (40+). Pinning the highest-blast
extensions (themes, ship in every bundle) is 80/20.

**Alternatives considered**:

- *Pin everything*: high churn, high false-positive rate (every
  patch release breaks installs).
- *Pin nothing, rely on signature verification*: VS Code's signature
  verification is publisher-controlled and not all themes are signed.

### Decision 6: Pin mismatch fails closed (no live-marketplace fallback)

**Choice**: if a pin is set and the SHA256 doesn't match, the install
fails. The script does NOT fall back to fetching the live
marketplace version.

**Rationale**: silent fallback defeats the entire pinning mechanism.
If the marketplace and the pinned version diverge, the user has been
told a specific version+hash is what they want; serving them
something else is a security regression masquerading as resilience.

**Mitigation for legitimate version bumps**:
`ARTAGON_VSCODE_BYPASS_PINS=1` (per-process) lets a user skip pinning
when they consciously want to update. The expected workflow is:
unset pin → install latest → record new hash → commit pin manifest.

**Alternatives considered**:

- *Soft-fall to live marketplace on mismatch*: silently undermines
  the security guarantee. Reject.
- *Fall back AND mark the install failed*: confusing UX; either it
  installed or it didn't.

### Decision 7: Pin manifest stores `vsix_url`, not `<id>@<version>`

**Choice**: store the full marketplace VSIX download URL (e.g.,
`https://<publisher>.gallery.vsassets.io/_apis/public/gallery/publisher/<publisher>/extension/<name>/<version>/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage`).
Not just a version string.

**Rationale**: `code --install-extension <id>@<version>` works in
recent VS Code versions but does not provide a content-addressable
fetch — it still goes through the marketplace's resolver. Storing
the direct URL plus the expected sha256 lets us fetch from a known
location and verify. If the URL 404s, the install fails (intended);
the user updates the manifest.

**Alternatives considered**:

- *Just version string*: relies on `code` CLI version-pinning, no
  hash verification possible from the shell.
- *Mirror to repo / GitHub Release*: a follow-up. Self-hosting VSIXs
  in a release asset would close the marketplace dependency entirely
  but adds storage cost and a release workflow.

## Risks / Trade-offs

- **[Pin manifest goes stale]** Theme extensions release minor
  versions; the pin doesn't update; installs fail until someone
  updates the manifest. → Mitigation: `ARTAGON_VSCODE_BYPASS_PINS=1`
  for the user who needs to install today; CI workflow (follow-up)
  to detect drift and open a PR.
- **[curl absent]** macOS ships curl; Linux usually has it. Some
  minimal CI containers don't. → Mitigation: `lib/vsix-pin.sh` checks
  for curl at function entry and fails clearly with the env var
  bypass option.
- **[VSIX URL format change]** The marketplace gallery URL pattern
  could change. → Mitigation: the URL is stored in the manifest, so
  changes only affect new pin entries. Old pins keep working.
- **[Allowlist false rejections]** A legitimate new publisher gets
  rejected. → Mitigation: env-var override + clear error message
  pointing at the file to edit.
- **[SHA256SUMS partial-write]** A failed export run could leave
  SHA256SUMS in an inconsistent state. → Mitigation: write to
  `SHA256SUMS.tmp.$$` then `mv -f`, matching the atomic-write pattern
  used by `compose-settings.sh:144-150` and the new
  `rewrite_settings_for_failed_themes` in import-profile.sh.

## Migration Plan

1. Add `lib/extension-id.sh` ALLOWED_PUBLISHERS + validate_publisher.
   Wire into `validate_extension_id`. Test against every
   `profiles/*/extensions.json` (existing test surface).
2. Add SHA256SUMS write to `export-profiles.sh`. Add SHA256SUMS read
   + verify to `import-profile.sh`. Test with a tampered fixture.
3. Add `lib/vsix-pin.sh` and `_catalog/extension-pins.json`. Wire
   into `import-profile.sh` install loop. Initial pins cover the 9
   theme/icon-theme extension IDs.
4. Wire the pin path into `install-extensions.sh` (same pattern).
5. Documentation: `README.md` adds a "Supply chain" subsection
   describing the three mechanisms and the bypass env vars. Brief.

**Rollback**: each mechanism is independent.
- Remove SHA256SUMS write/read calls; importer's "warn-and-proceed"
  path means existing bundles still work.
- Remove `validate_publisher` call from `validate_extension_id`;
  installs revert to ID-shape gating only.
- Remove pin lookup from install loops; installs revert to live
  marketplace.

## Open Questions

- *Should `install-extensions.sh --skip-install` also skip pin
  verification?* Resolved: no. The flag is "don't run install"; it
  bypasses the install loop entirely, so pin verification never runs.
  No special handling needed.
- *Should we sign SHA256SUMS itself?* Out of scope. Adding GPG/cosign
  is a meaningful follow-up for users in higher-trust environments;
  it's listed in tasks.md as a future enhancement, not implemented.

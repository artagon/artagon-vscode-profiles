## MODIFIED Requirements

### Requirement: Export bundle per profile

The system SHALL create `exports/<profile>.code-profile` for each
toolchain-flavor directory under `profiles/` (after the catalog
migration: 11 directories, one per flavor). The bundle SHALL contain
`settings` from `_merged/<profile>.json` AND `extensions.enabled` as a
list of extension IDs from `profiles/<profile>/extensions.json`.

After the catalog migration (per the `manage-profile-cli` capability's
"Catalog migration command" requirement), the legacy `*-crisp` and
`*-retina` bundle filenames are NOT regenerated as standalone files;
instead, `vspcli --migrate-catalog` replaces each legacy bundle file
with a symlink to the corresponding new `exports/<flavor>.code-profile`
per design.md Decision 18. The symlinks remain throughout the
deprecation window so external bookmarks and README's
`raw.githubusercontent.com/.../<flavor>-crisp.code-profile` URLs
continue to resolve. The release after the deprecation window,
`vspcli --migrate-catalog --finalize` removes the symlinks.

#### Scenario: Export includes settings and extensions (post-migration)

- **WHEN** export runs for flavor `rust` and both
  `_merged/rust.json` and `profiles/rust/extensions.json` exist
- **THEN** `exports/rust.code-profile` contains `settings` from
  `_merged/rust.json` AND `extensions.enabled` is a list of extension IDs
  from `profiles/rust/extensions.json`.

#### Scenario: Legacy bundle replaced with symlink during deprecation

- **WHEN** `vspcli --migrate-catalog` runs against a repo containing
  `exports/rust-profile-crisp.code-profile` and
  `exports/rust-profile-retina.code-profile`
- **THEN** `exports/rust.code-profile` is created;
  `exports/rust-profile-crisp.code-profile` is replaced with a symlink
  pointing to `rust.code-profile`; same for `rust-profile-retina`.
  External `raw.githubusercontent.com` URLs to the legacy filenames
  continue to resolve (GitHub serves the symlink target).

#### Scenario: --finalize removes legacy symlinks post-deprecation

- **WHEN** the deprecation window has closed AND the user runs
  `vspcli --migrate-catalog --finalize`
- **THEN** all `exports/<flavor>-{crisp,retina}.code-profile` symlinks
  are removed; only the canonical `exports/<flavor>.code-profile`
  files remain.

#### Scenario: Bundle settings include UX-agnostic content only

- **WHEN** export runs for flavor `rust`
- **THEN** the resulting bundle's `settings` object does NOT contain
  any of the keys reserved for the workspace UX layer
  (`editor.fontFamily`, `editor.fontSize`, `workbench.colorTheme`,
  `workbench.iconTheme`). UX is applied at the workspace layer per the
  `compose-profile-settings` capability.

## ADDED Requirements

### Requirement: Bundle regeneration after migration

The system SHALL regenerate the full `exports/` directory atomically as
part of `vspcli --migrate-catalog` per design.md Decision 19. The
migration SHALL:

1. Take an unconditional backup of the pre-migration `exports/`,
   `profiles/`, `_overrides/`, and `_merged/` to
   `<repo>/.cache/migrate-catalog-backup-<pid>/` (PID-suffixed to
   prevent collision between concurrent invocations, although `flock`
   already prevents that — defense in depth).
2. Build the new layout in sibling trees (`exports.new/`, `profiles
   .new/`, `_overrides.new/`, `_merged.new/`).
3. Atomically rename each `<dir>/` → `<dir>.old/` and
   `<dir>.new/` → `<dir>/` via POSIX `rename(2)` (atomic on a single
   filesystem).
4. Create the deprecation-window symlinks per the "Export bundle per
   profile" requirement.
5. Remove the `.old` siblings on success.

If any step fails (jq exit non-zero, disk full, SIGKILL):

- Steps before the rename(2) sequence: the `.new` tree is removed; the
  original `<dir>/` is untouched.
- Steps during the rename(2) sequence: a partial `.old`/`.new` state
  remains; `vspcli --migrate-catalog --doctor` detects this and
  completes or rolls back deterministically.
- Steps after rename(2) succeeds (e.g., symlink creation fails): the
  rename is committed; the failure is logged to stderr; the symlinks
  are best-effort and re-runnable via `vspcli --migrate-catalog
  --refresh-symlinks`.

#### Scenario: Atomic regeneration with sibling-tree rename

- **WHEN** `vspcli --migrate-catalog` runs successfully
- **THEN** the final state contains 11 canonical `exports/<flavor>
  .code-profile` files (real files, not symlinks), plus 22
  deprecation-window symlinks pointing to them; no `.new` or `.old`
  sibling directories remain.

#### Scenario: SIGKILL between rename steps

- **WHEN** `vspcli --migrate-catalog` is SIGKILL'd between the
  `exports/` and `profiles/` rename steps
- **AND** the user later runs `vspcli --migrate-catalog --doctor`
- **THEN** `--doctor` detects the inconsistent state (`exports.new/`
  swapped, `profiles.new/` not yet swapped), reports the remaining
  work to stdout, and exits 0 without modifying anything.

#### Scenario: Backup is unconditional

- **WHEN** any `vspcli --migrate-catalog` invocation begins
- **THEN** the backup directory under `<repo>/.cache/migrate-catalog-
  backup-<pid>/` is created BEFORE any rename or write; the backup is
  not optional.

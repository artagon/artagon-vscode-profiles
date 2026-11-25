# Supply Chain Locking Delta

## Purpose

Define the observably different lock tiers, compromised-extension remediation, and audit outcomes for pinned and personally enabled extensions.

## ADDED Requirements

### Requirement: Four-tier lock metadata

The catalog SHALL record compromised and removed extensions as Tier 0. The VSIX lock manifest SHALL distinguish Tier 1 platform-specific or universal hash locks, Tier 2 executable extension version locks, and Tier 3 unlocked declarative extensions. It SHALL distinguish pending refresh records from verified metadata.

#### Scenario: Inspect lock metadata
- **GIVEN** catalog entries that represent each tier and an unverified Tier 1 entry
- **WHEN** a maintainer inspects the catalog and `vsp.lock.json`
- **THEN** Tier 0 is represented by catalog lifecycle status, the other entries identify their lock tiers, and the unverified entry carries `pending_refresh`.

### Requirement: Compromised extensions are removed first

Named-profile sync SHALL attempt to remove installed compromised or removed extensions before changing profile settings or installing extensions. `vsp sync --all --compromised-only` SHALL cover Default and all registered profiles.

#### Scenario: A later installation fails
- **GIVEN** a selected profile with a compromised extension and a dependency whose installation later fails
- **WHEN** the user syncs the profile
- **THEN** the compromised extension is uninstalled before the first installation attempt.

### Requirement: Verify pinned VSIX content

When applying a VSIX pin without an explicit verification bypass, vsp SHALL require an approved HTTPS Marketplace source and a matching SHA-256 digest before handing the package to VS Code.

#### Scenario: Downloaded payload mismatches the pin
- **WHEN** the downloaded package does not match the recorded SHA-256 digest
- **THEN** vsp refuses the package with a safety failure and does not install it by ID as a fallback.

### Requirement: User-unlocked extension audit

Vsp SHALL identify a custom profile's catalog-external exact extension ID as Tier 3 `user-unlocked` and install it by exact ID without a catalog pin. Doctor SHALL record the ID as a warning and include the ID in `user_unlocked_extensions` in JSON output.

#### Scenario: Strict doctor fails on user-unlocked warning
- **GIVEN** successful observation, an otherwise clean audit, and a custom profile that enables a catalog-external exact ID
- **WHEN** a user runs `vsp doctor` and then `vsp doctor --strict`
- **THEN** both report `WARN` for Tier 3 `user-unlocked`; the first exits `0` and the strict audit exits `1`.

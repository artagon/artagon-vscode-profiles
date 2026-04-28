## ADDED Requirements

### Requirement: Audit capability scope
The repository SHALL provide a profile-extensions audit capability that reports per-profile installed versions, compatibility status, and a summary suitable for use as a CI signal. This requirement establishes the capability boundary; subsequent requirements specify the contract.

#### Scenario: Audit produces a summary
- **WHEN** the audit completes a run across one or more profiles
- **THEN** it emits a summary that includes counts of compatible, incompatible, and unknown extensions

### Requirement: Per-profile installed-version lookup
The audit SHALL determine the installed version of a profile's extensions by querying VS Code with `--profile <name>` so that results are scoped to the profile under audit.

#### Scenario: Profile-scoped lookup
- **WHEN** the audit runs against `java-spring-crisp`
- **THEN** installed-version queries are scoped to the `java-spring-crisp` profile

#### Scenario: Extension absent in target profile
- **WHEN** an extension is installed under another profile but not under the audited profile
- **THEN** the audit reports it as not installed for the audited profile

### Requirement: Concrete installed-version reporting
The audit SHALL report a concrete version string for every extension that is installed in the audited profile AND for which VS Code returns a version string. When VS Code returns no version information for an installed extension, the audit MAY report `unknown` for that extension only.

#### Scenario: Installed version is reported
- **WHEN** an extension is installed in the audited profile and VS Code returns a version string for it
- **THEN** the audit reports the returned version rather than a placeholder

#### Scenario: VS Code returns no version
- **WHEN** an extension is installed in the audited profile but VS Code returns no version information
- **THEN** the audit reports `unknown` for that extension only

### Requirement: Continue on non-compatible extensions
The audit SHALL record a finding for any extension whose compatibility check returns a non-zero result and SHALL continue processing the remaining extensions rather than aborting the run.

#### Scenario: First extension is incompatible
- **WHEN** the first extension under audit returns a non-compatible result
- **THEN** the audit records the finding and proceeds to the remaining extensions

### Requirement: Distinguish misuse from findings
The audit SHALL exit zero on a clean run, exit non-zero when findings are present, and reserve a distinct exit code for CLI misuse so that automation can distinguish argument errors from other outcomes.

#### Scenario: Unknown flag
- **WHEN** the audit is invoked with an unrecognized argument
- **THEN** it prints usage information and exits with the misuse exit code (distinct from both clean-run and findings-present exits)

#### Scenario: Findings present
- **WHEN** the audit completes with at least one incompatible or unknown extension
- **THEN** it exits non-zero with a code distinct from the misuse exit code

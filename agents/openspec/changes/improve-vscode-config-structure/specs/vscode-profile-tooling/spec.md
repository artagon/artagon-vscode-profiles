# Specification: VS Code Profile Tooling

## MODIFIED Requirements

### Requirement: Health Check Capability

The system shall provide automated validation of the VS Code profile configuration environment.

#### Scenario: System administrator validates configuration health

**Given** the VS Code profile system is installed
**When** the health-check script is executed
**Then** the system shall verify:
- Required dependencies are installed (jq, VS Code CLI)
- Git hooks are properly configured
- All JSON configuration files are valid
- Profile symlinks exist and point to correct merged files
- Merged file count matches profile count
- Schema version file exists

**And** the script shall exit with code 0 on success, 1 on failures
**And** the script shall output clear diagnostic messages for any failures

### Requirement: Profile Creation Capability

The system shall provide automated scaffolding for new VS Code profiles.

#### Scenario: Developer creates new profile with base configuration

**Given** the developer wants to create a new profile named "python-crisp"
**And** wants to base it on an existing "rust-profile-base.jsonc"
**When** the new-profile.sh script is executed with `--base rust-profile-base` flag
**Then** the system shall:
- Create profiles/python-crisp/ directory
- Generate profiles/python-crisp/extensions.json template
- Create symlink _overrides/python-crisp.jsonc → rust-profile-base.jsonc
- Display next-steps instructions

**And** the created profile structure shall be valid for composition

#### Scenario: Developer creates new profile without base

**Given** the developer wants to create a new profile "python-retina"
**When** the new-profile.sh script is executed without --base flag
**Then** the system shall:
- Create profiles/python-retina/ directory
- Generate profiles/python-retina/extensions.json template
- Create new _overrides/python-retina.jsonc with placeholder settings
- Display next-steps instructions

### Requirement: Symlink Portability

The system shall use relative symlinks for profile settings to support cross-user/system portability.

#### Scenario: Profile settings use relative symlinks

**Given** profiles exist in the system
**When** the compose-settings.sh script generates symlinks
**Then** each profiles/*/settings.json symlink shall use relative path format
**And** the symlink target shall be `../../_merged/<profile>.json`
**And** symlinks shall resolve correctly from profile directory

#### Scenario: Configuration backup and restore across systems

**Given** a VS Code configuration has been backed up on System A
**When** the backup is restored to a different user path on System B
**Then** all profile symlinks shall resolve correctly without modification
**And** profiles shall load successfully in VS Code

### Requirement: Script Error Reporting

The system shall provide contextual error messages when validation or composition fails.

#### Scenario: JSON validation failure shows problem file

**Given** a JSON configuration file contains invalid syntax
**When** validate-json.sh is executed
**Then** the error output shall include the filename that failed
**And** the error output shall include the jq error diagnostic
**And** the script shall exit with non-zero status

#### Scenario: Composition failure shows merge details

**Given** a profile composition fails during merge
**When** compose-settings.sh attempts the merge
**Then** the error output shall show the profile name
**And** the error output shall show the base file path
**And** the error output shall show the override file path
**And** the script shall exit with non-zero status

## ADDED Requirements

### Requirement: Schema Versioning

The system shall track configuration schema versions to enable migration detection.

#### Scenario: Schema version is queryable

**Given** the VS Code profile system is initialized
**When** a script or tool queries the schema version
**Then** the version information shall be available in agents/version.json
**And** the version shall follow semver format (major.minor.patch)
**And** minimum compatibility requirements shall be documented

#### Scenario: Scripts display schema version

**Given** the compose-settings.sh script is executed
**When** the composition begins
**Then** the script shall display the current schema version
**And** the output shall indicate the schema being used

### Requirement: Pre-commit Auto-staging

The system shall automatically stage generated artifacts during pre-commit hook execution.

#### Scenario: Configuration change auto-stages merged files

**Given** a developer modifies _shared/editor-crisp.jsonc
**And** runs git commit
**When** the pre-commit hook executes
**Then** the hook shall run compose-settings.sh to regenerate _merged/*.json
**And** the hook shall stage updated _merged/*.json files
**And** the hook shall stage updated exports/*.code-profile files
**And** the commit shall include both source changes and generated artifacts

#### Scenario: Commit proceeds only if validation succeeds

**Given** a developer attempts to commit configuration changes
**When** the pre-commit hook runs validation
**And** validation detects invalid JSON
**Then** the hook shall abort the commit with exit code 1
**And** the hook shall display validation errors
**And** no files shall be committed

### Requirement: Shell Profile Aliases

The system shall provide convenient shell aliases for VS Code profile management in both fish and POSIX shells.

#### Scenario: Configuration change auto-stages merged files

**Given** a developer modifies _shared/editor-crisp.jsonc
**And** runs git commit
**When** the pre-commit hook executes
**Then** the hook shall run compose-settings.sh to regenerate _merged/*.json
**And** the hook shall stage updated _merged/*.json files
**And** the hook shall stage updated exports/*.code-profile files
**And** the commit shall include both source changes and generated artifacts

#### Scenario: Commit proceeds only if validation succeeds

**Given** a developer attempts to commit configuration changes
**When** the pre-commit hook runs validation
**And** validation detects invalid JSON
**Then** the hook shall abort the commit with exit code 1
**And** the hook shall display validation errors
**And** no files shall be committed

### Requirement: Shell Profile Aliases

The system shall provide convenient shell aliases for VS Code profile management in both fish and POSIX shells.

#### Scenario: User opens VS Code with specific profile using alias

**Given** the user is in a fish shell
**When** the user executes `vsp java-profile-crisp`
**Then** VS Code shall open with the java-profile-crisp profile loaded
**And** the profile path shall be resolved from ~/.config/vscode/profiles/

#### Scenario: User lists available profiles using alias

**Given** the user is in a bash or zsh shell
**When** the user executes `vspl`
**Then** the system shall display all available profile names
**And** the list shall be derived from directories in ~/.config/vscode/profiles/

#### Scenario: Aliases work across different shells

**Given** VS Code profile aliases are configured
**When** the user switches between fish, bash, and zsh shells
**Then** all profile aliases (vsp, vspl, vspi, vspli) shall work consistently
**And** each alias shall reference the correct profile directory structure

## REMOVED Requirements

None - this change adds capabilities without removing existing functionality.

## RENAMED Requirements

None - existing capabilities retain their names.

## ADDED Requirements (Profile Configuration - 2025-11-09)

### Requirement: C++ Profile Consistency

The system shall provide crisp and retina visual variants for C++ profiles matching the pattern used by Java and Rust profiles.

#### Scenario: C++ developer selects visual preference

**Given** a C++ developer prefers retina rendering
**When** they browse available VS Code profiles
**Then** they shall find cpp-clangd-retina and cpp-intellisense-retina options
**And** the profiles shall use the retina shared base (auto font aliasing, larger fonts)
**And** the C/C++-specific settings shall be identical between crisp and retina variants

### Requirement: Java Profile Configuration Integrity

The system shall use functional VS Code settings mechanisms and avoid non-standard directives.

#### Scenario: Java Spring profile loads correctly

**Given** the java-spring-base.jsonc configuration file
**When** VS Code loads the profile
**Then** all base Java settings shall be present
**And** no non-functional "include" directives shall exist
**And** Spring-specific settings shall be merged correctly

### Requirement: Profile Documentation

The system shall provide setup documentation for each profile in the profile directory.

#### Scenario: User sets up new Rust profile

**Given** a user installs the rust-profile-crisp profile
**When** they navigate to profiles/rust-profile-crisp/
**Then** they shall find a README.md file
**And** the README shall list required rustup components
**And** the README shall provide setup commands
**And** the README shall list all included extensions

### Requirement: Java Environment Integration

The system shall integrate with jenv for automatic JDK management rather than requiring manual JAVA_HOME exports.

#### Scenario: Java developer switches JDK versions

**Given** a developer uses jenv to manage multiple JDK versions
**When** they run `jenv local 17` in a project
**And** they open VS Code with a Java profile
**Then** JDT LS shall use JDK 17 automatically
**And** Gradle shall use JDK 17 automatically
**And** no manual JAVA_HOME export shall be required


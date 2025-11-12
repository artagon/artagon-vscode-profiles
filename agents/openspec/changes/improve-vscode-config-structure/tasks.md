# Implementation Tasks: Improve VS Code Configuration Structure

**Change ID**: `improve-vscode-config-structure`
**Status**: Pending Approval
**Branch**: `refactor/improve-vscode-config-structure`

## Pre-Implementation Setup

### Context Corrections
- `_merged/` at the repo root is intentionally populated; do not add it to `.gitignore`.
- Only 12 profiles exist right now (Java ×8, Rust ×2, C/C++ ×2). Tasks that reference 16 profiles assume the future C++ crisp/retina variants will be created as part of this change.
- `agents/_merged/` is empty; removing it is a cleanup action, not a current bug breaking users.
- `scripts/health-check.sh` and `scripts/new-profile.sh` are to-be-created utilities—track them as deliverables, not regressions introduced by others.

### ENV.1: Create Feature Branch

```bash
cd ~/.config/vscode
git checkout -b refactor/improve-vscode-config-structure
```

**Branch naming**: `<type>/<change-id>` following conventional commits
- Type: `refactor` (structural improvements)
- Change ID: `improve-vscode-config-structure` (matches OpenSpec ID)

### ENV.2: Validate Change Proposal

```bash
cd agents
openspec validate improve-vscode-config-structure --strict
```

**Expected**: No validation errors, proposal is well-formed

### ENV.3: Set Up Git Commit Template (Optional)

```bash
# Create commit message template
cat > .git/COMMIT_TEMPLATE <<'EOF'
<type>(<scope>): <subject>

# Types: feat, fix, docs, refactor, test, chore
# Scopes: agents, scripts, profiles, git
# Subject: imperative mood, lowercase, no period

# [optional body]

# [optional footer]
EOF

# Configure git to use template
git config commit.template .git/COMMIT_TEMPLATE
```

## Task Breakdown

### Phase 1: Foundation

#### T1.1: Create Health-Check Script
- [ ] Create `scripts/health-check.sh` with executable permissions
- [ ] Check for required dependencies (jq, VS Code CLI)
- [ ] Verify git hooks configuration
- [ ] Validate all JSON files
- [ ] Check profile symlinks exist and are valid
- [ ] Verify merged files count matches profiles count
- [ ] Check schema version file exists
- [ ] Add summary output (pass/fail with details)
- [ ] Test on clean system

**Acceptance**: Script runs successfully, detects all known issues, exits 0 on healthy system

**Commit**:
```bash
git add scripts/health-check.sh
git commit -m "feat(scripts): add health-check validation script

Implements system validation for VS Code profile configuration:
- Checks dependencies (jq, VS Code CLI)
- Verifies git hooks configuration
- Validates JSON files via existing validator
- Confirms symlinks and merged files integrity
- Checks schema version file presence

Exits 0 on healthy system, 1 with helpful errors otherwise.
"
```

#### T1.2: Fix .DS_Store Tracking
- [ ] Add `.DS_Store` to `.gitignore`
- [ ] Remove `agents/.DS_Store` from git index
- [ ] Verify `.DS_Store` no longer shows in `git status`
- [ ] Commit changes

**Acceptance**: `git status` shows no `.DS_Store` files, .gitignore contains pattern

**Commit**:
```bash
echo ".DS_Store" >> .gitignore
git rm --cached agents/.DS_Store
git add .gitignore
git commit -m "chore(git): stop tracking .DS_Store files

Adds .DS_Store to .gitignore and removes from git index.
Prevents macOS metadata pollution in repository.
"
```

#### T1.3: Add Schema Versioning
- [ ] Create `agents/version.json` with initial structure
- [ ] Set schema_version to "1.0.0"
- [ ] Document format, compatibility requirements
- [ ] Add changelog structure
- [ ] Update scripts to read and display version
- [ ] Commit version file

**Acceptance**: version.json exists, scripts show version, health-check validates it

**Commit**:
```bash
git add agents/version.json scripts/compose-settings.sh
git commit -m "feat(agents): add schema versioning with v1.0.0

Introduces version.json to track configuration schema evolution:
- schema_version: 1.0.0 (initial)
- Compatibility requirements (jq, VS Code versions)
- Changelog structure for future tracking

Updated compose-settings.sh to display schema version.
Enables version validation in health-check script.
"
```

#### T1.4: Verify and Document Git Hooks
- [ ] Check current `git config core.hooksPath` value
- [ ] Document expected value in README or operations guide
- [ ] Add verification to health-check script
- [ ] Create setup instructions for new clones
- [ ] Test hook execution on test commit

**Acceptance**: Hooks path documented, health-check validates configuration

---

### Phase 2: Documentation

#### T2.1: Consolidate Documentation Files
- [ ] Create new `agents/architecture.md` from `project.md` content
- [ ] Rename `agents/instructions.md` to `agents/operations.md`
- [ ] Slim down `agents/README.md` to quick-start only
- [ ] Remove duplicated content across all files
- [ ] Add cross-references between documents
- [ ] Update `CLAUDE.md` and `AGENTS.md` if needed
- [ ] Verify <10% content duplication
- [ ] Commit documentation changes

**Acceptance**: 3 distinct docs (README, architecture, operations), minimal duplication

**Commit**:
```bash
git mv agents/instructions.md agents/operations.md
git mv agents/project.md agents/architecture.md
git add agents/README.md agents/architecture.md agents/operations.md
git commit -m "docs(agents): consolidate and restructure documentation

Reduces 30-40% content duplication through reorganization:
- README.md: Quick-start guide and overview
- architecture.md: System design and conventions (was project.md)
- operations.md: Day-to-day tasks and procedures (was instructions.md)

Achieves <10% duplication target while improving clarity.
All docs now have single, focused purpose.
"
```

**Content Distribution**:
- **README.md**: Overview, quick links, 5-minute setup guide
- **architecture.md**: System design, directory layout, composition algorithm, conventions
- **operations.md**: Day-to-day tasks, edit workflow, troubleshooting, best practices

#### T2.2: Document Symlink Pattern
- [ ] Add section to architecture.md explaining DRY symlink approach
- [ ] Document naming convention (*-base.jsonc pattern)
- [ ] List all current symlinked profiles
- [ ] Explain composer's base selection logic
- [ ] Add examples for future profiles

**Acceptance**: Symlink pattern fully documented in architecture.md

#### T2.3: Document Crisp/Retina Rationale
- [ ] Add section to architecture.md explaining separation
- [ ] List current differences (fontAliasing)
- [ ] Document future divergence possibilities
- [ ] Explain naming convention detection

**Acceptance**: Crisp/Retina strategy clearly documented

#### T2.4: Resolve agents/_merged/ Status
- [ ] Confirm directory is unused/vestigial
- [ ] Remove directory if unused
- [ ] OR document purpose if intentional
- [ ] Update .gitignore if needed
- [ ] Commit change

**Acceptance**: Either removed or documented with clear purpose

---

### Phase 3: Tooling

#### T3.1: Convert to Relative Symlinks
- [ ] Update `scripts/compose-settings.sh` line 26
- [ ] Change from absolute to relative path (`../../_merged/$name.json`)
- [ ] Run composer on all profiles
- [ ] Verify all symlinks work correctly
- [ ] Test profile loading in VS Code
- [ ] Commit symlink changes

**Acceptance**: All profile settings.json use relative symlinks, VS Code opens profiles successfully

**Commit**:
```bash
bash scripts/compose-settings.sh
git add scripts/compose-settings.sh profiles/*/settings.json
git commit -m "refactor(scripts): use relative symlinks for portability

Changes composer to generate relative instead of absolute symlinks:
  Before: /Users/.../vscode/_merged/profile.json
  After: ../../_merged/profile.json

Improves portability across users, systems, and backups.
Regenerated all profile symlinks with new format.
"
```

#### T3.2: Improve Script Error Messages
- [ ] Update `scripts/validate-json.sh`:
  - Show which file failed validation
  - Display actual jq error output
- [ ] Update `scripts/compose-settings.sh`:
  - Show base and override paths on merge failure
  - Add context for each error
- [ ] Update `scripts/export-profiles.sh` if needed
- [ ] Test error conditions
- [ ] Commit improvements

**Acceptance**: All scripts show helpful context on errors, easy to debug failures

#### T3.3: Create Profile Scaffolding Script
- [ ] Create `scripts/new-profile.sh` with executable permissions
- [ ] Implement usage/help text
- [ ] Create profile directory structure
- [ ] Generate extensions.json template
- [ ] Create or symlink override file
- [ ] Add next-steps instructions
- [ ] Support `--base` flag for symlink creation
- [ ] Add validation for profile name
- [ ] Test creating new profile end-to-end
- [ ] Document usage in operations.md
- [ ] Commit script

**Acceptance**: Script creates functional profile, documented in operations.md

**Commit**:
```bash
git add scripts/new-profile.sh agents/operations.md
git commit -m "feat(scripts): add profile scaffolding automation

Implements new-profile.sh to create profiles with:
- Profile directory with extensions.json template
- Override file (new or symlinked to base via --base flag)
- Next-steps instructions for completion
- Profile name validation

Reduces manual, error-prone profile creation process.
Documented in operations.md with usage examples.
"
```

#### T3.4: Auto-Stage Generated Files in Pre-Commit
- [ ] Update `scripts/git-hooks/pre-commit`
- [ ] Add `git add -u` for `_merged/*.json`
- [ ] Add `git add -u` for `exports/*.code-profile`
- [ ] Check if changes were staged
- [ ] Output informative message
- [ ] Test with actual commit
- [ ] Commit hook changes

**Acceptance**: Committing config changes auto-stages regenerated artifacts

#### T3.5: Fix Fish and Shell VS Code Profile Aliases
- [ ] Identify current VS Code profile aliases in fish config
- [ ] Identify current VS Code profile aliases in bash/zsh config
- [ ] Determine expected alias behavior:
  - `vsp <profile>` - Open VS Code with specific profile
  - `vspl` - List available profiles
  - `vspi <profile>` - Open VS Code Insiders with profile
  - `vspli` - List profiles for Insiders
- [ ] Fix or create aliases in `~/.config/fish/conf.d/xdg-vscode.fish` or `~/.config/fish/functions/`
- [ ] Fix or create aliases in `~/.config/shell/xdg.sh` or bash/zsh rc files
- [ ] Ensure aliases reference correct profile paths
- [ ] Test aliases in both fish and bash/zsh
- [ ] Update README.md with alias documentation
- [ ] Commit alias fixes

**Acceptance**: All VS Code profile aliases work correctly in fish and bash/zsh shells

**Commit**:
```bash
git add ~/.config/fish/conf.d/xdg-vscode.fish ~/.config/shell/xdg.sh agents/README.md
git commit -m "fix(shell): repair VS Code profile aliases for fish and bash/zsh

Fixes broken or missing VS Code profile management aliases:
- vsp/vspi: Open VS Code (Insiders) with specific profile
- vspl/vspli: List available profiles
- Corrects profile path references
- Works in both fish and bash/zsh shells

Documented in README.md for discoverability.
"
```

---

### Phase 4: Validation

#### T4.1: Complete Test Suite
- [ ] Review existing `scripts/tests/run.sh`
- [ ] Add tests for health-check.sh
- [ ] Add tests for new-profile.sh
- [ ] Add tests for error conditions
- [ ] Add tests for git hooks
- [ ] Verify 100% script coverage
- [ ] Document test execution in operations.md
- [ ] Add test run to pre-commit hook (optional)
- [ ] Commit test additions

**Acceptance**: All scripts tested, test suite passes

#### T4.2: Integration Testing
- [ ] Test full workflow: create profile → compose → export → open
- [ ] Test error recovery scenarios
- [ ] Test health-check on broken system
- [ ] Test pre-commit hook blocks bad commits
- [ ] Verify relative symlinks work on backup/restore
- [ ] Document test procedures

**Acceptance**: End-to-end workflows verified, documented

#### T4.3: Cross-Platform Verification
- [ ] Verify on macOS (primary platform)
- [ ] Document any platform-specific notes
- [ ] Note Linux/WSL compatibility if tested
- [ ] Update README with platform requirements

**Acceptance**: Platform compatibility documented

#### T4.4: Final Validation
- [ ] Run health-check script (should pass)
- [ ] Run full test suite (should pass)
- [ ] Verify git status is clean
- [ ] Review all documentation for accuracy
- [ ] Check all acceptance criteria met
- [ ] Create final commit

**Acceptance**: All must-have success criteria met

---

## Task Dependencies

```
T1.1 → T4.1 (health-check needed for tests)
T1.2 → T4.4 (cleanup before final)
T1.3 → T1.1 (version file needed for health-check)
T1.4 → T3.4 (understand hooks before modifying)

T2.1 → T2.2, T2.3 (architecture.md must exist)
T2.1 → T3.3 (operations.md must exist for docs)

T3.1 → T4.2 (symlinks must work for integration tests)
T3.3 → T4.1 (new script needs tests)
T3.4 → T4.2 (hooks tested in integration)

T4.1, T4.2, T4.3 → T4.4 (all validation before final)
```

## Suggested Execution Order

1. **Day 1**: T1.3, T1.1 (version file, health-check)
2. **Day 2**: T1.2, T1.4 (cleanup, hooks)
3. **Day 3**: T2.1, T2.2, T2.3 (docs consolidation)
4. **Day 4**: T2.4, T3.1 (cleanup, symlinks)
5. **Day 5**: T3.2, T3.3 (error messages, new-profile)
6. **Day 6**: T3.4, T4.1 (pre-commit, tests)
7. **Day 7**: T4.2, T4.3, T4.4 (integration, verification)

## Rollback Plan

If critical issues arise:

1. **Before T3.1**: Simple `git reset --hard` to previous state
2. **After T3.1**: Run `scripts/compose-settings.sh` to regenerate symlinks from backup
3. **Documentation changes**: Revert git commits, low risk
4. **Script changes**: Keep old versions in `scripts/backup/` temporarily

## Definition of Done

- [ ] All tasks marked complete
- [ ] All acceptance criteria met
- [ ] Health-check script passes
- [ ] Test suite passes
- [ ] Documentation reviewed and accurate
- [ ] Git history clean and logical
- [ ] No regressions in existing functionality

---

### Phase 5: Profile Configuration Refinement

**Added 2025-11-09 based on complete profile review**
**Critical and High priority issues from 12-profile analysis**

#### T5.1: Create C++ Crisp/Retina Variants (CRITICAL)
- [ ] Rename `_overrides/cpp-clangd.jsonc` → `_overrides/cpp-clangd-base.jsonc`
- [ ] Rename `_overrides/cpp-intellisense.jsonc` → `_overrides/cpp-intellisense-base.jsonc`
- [ ] Create symlinks:
  - `_overrides/cpp-clangd-crisp.jsonc` → `cpp-clangd-base.jsonc`
  - `_overrides/cpp-clangd-retina.jsonc` → `cpp-clangd-base.jsonc`
  - `_overrides/cpp-intellisense-crisp.jsonc` → `cpp-intellisense-base.jsonc`
  - `_overrides/cpp-intellisense-retina.jsonc` → `cpp-intellisense-base.jsonc`
- [ ] Create profile directories:
  - `profiles/cpp-clangd-crisp/`
  - `profiles/cpp-clangd-retina/`
  - `profiles/cpp-intellisense-crisp/`
  - `profiles/cpp-intellisense-retina/`
- [ ] Copy/update extensions.json for each new profile
- [ ] Run composer to generate merged files
- [ ] Test all 4 C++ profiles load correctly
- [ ] Update documentation to reflect new profiles
- [ ] Commit changes

**Acceptance**: C++ profiles follow same pattern as Java/Rust (4 profiles: 2 tools × 2 styles)

**Commit**:
```bash
git add _overrides/cpp-* profiles/cpp-* agents/*.md
git commit -m "refactor(profiles): add C++ crisp/retina variants

Brings C++ profiles in line with Java/Rust pattern:
- Renamed base configs (*-base.jsonc)
- Created crisp/retina symlinked variants
- Added 4 profiles total (clangd + intellisense × crisp + retina)
- Maintains DRY architecture consistency

Resolves architectural inconsistency between languages.
Fixes: Complete Profile Review issue C1
"
```

#### T5.2: Fix Java Spring "include" Field (CRITICAL)
- [ ] Investigate current merged output of java-spring profiles
- [ ] Verify if base settings are actually present
- [ ] Read java-profile-base.jsonc contents
- [ ] Manually merge base settings into java-spring-base.jsonc
- [ ] Remove non-functional `"include": "./java-profile-base.jsonc"` line
- [ ] Run composer and diff merged output before/after
- [ ] Test Java Spring profile functionality
- [ ] Document that VS Code doesn't support "include"
- [ ] Commit fix

**Acceptance**: Java Spring profiles have complete settings, no broken "include" reference

**Commit**:
```bash
git add _overrides/java-spring-base.jsonc agents/operations.md
git commit -m "fix(java): remove non-functional include from Spring profile

VS Code settings.json doesn't support 'include' directive.
Manually merged java-profile-base.jsonc into java-spring-base.jsonc.

Added documentation note that settings don't support includes.
Verified merged output contains all necessary base settings.

Fixes: Complete Profile Review issue C2
"
```

#### T5.3: Migrate Java Profiles to jenv Integration (HIGH)
- [ ] Update all 4 Java base files:
  - java-gradle-base.jsonc
  - java-maven-base.jsonc  
  - java-profile-base.jsonc
  - java-spring-base.jsonc (after T5.2)
- [ ] Replace `${env:JAVA_HOME}` with `${command:jenv.javaHome}` in:
  - `java.jdt.ls.java.home`
  - `java.import.gradle.java.home`
  - `java.configuration.runtimes[].path`
  - `spring-boot.ls.java.home` (Spring only)
- [ ] Update gradle.java.home settings
- [ ] Run composer on all 8 Java profiles
- [ ] Test jenv auto-switching behavior
- [ ] Update documentation
- [ ] Commit changes

**Acceptance**: All Java profiles use jenv command, auto-track active JDK

**Commit**:
```bash
git add _overrides/java-*-base.jsonc agents/instructions.md
git commit -m "feat(java): migrate to jenv command integration

Replaces ${env:JAVA_HOME} with ${command:jenv.javaHome}.
Enables automatic JDK switching without manual exports.

Affects all 8 Java profiles (Gradle, Maven, General, Spring).
Matches documented pattern in agents/instructions.md.

Fixes: Complete Profile Review issue H1
"
```

#### T5.4: Add Profile README Files (HIGH)
- [ ] Create README template with sections:
  - Profile purpose
  - Prerequisites (languages, tools, components)
  - Setup instructions
  - Extension list
  - Common issues
- [ ] Generate README.md for every existing profile (12 right now):
  - **C++ profiles** (currently 2; expands to 4 after T5.1 delivers the crisp/retina split for clangd + intellisense)
  - **Java profiles** (8): gradle, maven, general, spring × crisp + retina
  - **Rust profiles** (2): crisp + retina
- [ ] Include profile-specific setup (e.g., `rustup component add...`)
- [ ] Test README rendering
- [ ] Commit all README files

**Acceptance**: Every profile has README with prerequisites, setup, extension list

**Commit**:
```bash
git add profiles/*/README.md
git commit -m "docs(profiles): add README files to all 12 profiles

Each README includes:
- Profile purpose and focus
- Prerequisites (language versions, tools)
- Setup instructions
- Extension list with descriptions
- Common troubleshooting

Improves discoverability and reduces setup friction.
Fixes: Complete Profile Review issue H6
"
```

#### T5.5: Standardize C++ Compiler Paths (HIGH)
- [ ] Review why cpp-intellisense uses system clang
- [ ] Decide: Homebrew LLVM consistently OR document difference
- [ ] If standardizing to Homebrew:
  - Update cpp-intellisense-base.jsonc cmake.environment
  - Change `/usr/bin/clang*` → `/opt/homebrew/opt/llvm/bin/clang*`
- [ ] If keeping different:
  - Document rationale in profile READMEs
  - Add comment in cpp-intellisense-base.jsonc
- [ ] Test CMake builds with updated paths
- [ ] Commit changes

**Acceptance**: C++ compiler paths standardized OR documented with clear rationale

**Commit** (if standardizing):
```bash
git add _overrides/cpp-intellisense-base.jsonc profiles/cpp-intellisense*/README.md
git commit -m "refactor(cpp): use Homebrew LLVM consistently in intellisense

Changes system clang paths to Homebrew LLVM:
  /usr/bin/clang → /opt/homebrew/opt/llvm/bin/clang

Matches cpp-clangd configuration for consistency.
Provides access to latest LLVM features.

Fixes: Complete Profile Review issue H2
"
```

#### T5.6: Refine Rust Profile Settings (MEDIUM)
- [ ] Remove hard-coded rustfmt edition:
  - Delete `"rust-analyzer.rustfmt.extraArgs": ["--edition", "2021"]`
- [ ] Remove redundant checkOnSave setting:
  - Delete `"rust-analyzer.checkOnSave": { "command": "clippy" }`
  - Keep only `"rust-analyzer.check.command": "clippy"`
- [ ] Run composer on both Rust profiles
- [ ] Test rustfmt reads edition from Cargo.toml
- [ ] Test clippy still runs on save
- [ ] Commit refinements

**Acceptance**: Rust profiles work without hard-coding, let tooling auto-detect

**Commit**:
```bash
git add _overrides/rust-profile-base.jsonc
git commit -m "refactor(rust): remove hard-coded settings, use auto-detection

Removed:
- rustfmt edition flag (now reads from Cargo.toml)
- redundant checkOnSave.command (keep check.command only)

Allows rust-analyzer to adapt to workspace configuration.
Simplifies profile maintenance.

Fixes: Complete Profile Review issues H4, H5
"
```

#### T5.7: Add C++ Performance Optimizations (MEDIUM)
- [ ] Add `files.watcherExclude` to both C++ base files:
  ```json
  "files.watcherExclude": {
    "**/build": true,
    "**/cmake-build-*": true,
    "**/.vscode/ipch": true
  }
  ```
- [ ] Consider adding to clangd-base:
  - `cmake.parallelJobs` (if not present)
- [ ] Consider adding to intellisense-base:
  - Any missing optimizations from clangd
- [ ] Run composer
- [ ] Test build directory not watched
- [ ] Commit optimizations

**Acceptance**: C++ profiles exclude build directories from file watchers like Rust does

**Commit**:
```bash
git add _overrides/cpp-*-base.jsonc
git commit -m "perf(cpp): exclude build directories from file watchers

Adds files.watcherExclude for build artifacts:
- build/
- cmake-build-*/
- .vscode/ipch/

Reduces VS Code overhead during builds.
Matches Rust profile pattern (target exclusion).

Fixes: Complete Profile Review issue M3
"
```

#### T5.8: Document or Fix Maven/Gradle Mutual Exclusion (MEDIUM)
- [ ] Review intent: Are separate profiles intentional?
- [ ] Check if mixed projects common in practice
- [ ] Option A: Keep exclusions, document in README
  - Add to gradle profile README: "Maven disabled for focused Gradle workflow"
  - Add to maven profile README: "Gradle disabled for focused Maven workflow"
- [ ] Option B: Remove exclusions, allow both
  - Remove `"java.import.maven.enabled": false` from gradle
  - Remove `"java.import.gradle.enabled": false` from maven
  - Test mixed project support
- [ ] Commit decision

**Acceptance**: Mutual exclusion either removed OR clearly documented with rationale

**Commit** (if documenting):
```bash
git add profiles/java-gradle-*/README.md profiles/java-maven-*/README.md
git commit -m "docs(java): document Maven/Gradle mutual exclusion

Gradle profiles disable Maven intentionally for focused workflow.
Maven profiles disable Gradle intentionally for focused workflow.

For mixed build systems, use java-profile-* (supports both).

Clarifies design decision.
Addresses: Complete Profile Review issue M5
"
```

#### T5.9: Standardize CMake Configurations (OPTIONAL)
- [ ] Compare cpp-clangd vs cpp-intellisense CMake settings
- [ ] Identify best practices from each:
  - parallelJobs
  - ctestArgs
  - loggingLevel
  - preferredGenerators
- [ ] Decide which settings to unify
- [ ] Update both base files with merged best practices
- [ ] Test CMake workflows in both profiles
- [ ] Document any intentional differences
- [ ] Commit standardization

**Acceptance**: Both C++ profiles have consistent CMake configuration OR differences documented

**Commit**:
```bash
git add _overrides/cpp-*-base.jsonc
git commit -m "refactor(cpp): standardize CMake configuration

Unified best practices from both profiles:
- Parallel builds (8 jobs)
- CTest arguments
- Ninja preferred
- Warning-level logging

Ensures consistent CMake experience across C++ profiles.
Addresses: Complete Profile Review issue M4
"
```

---

## Task Dependencies (Updated with Phase 5)

```
Phase 1 → Phase 2 (foundation before docs)
Phase 2 → Phase 3 (docs before tooling)
Phase 3 → Phase 4 (tooling before validation)
Phase 4 → Phase 5 (validate before profile fixes)

T5.1 must complete before T5.4 (C++ READMEs need profiles to exist)
T5.2 must complete before T5.3 (Spring fix before jenv migration)
T5.3 should complete before T5.4 (Java READMEs should reflect jenv)
```

## Suggested Execution Order (Updated)

**Days 1-2**: Phase 1 (Foundation)
**Days 3-4**: Phase 2 (Documentation)  
**Days 5-6**: Phase 3 (Tooling)
**Day 7**: Phase 4 (Validation)
**Days 8-10**: Phase 5 (Profile Refinement) **NEW**
  - Day 8: T5.1, T5.2 (Critical issues)
  - Day 9: T5.3, T5.4 (High priority - jenv + READMEs)
  - Day 10: T5.5, T5.6, T5.7, T5.8, T5.9 (Remaining optimizations)

## Definition of Done (Updated)

- [ ] All Phase 1-4 tasks complete
- [ ] **All Phase 5 profile refinement tasks complete** **NEW**
- [ ] All acceptance criteria met
- [ ] Health-check script passes
- [ ] Test suite passes
- [ ] **All managed profiles have README files** (12 today; ensure the C++ crisp/retina variants from T5.1 are included once they exist)
- [ ] **All critical profile issues resolved** (C++ variants, Java Spring include)
- [ ] Documentation reviewed and accurate
- [ ] Git history clean and logical
- [ ] No regressions in existing functionality

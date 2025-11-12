# Change Proposal: Improve VS Code Configuration Structure

**Change ID**: `improve-vscode-config-structure`
**Type**: Refactoring, Documentation, Tooling
**Status**: Draft
**Created**: 2025-11-09
**Owner**: System Maintainer

## Summary

Refactor and enhance the VS Code profile management system to address structural issues, improve maintainability, reduce documentation redundancy, and add missing automation capabilities identified during comprehensive review.

## Motivation

### Current Issues

1. **Critical**:
   - Git hooks configuration not verified or enforced
   - Leftover empty `agents/_merged/` directory should be deleted as simple cleanup (avoid treating it as an active bug)
   - `.DS_Store` files tracked in git

2. **High Priority**:
   - ~30-40% documentation redundancy across 3 markdown files
   - Undocumented symlink pattern in `_overrides/` (8 symlinks)
   - No schema versioning for tracking breaking changes
   - Test infrastructure started but incomplete

3. **Medium Priority**:
   - Absolute symlink paths reduce portability
   - No health-check script for system validation (to be created)
   - No profile creation automation (error-prone manual process, to be created)
   - Script error messages lack context
   - Pre-commit hook doesn't auto-stage generated artifacts
   - Fish and shell VS Code profile aliases not working or missing

### Clarifications (2025-11-09 Review Sync)
1. `_merged/` at the repo root is intentionally populated; ensure `.gitignore` does not exclude it (only the stale comment implied removal).
2. Only 12 profiles exist today (Java ×8, Rust ×2, C/C++ ×2). Creating the additional C++ crisp/retina variants is part of this change, not a current-state discrepancy.
3. `agents/_merged/` is empty; removing it is a cleanup follow-up, not a live-breaking issue.
4. Scripts such as `scripts/health-check.sh` and `scripts/new-profile.sh` do not exist yet; their absence is why they are listed in the scope/tasks.
5. The proposal/spec describe the desired future state; ensure validation conversations call out whether an item is a TODO vs. a current regression.
6. The Java Spring `"@extends"/"include"` wiring exists but is broken; fixing it remains in-scope (critical).

### Benefits

- **Maintainability**: Single source of truth for documentation, clear conventions
- **Reliability**: Automated validation, health checks, comprehensive testing
- **Developer Experience**: Better error messages, self-documenting structure
- **Portability**: Relative symlinks, version tracking
- **Automation**: Profile scaffolding, streamlined workflows

## Scope

### In Scope

- Documentation consolidation and restructuring
- Git hooks verification and automation improvements
- Schema versioning system
- Health-check script creation
- Profile creation automation
- Script error message improvements
- Test suite completion
- Cleanup tasks (.DS_Store, empty directories)
- **Profile configuration refinement** (added 2025-11-09):
  - C++ crisp/retina variant creation
  - Java Spring "include" field resolution
  - Java jenv integration (JAVA_HOME → jenv.javaHome)
  - Profile README files (12 profiles)
  - Rust profile optimizations
  - C++ performance optimizations

### Out of Scope

- New language profiles (Python, Go, TypeScript, etc.)
- VS Code extension development
- Cloud sync integration
- Profile comparison/diffing tools
- Migration from current schema (stays at v1)
- Automated profile testing infrastructure
- CI/CD pipeline for profile validation
- Extension version pinning/management

## Impact Analysis

### Files Modified

**Documentation**:
- `agents/README.md` - Simplified to quick-start guide
- `agents/instructions.md` → `agents/operations.md` - Renamed, deduplicated
- `agents/project.md` → `agents/architecture.md` - Renamed, restructured
- New: `agents/version.json` - Schema version tracking

**Scripts**:
- `scripts/compose-settings.sh` - Update symlink generation to use relative paths
- `scripts/validate-json.sh` - Add detailed error context
- `scripts/git-hooks/pre-commit` - Auto-stage generated files
- New: `scripts/health-check.sh` - System validation
- New: `scripts/new-profile.sh` - Profile scaffolding automation

**Shell Configuration**:
- Fish shell: `~/.config/fish/conf.d/xdg-vscode.fish` or functions - Fix/add VS Code profile aliases
- Bash/Zsh: `~/.config/shell/xdg.sh` or bash_profile/zshrc - Fix/add VS Code profile aliases

**Configuration**:
- `.gitignore` - Add `.DS_Store`, document patterns
- Remove: `agents/_merged/` directory

### Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking existing workflows | Medium | High | Test each change in isolation, maintain backward compatibility |
| Relative symlink issues | Low | Medium | Verify with test suite before rollout |
| Documentation becomes stale again | Medium | Low | Add update reminders to scripts, enforce in code review |
| Git hooks not working cross-platform | Low | Medium | Test on macOS (primary), document Linux/WSL differences |

## Success Criteria

### Must Have
- ✅ Health-check script passes on clean system
- ✅ Git hooks configured and verified
- ✅ Documentation <10% duplication
- ✅ `.DS_Store` no longer tracked
- ✅ Schema version file exists with v1.0.0
- ✅ Symlink pattern documented

### Should Have
- ✅ Relative symlinks for all profiles
- ✅ `new-profile.sh` creates functional profiles
- ✅ All scripts show helpful error messages
- ✅ Pre-commit auto-stages artifacts
- ✅ Test suite covers all scripts

### Nice to Have
- ⬜ CI/CD workflow for validation
- ⬜ Automated backup before structural changes
- ⬜ Migration guide template

## Dependencies

**External**:
- `jq` >= 1.6 (already required)
- `bash` >= 4.0
- Git >= 2.9 (for core.hooksPath)

**Internal**:
- Existing profile structure
- Current composition algorithm
- Test infrastructure (`scripts/tests/run.sh`)

## Alternatives Considered

### 1. Keep Current Documentation Structure
**Rejected**: Maintenance burden too high, inconsistency risk increases over time

### 2. Single Shared Base Instead of Crisp/Retina Split
**Rejected**: Current split preserves flexibility for future rendering differences, minimal overhead

### 3. Absolute Symlinks for Reliability
**Rejected**: Portability benefits outweigh potential issues, relative paths work reliably on modern systems

### 4. External Profile Management Tool
**Rejected**: Overkill for current needs, custom scripts provide sufficient control

## Open Questions

1. **agents/_merged/ directory**: Remove completely or document as reserved?
   - **Status**: Confirmed empty on 2025-11-09
   - **Recommendation**: Remove (empty, unused, undocumented)

2. **Java base file reorganization**: Git shows deletion of java-gradle-base.jsonc, java-maven-base.jsonc, java-profile-base.jsonc but they exist on disk
   - **Status**: Files exist on disk, git state appears inconsistent
   - **Action**: Verify git index vs working directory

3. **Test coverage target**: What percentage of scripts/functionality should be tested?
   - **Recommendation**: 100% of public scripts (compose, validate, export, open, health-check, new-profile)

4. **Profile-specific issues discovered** (2025-11-09 complete review):
   - **C++ profiles**: Should they follow crisp/retina pattern like Java/Rust?
   - **Java Spring "include" field**: Is this intentionally non-functional or a bug?
   - **jenv vs JAVA_HOME**: Should all Java profiles migrate to `${command:jenv.javaHome}`?
   - **Maven/Gradle mutual exclusion**: Intentional strictness or limitation?

## Git Workflow

### Branch Naming

Following conventional commits and OpenSpec patterns:

```bash
# Create feature branch for this change
git checkout -b refactor/improve-vscode-config-structure
```

**Convention**: `<type>/<change-id>`
- **Type**: `refactor` (structural improvements without functionality changes)
- **Change ID**: `improve-vscode-config-structure` (matches OpenSpec change ID)

**Alternative types if applicable**:
- `feat/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation only
- `test/` - Test additions
- `chore/` - Maintenance tasks

### Commit Messages

Follow conventional commits format:

```
<type>(<scope>): <subject>

[optional body]

[optional footer]
```

**Examples for this change**:
```bash
git commit -m "docs(agents): consolidate documentation files"
git commit -m "feat(scripts): add health-check validation script"
git commit -m "refactor(scripts): use relative symlinks in composer"
git commit -m "chore(git): add .DS_Store to gitignore"
```

**Types**:
- `feat`: New feature or capability
- `fix`: Bug fix
- `docs`: Documentation changes
- `refactor`: Code restructuring without behavior change
- `test`: Test additions or modifications
- `chore`: Maintenance, tooling, dependencies

**Scopes** (for this project):
- `agents` - Documentation in agents/
- `scripts` - Shell scripts
- `profiles` - Profile configurations
- `git` - Git configuration

## OpenSpec Commands

### Creating This Change

```bash
# Initialize OpenSpec (already done)
cd ~/.config/vscode/agents
openspec init --tools claude

# Validate the change proposal
openspec validate improve-vscode-config-structure --strict

# List all changes
openspec list

# Show this change
openspec show improve-vscode-config-structure

# View in JSON format
openspec show improve-vscode-config-structure --format=json
```

### During Implementation

```bash
# Check change status
openspec list

# Validate as you work
openspec validate improve-vscode-config-structure

# Update main specs (if adding new specifications)
openspec spec list
```

### After Completion

```bash
# Archive completed change
openspec archive improve-vscode-config-structure

# This will:
# - Move change from changes/ to changes/.archive/
# - Update main specification files if needed
# - Mark as completed
```

## Implementation Notes

### Phase 1: Foundation (Priority: Critical)
- Create and validate health-check script
- Fix `.DS_Store` tracking
- Add schema version file
- Verify/document git hooks setup

### Phase 2: Documentation (Priority: High)
- Consolidate markdown files (README, architecture, operations)
- Document symlink pattern
- Document crisp/retina rationale
- Resolve agents/_merged/ status

### Phase 3: Tooling (Priority: High-Medium)
- Convert to relative symlinks
- Improve script error messages
- Create new-profile.sh automation
- Auto-stage in pre-commit hook
- Fix fish and shell VS Code profile aliases

### Phase 4: Validation (Priority: Medium)
- Complete test suite
- Add integration tests
- Document testing procedures
- Verify cross-platform compatibility

### Phase 5: Profile Configuration Refinement (Priority: High)
**Added 2025-11-09 based on complete profile review**
- Fix critical issues (C++ variants, Java Spring "include")
- Standardize configurations (jenv, compiler paths)
- Add profile documentation (README files)
- Optimize performance (build directory exclusions)
- Refine Rust profiles (edition, checkOnSave)

## Timeline Estimate

- **Phase 1**: 1-2 days (Foundation)
- **Phase 2**: 2-3 days (Documentation)
- **Phase 3**: 2-3 days (Tooling)
- **Phase 4**: 1-2 days (Validation)
- **Phase 5**: 2-3 days (Profile Refinement) **NEW**

**Total**: 8-13 days (calendar time, assuming 2-4 hours/day work)
**Previous**: 6-10 days
**Added**: 2-3 days for profile refinement

## Approval Required From

- System maintainer/owner
- Regular VS Code configuration users (if any)

## Related Work

- Recent additions: Java Spring profiles, jenv integration
- Test infrastructure: `scripts/tests/run.sh` already created
- Local settings: `agents/settings.local.json` for Claude Code integration
- Shell aliases: Recently documented vsext-*, vsregen, vsexport commands
- OpenSpec integration: Initialized 2025-11-09 for change management
- **Complete profile review completed**: 2025-11-09
  - 12 profiles analyzed (Java 8, Rust 2, C++ 2)
  - 13 issues identified (2 Critical, 6 High, 5 Medium)
  - Grade: B+ overall (Rust A-, Java B+, C++ B)

## References

- OpenSpec: https://github.com/openspec-framework/openspec
- VS Code Profiles: https://code.visualstudio.com/docs/editor/profiles
- jq Manual: https://jqlang.github.io/jq/manual/
- Git Hooks: https://git-scm.com/docs/githooks

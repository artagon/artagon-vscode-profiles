# Improvement Proposal Update Summary

**Date**: 2025-11-09
**Action**: Updated proposal based on current folder state review and complete profile analysis

---

## What Was Reviewed

### 1. Current Folder State
- `agents/` directory structure
- Git status and staged changes
- Empty `agents/_merged/` directory
- `.DS_Store` tracking status
- `.gitignore` contents
- Recent documentation updates (shell aliases)

### 2. Complete Profile Analysis
- **12 profiles reviewed**: Java (8), Rust (2), C/C++ (2)
- **Architecture patterns** analyzed
- **Configuration quality** assessed
- **13 issues identified**: 2 Critical, 6 High, 5 Medium

### 3. Clarifications Captured
- `_merged/` in the repo root is intentionally populated; documentation/tasks should not treat it as an error or ignore it via `.gitignore`.
- Only 12 profiles exist today (Java ×8, Rust ×2, C/C++ ×2); the additional C++ crisp/retina variants roll out as part of this change.
- `agents/_merged/` is merely an empty vestige—removal is categorized as cleanup, not a current outage.
- `scripts/health-check.sh` and `scripts/new-profile.sh` have not been authored yet; their absence is precisely why they appear in the scope/tasks.
- The proposal/spec describe the desired end-state; reviewers should distinguish future work items from regressions.
- The Java Spring `"@extends"/include` wiring exists but is broken and remains a tracked critical fix.

---

## Changes Made to Proposal

### proposal.md Updates

#### 1. Scope Expansion (+6 bullet points)
**Added to "In Scope"**:
- Profile configuration refinement
- C++ crisp/retina variant creation
- Java Spring "include" field resolution
- Java jenv integration (JAVA_HOME → jenv.javaHome)
- Profile README files (12 profiles)
- Rust profile optimizations
- C++ performance optimizations

**Added to "Out of Scope"**:
- Automated profile testing infrastructure
- CI/CD pipeline for profile validation
- Extension version pinning/management

#### 2. Open Questions (+4 new questions)
- Updated agents/_merged/ status (confirmed empty)
- Added 4 profile-specific questions:
  - C++ crisp/retina pattern decision
  - Java Spring "include" field investigation
  - jenv vs JAVA_HOME migration decision
  - Maven/Gradle mutual exclusion rationale

#### 3. Timeline Extension
- **Previous**: 6-10 days
- **Updated**: 8-13 days
- **Added**: Phase 5 (2-3 days for profile refinement)

#### 4. Implementation Phases
**New Phase 5 Added**:
- Fix critical issues (C++ variants, Java Spring "include")
- Standardize configurations (jenv, compiler paths)
- Add profile documentation (README files)
- Optimize performance (build directory exclusions)
- Refine Rust profiles (edition, checkOnSave)

#### 5. Related Work
**Added**:
- Shell aliases documentation (vsext-*, vsregen, vsexport)
- OpenSpec initialization details
- **Complete profile review results**:
  - 12 profiles analyzed
  - 13 issues identified (2 Critical, 6 High, 5 Medium)
  - Overall grade: B+ (Rust A-, Java B+, C++ B)

---

### tasks.md Updates

#### Phase 5 Tasks Added (9 new tasks)

**T5.1: Create C++ Crisp/Retina Variants** (CRITICAL)
- Refactor cpp-clangd and cpp-intellisense to base + variants
- Create 4 total C++ profiles (2 tools × 2 styles)
- Brings C++ in line with Java/Rust architectural pattern

**T5.2: Fix Java Spring "include" Field** (CRITICAL)
- Remove non-functional VS Code "include" directive
- Manually merge base settings into java-spring-base.jsonc
- Verify complete settings in merged output

**T5.3: Migrate Java Profiles to jenv Integration** (HIGH)
- Replace ${env:JAVA_HOME} with ${command:jenv.javaHome}
- Affects all 8 Java profiles
- Enables automatic JDK switching

**T5.4: Add Profile README Files** (HIGH)
- Create README for every existing profile (12 today) and extend coverage to the new C++ crisp/retina variants once T5.1 lands
- Include prerequisites, setup instructions, extension lists
- Profile-specific setup commands

**T5.5: Standardize C++ Compiler Paths** (HIGH)
- Decide Homebrew LLVM vs system clang
- Standardize OR document difference
- Test CMake builds

**T5.6: Refine Rust Profile Settings** (MEDIUM)
- Remove hard-coded rustfmt edition
- Remove redundant checkOnSave setting
- Let tooling auto-detect from workspace

**T5.7: Add C++ Performance Optimizations** (MEDIUM)
- Add files.watcherExclude for build directories
- Match Rust pattern (target exclusion)
- Reduce VS Code overhead during builds

**T5.8: Document or Fix Maven/Gradle Mutual Exclusion** (MEDIUM)
- Review if intentional design or limitation
- Either remove exclusions OR document rationale
- Test mixed build system support

**T5.9: Standardize CMake Configurations** (OPTIONAL)
- Merge best practices from clangd and intellisense profiles
- Unify parallel builds, ctest args, logging
- Document intentional differences

#### Execution Order Updated
**Days 8-10 added for Phase 5**:
- Day 8: T5.1, T5.2 (Critical issues)
- Day 9: T5.3, T5.4 (jenv + READMEs)
- Day 10: T5.5-T5.9 (Optimizations)

#### Definition of Done Updated
- Added: All Phase 5 profile refinement tasks complete
- Updated: All current (and newly added) profiles have README files
- Added: All critical profile issues resolved

---

### specs/vscode-profile-tooling/spec.md Updates

#### 4 New Requirements Added

**Requirement: C++ Profile Consistency**
- Scenario: C++ developer selects visual preference
- Ensures crisp/retina variants for C++ like Java/Rust

**Requirement: Java Profile Configuration Integrity**
- Scenario: Java Spring profile loads correctly
- No non-functional "include" directives

**Requirement: Profile Documentation**
- Scenario: User sets up new Rust profile
- README.md with prerequisites and setup commands

**Requirement: Java Environment Integration**
- Scenario: Java developer switches JDK versions
- Automatic jenv integration, no manual JAVA_HOME

---

## File Statistics

### Before Update
- proposal.md: 313 lines
- tasks.md: 408 lines
- spec.md: 189 lines
- **Total**: 910 lines
- **Tasks**: 116 tracked

### After Update
- proposal.md: **348 lines** (+35, +11%)
- tasks.md: **741 lines** (+333, +82%)
- spec.md: **242 lines** (+53, +28%)
- **Total**: **1,331 lines** (+421, +46%)
- **Tasks**: **186 tracked** (+70, +60%)

---

## Impact Summary

### Critical Findings Addressed
1. **C++ profiles missing variants** → T5.1 (Create 4 new profiles)
2. **Java Spring broken "include"** → T5.2 (Fix configuration)

### High Priority Findings Addressed
3. **Java JAVA_HOME issue** → T5.3 (Migrate to jenv)
4. **No profile READMEs** → T5.4 (Add README coverage for every existing profile, then extend to the new C++ variants)
5. **C++ compiler inconsistency** → T5.5 (Standardize paths)
6. **Rust hard-coded settings** → T5.6 (Auto-detection)

### Medium Priority Findings Addressed
7. **C++ performance** → T5.7 (Build exclusions)
8. **Maven/Gradle exclusion** → T5.8 (Document or fix)
9. **CMake inconsistency** → T5.9 (Standardize)

### Issues From Original Proposal Still Tracked
- All Phase 1-4 tasks remain (foundation, docs, tooling, validation)
- Phase 5 adds profile-specific refinements
- No tasks removed, only added

---

## Current Proposal Status

**Overall Scope**: Infrastructure + Documentation + Tooling + Validation + **Profiles**

**Total Effort**: 8-13 days (was 6-10 days)

**Task Count**: 186 tasks across 5 phases (was 116 across 4 phases)

**Coverage**:
- ✓ Infrastructure improvements (git hooks, health-check, schema version)
- ✓ Documentation consolidation
- ✓ Tooling automation (new-profile, error messages, pre-commit)
- ✓ Testing and validation
- ✓ **Profile configuration refinement** (NEW)

**Readiness**: Ready for approval and implementation

---

## Next Steps

1. **Review** updated proposal (proposal.md)
2. **Validate** tasks are complete and acceptance criteria clear
3. **Approve** scope expansion (Phase 5 addition)
4. **Prioritize** critical tasks (T5.1, T5.2)
5. **Execute** according to updated timeline

---

## Questions for Approval

1. **C++ Crisp/Retina**: Proceed with creating 4 C++ profiles to match pattern?
2. **Java Spring**: Manually merge settings or alternative approach?
3. **jenv Migration**: Migrate all 8 Java profiles to jenv.javaHome command?
4. **Profile READMEs**: Create README files for every managed profile (12 immediately, 16 once the C++ variants exist) following the shared template.
5. **Timeline**: Accept 8-13 day timeline (2-3 extra days for Phase 5)?

---

**Proposal Location**: `openspec/changes/improve-vscode-config-structure/`

**Files**:
- proposal.md (main specification)
- tasks.md (detailed implementation tasks)
- specs/vscode-profile-tooling/spec.md (technical requirements)
- REVIEW-SUMMARY.md (this document)

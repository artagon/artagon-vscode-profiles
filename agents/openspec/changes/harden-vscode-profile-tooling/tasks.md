# Implementation Tasks: Harden VS Code Profile Tooling

**Change ID**: `harden-vscode-profile-tooling`
**Status**: Pending Approval

## 1. Test Isolation
- [ ] 1.1 Set VSCODE_USER_DIR to a temp path in scripts/tests/run.sh before invoking open-profiles.sh or import-profile.sh.
- [ ] 1.2 Assert that test runs only write under the temp user directory.

## 2. Script Robustness
- [ ] 2.1 Add a TMPDIR fallback in scripts/validate-json.sh (use /tmp when unset).
- [ ] 2.2 Replace profile ID generation in scripts/import-profile.sh with a SIGPIPE-safe method.
- [ ] 2.3 If desired, align open-profiles.sh CLI detection with pre-commit (code then code-insiders).

## 3. C/C++ Portability
- [ ] 3.1 Remove absolute Homebrew paths in _overrides/cpp-clangd-base.jsonc; rely on PATH or documented overrides.
- [ ] 3.2 Remove or relax architecture-specific IntelliSense mode in _overrides/cpp-intellisense-base.jsonc.
- [ ] 3.3 Document how to set toolchain overrides (clangd, clang-format, cmake) for macOS and other platforms.

## 4. Extension Grouping
- [ ] 4.1 Add sourcegraph.cody-ai to the AI group in scripts/install-extensions.sh.
- [ ] 4.2 Add or update tests to verify AI group coverage for ai-plus.

## 5. Documentation Alignment
- [ ] 5.1 Update README.md to clarify AI policy vs ai-plus.
- [ ] 5.2 Update agents/project.md to match actual ai-profile contents.
- [ ] 5.3 Update agents/README.md to reflect open-profiles CLI behavior.

## 6. Benchmark Refactoring (artagon-uri)
- [ ] 6.1 Split Result/Either comparison into explicit reference vs primitive suites (update ResultEitherComparisonBenchmark or introduce dedicated classes).
- [ ] 6.2 Align EitherBenchmark and ResultBenchmark factories and map paths (Result.okInt vs Either.rightInt, Result.ok vs Either.right).
- [ ] 6.3 Normalize map-chain operations in OptPrimitiveBenchmark so per-op allocation metrics are meaningful (fixed chain lengths or OperationsPerInvocation).
- [ ] 6.4 Remove per-iteration lambda captures and string concatenation from hot loops; use preallocated functions/data and add separate allocation-heavy variants if needed.
- [ ] 6.5 Update benchmark analysis tips/docs with allocation profiling flags (-prof gc, -prof stack, jfr) and comparison guidance.
- [ ] 6.6 Add baseline construction/no-op benchmarks to bound container overhead and separate it from transform cost.

## 7. Validation
- [ ] 7.1 Run scripts/validate-json.sh.
- [ ] 7.2 Run scripts/compose-settings.sh for affected profiles.
- [ ] 7.3 Run scripts/tests/run.sh.
- [ ] 7.4 Run openspec validate harden-vscode-profile-tooling --strict (if OpenSpec is initialized at repo root).
- [ ] 7.5 In artagon-uri, run ./gradlew jmhOptPrimitive jmhEither jmhResult jmhResultEitherCompare with allocation profiling enabled.

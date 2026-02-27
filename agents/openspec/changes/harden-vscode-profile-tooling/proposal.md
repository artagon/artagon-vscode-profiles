# Change Proposal: Harden VS Code Profile Tooling

**Change ID**: `harden-vscode-profile-tooling`
**Type**: Reliability, Portability, Documentation, Benchmarking, Compatibility
**Status**: Approved
**Created**: 2025-12-28
**Updated**: 2026-01-01
**Owner**: System Maintainer

## Summary
Improve the reliability and portability of profile tooling scripts, align documentation with current behavior, add extension compatibility checking, and refactor Opt/Either/Result benchmarks to correctly compare performance and memory allocation.

## Why
The review identified several issues that can surprise users or fail on non-macOS setups:
- Script tests can write into the real VS Code profile cache instead of an isolated temp directory.
- validate-json.sh fails when TMPDIR is unset, and import-profile.sh can abort due to pipefail SIGPIPE.
- C/C++ overrides hardcode Homebrew paths and arm64 IntelliSense mode, which break on non-Homebrew or non-ARM hosts.
- install-extensions.sh group filters omit sourcegraph.cody-ai, so --group AI is incomplete for ai-plus.
- Documentation conflicts on AI assistant policy and on open-profiles CLI behavior.
- No automated way to detect incompatible extensions before installation or profile deployment.
The JMH benchmarks also need refactoring for accurate comparisons:
- Result/Either comparison currently mixes primitive and reference paths (apples-to-oranges).
- Map/flatMap chain loops are not normalized per operation, which skews allocation metrics.
- Per-iteration lambda captures and string concatenation add unrelated allocation noise.
- Result/Either comparison should be split into explicit reference vs primitive suites.

## Goals
- Make script tests hermetic and safe for local machines.
- Harden temp file handling and profile import ID generation.
- Ensure C/C++ settings are portable or clearly configurable.
- Ensure extension grouping is complete for AI bundles.
- Align README and agents docs with actual scripts and profile contents.
- Add automated extension compatibility checking against VS Code versions.
- Make Opt/Either/Result benchmarks comparable and allocation-aware.

## Non-Goals
- Adding new profiles or changing extension lists beyond AI grouping coverage.
- Introducing new external dependencies or complex platform detection.
- Reworking the OpenSpec layout or tooling initialization.
- Changing Either/Result/Opt behavior outside the benchmark suite.

## What Changes
- Isolate script tests with VSCODE_USER_DIR and tighten test assertions.
- Add a TMPDIR fallback in validate-json.sh and a SIGPIPE-safe profile ID generator in import-profile.sh.
- Remove or replace hard-coded C/C++ toolchain paths with portable defaults and documented overrides.
- Expand AI extension grouping to include sourcegraph.cody-ai.
- Update README and agents docs to reflect current AI profiles and open-profiles CLI support.
- **NEW**: Add check-extension-compatibility.sh script to query VS Code Marketplace API and verify extension compatibility.
- **NEW**: Add comprehensive extension compatibility documentation with usage examples, CI/CD integration, and troubleshooting.
- Refactor OptPrimitiveBenchmark, EitherBenchmark, ResultBenchmark, and ResultEitherComparisonBenchmark for apples-to-apples comparisons and allocation metrics.
- Split comparison suites into explicit reference vs primitive categories, add baseline construction/no-op benchmarks, and normalize per-operation costs.

## Impact
- Affected specs: vscode-profile-tooling, performance-benchmarking
- Affected scripts:
  - scripts/tests/run.sh
  - scripts/validate-json.sh
  - scripts/import-profile.sh
  - scripts/install-extensions.sh
  - scripts/open-profiles.sh (if CLI fallback is added)
  - **NEW**: scripts/check-extension-compatibility.sh
- Affected config: _overrides/cpp-clangd-base.jsonc, _overrides/cpp-intellisense-base.jsonc
- Affected docs:
  - README.md
  - agents/project.md
  - agents/README.md
  - **NEW**: docs/EXTENSION_COMPATIBILITY.md
- Affected benchmarks (artagon-uri):
  - src/jmh/java/org/artagon/OptPrimitiveBenchmark.java
  - src/jmh/java/org/artagon/EitherBenchmark.java
  - src/jmh/java/org/artagon/ResultBenchmark.java
  - src/jmh/java/org/artagon/ResultEitherComparisonBenchmark.java

## Risks / Mitigations
- Risk: Removing absolute toolchain paths could surprise users relying on Homebrew defaults.
  Mitigation: Document how to set local overrides for clangd/cmake/clang-format.
- Risk: Changing open-profiles CLI selection may impact users with custom PATHs.
  Mitigation: Prefer code first, then code-insiders, and document the behavior.
- Risk: Benchmark refactors change baselines relative to historical results.
  Mitigation: Archive current results and document comparison methodology changes.
- Risk: VS Code Marketplace API rate limiting could affect extension compatibility checks.
  Mitigation: Implement caching (1-hour TTL) and provide --no-cache option for fresh data.
- Risk: Extension compatibility checks add external dependency on Marketplace API availability.
  Mitigation: Make checks optional, cache results, and gracefully handle API failures with "unknown" status.

## Validation
- Run scripts/validate-json.sh after edits.
- Run scripts/tests/run.sh to confirm hermetic test behavior.
- Run scripts/check-extension-compatibility.sh --all --marketplace-only to verify extension compatibility checking works.
- If OpenSpec tooling is initialized at repo root, run: openspec validate harden-vscode-profile-tooling --strict
- In artagon-uri: run ./gradlew jmhOptPrimitive jmhEither jmhResult jmhResultEitherCompare with -Djmh.profiler.gc=true (and optionally -Djmh.profiler.stack=true).

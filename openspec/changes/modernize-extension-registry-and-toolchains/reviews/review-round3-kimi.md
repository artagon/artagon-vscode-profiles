# Adversarial Review Round 3: Moonshot Kimi

**Verdict**: REJECTED (Reconcile Java, Gradle, and Shell Detection)

## Key Technical Findings

1. **Gradle Build Server Setting Name**:
   - Reconciled `"java.gradle.buildServer.enabled": "on"` across all normative specs, design decisions, and tasks.
2. **Gradle Configuration Cache**:
   - Dropped global `--configuration-cache` from profile-wide default `java.import.gradle.arguments` to avoid failing non-compliant multi-project builds; preserved `--parallel --build-cache`.
3. **Shell Detection Scope (D1)**:
   - Replaced un-scoped `*.sh` matching with dedicated test suite/linter markers (`test/**/*.bats`, `tests/**/*.bats`, `*.bats`, `.shellcheckrc`) to prevent masking `github-workflows` or root polyglot repositories.
4. **JDK 28 EA Runtime Support**:
   - Defined supported LTS runtime matrix (`JavaSE-25`, `JavaSE-21`, `JavaSE-17`) with version-gated Javac-backed compilation contract (`"java.jdt.ls.javac.enabled": "on"`, `"java.completion.engine": "dom"`) for experimental JDK 28 EA preview.

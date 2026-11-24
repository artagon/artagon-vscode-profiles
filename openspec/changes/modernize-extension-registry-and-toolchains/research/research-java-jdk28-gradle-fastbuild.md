## Bottom line

For **JDK 28 EA**, the fastest *stable* setup is **separate toolchains**:

1. **VS Code Java language server**: use **Javac-backed JDT LS** only when you need JDK 28 EA / preview syntax; otherwise keep default **ECJ** for mature editor performance.
2. **Gradle daemon / Build Server**: **do not run Gradle on JDK 28 EA** right now. Gradle 9.7.0’s compatibility matrix says Gradle requires a JVM **17–26** to run and JVM **27+ is not yet supported**; use JDK 26 to run Gradle and use a JDK 28 toolchain for compilation experiments. ([docs.gradle.org](https://docs.gradle.org/current/userguide/compatibility.html))
3. **Biggest measured wins** come from Gradle work avoidance/parallelism: configuration cache, build cache, daemon, parallel execution, file-system watching, and Tooling API parallelism—not from hidden `javac` flags. Gradle’s docs report parallel-task tuning examples of **8s → 5s**, daemon reference builds at **3s**, build-cache reference builds at **5s**, and Gradle’s configuration-cache blog reports **2m04s → 55s** on a ~600-project adopter build plus synthetic cache store/load improvements of **27s → 15s** and **3.6s → 1.5s**. ([docs.gradle.org](https://docs.gradle.org/current/userguide/performance.html))

---

## Recommended `.vscode/settings.json`

Use this as the **JDK 28 EA / preview-language profile**. Replace paths.

```jsonc
{
  // --- Java LS / JDT LS ---
  "java.jdt.ls.java.home": "/opt/jdks/jdk-28-ea",
  "java.jdt.ls.javac.enabled": "on",
  "java.completion.engine": "dom",

  // Good default from vscode-java, enlarged for medium/large workspaces.
  // Use 2G for small projects, 4G for medium, 6-8G only for very large monorepos.
  "java.jdt.ls.vmargs": "-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable",

  "java.jdt.ls.appcds.enabled": "on",
  "java.sharedIndexes.enabled": "on",

  // --- Incremental editor builds ---
  "java.autobuild.enabled": true,
  "java.maxConcurrentBuilds": 1,

  // Use "interactive" if Gradle import is expensive; use "automatic" for small/medium projects.
  "java.configuration.updateBuildConfiguration": "interactive",

  // Avoid scanning non-source churn. Do not exclude generated-source dirs unless your sourceSets
  // correctly add them back.
  "java.project.resourceFilters": [
    "node_modules",
    ".git",
    ".gradle"
  ],

  // Optional: reduce expensive UI/lens work in large codebases.
  "java.referencesCodeLens.enabled": false,
  "java.completion.maxResults": 500,

  // --- Gradle / Build Server ---
  "java.gradle.buildServer.enabled": "on",
  "java.gradle.buildServer.openBuildOutput": "openOnBuildFailure",

  // Important: run Gradle daemon/Build Server on a supported JDK, not JDK 28 EA.
  "java.import.gradle.java.home": "/opt/jdks/jdk-26",

  "java.import.gradle.wrapper.enabled": true,
  "java.import.gradle.offline.enabled": false,

  // Keep IDE sync aligned with the command line. Prefer gradle.properties for persistent values.
  "java.import.gradle.arguments": "--configuration-cache --build-cache --parallel",

  // These are JVM args for Gradle import/Tooling API daemon interaction, not the JDT LS VM.
  "java.import.gradle.jvmArguments": "-Xms512m -Xmx4g -XX:MaxMetaspaceSize=1g -Dfile.encoding=UTF-8",

  // Gradle extension UX.
  "gradle.autoDetect": "on",
  "gradle.nestedProjects": false,
  "gradle.reuseTerminals": "task",
  "gradle.allowParallelRun": false,
  "gradle.debug": false
}
```

**Why `javac.enabled: "on"` only for JDK 28 EA?** `vscode-java`’s Javac-based compilation is explicitly **experimental**; it replaces ECJ compilation inside the language server with `javac`, while preserving language-server features. The setting is `java.jdt.ls.javac.enabled`, and the completion engine can be switched to `dom` so completion consumes the Javac-generated model. ([github.com](https://github.com/redhat-developer/vscode-java/wiki/Javac%E2%80%90based-compilation-support))

For day-to-day **non-EA Java**, prefer this conservative profile instead:

```jsonc
{
  "java.jdt.ls.java.home": "/opt/jdks/jdk-26",
  "java.jdt.ls.javac.enabled": "off",
  "java.completion.engine": "ecj",
  "java.jdt.ls.vmargs": "-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable",
  "java.autobuild.enabled": true,
  "java.maxConcurrentBuilds": 1,
  "java.gradle.buildServer.enabled": "on",
  "java.import.gradle.java.home": "/opt/jdks/jdk-26"
}
```

---

## Recommended `gradle.properties`

Put this in the **project root** for team-consistent behavior, or in `~/.gradle/gradle.properties` for local-only defaults.

```properties
# Run Gradle on supported JDK 26, not JDK 28 EA.
org.gradle.java.home=/opt/jdks/jdk-26

# Let Gradle find EA/stable toolchains.
org.gradle.java.installations.paths=/opt/jdks/jdk-28-ea,/opt/jdks/jdk-26
org.gradle.java.installations.auto-detect=true
org.gradle.java.installations.auto-download=false

# Core speedups.
org.gradle.daemon=true
org.gradle.parallel=true
org.gradle.caching=true
org.gradle.configuration-cache=true

# Gradle 8.11+ / current Gradle: faster configuration-cache store/load; incubating.
org.gradle.configuration-cache.parallel=true

# Gradle 9.4+: IDE/Tooling API model builders can run in parallel independently.
org.gradle.tooling.parallel=true

# Tune to cores and memory. Start at physical cores - 1, cap if tests/annotation processors are memory-heavy.
org.gradle.workers.max=8

# Gradle daemon heap. Use 2g small, 4g medium, 6-8g large monorepo.
org.gradle.jvmargs=-Xms1g -Xmx4g -XX:MaxMetaspaceSize=1g -XX:+UseParallelGC -Dfile.encoding=UTF-8

# Reuse file-system state between builds; default is true on supported OSes, but make it explicit.
org.gradle.vfs.watch=true

# Keep output cheaper for IDE terminals.
org.gradle.console=auto
org.gradle.warning.mode=summary
```

Gradle documents `org.gradle.configuration-cache`, `org.gradle.daemon`, `org.gradle.parallel`, `org.gradle.caching`, `org.gradle.jvmargs`, `org.gradle.workers.max`, `org.gradle.vfs.watch`, and `org.gradle.tooling.parallel` as supported build-environment properties; the default Gradle daemon heap is only `-Xmx512m` with `MaxMetaspaceSize=384m`, so larger multi-project builds commonly need a bigger daemon heap. ([docs.gradle.org](https://docs.gradle.org/current/userguide/build_environment.html))

---

## JDK 28 EA Gradle build configuration

Use **toolchains**, not `forkOptions.javaHome` or a custom `javac` executable, because Gradle warns that custom `executable` / `javaHome` disables some incremental-compile optimizations and deactivates incremental annotation processing. ([docs.gradle.org](https://docs.gradle.org/current/userguide/java_plugin.html?utm_source=openai))

### `build.gradle.kts`

```kotlin
plugins {
    java
}

java {
    toolchain {
        languageVersion.set(JavaLanguageVersion.of(28))
    }
}

tasks.withType<JavaCompile>().configureEach {
    // Needed for JDK 28 preview features.
    options.release.set(28)
    options.compilerArgs.add("--enable-preview")

    // Default with the Java plugin is true; keep it explicit for audits.
    options.isIncremental = true

    // Do NOT set options.forkOptions.javaHome or executable for JDK 28.
    // Let Gradle toolchains select the compiler.
}

tasks.withType<Test>().configureEach {
    jvmArgs("--enable-preview")
    maxParallelForks = (Runtime.getRuntime().availableProcessors() / 2).coerceAtLeast(1)
}
```

### `build.gradle`

```groovy
plugins {
    id 'java'
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(28)
    }
}

tasks.withType(JavaCompile).configureEach {
    options.release = 28
    options.compilerArgs += ['--enable-preview']
    options.incremental = true
}

tasks.withType(Test).configureEach {
    jvmArgs '--enable-preview'
    maxParallelForks = Math.max(1, Runtime.runtime.availableProcessors().intdiv(2))
}
```

JEP 401 value classes/objects are preview language/VM features and must be compiled with `--enable-preview` and run with `--enable-preview`; the JEP text also states value objects are intended to improve memory footprint, locality, and GC freedom, but you should benchmark your own code because the payoff depends on object shapes and access patterns. ([openjdk.org](https://openjdk.org/jeps/401?utm_source=openai))

---

## Compiler choice: ECJ vs JDT LS Javac vs Gradle `javac`

| Layer | Recommended default | Use for JDK 28 EA? | Why |
|---|---:|---:|---|
| **VS Code JDT LS + ECJ** | ✅ Yes | ⚠️ Only if ECJ supports the syntax you use | Mature, default, fast incremental editor compiler. Eclipse JDT has an incremental builder that recompiles changed/affected files and tracks dependencies. ([help.eclipse.org](https://help.eclipse.org/latest/topic/org.eclipse.platform.doc.isv/guide/resAdv_builders.htm)) |
| **VS Code JDT LS + Javac** | ⚠️ Experimental | ✅ Best EA option | Enables `javac`-based LS compilation via `java.jdt.ls.javac.enabled: "on"` and `java.completion.engine: "dom"`; intended to ease testing preview/unreleased Java, but still experimental. ([github.com](https://github.com/redhat-developer/vscode-java/wiki/Javac%E2%80%90based-compilation-support)) |
| **Gradle JavaCompile / `javac`** | ✅ Yes | ✅ For actual builds | Use Gradle toolchains + `--enable-preview`; Gradle incremental compilation and build-cache avoidance matter more than raw compiler flags. ([docs.gradle.org](https://docs.gradle.org/current/userguide/java_plugin.html?utm_source=openai)) |

**No public VS Code benchmark currently proves Javac-backed JDT LS is faster than ECJ.** Microsoft’s Javac-support announcement framed it as an alternative to ECJ and listed future work to remove performance slowdowns and investigate benchmark tests; therefore, treat it as a **correctness/EA-language enabler**, not a proven speed win. ([devblogs.microsoft.com](https://devblogs.microsoft.com/java/annoucing-javac-support-in-visual-studio-code-java/?utm_source=openai))

---

## Settings to avoid unless you measure a win

```jsonc
{
  // Avoid this for most Gradle workspaces; it can cause constant project re-imports while editing build logic.
  "java.configuration.updateBuildConfiguration": "automatic",

  // Avoid >1 unless you have lots of RAM and independent projects.
  "java.maxConcurrentBuilds": 4,

  // Avoid when relying on JDT LS for instant diagnostics.
  "java.autobuild.enabled": false
}
```

`java.autobuild.enabled` controls Java auto-builds and `java.maxConcurrentBuilds` controls simultaneous project builds; auto-builds give fast diagnostics, but in very large workspaces they can contend with Gradle Build Server, test runs, and annotation processors. The JDT incremental builder is valuable because it uses deltas and dependency state rather than rebuilding everything. ([github.com](https://github.com/redhat-developer/vscode-java))

---

## Benchmarked / documented performance benefits

| Optimization | Exact key / setting | Recommended value | Reported benefit |
|---|---|---:|---|
| Gradle daemon | `org.gradle.daemon` | `true` | Gradle says daemon reduces JVM startup, caches project info, benefits from JIT warmup, and watches FS; its reference project shows daemon build time of **3s**. ([docs.gradle.org](https://docs.gradle.org/current/userguide/performance.html)) |
| Parallel execution | `org.gradle.parallel` | `true` | Gradle example shows parallel scheduling/tuning reducing **8s → 5s**; benefit depends on project independence. ([docs.gradle.org](https://docs.gradle.org/current/userguide/performance.html)) |
| Build cache | `org.gradle.caching` | `true` | Avoids task execution by reusing outputs; Gradle example shows `:compileJava FROM-CACHE` after clean and reference build-cache time of **5s**. ([docs.gradle.org](https://docs.gradle.org/current/userguide/build_cache.html)) |
| Configuration cache | `org.gradle.configuration-cache` | `true` after compatibility fixes | Skips configuration phase on hits; especially useful in IDE test/run loops and large builds. ([docs.gradle.org](https://docs.gradle.org/current/userguide/best_practices_performance.html)) |
| Parallel configuration-cache store/load | `org.gradle.configuration-cache.parallel` | `true` for decoupled multiproject builds | Gradle blog: ~600-project adopter **2m04s → 55s** config time; synthetic **27s → 15s** store and **3.6s → 1.5s** load. ([blog.gradle.org](https://blog.gradle.org/road-to-configuration-cache)) |
| Tooling API parallelism | `org.gradle.tooling.parallel` | `true` | Gradle says this can improve IDE sync and can be controlled separately from task parallelism since Gradle 9.4.0. ([docs.gradle.org](https://docs.gradle.org/current/userguide/build_environment.html)) |
| File-system watching | `org.gradle.vfs.watch` | `true` | Lets Gradle reuse file-system state between builds; default is true where supported. ([docs.gradle.org](https://docs.gradle.org/current/userguide/build_environment.html)) |
| JDT LS heap/GC | `java.jdt.ls.vmargs` | ParallelGC + `-Xmx2G` to `-Xmx4G` | The extension’s default is ParallelGC with `-Xmx2G`; increasing heap mainly prevents GC thrash/OOM in large workspaces, not a guaranteed speedup. ([github.com](https://github.com/redhat-developer/vscode-java/blob/main/package.json)) |
| Javac-backed JDT LS | `java.jdt.ls.javac.enabled` | `"on"` for JDK EA | Helps test preview/unreleased Java through Javac; not yet a proven performance improvement over ECJ. ([github.com](https://github.com/redhat-developer/vscode-java/wiki/Javac%E2%80%90based-compilation-support)) |

---

## Recommended measurement commands

Run these before/after changing settings:

```bash
# Gradle configuration/build timing with scan data
./gradlew help --scan
./gradlew compileJava --scan
./gradlew test --scan

# Configuration cache validation
./gradlew help --configuration-cache
./gradlew compileJava --configuration-cache
./gradlew compileJava --configuration-cache --info

# Build cache validation
./gradlew clean compileJava --build-cache --info

# Check JDKs and toolchains
./gradlew javaToolchains
./gradlew --version
```

For JDK 28 EA specifically, keep a record of:

```bash
/opt/jdks/jdk-28-ea/bin/java --version
/opt/jdks/jdk-26/bin/java --version
./gradlew --version
```

This matters because Gradle currently supports running on JVM 17–26, while JDK 28 EA should be treated as an experimental **compile/test toolchain**, not the Gradle runtime. ([docs.gradle.org](https://docs.gradle.org/current/userguide/compatibility.html))

Sources:
[1] State of the Configuration Cache - On the Road to Gradle 9 (https://blog.gradle.org/road-to-configuration-cache)
[2] Announcing Javac support in Visual Studio Code Java - Microsoft for Java Developers (https://devblogs.microsoft.com/java/annoucing-javac-support-in-visual-studio-code-java/?utm_source=openai)
[3] Best Practices for Performance (https://docs.gradle.org/current/userguide/best_practices_performance.html)
[4] Build Cache (https://docs.gradle.org/current/userguide/build_cache.html)
[5] Build Environment Configuration (https://docs.gradle.org/current/userguide/build_environment.html)
[6] Compatibility Matrix (https://docs.gradle.org/current/userguide/compatibility.html)
[7] The Java Plugin (https://docs.gradle.org/current/userguide/java_plugin.html?utm_source=openai)
[8] Improve the Performance of Gradle Builds (https://docs.gradle.org/current/userguide/performance.html)
[9] GitHub - redhat-developer/vscode-java: Java Language Support for Visual Studio Code · GitHub (https://github.com/redhat-developer/vscode-java)
[10] vscode-java/package.json at main · redhat-developer/vscode-java · GitHub (https://github.com/redhat-developer/vscode-java/blob/main/package.json)
[11] Javac‐based compilation support · redhat-developer/vscode-java Wiki · GitHub (https://github.com/redhat-developer/vscode-java/wiki/Javac%E2%80%90based-compilation-support)
[12] Incremental project builders (https://help.eclipse.org/latest/topic/org.eclipse.platform.doc.isv/guide/resAdv_builders.htm)
[13] JEP 401: Value Classes and Objects (Preview) (https://openjdk.org/jeps/401?utm_source=openai)

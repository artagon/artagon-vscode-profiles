## ADDED Requirements

### Requirement: Modern Java Runtimes & JDK 28 EA Preview Support
The Java profile overrides (`_overrides/java-gradle.jsonc`, `_overrides/java-maven.jsonc`, `_overrides/java-spring.jsonc`) SHALL configure `java.configuration.runtimes` for supported LTS/GA releases (`JavaSE-25`, `JavaSE-21`, `JavaSE-17`), with dynamic fallback to `${command:jenv.javaHome}` or standard system `JAVA_HOME`. For JDK 28 Early Access preview features, the system SHALL support version-gated Javac-based JDT LS compilation (`"java.jdt.ls.javac.enabled": "on"`, `"java.completion.engine": "dom"`).

#### Scenario: Standard LTS runtime resolution
- **WHEN** a Java workspace targeting Java 17, 21, or 25 is opened
- **THEN** VS Code Java language server matches the appropriate runtime without configuration warnings.

#### Scenario: JDK 28 EA preview compilation
- **WHEN** a workspace opts into JDK 28 EA preview features
- **THEN** Javac-backed compilation activates without breaking standard language server features.

### Requirement: Fast Java Building & Language Server Acceleration
The Java settings overrides SHALL configure high-throughput compilation and language server performance parameters:
1. Low-latency parallel GC and heap allocation (`java.jdt.ls.vmargs: "-XX:+UseParallelGC -XX:GCTimeRatio=4 -XX:AdaptiveSizePolicyWeight=90 -Dsun.zip.disableMemoryMapping=true -Xms512m -Xmx4G -Xlog:disable"`).
2. AppCDS and shared index caching enabled (`"java.jdt.ls.appcds.enabled": "on"`, `"java.sharedIndexes.enabled": "on"`).
3. Non-blocking autobuild execution (`"java.autobuild.enabled": true`, `"java.maxConcurrentBuilds": 1`).
4. Automatic null analysis (`"java.compile.nullAnalysis.mode": "automatic"`).
5. Resource filters excluding non-source churn (`"java.project.resourceFilters": ["node_modules", ".git", ".gradle", "build"]`).

#### Scenario: Incremental compilation
- **WHEN** Java source files are modified
- **THEN** incremental compilation triggers asynchronously without blocking UI or language features.

### Requirement: Modern Gradle Integration and Build Server
The `java-gradle` and `java-spring` profiles SHALL enforce official `vscjava.vscode-gradle` tooling with build acceleration:
1. Purge legacy `richardwillis.vscode-gradle` extension from all layers.
2. Enable the high-performance Gradle Build Server (`"java.gradle.buildServer.enabled": "on"`).
3. Enforce toolchain separation: run the Gradle daemon / Build Server on a supported JDK (21/25/26) while targeting language preview versions via Gradle toolchains.
4. Enable parallel execution and build caching (`"java.import.gradle.arguments": ["--parallel", "--build-cache"]`).
5. Allocate dedicated high-throughput JVM memory for Gradle daemon/import (`"java.import.gradle.jvmArguments": "-Xms512m -Xmx4g -XX:MaxMetaspaceSize=1g -Dfile.encoding=UTF-8"`).

#### Scenario: Gradle build acceleration
- **WHEN** Gradle projects are imported or synchronized
- **THEN** the Gradle Build Server utilizes build caching and parallel execution for fast sync and compilation.

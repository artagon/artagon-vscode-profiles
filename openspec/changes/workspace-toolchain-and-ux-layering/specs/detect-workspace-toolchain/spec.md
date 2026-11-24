## ADDED Requirements

### Requirement: Toolchain detection from workspace signals

The system SHALL detect the workspace toolchain by inspecting files at the
workspace root in a deterministic precedence order. The precedence (highest
to lowest, used for both single-match selection and polyglot stacking
order) SHALL be:

1. `rust` — `Cargo.toml` present
2. `cpp-clangd` — `CMakeLists.txt` present AND `.clangd` present
3. `cpp-intellisense` — `CMakeLists.txt` present AND `.clangd` absent
4. `java-spring` — at least one `build.gradle` or `build.gradle.kts` present
   AND at least one file under `src/main/java/**/*Application.java` whose
   contents include the literal `@SpringBootApplication`
5. `java-gradle` — `build.gradle` or `build.gradle.kts` present, no Spring
   marker
6. `java-maven` — `pom.xml` present
7. `astro` — `package.json` present AND `astro.config.{mjs,ts,js,cjs}`
   present
8. `web` — `package.json` present, no `astro.config.*` (reserved for future
   typescript flavor; emits no toolchain layer in this round)
9. `github-workflows` — `.github/workflows/*.yml` present AND no other
   toolchain signal

The `ai` and `ai-plus` flavors SHALL NEVER be auto-detected; they are
opt-in via `--toolchain ai` or `--toolchain ai-plus` only.

#### Scenario: Single rust workspace

- **WHEN** the workspace root contains `Cargo.toml` and no other matching
  signals
- **THEN** `vspcli --detect` writes `rust` as the detected toolchain to
  stdout (one line, exit 0).

#### Scenario: Polyglot Tauri repo

- **WHEN** the workspace root contains both `Cargo.toml` and
  `package.json` + `astro.config.mjs`
- **THEN** `vspcli --detect` writes `rust astro` to stdout (space-separated,
  in precedence order) and exit 0.

#### Scenario: Empty workspace

- **WHEN** none of the precedence-list signals match
- **THEN** `vspcli --detect` exits with status 3 and writes
  `no toolchain detected; pass --toolchain <flavor> or --no-detect` to
  stderr.

#### Scenario: Spring promotion

- **WHEN** the workspace root contains `build.gradle.kts` AND a file
  under `src/main/java/com/example/MyApplication.java` containing
  `@SpringBootApplication`
- **THEN** `--detect` reports `java-spring`, not `java-gradle`.

#### Scenario: clangd promotion

- **WHEN** the workspace root contains `CMakeLists.txt` AND a `.clangd`
  file
- **THEN** `--detect` reports `cpp-clangd`, not `cpp-intellisense`.

### Requirement: Manual toolchain override

The system SHALL accept a `--toolchain <flavor>` flag that bypasses
detection entirely and uses the named flavor as the only toolchain layer
for the run. The system SHALL accept a `--no-detect` flag that disables
detection and requires either `--toolchain` or a profile name to be
supplied.

#### Scenario: Override forces single layer

- **WHEN** the user runs `vspcli --detect --toolchain rust` in an empty
  workspace
- **THEN** the CLI emits `rust` (overriding the empty-workspace error)
  and exits 0.

#### Scenario: Override beats polyglot stacking

- **WHEN** the user runs `vspcli --detect --toolchain rust` in a Tauri
  repo (Cargo.toml + astro.config.mjs)
- **THEN** the CLI emits only `rust` (suppressing the astro stacking)
  and exits 0.

#### Scenario: --no-detect requires explicit toolchain

- **WHEN** the user runs `vspcli --no-detect` without `--toolchain`
- **THEN** the CLI exits with status 4 and writes
  `--no-detect requires --toolchain <flavor>` to stderr.

#### Scenario: Unknown toolchain flavor

- **WHEN** the user runs `--toolchain unknown-flavor`
- **THEN** the CLI exits with status 4 and writes
  `unknown toolchain 'unknown-flavor'; valid: ai, ai-plus, astro,
  cpp-clangd, cpp-intellisense, github-workflows, java-gradle, java-maven,
  java-spring, rust, web` to stderr.

### Requirement: JSON output mode

The system SHALL accept `--json` to emit detection results as a JSON
object on stdout suitable for piping to `jq`. The object SHALL contain
keys `toolchains` (array of detected flavors in precedence order),
`signals` (object mapping each detected flavor to the array of files that
matched its signal), and `workspace` (absolute path of the inspected
root).

#### Scenario: JSON output for polyglot repo

- **WHEN** the user runs `vspcli --detect --json` in a Tauri repo
- **THEN** stdout contains a JSON object equivalent to
  `{"toolchains":["rust","astro"],"signals":{"rust":["Cargo.toml"],
  "astro":["package.json","astro.config.mjs"]},"workspace":"/abs/path"}`

#### Scenario: JSON output for empty workspace

- **WHEN** the user runs `vspcli --detect --json` in a directory with
  no detection signals
- **THEN** stdout contains `{"toolchains":[],"signals":{},"workspace":
  "/abs/path"}`; the CLI exits with status 3 (no detection); stderr
  carries the `no toolchain detected` notice as in the non-JSON case.

#### Scenario: JSON output with --toolchain override

- **WHEN** the user runs `vspcli --detect --json --toolchain rust` in
  an empty directory
- **THEN** stdout contains `{"toolchains":["rust"],"signals":{"rust":
  []},"workspace":"/abs/path","override":true}`; exit 0. The
  `signals.rust` array is empty (no file matched; the toolchain came
  from the override) and the JSON object contains an `override: true`
  marker so consumers can distinguish detection from override.

#### Scenario: JSON output with --no-detect missing --toolchain

- **WHEN** the user runs `vspcli --detect --json --no-detect`
- **THEN** stdout contains nothing (no JSON written for argument-error
  exit codes); stderr contains `--no-detect requires --toolchain
  <flavor>`; exit 4.

### Requirement: Exit code contract

The system SHALL use the following exit codes:

- 0 — detection succeeded and at least one toolchain was identified.
- 2 — internal error (e.g., unable to read the workspace root).
- 3 — no toolchain detected and no `--toolchain` override provided.
- 4 — invalid arguments (`--no-detect` without `--toolchain`, unknown
  flavor, mutually-exclusive flags).

#### Scenario: Detection success

- **WHEN** detection finds at least one toolchain
- **THEN** the script exits 0.

#### Scenario: No detection no override

- **WHEN** detection finds nothing and the user did not supply
  `--toolchain`
- **THEN** the script exits 3.

### Requirement: Detection script independence

The detection SHALL be implemented as a standalone shell script
(`scripts/detect-toolchain.sh`) that the `vspcli --detect` subcommand
delegates to. The script SHALL be invokable directly without `vspcli` so
CI workflows and other tools can consume it. The detection contract
(stdout = toolchain names, stderr = errors, exit codes 0/2/3/4) is
identical between the two entry points; `vspcli --detect` additionally
chains into the install + compose pipeline (writing
`.vscode/extensions.json`, `.vscode/settings.json`, `.vscode/tasks.json`)
which the standalone script does not perform.

#### Scenario: Script invoked directly produces same detection output

- **WHEN** the user runs `bash scripts/detect-toolchain.sh /path/to/repo`
  AND in a separate invocation `vspcli --detect --no-install /path/to/repo`
- **THEN** both produce the same stdout (toolchain names), the same
  stderr (errors, if any), and the same exit code. (The
  `--no-install` flag is the documented way to invoke `vspcli --detect`
  with detection-only semantics; without it, vspcli proceeds to install
  + compose, which produces additional stdout and may exit non-zero on
  install failures.)

#### Scenario: Script supports --json with same shape

- **WHEN** the user runs `bash scripts/detect-toolchain.sh --json /path`
- **THEN** the JSON output shape matches `vspcli --detect --json /path`
  exactly per the "JSON output mode" requirement.

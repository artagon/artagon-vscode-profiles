# Advanced Tooling and Configuration for Shell Script Development in Visual Studio Code

The development of shell scripts—encompassing Bash, Zsh, and POSIX `sh`—has historically been relegated to rudimentary text editors that lack the robust, intelligent tooling characteristic of higher-level programming environments. However, the modernization of continuous integration and continuous deployment (CI/CD) pipelines, infrastructure as code (IaC) paradigms, and complex containerized automation systems has elevated shell scripts to critical infrastructure components [cite: 1]. The necessity for an Integrated Development Environment (IDE) that provides deterministic formatting, deep static analysis, interactive debugging, and automated testing for shell scripts has become a paramount concern for DevOps engineers and site reliability experts.

Visual Studio Code (VS Code) has firmly established itself as the premier environment for this undertaking. By 2025, VS Code captured an overwhelming 75.9% of the professional developer market share, driven primarily by its modular architecture and vast extension ecosystem [cite: 2]. Transforming VS Code into a comprehensive IDE for shell scripting requires assembling a highly specific matrix of extensions. This report delivers an exhaustive architectural analysis of the optimal VS Code extensions, their underlying mechanisms, and the configuration parameters required to construct a state-of-the-art shell script development environment. The analysis strictly focuses on Language Server Protocol (LSP) integrations, the Bash Automated Testing System (BATS), syntax highlighting enhancements derived from the `bat` ecosystem, linting via ShellCheck, Abstract Syntax Tree (AST)-based formatting via `shfmt`, and Debug Adapter Protocol (DAP) implementations.

## Architectural Foundations: The Language Server Protocol (LSP)

Visual Studio Code’s intelligent features—such as context-aware auto-completion, hover documentation, signature help, and jump-to-definition—are decoupled from the core editor engine through the Language Server Protocol (LSP) [cite: 3]. This architectural decision is highly consequential for shell scripting. Parsing shell dialects necessitates complex AST generation that, if executed within the editor's main thread, would severely degrade runtime performance, particularly in large monolithic repositories [cite: 3]. The LSP delegates this resource-intensive parsing to an external, specialized language analysis tool running in a separate process [cite: 3].

### The Bash Language Server Engine

The definitive extension for providing LSP capabilities for shell scripts within VS Code is `mads-hartmann.bash-ide-vscode` [cite: 4]. This extension operates as a client that instantiates and communicates natively with the backend `bash-language-server` [cite: 3].

Unlike the built-in `vscode.shellscript` extension, which relies on rudimentary regular expressions for TextMate grammar highlighting and basic bracket matching [cite: 5], the `bash-language-server` is built upon the Tree-sitter parsing library. Tree-sitter constructs a concrete syntax tree incrementally and in real-time as the developer modifies the code, enabling the language server to comprehensively understand the structural and semantic context of the script [cite: 6]. This semantic awareness allows the `mads-hartmann.bash-ide-vscode` extension to provide capabilities that bridge the gap between shell scripts and typed languages [cite: 4]:

*   **Symbol Resolution:** The extension provides accurate "Jump to Declaration" and "Find References" functionalities across multiple files sourced within a workspace, an operation that simpler regex-based engines fail to execute reliably [cite: 4, 7].
*   **Contextual Auto-completion:** It delivers intelligent suggestions for variables and functions based on local, global, and exported scope topologies [cite: 4].
*   **Code Outline & Workspace Symbols:** The AST powers a hierarchical outline view, allowing engineers to navigate large shell scripts by function definitions rather than line numbers [cite: 4, 6].

### Advanced ExplainShell Integration

A highly sophisticated capability inherent to the `mads-hartmann.bash-ide-vscode` extension is its integration with `explainshell`. By parsing the AST, the language server identifies specific command-line utilities and their associated flags. When a developer hovers over a complex pipeline (e.g., `tar -xzvf archive.tar.gz | awk '{print $1}'`), the extension queries an explainshell endpoint to provide a precise, interactive manual page breakdown of what each flag (`-x`, `-z`, `-v`, `-f`) executes [cite: 4].

To activate this feature securely, a local instance of the explainshell server must be deployed, typically via a localized Docker image to circumvent network latency and corporate security egress restrictions [cite: 4]. The connection is subsequently established in the `settings.json` configuration space via the `"bashIde.explainshellEndpoint"` attribute [cite: 4].

### Addressing Zsh and POSIX sh Limitations

While the `bash-language-server` is heavily optimized for Bash, shell dialects introduce significant edge cases. Zsh possesses an expansive, heavily modified syntax that deviates profoundly from Bourne-compatible shells. The Tree-sitter Bash parser occasionally struggles with advanced Zsh parameter expansions, associative arrays, and globbing qualifiers. Furthermore, alternative implementations like `rail5.bashpp`, which connects to a `bpp-lsp` backend, are tailored strictly for the Bash++ superset and silently degrade to basic syntax highlighting if the external server binary is absent [cite: 8].

To mitigate these limitations for Zsh development, engineers must leverage recent advancements in VS Code's Integrated Terminal shell integration. Features such as `terminal-suggest` dynamically pull aliases and built-ins for Zsh directly from the active shell session's memory, presenting them as IntelliSense ghost text in the editor terminal [cite: 9]. For pure Zsh environments, the `tolkonepiu.vscode-shell-integration-zsh-plugin` ensures that VS Code’s shell integration script is forcefully loaded, enabling rich directory tracking and command decorations that bridge the intelligence gap where the traditional LSP falls short [cite: 10].

| Extension ID | Publisher | Architectural Role | Dialect Optimization |
| :--- | :--- | :--- | :--- |
| `mads-hartmann.bash-ide-vscode` | Mads Hartmann | LSP Client & IntelliSense | Bash, POSIX `sh` |
| `tolkonepiu.vscode-shell-integration-zsh-plugin`| Tolkonepiu | Integrated Terminal Sync | Zsh |
| `rail5.bashpp` | Andrew S. Rightenburg | Fallback Syntax/LSP | Bash++ |

## Static Code Analysis and the ShellCheck Engine

Shell scripts are notoriously prone to silent failures, variable expansion errors, unintended word splitting, and scope leakage. Because shell is an interpreted language, these flaws frequently manifest exclusively at runtime, often causing catastrophic operational failures in production environments. Rigorous static analysis is therefore a non-negotiable requirement for enterprise shell scripting.

### The ShellCheck Ecosystem

The undisputed industry standard for shell script static analysis is ShellCheck. Written in Haskell, ShellCheck operates as a highly specialized parser that cross-references scripts against an exhaustive database of common pitfalls, POSIX compliance violations, and deprecated syntax [cite: 11]. It identifies semantic vulnerabilities such as unquoted variables that are susceptible to globbing, misused `eval` statements, and incorrectly formatted shebangs.

Within the VS Code ecosystem, the `timonwong.shellcheck` extension provides the most robust and architecturally sound integration [cite: 12]. This extension functions as an execution bridge, asynchronously running the ShellCheck binary against the current document and mapping the standard output into VS Code Diagnostics. These diagnostics are rendered natively as squiggly underlines in the editor viewport and categorized chronologically in the IDE's Problems panel.

### Execution Strategies and Binary Management

A significant architectural advantage of `timonwong.shellcheck` over competing linters is that it bundles precompiled ShellCheck binaries for `x86_64`, `arm64`, and `arm` architectures across Linux, macOS, and Windows environments [cite: 13]. This self-contained distribution eliminates complex external dependency management.

However, organizations enforcing strict binary supply chain security may require the extension to execute an enterprise-approved version of ShellCheck. Developers can override the bundled binary via the `shellcheck.executablePath` setting [cite: 12]. Some infrastructures necessitate running ShellCheck via a Docker container shim to ensure environment parity; while the extension supports mapping the workspace root as the current working directory (`shellcheck.useWorkspaceRootAsCwd`) for Docker volume mounts, executing a containerized binary introduces an unavoidable performance latency overhead during continuous typing [cite: 12].

### Dialect Specificity and Ignored Patterns

ShellCheck dynamically adjusts its strictness heuristically based on the script's shebang. If a script initiates with `#!/bin/sh`, ShellCheck enforces strict POSIX compliance, immediately throwing diagnostic errors for Bashisms such as `[[ ... ]]` conditionals, array definitions, or the `source` command [cite: 11]. Conversely, `#!/bin/bash` relaxes these constraints up to the supported Bash version.

Zsh presents a unique challenge. ShellCheck does not natively support Zsh semantics and will generate widespread false positives when parsing Zsh-specific syntax [cite: 12]. To maintain a clean VS Code Problems panel in polyglot shell environments, engineers must utilize the `shellcheck.ignorePatterns` configuration. This attribute accepts glob patterns to forcefully prevent the linter from evaluating Zsh profiles, such as `**/*.zshrc`, `**/*.zprofile`, and `**/zshenv` [cite: 12].

## Deterministic AST-Based Code Formatting

Code formatting in shell scripting transcends aesthetic preference; due to the language's reliance on spaces for argument parsing and conditional evaluations, incorrect formatting frequently causes syntax errors. Standardizing shell scripts against a unified style guide—most notably the Google Shell Style Guide—is a standard practice in enterprise CI/CD pipelines to eliminate trivial code review disputes [cite: 14].

### The shfmt Parser

The command-line tool `shfmt`, authored in Go, is the authoritative standard for shell script formatting [cite: 15]. Unlike simpler formatters that utilize fragile regular expressions to indiscriminately inject whitespace, `shfmt` acts as a complete shell parser. It ingests the script, builds an AST, normalizes the tree based on predefined stylistic rules, and reprints the code. This AST-driven approach guarantees that the formatting process cannot inadvertently alter the operational logic of the script [cite: 15].

For VS Code integration, the `mkhl.shfmt` extension is highly recommended [cite: 16]. While other extensions, such as `foxundermoon.shell-format`, exist and wrap similar underlying binaries [cite: 17, 18], `mkhl.shfmt` is specifically tailored to integrate tightly with VS Code's native `Format Document` API and respects EditorConfig specifications natively [cite: 16]. Furthermore, `bdq460.shell-formatter` attempts to combine both formatting and error fixing, but separating concerns between `mkhl.shfmt` and `timonwong.shellcheck` yields a more stable and modular IDE configuration [cite: 19].

### Formatting Execution and Constraints

`shfmt` handles multiple shell dialects, which are internally controlled via the `-ln` (language dialect) flag. It natively supports parsing `bash`, `posix`, `mksh`, and `bats` [cite: 15]. The VS Code extension attempts to detect the appropriate dialect automatically based on the file extension and shebang, but engineers must explicitly pass structural flags to the executable to enforce team standards.

Crucial formatting arguments supported by `shfmt` (which are configured in VS Code via the `shfmt.executableArgs` array) dictate the strictness of the code's visual layout:

| Argument Flag | Structural Impact | Enterprise Relevance (Google Shell Style) |
| :--- | :--- | :--- |
| `-i <uint>` | Sets indentation. `0` for tabs, `>0` for specific spaces. | `-i 2` is mandated by the Google Style Guide [cite: 15]. |
| `-ci` | Enforces indentation for switch `case` statements. | Improves visual hierarchy in complex routing logic [cite: 15]. |
| `-bn` | Binary operators (`&&`, `\|\|`) may start a new line. | Enhances readability of long, chained execution pipelines [cite: 15]. |
| `-sr` | Redirect operators (`>`, `<`) are followed by a space. | Ensures consistent visual spacing across I/O operations [cite: 15]. |

Because `shfmt` operates fundamentally by reconstructing the entire AST, it cannot format isolated code blocks. Consequently, the `mkhl.shfmt` extension cannot support the VS Code "Format Selection" command; any formatting action recalculates and reprints the entire document simultaneously [cite: 16]. Additionally, engineers must carefully configure the `shfmt.formatIgnored` setting in conjunction with `.editorconfig` rules to prevent the formatter from bypassing intentionally excluded vendor scripts [cite: 16].

## Automated Testing Frameworks and BATS Integration

Automated software testing is the bedrock of reliable deployments, yet shell scripts are frequently neglected in test coverage, leading to subtle bugs, fragile infrastructure workflows, and catastrophic CI/CD pipeline failures [cite: 1]. The Bash Automated Testing System (BATS) provides a robust, TAP-compliant (Test Anything Protocol) testing framework that finally brings unit testing methodologies to the shell ecosystem [cite: 20].

A `.bats` file is essentially a Bash script enhanced with specialized syntactic decorators [cite: 20]. Test cases are defined using the `@test` decorator, and the framework provides an isolated `run` command to execute external scripts within a sandboxed subshell. This isolation allows the framework to capture the execution `$status` (exit code) and the `$output` (stdout/stderr) for deterministic assertions without polluting the host environment [cite: 1]. Furthermore, BATS isolates the filesystem for each test case by provisioning a fresh `$TEST_TEMP` directory, eliminating cross-test contamination and the "it works on my machine" anti-pattern [cite: 21].

### Language Support and Ast-Driven Snippets

By default, VS Code’s TextMate engine does not recognize `.bats` files, frequently misinterpreting the `@test` decorator as a gross syntax error and failing to provide keyword highlighting [cite: 22]. The `jetmartin.bats` extension resolves this deficiency by injecting a dedicated grammar for the BATS ecosystem [cite: 22]. To ensure BATS syntax stands out from standard Bash code, engineers can utilize the `editor.tokenColorCustomizations` setting to apply distinct foreground colors and italicization to BATS-specific scopes such as `keyword.control.bats` and `support.function.bats` [cite: 22].

Beyond syntax highlighting, `jetmartin.bats` profoundly accelerates test-driven development (TDD) by providing extensive code snippets. Triggered by the `BATS:` prefix, developers can instantly scaffold complex test architectures [cite: 22]:

| Snippet Trigger | Generated Ast Structure | Operational Purpose |
| :--- | :--- | :--- |
| `BATS:env` | `#!/usr/bin/env bats` | Injects the standard BATS shebang for executable tests [cite: 22]. |
| `BATS:setup` | `setup() { ... }` | Defines the initialization lifecycle hook executed before every test [cite: 23]. |
| `BATS:teardown` | `teardown() { ... }` | Defines the cleanup lifecycle hook executed after every test [cite: 23]. |
| `BATS:test` | `@test "description" { ... }` | Generates an isolated test case block with structural scaffolding [cite: 22]. |
| `BATS:load` | `load 'test_helper'` | Sources external assertion libraries like `bats-assert` or `bats-mock` [cite: 21, 22]. |

### Test Runner Execution and Visualization

While writing tests is facilitated by `jetmartin.bats`, executing them requires a seamless interface. The `kherring.bats-test-runner` extension acts as the bridge between the BATS CLI and VS Code's native Test Explorer API [cite: 24]. 

This integration allows engineers to execute individual tests, target specific test suites, or run the entire repository's testing directory directly from the editor margin. The visual interface mirrors the testing ergonomics traditionally reserved for Java or Python [cite: 23, 25]. When utilizing community libraries such as `bats-assert` (which provides commands like `assert_success` and `assert_output`) or `bats-mock` (which intercepts external binary calls), the test runner surfaces detailed diffs between the expected and actual outputs directly within the VS Code Test Results panel, enabling rapid debugging of failing conditions [cite: 21].

## Visual Parsing, TextMate Grammars, and the `bat` Ecosystem

The visual parsing of code is a critical component of developer ergonomics, directly correlating with cognitive load and defect rates. The default `vscode.shellscript` grammar shipped with VS Code is structurally conservative [cite: 5]. It frequently fails to properly tokenize highly nested parameter expansions (e.g., `${var:-default}`), complex arithmetic substitutions (`$(( ... ))`), and deeply nested quoting arrays.

### TextMate Scope Overrides

To rectify this visual deficiency, the `jeff-hykin.better-shellscript-syntax` extension acts as a complete drop-in replacement for the default VS Code grammar [cite: 26, 27]. By re-engineering and overriding the foundational TextMate scopes, it provides hyper-granular tokenization for Bash and Zsh structures. This granular scoping allows advanced VS Code themes to apply distinct chromatic rendering to array indices, string interpolations, and specific control flow keywords, significantly accelerating a developer's ability to visually parse complex scripts [cite: 26, 28].

### The CLI `bat` Integration and Terminal Paging

*Note: It is necessary to explicitly disambiguate legacy Windows batch files (`.bat`), which require extensions like `wwnao.bat-formatter` [cite: 29], from the modern Unix CLI tool `bat` (a `cat` clone written in Rust) [cite: 30].*

For modern shell script developers, the Rust-based `bat` utility has become an indispensable command-line interface tool [cite: 31]. It serves as a superior architectural replacement for the legacy `cat` command, automatically injecting syntax highlighting for over 200 languages, integrating automatic paging via `less`, and displaying real-time Git modification markers directly in the terminal output [cite: 31, 32, 33].

While `bat` operates exclusively within the terminal, its integration into the VS Code workflow is highly synergistic [cite: 33]. Developers frequently alias `cat` to `bat` within their global `.zshrc` or `.bashrc` profiles [cite: 31]. When executing shell scripts, viewing JSON data piped through `jq` [cite: 34], or analyzing output logs within VS Code's Integrated Terminal, `bat` ensures that the output retains the syntax highlighting paradigms of the host editor. 

Advanced configurations allow engineers to map their specific VS Code theme directly to the `bat` terminal output by exporting the `BAT_THEME` environment variable [cite: 35, 36]. The Git integration is particularly powerful for workflow continuity; `bat` utilizes a margin gutter to display green plus signs for additions, red minus signs for deletions, and yellow tildes for modifications, effectively providing a localized code review interface inside the VS Code terminal without requiring a context switch to the editor's Git graph [cite: 33, 36]. Furthermore, `bat` can be integrated tightly with tools like `fzf` to provide live, syntax-highlighted previews of shell scripts during fuzzy file searches [cite: 31].

## Interactive Debugging Implementations via DAP

Historically, shell scripts were debugged using the `set -x` command, which traces execution by blindly printing every command to standard error before it is evaluated. While functional for trivial scripts, this methodology produces overwhelming, unnavigable log streams in large codebases and entirely lacks interactive state-inspection capabilities [cite: 37].

Visual Studio Code solves this architectural deficiency through the Debug Adapter Protocol (DAP) [cite: 38]. DAP establishes a standardized, language-agnostic JSON-RPC protocol between the VS Code debugging user interface and underlying external debuggers, providing a unified interface for breakpoints, call stacks, and variable watching [cite: 38, 39].

### Bash and Zsh Debug Adapters

For Bash scripting, the `rogalmic.bash-debug` extension intercepts DAP requests from VS Code and translates them into execution commands for `bashdb` (The GNU Bash Debugger) [cite: 40, 41, 42]. Similarly, for Zsh, the `rogalmic.zsh-debug` extension performs an identical translation mapping for `zshdb` [cite: 43, 44]. 

When a developer sets a breakpoint in the VS Code editor margin and initiates a debug session (F5), the extension spawns a hidden terminal running the target script under the strict supervision of `bashdb` or `zshdb` [cite: 39, 42]. As the execution pointer hits a breakpoint, the adapter pauses the execution state, allowing the developer to deeply inspect standard variables, global environment variables, arrays, and the exact `$?` status code in the VS Code Variables pane [cite: 39, 42].

### Architectural Limitations and Workarounds

Because shell debuggers are inherently bound to standard input topologies and pseudo-terminals (PTYs), they present highly specific execution challenges that require careful configuration:

*   **Interactive Scripts and Standard Input:** Scripts requiring manual user input via the `read` command will indefinitely deadlock if executed in the default VS Code internal debug console. Engineers must configure the `launch.json` to explicitly use the Integrated or External terminal by setting the `terminalKind` attribute to `"integrated"` [cite: 41].
*   **Subshells and Piped Execution:** Setting breakpoints inside a piped `while` loop (e.g., `cat file.txt | while read line; do ... done`) may fail to trigger reliably. Because the pipeline forces the loop to execute within an asynchronous subshell utilizing a distinct Process ID (PID), the primary `bashdb` instance may lose scope tracking.
*   **Path Mapping and WSL2:** On Windows environments relying on the Windows Subsystem for Linux (WSL2), the `pathBash` parameter in the debugger configuration must accurately point to the native Linux binary path (e.g., `/usr/bin/bash`), not the Windows executable [cite: 41]. Furthermore, environmental variables during a debug execution in WSL2 differ significantly from Git Bash environments on Windows, requiring careful path translation configurations [cite: 42].
*   **Overwritten Variables:** The execution of the debug adapter itself relies heavily on Bash's regex engine, meaning that the `BASH_REMATCH` array gets silently overwritten as the debugger steps through the code. Developers cannot reliably watch this specific variable during an interactive session [cite: 41].

| Debugging Feature | `bash-debug` Capability | `zsh-debug` Capability |
| :--- | :--- | :--- |
| Standard Breakpoints | Supported | Supported |
| Conditional Breakpoints | Supported (Expression evaluation) | Supported |
| State/Variable Watch | Supported (Local and Environment) | Supported |
| Interactive `stdin` | Requires `"terminalKind": "integrated"` | Requires `"terminalKind": "integrated"` |
| Platform Compatibility | Windows (WSL), Linux, macOS (Homebrew Bash) | Windows (WSL), Linux, macOS (Native) |

## Master Workspace Configuration Parameters

The absolute synergy of the aforementioned tools is unlocked solely through explicit and rigorous configuration within VS Code’s `settings.json` and `launch.json` files. The following sections outline the exact, production-ready configurations required to establish a highly performant shell scripting environment.

### Target Extension Identifiers

To replicate this exact IDE architecture, the following extensions must be installed via the VS Code Marketplace, utilizing their exact publisher identifiers [cite: 45]:

1.  **LSP & IntelliSense:** `mads-hartmann.bash-ide-vscode` [cite: 4]
2.  **Advanced Syntax Highlighting:** `jeff-hykin.better-shellscript-syntax` [cite: 26]
3.  **Static Analysis & Linting:** `timonwong.shellcheck` [cite: 12]
4.  **Deterministic Code Formatting:** `mkhl.shfmt` [cite: 16]
5.  **BATS Language Grammar:** `jetmartin.bats` [cite: 22]
6.  **BATS Test Runner Integration:** `kherring.bats-test-runner` [cite: 24]
7.  **Interactive Bash Debugger:** `rogalmic.bash-debug` [cite: 37]
8.  **Interactive Zsh Debugger (If applicable):** `rogalmic.zsh-debug` [cite: 44]

### Workspace `settings.json`

The following JSON block represents the exhaustive configuration required for the workspace `.vscode/settings.json`. This architecture assumes that the `shfmt` executable is installed within the system PATH and that `shellcheck` utilizes its robust bundled binaries.

```json
{
  // ---------------------------------------------------------
  // FILE ASSOCIATIONS & GRAMMAR MAPPING
  // ---------------------------------------------------------
  // Ensures extension-less executables in bin/ folders and hidden files are parsed correctly
  "files.associations": {
    "*/bin/*": "shellscript",
    ".envrc": "shellscript",
    "*.bats": "bats",
    "*.zsh": "shellscript",
    ".zshrc": "shellscript"
  },

  // ---------------------------------------------------------
  // BASH IDE / LANGUAGE SERVER (mads-hartmann.bash-ide-vscode)
  // ---------------------------------------------------------
  // Connects the language server to a local Dockerized explainshell instance for flag documentation
  "bashIde.explainshellEndpoint": "http://localhost:5000",
  // Ensures the AST engine analyzes all files in the workspace to accurately resolve 'source' trees
  "bashIde.globPattern": "**/*@(.sh|.inc|.bash|.command|.zsh)",

  // ---------------------------------------------------------
  // STATIC ANALYSIS (timonwong.shellcheck)
  // ---------------------------------------------------------
  "shellcheck.enable": true,
  // Enables the automatic application of safe heuristic fixes provided by the ShellCheck engine
  "shellcheck.enableQuickFix": true,
  // Lints the file automatically upon saving to drastically reduce CPU overhead during continuous typing
  "shellcheck.run": "onSave",
  // Prevents ShellCheck from linting pure Zsh configuration files where POSIX checks yield false positives
  "shellcheck.ignorePatterns": {
    "**/*.zsh": true,
    "**/*.zshrc": true,
    "**/*.zprofile": true,
    "**/zshenv": true
  },
  // Allows ShellCheck to parse external sourced files relative to the workspace root directory
  "shellcheck.customArgs": [
    "-x",
    "--source-path=${workspaceFolder}"
  ],

  // ---------------------------------------------------------
  // CODE FORMATTING (mkhl.shfmt)
  // ---------------------------------------------------------
  "[shellscript]": {
    "editor.defaultFormatter": "mkhl.shfmt",
    "editor.formatOnSave": true,
    "editor.tabSize": 2,
    "editor.insertSpaces": true,
    // Overrides default VS Code heuristics to strictly align with shfmt enforcement
    "editor.detectIndentation": false
  },
  // Force BATS testing files to utilize the same formatting parameters
  "[bats]": {
    "editor.defaultFormatter": "mkhl.shfmt",
    "editor.formatOnSave": true,
    "editor.tabSize": 2,
    "editor.insertSpaces": true
  },
  // Enforces Google Shell Style Guide parameters via underlying shfmt executable arguments
  // -i 2: Indent with 2 spaces
  // -ci: Indent switch cases
  // -bn: Binary operators like && and || may safely start a new line
  // -sr: Enforce a trailing space after redirect operators
  "shfmt.executableArgs": [
    "-i", "2",
    "-ci",
    "-bn",
    "-sr"
  ],
  // Prevents shfmt from bypassing files specified in .editorconfig to ensure strict global enforcement
  "shfmt.formatIgnored": false,

  // ---------------------------------------------------------
  // SYNTAX HIGHLIGHTING & THEME CUSTOMIZATION
  // ---------------------------------------------------------
  // Enhances the visual parsing of BATS scripts to differentiate testing decorators from standard bash logic
  "editor.tokenColorCustomizations": {
    "textMateRules": [
      {
        "scope": "keyword.control.bats",
        "settings": {
          "foreground": "#9f1fd1",
          "fontStyle": "italic"
        }
      },
      {
        "scope": "support.function.bats",
        "settings": {
          "foreground": "#d41515",
          "fontStyle": "italic"
        }
      },
      {
        "scope": "support.variable.bats",
        "settings": {
          "foreground": "#329432",
          "fontStyle": "italic"
        }
      }
    ]
  },

  // ---------------------------------------------------------
  // ZSH TERMINAL INTEGRATION (tolkonepiu)
  // ---------------------------------------------------------
  // Enables advanced ghost-text auto-completion natively sourced from the active shell session
  "terminal.integrated.suggest.enabled": true,
  "terminal.integrated.suggest.selectionMode": "focus",
  "terminal.integrated.suggest.insertTrailingSpace": true
}
```

### Debugging `launch.json`

To execute scripts interactively utilizing the Debug Adapter Protocol, a `.vscode/launch.json` file must be provisioned. The configuration below provides two distinct execution targets: one for standard execution within the integrated terminal (allowing for interactive `read` inputs), and one specifically engineered to execute BATS tests via the debugger to trace failing TAP assertions [cite: 41, 46].

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "bashdb",
      "request": "launch",
      "name": "Debug Bash Script (Integrated Terminal)",
      // Dynamically selects the active file currently open in the editor viewport
      "program": "${file}",
      // Prompts the user to pass dynamic runtime arguments to the script upon initialization
      "args": [
        "${command:PickProcess}"
      ],
      // Required for scripts that rely on user input (stdin) or specific terminal escape sequences
      "terminalKind": "integrated",
      "cwd": "${workspaceFolder}",
      // Explicitly maps the execution path, a critical requirement if running on Windows WSL2
      "pathBash": "bash",
      "pathBashdb": "bashdb",
      "pathBashdbLib": "",
      "pathCat": "cat",
      "pathMkfifo": "mkfifo",
      "pathPkill": "pkill"
    },
    {
      "type": "bashdb",
      "request": "launch",
      "name": "Debug BATS Test Suite",
      // Points directly to the local BATS executable installed via npm or git submodule
      "program": "${workspaceFolder}/node_modules/.bin/bats",
      "args": [
        // Passes the currently active .bats file directly to the test runner engine
        "${file}"
      ],
      "terminalKind": "integrated",
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

## Synthesizing the Environment: Achieving CI/CD Parity

The ultimate objective of configuring this exhaustive, multi-layered tooling matrix within VS Code is to achieve absolute local parity with the remote CI/CD pipeline [cite: 14]. 

When a developer authors a shell script in this environment, the `bash-language-server` acts as the first line of defense, proactively preventing typographical errors by instantly cross-referencing variable availability locally and across the workspace [cite: 6]. Simultaneously, the `ShellCheck` engine enforces semantic logic in the background, intercepting architectural flaws such as quoting vulnerabilities, identifying shadowed variables, and ensuring `eval` statements are not dangerously misused [cite: 11]. Upon saving the file, `shfmt` executes its deterministic formatting, homogeneously styling the codebase and entirely neutralizing trivial code review debates regarding indentation depths or operator placement [cite: 15]. 

The integration of the BATS testing framework via the VS Code Test Explorer ensures that unit testing shell infrastructure operates with the exact same frictionless ergonomics as testing a modern Node.js or Python application [cite: 24]. Finally, the deployment of the `bash-debug` extension abolishes the archaic reliance on rudimentary `echo` debugging, allowing engineers to walk through the AST execution state by state, inspecting memory configurations in real-time [cite: 42].

By synthesizing these highly specific extensions—LSP clients, AST formatters, Haskell-based static analyzers, and DAP adapters—and rigorously applying the `settings.json` and `launch.json` configurations detailed in this analysis, engineering organizations can successfully elevate shell scripting from a brittle, secondary automation task into a highly robust, maintainable, and verifiable software engineering discipline.

**Sources:**
1. [dev.to](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQFMf4gWSl9yVMxCzJpDa1MdWSktes22pB4Vmecp15-gF1j0wM0QWJAtszP6H3KBCrWhf0nqqTASVE4XPLZ9VsfL07ob-REyhbFNuZLh350-ejD-fCUM2C4SD_ZMBDVKPWWUhjO3L7HlyNOD8w4z1oZa1DR4-Qmzkl-PlJIqykf90PuX)
2. [wikipedia.org](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGl7-l1OucTcLkwuzsEUuO8RGklmPP-g8prrU01q4dg85tJGsZrmWW3JiLwdyWyCAumNDapApqogTuyFXZvaoL9KsiwQJ5MBATpEAvu-e6ZtlPs67APKFeEpQewvtEq-cRJXezj-w==)
3. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGuA4Hhk1_ZxlzNyhlmSKcNFHa24bbKZMSjaUCgm7JOb1gJSQA7fbVyWSi8IQxVaWWTcv25Epd8h6zseoXasuNdN69MZoLSaB0RKN1ZcSbt7cGpOypeN1Z1IAdFsmbkD44AGo3Yc2FIpbzXltjk5zjKEBcSzpvwGlWNCLknf-GK08tyPg4Ivym5blk=)
4. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQG-2EIBVGeTFJBg-Yb2BNlk82kJ0RRtOjCU_9Lzyy0FDHIEZd56qPAjrdQQ8wfL_oawJPjTjC_pF-CtTCcOLl96zM_GwuSaMLBiuPNPS5WnD2gvr_Nz3aHBddtNT_HyO9JsyRJJm497BldZ_u8I46qH6FDPIVC3KiTqmmsrNFY7D-rBh5YltA==)
5. [open-vsx.org](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQFCpesQPMJpohPvORVlMTD45qIkNEPpLRclB6vNU-m2kLcGDHvd1iUzlQJpOgm9egj1crPtQRrYfe7ia-L-Eh_Uit5IqcHNd155IY2HQAcq_bhFYjc_ql7_B-JOo5TEHOvQFjiwZUoz-P51)
6. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQElOQ5CSw4NxGFWtSwy5HmcIMXtyp87Us-0QXna5PK1eQyEsz4zhWJGDWEUrZfcxtyFpCws428LQok2gjCtWAa1n38U-fslSMXFcrRhQaXK-dHjG8zDqH9dlrSxGnV74qwqQCt_-g==)
7. [bashsupport.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGwni-USFT8IY8D8_F3KxMs-Om57-IwzLmz7KVdm9D8a_q_DKa_CrRzJ96LVm_VJ61OsAG1hvXmfZ_9RZbe4HKeOBYnGRXvX7zT3qi9cCcB6lS6u_22pUYLvhw=)
8. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQF7HnXRAUpyJea0_VQnQDqyE_Bn06IafM5DmRVGptX0Z-WDo-VSRabUKmpTUte6EM7hYii_prMhTr1KPL4tBtDS3l3zbBz9BYyCwaheGcJy_qn8eWTkB9vx-TzZRcgkIxqaAH78iTG5dH7ftz3DgRBBAtv6-qg=)
9. [reddit.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGwRBRWgXc4bI_w7FWABXNbre8G3H3vPH4ljTv1lPP8V_EqSs8WpY99ivzlZYm2S26KzOVBAgxYdh539r514IjmINUFaea8RRXrkMRQh3QKUZEf4YVUlNRDRbkwRAddgj9YKccHOijwFoMQlvDa8wcooSfqyE8EuN-QNdAh8mMEaAKx9-rHr2_O0-EcT4FZKbI95-P_HRBPa2M=)
10. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHkRPhwcd_EUakMzmDXBcz8MWT_oOAM0Zy4x2VdWcyZVVgmzcBPcZO1WLOGFz45uAFYJA69K9Lt2uNWs7dyPQEaMBxo3Sbe_czYY3OKygZByBPgjLvRRN6JQ0GKZQhfO0uvHDa3sYSyTtTFos_dKji2Wr7NWnLj)
11. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGrsQSetBWwVayNYqBGFLSHYggdyeZBBW-9TpsHL32vY4PocAOF8F9uUgzjLtizv4zpocsqHgdND8SDRoMJrcOr3EoWSMr7TYInvkYNcclECJnJCU6iFpvErSuN)
12. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQE8r9p_Yiha69zn_gG8guqfD4S8mM12pLHIDE7xWHVpG-PPssSuCBgGU1KoL7axWjRcEZc_AEHHr-97jyTql9PczgRsI_fvAZT0VcWJg4HrLLWLErPfaenG5z229Z7t0j5U0Evcd4gdxx7cC545CfK7hm5ls_QDAgrDIkxleQ==)
13. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQELqilHyTwNXhvPl3OEVqy1eRb4mja7CwnOGPPwPILo0O7ypErgxk20ALUHJvFB4tkQFb40-eM3R6IsRM6FVSqKYSOMtAkKk3OrLDm8a5cNjhwkbyBvZ1oPLOcH9uMVelS-Ltt7P-R8tAbc8w==)
14. [github.io](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGHj1oeJlxv3VQZ1Culm0o6wtEd5YZafLkPjOPoPpLFxZQDVKK-MFDMcczBdjKLxJYXg5MSxcksxxJifEo_imGrb8O-yKdMawUUa7ouYKyZyatymM2OU4F_FWTyBC-WWTr72k-pA1j0KfpWhRVLvQOMEp_uV8LnNPIfsIStaoSNh0zvrxNGHHGRno0=)
15. [megalinter.io](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHhLMWXCLQX9sA-YEIF_QTMzW51finDIej-muO-9lCn4lBlbFY1EyuAuPqXjQ9wvwDgBYxS_zkB6EWi6A3U_6h78EChWkc8CDFi7pJOH2TdHixc-YJkp5zv1j7vrx3g5crerlOQ)
16. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGP3LtAfvob3DqUJnuXCR_-Hc3fDq4gB1MrzQATUqMAUAsBQbLkBprjrxdSyiF3_AtLp0mo8Ss_yW_Gewbs8sj5G1dSHmV5Kaz_AgR_rw7K5e_A_E_QMcgj1NaTasWc2wgfqWyF8Edb-UysbZjq-F8cTLCH)
17. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEQyQ2lCbwjodTRF327mgWCsK0hiRlsu4QwO3KlMLcz_WX4ygk67fB2q56RQRMRoGBCnq8QANwUo6MBnAayhUXa3eGEnua0M9V7BgQxcIWjimozeMI8fg015gIWPlH7W9wSth0lb_wqrrZYWwBwnnLKiUj580BvJIgR8D1JVxyTAmhX)
18. [issuehunt.io](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEwoabwJ0MZeEp5wW1NYYFInRjjZRHL4rh48CMNjb5FgrVMjgE3RU-aG8BmA8JxBk8GJvCCUY0A4UQA2KV7JnhD3rh8sidUueZHwQ0fFxtqsu7-EjNa3ksaPhPjG67r1WgTnADVjzvnJUahokWdrU8Ge2vYeY77MQ==)
19. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGIswkDaWz2Ke_G5q-9nrpFiyzFf6WDvHYoPui5qGKJkHFLheBY6UDQ4z44w6s2MbK7bc-T1g09vd4zs_b_39-5pyWF5FCDq3ulMZ2DDJeIvTB0-AijSnfz4F33WXvGcS-Dp5QcIB7UQXyBIUqN2ViWUhhOYxqP8k3Lx8oSwrfW)
20. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHAxKa1LyF10FWSF31Wfo-gzm9MWOvgOOJ4QLr7YEg5Oxo_KKXLwWp43WQpdRQj8IYTnsxYBshgwW0SKJ2uMN5cpNY1CgDM4NjMvbE5NkDC_Sv2OhEyRA0lmGPCc0s=)
21. [developmeh.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQExVviv2a79jYyJg4qPVteId8xSeScDNMwAD14P40Vct19bAsKriZwfCfMNEW4mTmb4rDasDERHyjbVcH3BkLPEGP5thfE2RH_6fd0PfEELA_6jxNAI7tUxFmVU0tKOGQjwn1PpXU2UnNp6V9EDj-jvmCX631hvii_f7g==)
22. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHc8iooArBu57qJsB-Jrp1uBWKw87Nu1MPPiaMyRsU6WwOGBg17pJXr_ZkHKWLpbUbYlNkGMKelETGknjOqqlBIizYq_K-1u3RK-F-HI8gGDPIIxpHJeJZ9pLeocbVBMCG1p8A5nDqOGHhnU3urMcawkKOnWYVo_g==)
23. [opensource.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHtPlLkEJ-yO65jQowPxv3DfhTNDPIbQdOgx4W_gy-Ymk1L-2G9asTBaL1GBGLu_9jGsqHAE708G7jkT3W9gLhUKN0nap-i6hl342CJL4O3S8j8wCfelt7HfFmjyVtlnr27gQ3m7pAd81lK)
24. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEASdm_Y9nkJO8UGuMRtIlAUMlhdbgLxhXgqtlVsli5xgx15gx6jSCbIzDhQoZIWxXh_i1apiBLPHXfs8Ug5xrVZmFojEiPcVi1GCx_HDe2Jm04K7aDZmpCU8tqiJJgzqdEZWzOo03IVjmbKfHLVySmMXzcI_uh5OaXPtSS1yqIVWTW)
25. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHUSGrlW3Mv7P64bFj-bU8RIzkbjRJ5wJZJCRGCBRrMPfdHlWDcjwSIbp8T8Ri2GMU8pXRGrP_QxJD_ckDcQcoPD4DUNOV_XIKAIVV746eTV6VKvcavvdXLdLg1VanispDaklV6LGAsIZkeBqANYg==)
26. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGgkhZO2I6tNCFTG_kjxFzhaJlC1SxWrKlYVWRMgajfmxI9AqpOthnDAlPjXfMzwjp2RYiVZGsv-yZNawlUsslNCXj-xE-7gNS1Q_h1d48Oxo_wQ8SxWWE2GVE_S_AnXvUjCsDh3D5zjJKupJdYL5aICZ1fKmxNpFo51oq9Gkc7jZ14YRqZJVoJR3eG9Ww=)
27. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGnSooMhu8ac9oZmCLNmNAiX-TCRiu33zgoy5LXm79MmGIGY0INqhT4ZtuhUf5xCc-ZLfbh8zaL9pNztZpAeRj5Dm8mWZ_oUgOenXpnzsugh-5aPlEUjfTfpHrO4tB_E0V13QekJQ4k6Uha4B88Tc2J9ejNEONuxTJvk2jMhXgMORlT-ong5ijo)
28. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQH2Ptb-dURPVVIZu5h9pgwpaHYJVdk7nh1ilHWrAD-nOZXpRmxxhc8tsD97iDIaYdv7toknpxjWGSUPHXeGrwXbodwkMO8oPrhjICrMg2ASCwRyT--mLoDRXURRnRloAI_gx9iHLLqEICTibY7E38HXWMnMdXfuVBZJiONM-1AgtWQ=)
29. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQFTNk9Fat7z6Uc2UdYIbC0gs0LTNLvFvKVwfT8pRJYS0tEDqQB02f8evYlnXwVi_cZJVDY_2jXjpPQO8IQEDXJ58yqU_8PDYR8h38nYBJ3PhjiZDsf1KeeUSB1B-HhcYfboGLCjIfhg4BEVidzJWh2j4JOdUROohcFwUszf)
30. [peterwarnock.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEl1RtZSP-bgN3RJ96Z1y6xkYd2XteH_IHJjPMau968hBSoWi4EBbaNulXTvNBEpFQf7BlROHhzc058XEOF6TWu_7ChE9GQ0X-KxqNrjW0i2hsFWE-1Wef-fWqskS8HKWLuDNrt-qI5udbmvBNiDcEv1H8L7vs=)
31. [hashhackers.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEcXVR_W3GJJ4JM2fMbeLeeml1w8xEAFmWdJ2pofgFratlGgE2vni1r8lmntr0ASkcN5TbX2AE4n3WBtjERrVptpV5qMdZfQsS_gMg_9drjgopZwJlylrDd6mMdTEWMnTZzgY-mcQ==)
32. [geeksforgeeks.org](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHUMZukxNMaoAX-lJ3XS_QOh4GCWtNzm3h1cOAO2vZPFjjTrki3-IBq9tJrhBG8_bGCpWJRdqVul6ZsDfsszW57eJBlp3ckLM5KZP2cC-z7EYHSslJs8HMnw-RjfKkJxMJCUBdkL7w8rhQRfWtJNq1X0ZL28ACgNeQTWFL_mw6vMfnreZU27tVUaULSnlxraRLThPJLiwLSJzBJ5w==)
33. [youtube.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQFd9bcs3s1c7IvKU_cjxB0Cbd6Hz2YKD6gw2JZKyg_huo7K3sr9ZXpiUPhcYXugZxszhHk12OAjX0_CxcAehv7yqiXLmP7xn3bbbVS-SjKMCgB4AQTrPVQfOWXeVWCqRCY=)
34. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHeo8bmbnVHhT8Jtjluk14bEKu7lP-uj8zxrfy_xryi7xwSvyziUpzsgOSyOczgn8WR0eUU7Q2R_3n0Pgv8j-I7IQRYLFCnKC5kNmiWj-uyMr4JmvAWNRZVcg==)
35. [tecmint.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQG2thlNVEyfMV8YotOej2qH-95Xt9hvYzCYsC4VkRnjLWz8Nl8nPQziLNEHr17Feg8uQlQk3N_IqlrAAx78bcAkOkNfI5NXgWkhgh6ACkXljP31nQO6-_xQQ-E-VYRLObxyV1pW0JH2NRt_hvH-PHspho4tG-xw)
36. [ohok.org](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEuyDUvbt_xOuzQFlbYvQY7r5OWRNU9V6O3yQ-bq0OenMbOK-12m4NPTjZ5jmsSPZTMo5U57fDQu4Dt7KONYSakp8b5LnUUJeB1UJfGTm1LZoSi5DKDG11AvhRDsg==)
37. [reddit.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGTOT_duT85GGeCoCxm2iGtlIDnBJ-qnm8BQP63WrCPNiCF73oGAnCySmLBwLSdZYbCMGxWEG6--1xakKZ_NzgXG1ahjW6cLe-y5sV_gbJVx7gswbSbeS67u5ObFRn3kiEJjAIZO0bEy2U3gcrz2toJow8gpB9IKqS1u7FhrdVbCRZdGuqumtDGDTkQ5TcT0LDN_kB4Zh5G)
38. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQFde8A7LbZ8TPiAH2ZBe9SY_-eQIZlPWNCe54R73QajikpyeMGngfSm4ZguCJr60rIYu88xzXdUL3r0W2K-ZexYvoH90FsxXfHCteRMEpDjC5yHxFPs1G988MQdT6ZYv3F96KG8P2l9tIJwlljT6BbgTexFyzlG2X0fuQ==)
39. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQHXSphs3fUkBQfi1CCm8vRP2sRWtpz4q5AzMpQh__hY1aK5eb8bGeD7vGTa6degOv6wYgGCriKhVv_xkEJ9tVginpHVuf9wmMdJfAtK68AoOB1vvTWbczqGwZTJfPMPE-od2k8xA26nij7Prg==)
40. [medium.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGBmqToxtFsD7RAu2ioWBq7Gmej78ClCAgtDX4aATdGrZEd9WUKh7GE1HXZ1bibpVuhrrKIqHBn9Q6fbGkFOmi-QExwM9fbtOarujI71oTJg2vQSohGLD9OpS-RIjOG8U9GfDWVJH-ObN73DxJ6BTeFp85ESMFFSF_BXvwXbWAz)
41. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQH3S0eVbmwGDoAA4HXR7zs4DFpetWYQBUWTqYp99gEHJHavFV7oq8MF98jIQE80-GBpGEqboPCFK9XR9tbGnPlhnYE5xMXYb5rFbN0Owt2ACaB06rVSueZjBPhECCZV6j2VfQ==)
42. [zenn.dev](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGfmRYH0k2UO9c_MioMuQJ6VpCV_clmWURehdbzgjPAhSI0cTd8VhO289U8O5wHj42eEPoN-26D0hkaamu-3uWzmDNrX8lZnPOy9J0htHw_6DfPrOCwuCXMvcH2gi8IO2GgP_hS6ejnU8UM7sQWdCuBYg==)
43. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQG5LmUeBjzWhnsNb23bktLXZEF31hmjVu0qy2ZT9eNNxcDsBh2StGrtqsqxhsyDqpzmxK208v2priSWI9kwEozS0u0UIhmzWEG4ilunEohEKcRunYhtVmAEkyMGeZzSg-eJiCTSkO9OZPDFY85DNoRP2m7q3Hd6AUhlRrI=)
44. [github.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQH2qjMUKTR6RTxeSN5x8i8jIByo-6v0SEvD0nRirjCdzPFHg4IWvuSo9nD9H2w-SRnTAlLl-U1ipxZ4UAMPIxn0NrT_H4_a0JDWf8zyTzzGsn1oZkfoE06LdsLJKzsP9QqtIZRWWCnRTgFgSCFKWo2pIbouL-ZRZdnwYg==)
45. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQGO-nWLlckx_RP84SP2bz8r1UGfyZZKSQN4s9pD33YVT8YQA5Uin6ys1ESCmT-8BW8rnfvIR_w64RPdcdA_4-eu8jOk6cIHjKLbe4Oy6YYq_WzZM5XK6Wm3wqN7yjwVhy91bIuhHTea2_wkSGvzZZ_GR6uSZTlJnpVO4Pm56t2u31Gm)
46. [visualstudio.com](https://vertexaisearch.cloud.google.com/grounding-api-redirect/AUZIYQEbZL7h_a4JYgJD84E-2WS2LVPWK_Q-wNIPpaaBs_c-jCtjDdgBIoE1t3rv13GKOIOWrsAXjfwFlXoECETQ6N5PjQjgzlDULpM5-bLSyiIaWFOyDXVOkRzpcqqQ3hM0q5q-vcMysN8hyzl6PHDI5-4ztqW89tWnEWh4)


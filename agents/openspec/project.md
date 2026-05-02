# Project Context

## Purpose
[Describe your project's purpose and goals]

## Tech Stack
- [List your primary technologies]
- [e.g., TypeScript, React, Node.js]

## Project Conventions

### Code Style
[Describe your code style preferences, formatting rules, and naming conventions]

### Architecture Patterns
[Document your architectural decisions and patterns]

### Testing Strategy
[Explain your testing approach and requirements]

### Git Workflow
[Describe your branching strategy and commit conventions]

## Domain Context
[Add domain-specific knowledge that AI assistants need to understand]

## Important Constraints
[List any technical, business, or regulatory constraints]

## External Dependencies
[Document key external services, APIs, or systems]

## JMH Benchmarking Guidance
- Use JMH for performance and allocation comparisons; prefer fixed-length chains or `@OperationsPerInvocation` for per-op normalization.
- Keep allocation noise out of hot loops by reusing preallocated functions and isolating allocation-heavy transforms into dedicated benchmarks.
- Capture allocation data with `-prof gc` and optionally `-prof stack` or JFR for deeper analysis.
- For artagon-uri benchmarks, run suite tasks like `./gradlew jmhOptPrimitive jmhEither jmhResult jmhResultEitherCompare`.

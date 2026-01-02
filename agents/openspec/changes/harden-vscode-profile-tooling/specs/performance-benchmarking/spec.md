# Specification: Performance Benchmarking

## ADDED Requirements

### Requirement: Comparable Either and Result Benchmarks
The system SHALL measure Either and Result performance using matched data types and operations.

#### Scenario: Reference-type comparison
- **WHEN** running the comparison suite for reference types
- **THEN** Either SHALL use right(...) and mapRightAsRef with reference payloads
- **AND** Result SHALL use ok(...) and mapOk with the same reference payloads
- **AND** both chains SHALL apply the same number of operations

#### Scenario: Primitive-type comparison
- **WHEN** running the comparison suite for primitive types
- **THEN** Either SHALL use rightInt/rightLong/rightDouble (as appropriate) and mapRightAsInt/mapRightAsLong/mapRightAsDouble
- **AND** Result SHALL use okInt/okLong/okDouble (as appropriate) and mapOkAsInt/mapOkAsLong/mapOkAsDouble
- **AND** both chains SHALL apply the same number of operations

### Requirement: Per-Operation Normalization
The system SHALL report time and allocation metrics normalized per map/flatMap operation.

#### Scenario: Map-chain normalization
- **WHEN** a benchmark performs a chain of N map/flatMap operations
- **THEN** the reported metrics SHALL be normalized per operation (for example via fixed chain lengths or OperationsPerInvocation)

### Requirement: Allocation Noise Control
The system SHALL avoid unrelated allocations inside hot benchmark loops.

#### Scenario: Allocation sources are controlled
- **WHEN** running map/flatMap benchmarks
- **THEN** per-iteration lambda allocation SHALL be avoided by reusing preallocated functions
- **AND** string concatenation or other allocation-heavy work SHALL be isolated to dedicated benchmarks

### Requirement: Benchmark Suite Separation
The system SHALL keep Either and Result benchmarks in separate suites, with a dedicated comparison suite for matched operations.

#### Scenario: Suite boundaries are clear
- **WHEN** running Either benchmarks
- **THEN** only Either-specific benchmarks SHALL execute and write to either-benchmark.json
- **AND** Result benchmarks SHALL execute separately and write to result-benchmark.json
- **AND** comparison benchmarks SHALL run in their own suite and write to result-either-compare.json

### Requirement: Allocation Profiling Guidance
The system SHALL document how to capture allocation metrics during benchmark runs.

#### Scenario: Profiling guidance is discoverable
- **WHEN** a developer runs a benchmark suite
- **THEN** the output or documentation SHALL note -prof gc (and optional -prof stack or JFR) for allocation analysis

### Requirement: Baseline Benchmarks
The system SHALL include baseline construction/no-op benchmarks to separate container overhead from transform cost.

#### Scenario: Baselines are available
- **WHEN** running Either/Result/Opt benchmark suites
- **THEN** baseline construction/no-op benchmarks SHALL be included alongside map/flatMap chains

## ADDED Requirements

### Requirement: TMPDIR default
The validator SHALL run successfully under `set -u` in environments that do not export `TMPDIR` by defaulting to `/tmp`.

#### Scenario: TMPDIR is unset
- **WHEN** `validate-json.sh` runs in an environment where `TMPDIR` is not set (for example `env -u TMPDIR bash scripts/validate-json.sh`)
- **THEN** the validator proceeds without aborting on an unbound variable

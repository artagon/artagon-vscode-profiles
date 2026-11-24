## ADDED Requirements

### Requirement: Toolchain Auto-Detection for Shell and Python
The system SHALL detect `shell` when dedicated Shell test suites or linter configurations exist in the workspace (`test/**/*.bats`, `tests/**/*.bats`, `*.bats`, or `.shellcheckrc`), avoiding un-scoped `*.sh` script matches that would conflict with polyglot root repositories. The system SHALL detect `python` when `pyproject.toml`, `requirements.txt`, `setup.py`, `setup.cfg`, `uv.lock`, or `.python-version` exist in the workspace, with defined polyglot precedence in `VALID_FLAVORS`.

#### Scenario: Shell detection
- **WHEN** `detect-toolchain.sh` runs in a directory containing `test/app.bats` or `.shellcheckrc`
- **THEN** it outputs `shell` in the detected toolchain list without false-positive triggers on standalone utility scripts in non-shell repositories.

#### Scenario: Python detection
- **WHEN** `detect-toolchain.sh` runs in a directory containing `pyproject.toml`, `setup.py`, or `uv.lock`
- **THEN** it outputs `python` in the detected toolchain list.

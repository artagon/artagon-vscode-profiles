## ADDED Requirements

### Requirement: Surface launch failures
`open-profiles.sh` SHALL track the exit status of each `code --profile <name> --new-window` invocation and SHALL exit non-zero when any profile fails to launch.

#### Scenario: One profile fails to launch
- **WHEN** `code --profile <name> --new-window` returns a non-zero status for at least one profile
- **THEN** the script reports the failed profile name and exits non-zero after attempting all remaining profiles

#### Scenario: All profiles launch successfully
- **WHEN** every `code --profile` invocation returns zero
- **THEN** the script exits zero

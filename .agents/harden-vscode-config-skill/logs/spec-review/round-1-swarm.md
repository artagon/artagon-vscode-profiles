# Spec Review — Round 1 (gh-meta-review swarm)

This loop's spec review was performed by the 3-agent gh-meta-review swarm
on commit 38910c8 (PR #2). Three reviewers ran in parallel:

- Claude coordinator (`comprehensive-review:code-reviewer` subagent)
- Codex adversary (`codex-cli 0.128.0` direct CLI tier, last-resort fallback)
- Gemini adversary (`cc-gemini-plugin:gemini-agent`)

## Outcome

22 findings consolidated. Spec-affecting issues:
- **R1-05 [High · Architecture]** — Spec covers meta-properties (NUL, locale, magic counts, description budget, literal assertions) but NOT audit/validator input-shape contracts. R1-01/R1-02/R1-03/R1-04/R1-08 all shipped behind the harden pass for this reason.
- **R1-13 [Medium · Testing]** — NUL stripper Scenario references the awk layer, but the implementation moved NUL detection into the wrapper layer (lib/nul-check.sh).

## Spec changes landed

`specs/vscode-config-skill/spec.md`:
- Reworded "NUL-byte rejection in JSONC stripper" → "NUL-byte rejection at the wrapper layer" with three updated Scenarios (validator/audit/profile-diff). Aligns spec with the actual implementation.
- ADDED Requirement: "Audit and validator input-shape contracts" with 4 scenarios:
  1. Audit rejects array-root extensions.json (R1-01)
  2. Validator rejects non-string recommendation entries (R1-02)
  3. MCP type enum tracks the current MCP protocol revision (R1-03)
  4. Wrong-shape tasks/configurations rejected within documented exit range (R1-08)

## Validation

```
$ openspec validate harden-vscode-config-skill --strict
Change 'harden-vscode-config-skill' is valid
```

## Artifacts

- ~/.workspace/harden-vscode-config-skill-pr2-r1/round-1/consolidated.md
- ~/.workspace/harden-vscode-config-skill-pr2-r1/round-1/{claude,codex,gemini}-findings.md
- https://github.com/artagon/artagon-vscode-profiles/pull/2#issuecomment-4362924485

## Reviewers
| Lane | Status |
| --- | --- |
| Claude coordinator (specialist sweep, multi-dimensional) | ✅ |
| Codex adversarial (parser internals + bats false-coverage) | ✅ |
| Gemini adversarial (threat model + docs accuracy) | ✅ |

Adversarial pressure on the spec produced R1-05 (the input-shape contract gap), which both adversaries called out independently. Spec was updated and re-validated.

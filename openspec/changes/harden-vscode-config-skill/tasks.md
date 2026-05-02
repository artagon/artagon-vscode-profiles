## 1. Description tightening (CR-001)

- [x] 1.1 Replace the `description:` frontmatter in `~/.claude/skills/vscode-config/SKILL.md` with a single ~500-char paragraph: one sentence stating purpose ("authoritative knowledge of VS Code-family configuration"), one stating generic intent triggers ("where a config lives, why a setting isn't taking effect, reading/writing/validating/migrating any VS Code-family JSONC config")
- [x] 1.2 Confirm SKILL.md body's `## When this skill applies` section already covers the example phrasings that were previously in the description (it does); leave body unchanged in this change
- [ ] 1.3 Verify by manual eyeball that the trimmed description still triggers on Cursor / Windsurf / VSCodium queries — the variants are named in the new description text

## 2. Literal-match test helper (CR-002)

- [x] 2.1 Replace `assert_contains` in `~/.claude/skills/vscode-config/scripts/tests/test_helper.bash` with a `grep -qF` implementation so the needle is treated literally
- [x] 2.2 Replace `assert_not_contains` analogously for symmetry
- [x] 2.3 Re-run `bash scripts/run_tests.sh` and confirm the bracketed assertion at `vscode-profile-diff.bats:126` (`assert_contains "[python]"`) still passes — this time for the right reason

## 3. Locale regression coverage (CR-003)

- [ ] 3.1 Add a `@test "validates correctly under non-C locale (Turkish)"` to `vscode-jsonc-validate.bats` that exports `LC_ALL=tr_TR.UTF-8 LANG=tr_TR.UTF-8` and asserts the same exit / output as the C-locale path. Use a fixture with a key like `INDEX` and `index` so dotless-i collation differences would surface
- [ ] 3.2 Add an analogous Turkish-locale test to `vscode-extensions-audit.bats` exercising the `comm`-based set difference path (this is where locale matters most)
- [ ] 3.3 Add an analogous Turkish-locale test to `vscode-profile-diff.bats` covering the diff set operations
- [ ] 3.4 Run the suite once with `LC_ALL=tr_TR.UTF-8 bash scripts/run_tests.sh` to spot-check that nothing else implicitly depends on C collation

## 4. NUL-byte guard (CR-005)

- [x] 4.1 Add a per-line NUL-byte scan to `~/.claude/skills/vscode-config/scripts/jsonc-strip.awk`. Use a `substr`-based loop rather than `index(line, sprintf("%c", 0))` because BSD awk treats embedded NULs as string terminators and `index` returns spurious 0
- [x] 4.2 Emit `ERROR: input contains raw NUL byte` on stderr and `exit 1`
- [x] 4.3 Verify the three scripts (`vscode-jsonc-validate`, `vscode-extensions-audit`, `vscode-profile-diff`) all surface the new error correctly, since they all share `jsonc-strip.awk`
- [ ] 4.4 Add a bats test that feeds `printf '{"a":1}\0{"hidden":true}'` to the stripper and asserts a non-zero exit and an `ERROR: ... NUL byte` message on stderr

## 5. Drop magic test count (CR-015)

- [x] 5.1 Replace "covering 89 cases" in SKILL.md's "Bundled scripts" section with a stable phrasing that doesn't drift ("covering syntax, schema, and edge cases for each tool")

## 6. Spec deltas

- [x] 6.1 Author `specs/vscode-config-skill/spec.md` describing the contract: progressive disclosure (description budget, body length, references), shell tooling guarantees (literal-match test assertions, locale determinism, NUL-byte rejection), and validation/audit-bypass resistance
- [x] 6.2 Run `openspec validate harden-vscode-config-skill --strict` and resolve any issues

## 7. Verification

- [ ] 7.1 `bash scripts/run_tests.sh` shows all tests passing (currently 89; will be ~89+3 after task 3 lands)
- [ ] 7.2 Manual smoke test: `printf '{"a":1}\0{"hidden":true}' | awk -f scripts/jsonc-strip.awk` exits 1 with the NUL-byte error
- [ ] 7.3 Re-spawn `cc-gemini-plugin:gemini-agent` and `codex:codex-rescue` for a second adversarial pass on the patched skill to confirm CR-001 → CR-005 resolved
- [ ] 7.4 Update `vscode-config.skill` and `docs/vscode-config.skill` zip bundles in this repo to reflect the patched skill (re-zip from `~/.claude/skills/vscode-config/`)

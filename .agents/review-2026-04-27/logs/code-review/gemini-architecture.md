# Architecture + Security + Repo-Hygiene Review
Branch: feature/github-workflows-profiles-and-tooling-hardening
Date: 2026-04-27
Reviewer: Claude (Opus 4.7, 1M ctx) — Gemini bridge unavailable in repo (no `scripts/gemini-bridge.js`); review performed locally.

Severity legend: BLOCKER (ship-stopper) / HIGH / MEDIUM / LOW.

---

## 1. SECURITY

### S1 [HIGH] Workspace Trust effectively disabled in shared base
File: `_shared/editor-crisp.jsonc:80`, `_shared/editor-retina.jsonc:80`
```
"security.workspace.trust.untrustedFiles": "open"
```
This shipped to every one of the 22 profiles via composer. Every untrusted folder a user opens with these profiles will skip the trust prompt and run `tasks.json`, `.vscode/settings.json` automation, debug launches, formatters, etc. without the safety dialog. Combined with widely-installed extensions that auto-execute on activation (Java, Rust-analyzer, ESLint, Prettier, Continue, Copilot Chat), this is the single most exploitable line in the repo. Recommend `"prompt"` (default) or remove entirely.

### S2 [HIGH] No validation of extension IDs before `code --install-extension`
Files: `scripts/install-extensions.sh:174`, `scripts/import-profile.sh:100`, `scripts/vspcli:77`
The flow is `jq -r '.[].identifier.id' extensions.json` → loop → `code --profile "$PROFILE" --install-extension "$entry_ext"`. There is no allowlist regex (e.g. `^[A-Za-z0-9][A-Za-z0-9._-]+\.[A-Za-z0-9._-]+$`). If a malicious or compromised PR mutates `profiles/*/extensions.json`, an unsuspecting user running `vspcli --install all` installs whatever publisher.id is listed, including typosquats. `import-profile.sh:100` further swallows the install failure with `|| true`, so a bundle from an untrusted source can silently push installs. Recommend: validate ID shape, optionally pin a publisher allowlist, and drop the `|| true`.

### S3 [MEDIUM] Possible word-splitting on jq output in `check-extension-compatibility.sh`
File: `scripts/check-extension-compatibility.sh:223-269`
`extensions=$(jq -r '.[].identifier.id' "$ext_file")` then `while IFS= read -r ext_id ... done <<< "$extensions"`. The IDs themselves are well-formed, but the script also concatenates them into JSON via string interpolation at `:250`:
```
RESULTS+=("{\"profile\":\"$profile\",\"extension\":\"$ext_id\",...}")
```
If a future extensions.json contained a malformed id with a quote/backslash, the assembled JSON breaks and `jq -n --argjson results "[$(IFS=,; echo "${RESULTS[*]}")]"` at `:276` could be coerced into unexpected jq evaluation. Use `jq -n` with `--arg` per field instead of string-glued JSON.

### S4 [MEDIUM] `compose-settings.sh` follows `@extends` with no path containment
File: `scripts/compose-settings.sh:22,26`
```
jq -r '."@extends"? // empty | (if type=="string" then . else .[] end)' "$file"
local parent_path="$OVR/$parent"
```
A parent string like `../../etc/passwd` or `../scripts/install-extensions.sh` would be opened as a JSON input. If the parent isn't valid JSON, jq fails closed (good), but on path traversal the resolved file is still read and the contents (without `@extends` key) are unioned into the merged settings. Cycle detection at `:13-18` is correct. Recommend rejecting any `@extends` value containing `..` or starting with `/`.

### S5 [MEDIUM] `compose-settings.sh` swallows jq errors (already-flagged, confirmed)
File: `scripts/compose-settings.sh:22` — `... 2>/dev/null || true`. A malformed JSONC override silently produces an empty parents array and the merge proceeds. This is what lets the JSONC-comments-vs-jq mismatch hide.

### S6 [LOW] `.code-profile` exports do not leak local paths
Files: `exports/*.code-profile`, generator `scripts/export-profiles.sh:41-51`
Confirmed safe: `jq -n --slurpfile` produces only `{settings, extensions:{enabled:[ids]}}`. No machine ID, $HOME, $USER, telemetry tokens. Sample tail of `exports/github-workflows-crisp.code-profile` shows only ext IDs.

### S7 [LOW] `agents/openspec/.../tasks.md:227` contains placeholder absolute path
That path uses `/Users/.../vscode/_merged/profile.json` as illustrative. No real $USER leak. Confirmed clean.

### S8 [LOW] `.claude` and `.gemini` are symlinks → `agents/`
Both are tracked as symlinks (verified `ls -la .claude .gemini`). `agents/` contains markdown instructions plus `agents/settings.local.json` (Claude permission allowlist) and `agents/openspec/` proposals. None contain secrets, tokens, or prompts that exfiltrate. The risk is asymmetric write: Claude or Gemini writing `settings.local.json` updates is now committed by default. Add `.claude/settings.local.json` and `.gemini/settings.local.json` to `.gitignore`, or stop symlinking the whole tree.

### S9 [LOW] SECURITY.md placeholder email
File: `SECURITY.md` advertises `security@example.com`. Vulnerability reports may go nowhere.

### S10 [INFO] `extensions.autoUpdate: true` + `extensions.autoCheckUpdates: false` (contradictory)
Files: `_shared/editor-{crisp,retina}.jsonc:21-23`. VS Code uses `autoCheckUpdates` to gate the auto-update fetch; with it false, `autoUpdate: true` is dead code. Either pick reproducibility (both false + pin) or staying current (both true). For a "profiles" repo whose contract is "users get a known set", recommend both false and rely on `vspcli --install all` for refresh.

---

## 2. ARCHITECTURE

### A1 [HIGH] `_merged/` AND `exports/` AND `profiles/<name>/settings.json` symlinks are ALL committed and ALL regenerable
This is the diff-churn engine. Every shared base edit produces ~22 modified `_merged/*.json` and ~22 modified `exports/*.code-profile` files (47 working-tree mods on the current branch confirm this). Pre-commit hook `scripts/git-hooks/pre-commit` *requires* the VS Code CLI just to run compose+export — meaning any maintainer without `code` on PATH cannot commit unrelated changes. Pick one source of truth:
- Option A (recommended): commit `_overrides/` + `_shared/` only; gitignore `_merged/`, `exports/`, `profiles/*/settings.json`. Consumers run `vspcli --compose && vspcli --export`. Add a CI job that builds and uploads `exports/` as a release artifact.
- Option B: commit `exports/` only (the user-facing artifact) and gitignore the rest.

### A2 [HIGH] Pre-commit hook compiles AND requires VS Code CLI but no CI runs it
File: `scripts/git-hooks/pre-commit:30-34`
The hook calls `compose-settings.sh` and `export-profiles.sh` on every commit; this hook is unenforced (it lives under `scripts/git-hooks/` and must be manually wired with `core.hooksPath`). Combined with A4 (no CI), there is no objective check that committed `_merged/` / `exports/` match committed sources. Drift is invisible.

### A3 [HIGH] Branch promises GitHub Workflows hardening but `.github/workflows/` does not exist
The current branch is `feature/github-workflows-profiles-and-tooling-hardening`. `.github/` contains only `ISSUE_TEMPLATE/` and `PULL_REQUEST_TEMPLATE.md`. There is no `workflows/` directory — meaning the "github-workflows-*" *profile* (a VS Code profile for editing GitHub Actions YAML) was added, but no actual CI workflow validates the repo. The branch name is misleading; either rename or add CI (`validate-json.sh`, `compose-settings.sh` round-trip, `tests/run.sh`).

### A4 [HIGH] `crisp` vs `retina` is a single integer of difference
Confirmed: `_shared/editor-crisp.jsonc` and `editor-retina.jsonc` differ only in `editor.fontSize` (15 vs 14), `terminal.integrated.fontSize` (15 vs 14), `editor.lineHeight` (1.65 vs 1.6), `workbench.tree.indent` (14 vs 16), `workbench.fontAliasing` (auto vs antialiased), `workbench.colorTheme` (Tokyo Night vs Catppuccin). Roughly 6 settings differ. This doubles every profile (11 stacks × 2 dpi = 22). At 50 stacks it becomes 100 directories. Collapse the dpi axis to a parameter:
- Single `_shared/editor.jsonc` + `_shared/dpi-{crisp,retina}.jsonc`, composer takes a `--dpi` flag, profile dir is named `<stack>` and exported twice. Cuts directories in half and the `_overrides/<stack>-{crisp,retina}.jsonc` "two-file passthrough" pattern (e.g. `ai-profile-crisp.jsonc` is just `{"@extends":["ai-profile-base.jsonc"]}`) disappears.

### A5 [MEDIUM] Per-profile crisp/retina overrides are pure pass-throughs
Verified by reading: `_overrides/ai-profile-crisp.jsonc`, `ai-profile-retina.jsonc`, `cpp-clangd-crisp.jsonc`, `cpp-clangd-retina.jsonc`, `web-astro-crisp.jsonc`, `web-astro-retina.jsonc`, `github-workflows-crisp.jsonc`, `github-workflows-retina.jsonc`. They contain only `"@extends": ["<base>.jsonc"]` and add nothing. 14 of the 22 override files are pure shims. Confirms A4: the dpi axis is dead weight in `_overrides/`.

### A6 [MEDIUM] User-flagged: crisp/retina font size inversion — **NUANCED**
You're right that the working-tree changes feel inverted *if* "crisp" means the lower-DPI display where a larger fontSize is needed for legibility. But `editor.fontSize` is logical points; on a Retina display the same point is rendered at higher resolution, so 14 there is sharper, while 15 on a non-Retina display compensates for blockier subpixel rendering. The current crisp=15/retina=14 is defensible. **However** if you intended "crisp" to mean "small, dense, pixel-perfect" and "retina" to mean "comfortable on big screens", the names are doing the opposite of what they say. The naming is ambiguous, not the values. Recommend renaming to `compact` / `comfortable` or `low-dpi` / `high-dpi`.

### A7 [MEDIUM] User-flagged: `editor.fontVariations: false` regression — confirmed
`_shared/editor-crisp.jsonc:35` and `_shared/editor-retina.jsonc:35` both have `false`. With `editor.fontLigatures: true` and a variable font (JetBrains Mono is variable), `fontVariations: false` disables weight axis interpolation; bold/italic fall back to nearest static face. Likely an unintended regression from a typography sweep.

### A8 [MEDIUM] `@extends` chains are clean DAGs (no cycles, no diamonds)
Built mental graph from full `grep -A3 '@extends' _overrides/*.jsonc`:
- `ai/copilot.jsonc` ← `ai-profile-base.jsonc` ← `{ai-profile-crisp, ai-profile-retina, ai-plus-base}`
- `ai-plus-base.jsonc` ← `{ai-plus-crisp, ai-plus-retina}`
- `<stack>-base.jsonc` ← `{<stack>-crisp, <stack>-retina}` for cpp-clangd, cpp-intellisense, web-astro, github-workflows, java-gradle, java-maven, java-profile, rust-profile
- `java-profile-base.jsonc` ← `{java-spring-base, java-spring-crisp, java-spring-retina}` — note java-spring-crisp/retina extend `java-profile-base` *not* `java-spring-base`, so the spring-specific settings in `java-spring-base.jsonc` are NOT reaching the spring DPI variants. **This is a latent bug.** Fix: `java-spring-{crisp,retina}.jsonc` should `@extends: ["java-spring-base.jsonc"]`.

### A9 [LOW] Profile naming `<stack>-<flavor?>-<dpi>` doesn't scale
Today: `cpp-clangd`, `cpp-intellisense`, `java-gradle`, `java-maven`, `java-profile`, `java-spring`, `ai-profile`, `ai-plus`, `web-astro`, `github-workflows`, `rust-profile`. Mixed conventions: some have explicit flavor (clangd/intellisense), some have a redundant `-profile` suffix. With the dpi collapse from A4, normalize to `<stack>[-<flavor>]`.

---

## 3. REPO HYGIENE

### H1 [HIGH] `.cache/extensions-installed/` committed (4 files)
Confirmed: `git ls-files .cache/` returns 4 SHA-1 marker files. These are runtime install caches and should never be tracked. Already on your radar — confirmed.

### H2 [HIGH] `.gitignore` is grossly incomplete
Current contents (8 lines) only ignore VS Code user-data sub-paths that aren't even in this repo (`User/globalStorage/`, etc.). It does not ignore:
- `.cache/`
- `_merged/` (if you adopt A1)
- `exports/` (if you adopt A1)
- `profiles/*/settings.json` (regenerable symlinks)
- `.DS_Store` is the only sane line.

### H3 [MEDIUM] CONTRIBUTING.md tells contributors `_overrides/*.jsonc` is "strict JSON"
File: `CONTRIBUTING.md:21` says "Per‑profile JSON overrides live under `_overrides/` (strict JSON)". Coupled with `agents/instructions.md:14` ("Keep _overrides/*.jsonc strictly valid JSON (no comments). jq cannot parse JSONC comments."). The extension is `.jsonc` but the contract is JSON. This is a foot-gun (rename to `.json`) or a real JSONC parser is needed (e.g., `jq` 1.7+'s `-r --slurpfile` doesn't help; consider `node -e "JSON.parse(require('fs').readFileSync(...))"` or `npm i -g json5`/`hjson`).

### H4 [MEDIUM] No CI / no enforcement
- `.github/workflows/`: missing.
- `scripts/git-hooks/pre-commit`: present but not auto-installed; CONTRIBUTING.md never tells contributors `git config core.hooksPath scripts/git-hooks`.
- `scripts/tests/run.sh`: present, has decent coverage including a mock `code` CLI; not run anywhere except manually.

### H5 [MEDIUM] OpenSpec is half-active
- Top-level `openspec/changes/`: only `archive/` and that's empty.
- `agents/openspec/changes/`: contains `harden-vscode-profile-tooling/proposal.md` (fits this branch) and `improve-vscode-config-structure/`.
- Two openspec roots: `openspec/` (real) and `agents/openspec/` (where the actual work lives). `.claude` → `agents` symlink means Claude reads `agents/openspec/` while the project-level mandate points to `openspec/`. Pick one.

### H6 [MEDIUM] CODEOWNERS lives at repo root, not under `.github/`
File: `/CODEOWNERS:1`. GitHub honors either location, but combined with the missing `.github/workflows/` and the `.github/` dir holding only templates, the governance surface is inconsistent. Move CODEOWNERS, FUNDING.yml, SECURITY.md into `.github/`.

### H7 [LOW] `validate-json.sh` writes to `$TMPDIR/validate-json.err.$$` without checking `$TMPDIR`
File: `scripts/validate-json.sh:33`. On a machine where `$TMPDIR` is unset, it writes to `/validate-json.err.$$` (root). Use `mktemp` or fall back to `${TMPDIR:-/tmp}`.

### H8 [LOW] Pre-commit hook calls `compose-settings.sh` with no profile filter
File: `scripts/git-hooks/pre-commit:32`. Even commits that touch only README will rebuild all 22 merged outputs and 22 exports. With A1, this disappears. Without A1, scope to changed `_overrides/` files.

---

## 4. SUPPLY CHAIN

### SC1 [MEDIUM] No version pinning anywhere
`profiles/*/extensions.json` lists only `{ "identifier": { "id": "<id>" } }`. VS Code's marketplace always fetches latest. Combined with `_shared/editor-*.jsonc:21 "extensions.autoUpdate": true`, every `vspcli --install all` is a fresh roll of the supply-chain dice. For a "reproducible profile" repo, add `"version"` to each entry (the schema supports it) and disable autoUpdate.

### SC2 [MEDIUM] Duplicate / case-different IDs in same profile
`profiles/ai-plus-{crisp,retina}/extensions.json` and `profiles/java-gradle-{crisp,retina}/extensions.json` (and others — see grep) contain BOTH `Continue.continue` and `continue.continue`. VS Code marketplace normalizes case but the `install-extensions.sh:107` loop will issue two `code --install-extension` calls for the same extension and `check-extension-compatibility.sh` will double-count and double-cache.

### SC3 [LOW] Dead/legacy IDs referenced in install grouper
File: `scripts/install-extensions.sh:142` lists `panicbit.cargo` and `serayuzgur.crates`. Neither is in any `profiles/*/extensions.json` today, and `serayuzgur.crates` was deprecated in favor of `fill-labs.dependi` (which IS in your lists). Drop them from the case statement to avoid maintaining ghost classifications.

### SC4 [LOW] AI publisher list is permissive
`scripts/install-extensions.sh:133` whitelists `tabnine.tabnine-vscode`, `codeium.codeium`, `sourcegraph.amp` etc. as "AI" group even though none of those publishers' extensions are listed in any profile. If you add an allowlist (S2), constrain the AI group to what you actually ship.

### SC5 [LOW] All extensions consumed from public Marketplace, no integrity check
No SHA pinning, no offline `.vsix` mirrors, no `code --install-extension <vsix-file>` flow. For an internal Comcast/Artagon use-case (the working dir hints at corporate use), consider mirroring high-trust extensions to an internal artifact store and switching install to vsix paths.

---

## CONFIRMATIONS OF USER PRE-FLAGS

| User flag                                              | Verdict |
|---|---|
| crisp/retina font sizes appear inverted                 | Nuanced — see A6. Values are technically defensible; names are ambiguous. |
| `editor.fontVariations` changed true → false            | Confirmed at `_shared/editor-{crisp,retina}.jsonc:35`. See A7. |
| `.cache/extensions-installed/` should be gitignored     | Confirmed. See H1, H2. |
| `compose-settings.sh` swallows jq errors                | Confirmed at `scripts/compose-settings.sh:22`. See S5. |
| jq doesn't parse JSONC; code may rely on its absence    | Confirmed. See H3. The `.jsonc` extension is a lie — contributor docs even say "strict JSON". |

---

## TOP RECOMMENDED ORDER OF FIXES

1. S1 — flip `security.workspace.trust.untrustedFiles` back to `prompt`.
2. A8 — fix `java-spring-{crisp,retina}.jsonc` to extend `java-spring-base.jsonc`.
3. H1 + H2 — gitignore `.cache/`; pick one of `_merged/`/`exports/` to stop tracking.
4. A3 + H4 — add `.github/workflows/ci.yml` that runs validate + compose + tests.
5. SC2 — dedupe `Continue.continue` / `continue.continue` from extensions.json.
6. S2 — add extension-ID allowlist regex before `code --install-extension`.
7. A4 + A5 — collapse the dpi axis or accept the multiplication and document it.
8. A7 — restore `editor.fontVariations: true`.
9. SC1 — pin extension versions or document explicitly that this repo is "always-latest".

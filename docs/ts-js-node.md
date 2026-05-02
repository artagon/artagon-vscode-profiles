# Templating VS Code for TS/JS/Node, with adversarial review

**Bottom line up front:** Build a layered, factory-driven config system distributed as a small family of npm packages plus a thin scaffolder/doctor CLI — *not* VS Code Profiles, *not* a giant template repo. Profiles are monolithic snapshots that don't compose, while npm `extends` (TS 5.0+ array form), ESLint flat-config factories, and Biome v2's `extends`/`"//"` semantics already give you real composition with semver. The work that actually pays off is choosing which surface owns which layer: `.editorconfig` + tool-native config files travel across every editor, `.vscode/settings.json` covers VS Code/Cursor/Windsurf, and a devcontainer covers full reproducibility for teams that want it. After three adversarial rounds, the architecture is **simpler, not more elaborate**: one published `@org/eslint-config` factory, one `@org/tsconfig` package with leaf bases, one Prettier or Biome config package, a tiny `create-@org` scaffolder, and a `doctor` command — everything else is YAGNI until proven otherwise.

This matters because most teams over-invest here. Config sprawl is a real tax: every layer added is a debugging surface, an upgrade obligation, and a place where a junior engineer gets stuck. The architecture below deliberately collapses layers wherever the ecosystem already does composition for you, and treats "legacy support" as a *separate, isolated profile* rather than a permanent pollutant in the modern config.

## Executive summary of the recommended architecture

The system has six logical layers but only **three actual distribution artifacts**, because most layering is solved by the tools themselves.

| Layer | What it controls | Where it lives |
|---|---|---|
| Base | Whitespace, EOL, charset; org-wide editor norms | `.editorconfig` + minimal `.vscode/settings.json` (committed) |
| Language | TS rules, JS rules, parser | `@org/eslint-config` factory; `@org/tsconfig/base` |
| Runtime | Node/Bun/Deno/browser globals + module resolution | Factory option (`node: true`/`bun: true`); `@tsconfig/node22`, `@tsconfig/bun`, `@tsconfig/deno` (extended via TS 5.0 array) |
| Stack-era | Modern (default) vs legacy (opt-in) | Factory `legacy: true` flag; `@org/tsconfig/legacy` |
| Framework | React/Vue/Svelte/Next/Astro detection | Factory auto-detect via `package.json` (antfu pattern) |
| Tooling choice | Biome OR ESLint+Prettier — not both | A scaffolder prompt; `@org/eslint-config` and `@org/biome-config` are mutually exclusive |

The three artifacts: **(1)** an `@org/eslint-config` (or `@org/biome-config`) factory package, **(2)** an `@org/tsconfig` package shipping leaf JSON bases, **(3)** a `create-@org` CLI that scaffolds the four files a project needs and runs a `doctor` subcommand to detect drift. Everything else — VS Code Profiles, devcontainers, monorepo template-sync — is **optional augmentation**, not core infrastructure.

## Why this shape, layer by layer

### Base layer: the cheapest wins travel furthest

`.editorconfig` is the single highest-leverage file you'll commit. Zed (native since 2025), Neovim 0.9+, JetBrains, and every VS Code variant honor it; it eliminates tab-vs-space diff churn permanently and costs nothing. Pair it with a deliberately tiny **committed** `.vscode/settings.json` containing only project-correctness keys: `editor.formatOnSave`, `editor.defaultFormatter` per language, `editor.codeActionsOnSave` (`source.fixAll.eslint` or `source.organizeImports.biome`), `eslint.validate`, and `typescript.tsdk: "node_modules/typescript/lib"`. Personal preferences belong in user-level Settings Sync, never in the repo. A `.vscode/extensions.json` with `recommendations` and `unwantedRecommendations` triggers VS Code/Cursor/Windsurf's first-open install prompt and blocks known-bad extensions from auto-suggesting.

### Language and runtime layers: let the tools layer themselves

TypeScript 5.0+ accepts an **array of `extends`**, with later entries winning on conflicts; this is the cleanest composition primitive in the ecosystem. The standard pattern is `"extends": ["@org/tsconfig/node22", "@org/tsconfig/strictest"]` plus a small consumer override block. The trap to document loudly: **array properties (`lib`, `types`, `include`, `exclude`, `paths`) are *replaced*, not merged**, so a base setting `lib: ["ES2023"]` is wiped if the consumer also sets `lib`. Keep array fields in exactly one layer.

For ESLint, **flat config plus a factory function** is the dominant 2024–2026 idiom (antfu, Sheriff, Epic Web, Shopify, and Turborepo's templates all converge on it). The factory takes an options bag (`{ typescript, node, react, legacy, stylistic }`), composes a flat-config array internally using `eslint-flat-config-utils`'s `composer()`, and accepts user configs as additional arguments. This sidesteps deep-merge entirely — composition is array concatenation, with the rule "later objects override earlier ones for matching `files` globs."

### Stack-era layer: legacy as an opt-in island, never a default

In 2026, "legacy" realistically means Node 18 or below (Node 18 hit EOL April 2025, Node 20 EOL April 2026), pre-strict TS \<4.x, AngularJS 1.x (still on ~104K–158K domains and supported commercially by HeroDevs), Webpack 4, jQuery, and `.eslintrc.*` (deprecated in ESLint 9, removed in ESLint 10). The architectural rule: **legacy gets its own `.vscode/settings.json` flags, its own `tsconfig` base (`@org/tsconfig/legacy` with `target: ES2018`, `module: CommonJS`, looser strict), and a factory flag (`legacy: true`) that disables modern-only rules and adds AMD/UMD/jQuery globals**. The five settings that make legacy survivable in modern VS Code: `typescript.tsdk: "node_modules/typescript/lib"` (forces editor to use the project's old TS), `eslint.useFlatConfig: false`, `terminal.integrated.env.*: { NODE_OPTIONS: "--openssl-legacy-provider" }` (Webpack 4 + Node 17+/OpenSSL 3 fix), `volta.node` pin, and `unwantedRecommendations` blocking Biome/Deno extensions.

### Framework layer: auto-detection beats explicit configuration

The antfu pattern of sniffing `package.json` for `vue`, `svelte`, `next`, `astro`, `solid`, `react` is the right default — fewer flags, fewer mistakes. Provide explicit `react: true` overrides for edge cases (multiple frameworks in one repo, intentional opt-out).

### Tooling layer: pick one and never run both

Biome v2 (May 2025+) is genuinely production-ready for TS/JS/JSX/TSX/JSON projects: ~15× faster lint, ~25× faster format, single binary, native `extends` from npm packages since v1.6, and the `"extends": "//"` microsyntax for monorepo nesting. The remaining gaps are real: limited Vue/Svelte/Astro SFC support, no React Compiler rules from `eslint-plugin-react-hooks` v6, smaller plugin ecosystem (GritQL plugins exist but aren't yet npm-distributable), and weaker type-aware linting than `typescript-eslint` (Vercel is funding the closure). The default recommendation: **Biome for new Node-only and React projects**, **ESLint+Prettier for Vue/Svelte/Astro-heavy stacks and anywhere you depend on niche plugins (Storybook, Effect, Tailwind classRegex, Testing Library)**. Hybrid (Biome formatter + ESLint type-aware lint) works and is increasingly common.

## Templating-mechanism comparison

| Mechanism | Composes? | Live updates | Reproducible | Best for |
|---|---|---|---|---|
| `tsconfig` extends (5.0+ arrays) | Yes (later wins; arrays *replace*) | Yes via npm | Per-project | Compiler config |
| ESLint flat + factory | Yes (array concat, files-scoped) | Yes via npm | Per-project | Lint config |
| Biome v1.6+ extends + v2 `"//"` | Yes (monorepo-aware) | Yes via npm | Per-project | One-tool stacks |
| Prettier shared config | Manual spread only | Yes | Per-project | Format only |
| `.vscode/settings.json` hierarchy | Workspace-scoped only | Yes via git | Per-machine | Editor behavior |
| **VS Code Profiles** (built-in) | **No — monolithic snapshots** | No (one-time copy) | Via gist export | Role switching, not layering |
| Devcontainer + Features | **Yes — Feature metadata merges with `devcontainer.json`** | On rebuild | **Excellent (Docker)** | Reproducible IDE+OS |
| Settings Sync | Sync, not compose | Yes | Per-user | Personal preferences |
| degit / giget / template repo | No (one-shot copy) | No (drift) | Initial only | Starters, scaffolds |
| `create-*` CLIs | No (one-shot) | No | Initial only | Public starters |
| plop / hygen / Turborepo gen | Per-generator | No | In-repo | Component scaffolding |

The two genuinely composable surfaces are **npm packages** (TS, ESLint, Biome, Prettier) and **devcontainer Features**. VS Code Profiles look like the answer but explicitly **don't support inheritance** — Microsoft acknowledges this in the docs and the tracking issue (microsoft/vscode#156144) has been open since 2022. Treat profiles as role bundles ("Frontend", "Data Notebook"), not as a layering mechanism.

## Curated extension list

| Category | Extension (publisher.id) | Status (2026) | Notes |
|---|---|---|---|
| **Essential** | `dbaeumer.vscode-eslint` | Active, v3.0.21 | Supports flat + legacy; on Open VSX |
| | `esbenp.prettier-vscode` | Active | Skip if using Biome |
| | `biomejs.biome` | Active, v3 (May 2025) | **Pick Biome OR Prettier+ESLint, never both on same files** |
| | `editorconfig.editorconfig` | Stable | Required cross-team |
| | TypeScript | Built-in | Use **Select TypeScript Version → Use Workspace** for legacy |
| **Modern** | `yoavbls.pretty-ts-errors` | Active | Highly recommended |
| | `vitest.explorer` | Active | Vitest 1.x→4.x |
| | `ms-playwright.playwright` | Active, official MS | First-party |
| | `bradlc.vscode-tailwindcss` | Active, Tailwind v4 | Native in Zed |
| | `astro-build.astro-vscode`, `svelte.svelte-vscode`, `Vue.volar`, `Angular.ng-template` | All active, official | **Volar 2.0 removed need for separate TypeScript Vue Plugin** |
| **Legacy** | `dbaeumer.vscode-eslint` with `eslint.useFlatConfig: false` | Same extension | No legacy fork needed |
| | `johnpapa.angular2`, ES6 string-HTML highlighters | Lightly maintained | Best available for AngularJS/KO/Backbone |
| | Built-in `js-debug` | First-party | Replaces deprecated `vscode-chrome-debug`/`node-debug2` |
| **Optional** | `usernamehw.errorlens` | Active fork | Recommended pairing with ESLint |
| | `streetsidesoftware.code-spell-checker` | Very active | Commit `.cspell.json` |
| | `eamodio.gitlens` | Active (free tier sufficient) | Set `gitlens.plusFeatures.enabled: false` |
| | `humao.rest-client` | Stable | **Prefer over Thunder Client** (paywall + login required since 2024) |
| | `mikestead.dotenv` | Stable | Syntax highlighting only |
| **AVOID** | `eg2.tslint` | Deprecated since 2019 | Migrate via `tslint-to-eslint-config` |
| | `CoenraadS.bracket-pair-colorizer*` | Deprecated | Built-in is ~10,000× faster |
| | `octref.vetur` | Superseded by Volar | Remove on Vue 3 projects |
| | `rangav.vscode-thunder-client` | Freemium drift | Login + paywall since 2024 |
| | `wix.vscode-import-cost` | Effectively unmaintained | High CPU/RAM in modern VS Code |
| | `christian-kohler.path-intellisense`, `christian-kohler.npm-intellisense` | Redundant | Built-in TS auto-import covers it |
| | `Equinusocio.vsc-material-theme` | Privacy concerns 2023 | Use `zhuangtongfa.material-theme` instead |
| | Old `msjsdiag.debugger-for-chrome`, `ms-vscode.node-debug2` | Deprecated | Built-in `js-debug` covers both |

The single most actionable curation rule: **trust the built-ins**. Bracket colorization, auto-import, path completion, settings sync, debugger, and task runner are all native now; most "top 10 must-have" lists from 2018–2021 recommend extensions that are now obsolete.

## Cross-editor compatibility matrix

Legend: ✅ native · 🔌 official extension/plugin · ⚙️ via LSP · ❌ not supported

| Config file | VS Code | Cursor | Windsurf | Zed | WebStorm | Neovim |
|---|---|---|---|---|---|---|
| `.editorconfig` | 🔌 | 🔌 | 🔌 | ✅ (native + merged) | ✅ | ✅ (0.9+) |
| `eslint.config.js` / `.eslintrc.*` | 🔌 | 🔌 | 🔌 | ⚙️ built-in LSP | ✅ | ⚙️ |
| `.prettierrc.*` | 🔌 | 🔌 | 🔌 | ✅ built-in | ✅ (WebStorm) | ⚙️ via conform.nvim |
| `biome.json` | 🔌 | 🔌 | 🔌 | 🔌 official | 🔌 third-party | ⚙️ |
| `tsconfig.json` | ✅ | ✅ | ✅ | ⚙️ vtsls/tsserver | ✅ | ⚙️ |
| `.vscode/settings.json` | ✅ | ✅ (fork) | ✅ (fork) | ❌ (one-shot import only) | ❌ | ❌ |
| `.vscode/extensions.json` | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |

The clean separation: **tool-native files travel everywhere**; `.vscode/*` is a VS Code-family bonus. Cursor and Windsurf are direct VS Code forks that read `.vscode/settings.json` natively but use Open VSX (Cursor since 2025) or their own marketplace (Windsurf), and **Microsoft proprietary extensions** (Pylance, C/C++, Remote-SSH/WSL/Containers, Live Share) are blocked in forks since April 2025 — verify every recommended extension on Open VSX. Zed has its own `.zed/settings.json` and a one-shot VS Code settings importer; it integrates ESLint/Prettier/Biome as built-in or LSP. JetBrains has built-in support for `.editorconfig`, ESLint, Prettier, and `tsconfig`; Biome requires a third-party plugin with documented stability bugs.

## Adversarial round 1: the YAGNI senior engineer

**Critique:** "You've described a six-layer architecture, three npm packages, a CLI with `init`/`doctor`/`migrate`/`sync-vscode` subcommands, plus optional devcontainers. For a JS/TS team. This is config sprawl as a service. At scale, you'll have version drift across `@org/eslint-config@4`, `@org/tsconfig@2`, `@org/prettier-config@1.3`, where every CI failure becomes 'which package is wrong?' Onboarding doubles because new hires must understand both the factory's options and the underlying ESLint flat-config model. Every layer is a maintenance commitment in perpetuity. The team you're advising is probably 8 engineers, not 800. They'd be happier with one `eslint.config.js` in the org template repo, copied at project creation, and updated when someone notices it's stale. Stop building DX infrastructure that you'll then have to maintain instead of shipping product."

**Steelman points worth taking seriously:**
1. The maintenance burden of an in-house factory is real and recurring; ESLint/Biome/TS minor versions break things constantly.
2. Version drift across multiple `@org/*` packages produces confusing failures.
3. A `doctor` CLI is itself a piece of software that needs tests, releases, and a maintainer.
4. Most teams genuinely need *fewer* moving parts, not more; a single template repo + manual sync handles ~80% of cases.

**Revisions to the architecture:**
- **Collapse to one published package, not three.** Ship a single `@org/dx` package that re-exports the ESLint factory, owns the tsconfig JSON files in subpaths (`@org/dx/tsconfig/node22.json`), and exposes the Prettier config. One semver line, one Renovate PR per bump. This is the antfu/Sheriff pattern, not the Vercel/Shopify pattern.
- **Make the CLI optional, not core.** A `create-@org` for first-run is fine; a permanent `doctor` is not. Move `doctor` behind an opt-in script in `package.json` and skip it for teams that don't want it. The CLI exists to scaffold; it shouldn't run continuously.
- **Skip `sync-vscode` entirely as initial scope.** Generating `.vscode/settings.json` from JSON fragments is exactly the kind of clever indirection that costs more than it saves. Commit a 15-line `.vscode/settings.json` directly. If you ever need per-profile variants, address it then.
- **Default to "no devcontainer".** Ship one only for teams that explicitly request reproducibility (regulated environments, teams with mixed OS, Codespaces users). Most teams won't use it.
- **YAGNI test for new layers:** any proposed new layer must justify itself against the alternative of "just write it in the consumer's `eslint.config.js`."

The result: **one package, one tiny CLI, four committed files per project**. The original recommendation stands in spirit but should be ruthlessly slimmed at the edges.

## Adversarial round 2: attacking the tooling choices

**Critique against Biome:** "You waved at Biome as the new default for new projects. In 2026 it still has no React Compiler rules from `eslint-plugin-react-hooks` v6, no first-class Vue/Svelte/Astro SFC support, no npm-distributable plugin ecosystem (GritQL is interesting but every team's custom rule is a project, not a package), and weaker type-aware linting than `typescript-eslint`. The Vercel partnership funds type-inference work that isn't shipped yet. Meanwhile ESLint flat config is mature, `typescript-eslint` v8 is solid, antfu/Sheriff/Epic Web are battle-tested, and Oxlint is faster than Biome anyway. Your 'Biome for new Node-only and React projects' default ships teams into a tool whose ecosystem is two years behind."

**Critique against ESLint+Prettier:** "Conversely: ESLint flat config has been a churn nightmare for two years. The `extends` reintroduction in March 2025 admits the original flat-config design was wrong. typescript-eslint type-aware linting is 5–10× slower than non-typed; on a 200K-line monorepo your CI lint stage is genuinely 3–5 seconds with Biome and 30+ seconds with ESLint. Prettier-rust is in development specifically because Prettier-JS is too slow. Anthony Fu has written publicly about why he doesn't use Prettier (unstable diffs from `printWidth` reflows). You're recommending a stack the original authors are migrating away from."

**Critique against npm `extends`:** "Factory-as-config means every consumer's lint runs an arbitrary chunk of TypeScript at startup. Mistakes in the factory propagate to every repo on next install. A scaffolder that writes a stable `eslint.config.js` is more debuggable: the file in front of you is the file ESLint runs. With a factory, the file in front of you is `antfu({ typescript: true })` and you have no idea which 47 rules are now active."

**Critique against profile composition vs single flat config:** "You're proposing layered factory options. The honest version is one flat `eslint.config.js` per project, ~60 lines, fully visible. No factory, no options, no surprises. When a rule misfires you grep the file."

**Resolution:** All three critiques have merit and the answer is **stratified by project size and skill**, not a single recommendation:

- **Solo/small projects (\<5 engineers):** Biome only, no factory, no `@org/*` package. `biome init`, commit `biome.json`, done. The plugin gaps don't apply because you don't need niche plugins.
- **Mid-size with React/Node:** ESLint flat config + `@antfu/eslint-config` directly (don't even build `@org/eslint-config` — antfu's package is better than what you'd build). Use Biome formatter alongside if you want speed. Scaffold once, maintain manually.
- **Large org / fleet management (15+ repos):** This is the only case where building `@org/eslint-config` as a wrapper around antfu pays off — you add org-specific rules, control rollout, version-pin via Renovate. Even here, prefer wrapping antfu's factory rather than reimplementing it.
- **Vue/Svelte/Astro-heavy:** ESLint+Prettier is mandatory until Biome closes SFC support; not optional.
- **Enterprise with type-aware lint perf concerns:** Hybrid — Biome for format + non-typed lint, `typescript-eslint` only for the rules that genuinely require type info, scoped via `files` globs to limit type-checker spin-up.

**Revised default:** Don't build your own factory unless you're managing 15+ repos. Wrap or directly use `@antfu/eslint-config` or `eslint-config-sheriff`. The "build a factory" path was over-engineered for the median case.

## Adversarial round 3: legacy support — pollution vs. abandonment

**Pollution argument:** "Every legacy concession in the modern config is a future bug. `allowJs: true`, looser parsers, AMD/UMD globals, `eslint.useFlatConfig: false` — none of this should be one option flag away from the modern profile. Inevitably someone copies a legacy `eslint.config.js` to a greenfield project, the `legacy: true` flag stays on, and now your new TS-strict project silently allows `var`. The `@org/tsconfig/legacy` base sits in the same npm package as `@org/tsconfig/strictest` and gets bumped together. Legacy support is a tar pit — don't bring it inside the modern config's tent."

**Abandonment argument:** "Refusing to support legacy means every team running Node 18, AngularJS, or `.eslintrc.*` writes their own one-off config. They get no Renovate updates, no shared lint rules, no spell-check config, no extension recommendations. They become second-class citizens in the org and the configs drift further. Worse: the modern config team treats these projects as 'not their problem' and the legacy projects accumulate security debt. ~104K–158K AngularJS domains and 72%+ jQuery web presence aren't going away in 2026."

**Resolution — physical isolation, shared base:**
- **Share the base layer only:** `.editorconfig`, `.cspell.json`, `.vscode/extensions.json`, organizational lint rules (no-secrets, banned-imports). These apply identically modern and legacy.
- **Separate the language/runtime layers physically:** Ship `@org/eslint-config-legacy` as a **distinct npm package**, not a flag on the modern factory. Likewise `@org/tsconfig-legacy`. This kills the "flag stays on by accident" failure mode because adopting legacy is an explicit dependency choice.
- **Pin legacy projects to ESLint 8 / typescript-eslint 5 / their TS major** via the legacy package's peer dependencies. The legacy package can be archived and unmaintained without affecting the modern package's release cadence.
- **Document a sunset:** Each legacy project's README states which package versions it pins and when those go EOL (typically 2–3 years out). This converts ambient debt into an explicit liability with a date.
- **Use VS Code Profiles here, where they actually fit:** create a "Legacy" profile that bundles AngularJS/jQuery extensions, `eslint.useFlatConfig: false`, the older debug-launch templates. Profiles are monolithic snapshots — perfect for "I'm in maintenance mode this afternoon."
- **Devcontainer is the cleanest tool for genuinely old runtimes:** Node 16 + Webpack 4 + the OpenSSL legacy provider in a containerized environment, shipped as `ghcr.io/org/devcontainer-legacy-node16`, means the legacy mess never touches the developer's host machine.

The synthesis: **legacy gets first-class but isolated treatment**. Modern config doesn't carry legacy flags. Legacy config is its own package with its own version line and an explicit sunset. Profiles and devcontainers — which compose poorly for the modern case — are exactly right for the legacy case because they're snapshots, and a legacy project *is* a snapshot in time.

## Final implementation roadmap

**Week 1 (must have):** Commit `.editorconfig`, `.vscode/settings.json` (≤20 lines, project-correctness only), `.vscode/extensions.json` (recommendations + unwantedRecommendations), `tsconfig.json` extending `@tsconfig/node22` and `@tsconfig/strictest`, and either `biome.json` or an `eslint.config.js` wrapping `@antfu/eslint-config` directly. Pin tool versions in `package.json`. Add `lint`, `format`, `typecheck` scripts so CI matches the editor.

**Month 1 (if scale justifies):** Publish `@org/eslint-config` only if you manage 15+ repos and need org-specific rules; otherwise, stay on antfu/Sheriff. Publish `@org/tsconfig` with two leaves (`base.json`, your strict variant). Build a tiny `create-@org` scaffolder using `@clack/prompts` that writes the four files. Set up Renovate for automated bumps.

**Quarter 1 (only if needed):** Add `@org/eslint-config-legacy` as a separate package with its own pin set when you have ≥3 legacy projects. Build a devcontainer image for the legacy runtime if those projects are routinely touched. Add a `doctor` CLI command if drift is observed in practice — not preemptively.

**Never (unless explicitly forced):** Don't build VS Code Profile composition tooling. Don't build `sync-vscode` JSON-fragment merging. Don't ship a monorepo-template + sync-bot. Don't run Biome and ESLint+Prettier on the same files.

## Reference snippets

`.editorconfig` (universal):
```ini
root = true
[*]
charset = utf-8
end_of_line = lf
indent_style = space
indent_size = 2
insert_final_newline = true
trim_trailing_whitespace = true
[*.md]
trim_trailing_whitespace = false
```

`tsconfig.json` (modern Node, TS 5.0+ array extends):
```jsonc
{
  "extends": [
    "@tsconfig/node22/tsconfig.json",
    "@tsconfig/strictest/tsconfig.json"
  ],
  "compilerOptions": {
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "noUncheckedIndexedAccess": true,
    "outDir": "./dist",
    "rootDir": "./src"
  },
  "include": ["src/**/*"]
}
```

`eslint.config.js` (using antfu directly — the recommended starting point):
```js
import antfu from '@antfu/eslint-config'
export default antfu({
  type: 'app',
  typescript: true,
  stylistic: { indent: 2, quotes: 'single', semi: false },
  formatters: { css: true, markdown: 'prettier' },
  gitignore: true,
})
```

`.vscode/settings.json` (committed, minimal, travels to Cursor/Windsurf):
```jsonc
{
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.fixAll.eslint": "explicit"
  },
  "eslint.useFlatConfig": true,
  "eslint.rules.customizations": [
    { "rule": "style/*", "severity": "off", "fixable": true }
  ],
  "typescript.tsdk": "node_modules/typescript/lib",
  "typescript.enablePromptUseWorkspaceTsdk": true,
  "[typescript]": { "editor.defaultFormatter": "dbaeumer.vscode-eslint" },
  "[javascript]": { "editor.defaultFormatter": "dbaeumer.vscode-eslint" }
}
```

`.vscode/extensions.json` (Open VSX-verified for fork compatibility):
```jsonc
{
  "recommendations": [
    "dbaeumer.vscode-eslint",
    "editorconfig.editorconfig",
    "yoavbls.pretty-ts-errors",
    "usernamehw.errorlens",
    "streetsidesoftware.code-spell-checker",
    "vitest.explorer"
  ],
  "unwantedRecommendations": [
    "eg2.tslint",
    "CoenraadS.bracket-pair-colorizer",
    "CoenraadS.bracket-pair-colorizer-2",
    "octref.vetur",
    "rangav.vscode-thunder-client",
    "wix.vscode-import-cost"
  ]
}
```

Legacy `tsconfig.json` (separate package, separate sunset):
```jsonc
{
  "extends": "@org/tsconfig-legacy/base.json",
  "compilerOptions": {
    "target": "ES2018",
    "module": "CommonJS",
    "moduleResolution": "node",
    "allowJs": true,
    "checkJs": false,
    "strict": false,
    "skipLibCheck": true,
    "esModuleInterop": true
  },
  "exclude": ["node_modules", "dist", "vendor"]
}
```

## Conclusion: the second-order lesson

The deepest takeaway from three rounds of adversarial review isn't about Biome vs ESLint or factories vs scaffolders. It's that **most teams over-architect their config layer because the work feels productive while shipping nothing**. The ecosystem already gives you composition for free in three places — TS 5.0 array `extends`, ESLint flat-config arrays, and Biome's npm extends — and ignoring those to build your own meta-system is the failure mode. The same logic kills the temptation to use VS Code Profiles for layering: they're snapshots, not presets, and trying to make them composable wastes weeks for a feature Microsoft explicitly says doesn't exist. Legacy support follows the same rule from the opposite direction: don't try to make one config serve both eras; isolate legacy as a frozen island with its own package, its own pin set, and its own sunset date.

The architecture that survives all three rounds of attack is the one that does the *least*: one shareable config (probably antfu's, possibly your wrapper around it), one tsconfig package with two leaves, four committed files per project, one tiny scaffolder, and clear physical separation between modern and legacy. Everything else — the doctor CLI, the sync-vscode merger, the devcontainer matrix, the profile composition tooling — is optional and should remain unshipped until concrete pain demands it. In a domain where every layer is a tax, the right answer is almost always to build less and let the tools' native composition do the work.
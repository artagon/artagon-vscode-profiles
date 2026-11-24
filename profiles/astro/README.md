# Web / Astro Profile (`astro`)

Compiled VS Code profile for Astro, TypeScript/JavaScript, Tailwind CSS, HTML/CSS, and Node/pnpm/Bun development.

- Modules: `core-editor`, `ai-agents`, `markdown`, `shell`, `ts-base`, `ts-eslint-prettier`, `astro`.
- Switch a workspace from ESLint+Prettier to Biome with `--with biome --without ts-eslint-prettier` (automatic settings-key transition cleans up stale Prettier/ESLint formatter keys).

## Usage

```bash
# Apply team-shareable workspace settings + recommendations (no personal UX/AI leakage)
bash scripts/vspcli apply --target=workspace /path/to/astro-project

# Switch a Biome-based Astro repo to Biome
bash scripts/vspcli apply --target=workspace --with biome --without ts-eslint-prettier /path/to/astro-project

# Import full compiled profile bundle
bash scripts/import-profile.sh astro exports/astro.code-profile
```

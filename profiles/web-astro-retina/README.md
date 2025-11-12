Web (Astro/Node) — Retina

Purpose
- Front‑end web development with Astro, TypeScript/JavaScript, HTML/CSS, and Node/npm.
- Retina rendering baseline (Catppuccin Mocha theme, Catppuccin icons; font aliasing auto for HiDPI).

Included Extensions
- Astro: astro-build.astro-vscode
- Lint/Format: dbaeumer.vscode-eslint, esbenp.prettier-vscode
- CSS: bradlc.vscode-tailwindcss (optional if using Tailwind)
- NPM: christian-kohler.npm-intellisense, eg2.vscode-npm-script
- Utilities: mikestead.dotenv, eamodio.gitlens, usernamehw.errorlens, streetsidesoftware.code-spell-checker, redhat.vscode-yaml, ms-azuretools.vscode-docker
- Themes/Icons: Catppuccin, Tokyo Night, One Dark, Night Owl, Dracula, Material Theme; Material Icon Theme, vscode-icons
- AI: GitHub Copilot, GitHub Copilot Chat

Prerequisites
- Node.js (LTS) + npm (or yarn/pnpm) installed
- Optional: nvm/fnm/volta to manage Node versions

Project Setup
- New project: `npm create astro@latest` (or `pnpm create astro@latest`)
- Install deps: `npm install`
- Dev server: `npm run dev`

Formatting & Linting
- Uses project‑local Prettier/ESLint configs when present (.prettierrc, .eslintrc*)
- VS Code settings default to Prettier and format on save for JS/TS/HTML/CSS/JSON; Astro uses the Astro extension formatter

Environment
- .env files supported via mikestead.dotenv (loaders depend on your framework tooling)

Usage
- Open profile without installing extensions (first run/compare themes):
  - `vspcli --open-profiles --skip-install web-astro-retina`
- Install declared extensions:
  - `bash ~/.config/vscode/scripts/install-extensions.sh web-astro-retina`

Maintenance
- Recompose settings: `bash ~/.config/vscode/scripts/compose-settings.sh web-astro-retina`
- Export bundle: `bash ~/.config/vscode/scripts/export-profiles.sh web-astro-retina`


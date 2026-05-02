# Per-Project Files: The `.vscode/` Folder

Reference for every file VS Code looks for in a workspace's `.vscode/` directory, plus the `.code-workspace` multi-root format. All files are JSONC.

## `settings.json`

Workspace-scoped settings. Same key/value vocabulary as user settings, but constrained by the setting's declared scope (see `precedence.md` — `application` and `machine`-scoped settings can't be set here).

```jsonc
{
  "editor.tabSize": 2,
  "editor.formatOnSave": true,
  "[python]": {
    "editor.tabSize": 4,
    "editor.formatOnSave": true
  },
  "files.exclude": {
    "**/__pycache__": true,
    "**/.pytest_cache": true
  }
}
```

Commit this to version control if the team should share it. Don't commit personal preferences (font size, color theme) — those belong in user settings.

**Variable expansion caveat:** `${workspaceFolder}`, `${env:VAR}`, etc. work in `launch.json` and `tasks.json` everywhere, but in `settings.json` they're only honored for an allowlist of keys (paths to executables, search exclusions, terminal cwd, a few extension-contributed settings). Setting `"editor.fontFamily": "${workspaceFolder}/fonts/Foo"` does *not* expand — VS Code stores the literal string. When in doubt, test by reopening the workspace and inspecting the resolved value via `Developer: Inspect Editor Tokens and Scopes` or the extension's own diagnostic command.

## `launch.json`

Debugger configurations. Top-level shape:

```jsonc
{
  "version": "0.2.0",
  "configurations": [ /* ... */ ],
  "compounds": [ /* optional: launch multiple configs together */ ]
}
```

Each configuration must declare `type` (debugger ID, e.g., `node`, `python`, `coreclr`, `lldb`), `request` (`launch` or `attach`), and `name`. Schema for the rest depends on the debugger; install the relevant language extension and IntelliSense fills in.

Variables you can use: `${workspaceFolder}`, `${file}`, `${fileBasename}`, `${fileDirname}`, `${env:VAR}`, `${input:varName}` (with an `inputs` block at the top level for prompts). Note `args` is a JSON array of strings, and Windows path escapes (`\\`) are preserved literally — they reach the program with the doubled backslashes, which has bitten Python users repeatedly.

Compounds are useful for multi-process debugging:

```jsonc
{
  "compounds": [
    { "name": "Server + Client", "configurations": ["Server", "Client"] }
  ]
}
```

## `tasks.json`

Tasks for build, test, lint, anything else you'd run in a terminal. Top-level **must** declare `"version": "2.0.0"` — the older 0.1.0 format is unsupported and rejected.

```jsonc
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "build",
      "type": "shell",
      "command": "npm run build",
      "group": { "kind": "build", "isDefault": true },
      "problemMatcher": "$tsc"
    }
  ]
}
```

Key fields:
- `type` — `shell`, `process`, or extension-contributed types (`npm`, `gulp`, `typescript`, `docker-build`, etc.)
- `group` — `build` or `test`, with `isDefault: true` to make `Cmd/Ctrl+Shift+B` run it
- `presentation` — controls terminal panel behavior (`echo`, `reveal`, `focus`, `panel`, `clear`)
- `problemMatcher` — parses output for errors/warnings; can be a built-in name (`$tsc`, `$eslint-stylish`) or a custom regex object
- `dependsOn` and `dependsOrder` — task chaining
- `runOptions.runOn: "folderOpen"` — auto-run when the folder opens (requires user trust)

Tasks defined here can be referenced from `launch.json`'s `preLaunchTask` and `postDebugTask`.

## `extensions.json`

Extension recommendations and warnings.

```jsonc
{
  "recommendations": [
    "dbaeumer.vscode-eslint",
    "esbenp.prettier-vscode"
  ],
  "unwantedRecommendations": [
    "ms-vscode.vscode-typescript-tslint-plugin"
  ]
}
```

When the workspace is opened, VS Code prompts the user once to install missing recommendations. `unwantedRecommendations` suppresses suggestions VS Code might otherwise make based on detected file types.

**Important distinction — `unwantedRecommendations` is *not* enforcement.** It only hides the "you should install this" prompt. Users can still install anything they want. To actually *block* extension installation, you need an enterprise policy (`AllowedExtensions` on Windows ADMX, the equivalent macOS `.mobileconfig` key, or `policy.json` on Linux). See the enterprise policies section of `precedence.md`. Common confusion: a team puts an extension in `unwantedRecommendations` thinking it's banned, then a developer installs it anyway and breaks something.

Extension IDs use the form `<publisher>.<name>` exactly as they appear in the marketplace URL. Mistyped IDs fail silently — no error, just no prompt.

For auditing: read the file, compare `recommendations` to your approved list, flag entries that contradict `unwantedRecommendations` (this is invalid but VS Code won't catch it), and flag any extensions not on the approved list.

## `mcp.json`

Model Context Protocol server configurations, used by Copilot Chat and other AI assistants. Workspace-scoped servers go here; user-scoped go in `User/mcp.json`.

```jsonc
{
  "servers": {
    "myServer": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@example/mcp-server"]
    },
    "remoteServer": {
      "type": "http",
      "url": "https://api.example.com/mcp"
    }
  },
  "inputs": [
    { "id": "apiKey", "type": "promptString", "password": true, "description": "API key" }
  ]
}
```

Server `type` is `stdio` or `http`. Sensitive values should reference `inputs` via `${input:apiKey}` rather than being hard-coded. Workspace MCP servers prompt for trust on first run; user-scoped servers don't.

`mcp.json` supports VS Code's standard variable substitution: `${workspaceFolder}`, `${userHome}`, `${env:VAR}`, and `${input:id}` all resolve. This was a long-standing gap that has since been closed — older blog posts and issues recommending absolute paths are out of date. The `dev` block is also supported (`watch` glob and `debug.type` for Node/Python MCP server debugging).

## Snippets — `<name>.code-snippets`

Project snippets live as `.vscode/<anything>.code-snippets`. Multiple files allowed; merged at load. Format:

```jsonc
{
  "Print to console": {
    "scope": "javascript,typescript",
    "prefix": "log",
    "body": ["console.log('$1');", "$2"],
    "description": "Log output to console"
  }
}
```

`scope` is a comma-separated list of language IDs. Omit it for global snippets. `$1`, `$2`, ... are tab stops; `${1:default}` provides a placeholder; `$0` is the final cursor position.

## Other extension-specific files

These show up in `.vscode/` for various extensions; not core but common:

- `c_cpp_properties.json` — Microsoft C/C++ extension include paths and IntelliSense mode
- `cmake-kits.json`, `cmake-variants.yaml` — CMake Tools extension
- `.cspell.json` — Code Spell Checker
- `argv.json` — **does not exist here**; argv.json is a *user-level* file in `~/.vscode/` (see `file-locations.md`)

## Multi-root workspace files (`.code-workspace`)

A `.code-workspace` file describes a workspace that includes multiple folders. It can sit anywhere — commonly in the parent directory of the included folders.

```jsonc
{
  "folders": [
    { "path": "frontend" },
    { "path": "backend" },
    { "name": "Shared Lib", "path": "../shared-utilities" }
  ],
  "settings": {
    "editor.tabSize": 2
  },
  "launch": {
    "version": "0.2.0",
    "configurations": [ /* ... */ ]
  },
  "tasks": {
    "version": "2.0.0",
    "tasks": [ /* ... */ ]
  },
  "extensions": {
    "recommendations": [ /* ... */ ]
  }
}
```

Folder paths are resolved relative to the `.code-workspace` file's location. Use `name` to give a folder a display name different from its directory name.

**Precedence reminder:** if a folder inside a multi-root workspace has its own `.vscode/settings.json`, that file's settings override the multi-root file's `settings` block *for files in that folder only*. The multi-root settings act as the workspace layer; the folder file acts as the workspace folder layer.

## Validation tips

For any of these files, the Problems panel will surface JSONC syntax errors and (for files with schemas like `tasks.json`, `launch.json`, `extensions.json`) schema validation errors. If editing programmatically:

- Use a JSONC-aware parser (Python: `pyjson5` or strip comments first; Node: `jsonc-parser`).
- Preserve comments when writing back — the user wrote them for a reason.
- For `tasks.json`, always include `"version": "2.0.0"` on creation.
- For `launch.json`, the `version` is `"0.2.0"` (yes, different) — easy to typo.

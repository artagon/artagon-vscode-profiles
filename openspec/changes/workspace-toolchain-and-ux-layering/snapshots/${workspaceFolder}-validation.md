# VS Code terminal validation

Validated on 2026-08-17 in the installed Visual Studio Code stable application.

## Absolute-path fixtures

The CLI generated `.vscode/` for two distinct workspace roots:

- `/private/tmp/vspcli-gui.qCyhIj/test1`
- `/private/tmp/vspcli-gui.qCyhIj/test2`

Each fixture contained `Cargo.toml` and a copied `_shared/rtk/` tree. Both
workspaces were opened through the VS Code UI and explicitly trusted.

## Results

- Both terminals selected the generated `rtk-fish` profile.
- A fresh terminal in `test2` printed `rtk wrappers active — workspace trust granted`.
- `type rg` in `test1` resolved the wrapper defined from
  `/private/tmp/vspcli-gui.qCyhIj/test1/_shared/rtk/rtk-init.fish`.
- `type rg` in `test2` resolved the wrapper defined from
  `/private/tmp/vspcli-gui.qCyhIj/test2/_shared/rtk/rtk-init.fish`.
- After regenerating settings with `workbench.sideBar.location: "left"` and
  reopening `test2`, the VS Code UI showed Explorer in the left sidebar and
  Chat in the right secondary sidebar.

The `${workspaceFolder}` substitution in terminal-profile `args` therefore
resolved independently for both absolute workspace roots. No absolute
path-rewrite fallback is required for this VS Code version.

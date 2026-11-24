# VS Code task shell precedence

Validated on 2026-08-17 in the trusted `/private/tmp/vspcli-gui.qCyhIj/test2`
workspace.

The workspace settings selected the generated `rtk-fish` terminal and
`terminal.integrated.automationProfile.osx` pointed to the same fish profile.
The fixture task at
`scripts/tests/fixtures/terminal-shell-precedence/tasks.json` set an explicit
`options.shell.executable` of `/bin/sh`.

Running `fixture: explicit sh task` through the VS Code Tasks UI produced:

```text
task-shell=/bin/sh
```

`tasks[].options.shell` wins over `terminal.integrated.automationProfile`.
The automation profile selects the default task shell only when the task does
not provide its own shell override.

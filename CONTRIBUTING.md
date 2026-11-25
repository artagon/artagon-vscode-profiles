# Contributing

The unified `vsp` CLI and composition engine live in `crates/vsp/`. Install Rust and Cargo to build or test the engine. For shell and documentation checks, install Bash, `jq`, ShellCheck, Zsh, and Fish. Use the VS Code CLI (`code`) for live profile checks.

## Edit source definitions

| Change | Source |
| --- | --- |
| Capability, detection signal, or toolchain | `packs/*.jsonc` and `packs/rtk-terminal/` |
| Profile recipe, legacy alias, or display preset | `presets.jsonc` |
| Extension lifecycle status, catalog defaults, or publisher policy | `catalog/extensions.jsonc` |
| Setting scopes, forbidden keys, or autonomy policy | `catalog/policy.jsonc` |
| VSIX verification metadata | `vsp.lock.json` |
| CLI, resolver, planner, or synchronization behavior | `crates/vsp/` |

Keep pack dependencies and conflicts explicit. Update both scope and policy metadata when introducing a managed setting. Review the platform, version, digest, and approved Marketplace URL for VSIX changes. Keep unverified records marked `pending_refresh`.

## Verify changes

From the repository root:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo build --workspace
./target/debug/vsp --catalog . dev validate
./target/debug/vsp --catalog . dev dist --out ./dist
bash scripts/generate-readme-matrix.sh --check
bash scripts/lib/apply-rust-policy.sh --check
```

The `vsp dev validate` command checks the source catalog, packs, dependency graph, scope rules, and extension policy. Use `vsp dev dist` to render release bundles. For changes to composition or export, compare two distribution runs in separate directories and review `SHA256SUMS`. Also run validation with the embedded catalog for changes to the snapshot or its loader.

For changes under `packs/rtk-terminal/scripts/`, run:

```sh
shellcheck packs/rtk-terminal/scripts/rtk-init.bash
zsh -n packs/rtk-terminal/scripts/rtk-init.zsh
for file in packs/rtk-terminal/scripts/zsh/.zshenv \
    packs/rtk-terminal/scripts/zsh/.zprofile \
    packs/rtk-terminal/scripts/zsh/.zshrc \
    packs/rtk-terminal/scripts/zsh/.zlogin; do
  zsh -n "$file" || exit
done
fish -n packs/rtk-terminal/scripts/rtk-init.fish
```

## Prepare a change

1. Stage source and test changes. Leave `_merged/`, `exports/`, `dist/`, and `profiles/*/{extensions,settings}.json` outside the commit.
2. If the matrix check reports drift, regenerate its marked section with `bash scripts/generate-readme-matrix.sh --write`.
3. Include relevant test results and any change to a CLI flag, generated bundle, or security policy in the pull request.

The optional `bash scripts/install-hooks.sh` command sets `core.hooksPath` to `scripts/git-hooks` when the setting is unset. The hook validates the repository and rejects staged generated profile artifacts.

# Contributing

The unified `vsp` CLI and composition engine live in `crates/vsp/`. Install Rust and Cargo to build or test the engine. For shell and documentation checks, install Bash, `jq`, ShellCheck, Zsh, and Fish. Use the VS Code CLI (`code`) for live profile checks.

## Edit source definitions

Edit the canonical v3 source definitions below. Legacy `modules/`, `profiles/`, and `catalog/*.json` trees remain in the repository only for legacy BATS compatibility suites.

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
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --all-features --no-deps
cargo build --locked --release --bin vsp
./target/release/vsp --catalog . dev validate
./target/release/vsp --catalog . dev dist --out ./dist
./target/release/vsp --catalog . dev docs --check
bash scripts/lib/apply-rust-policy.sh --check
ruby -c Formula/vsp.rb
```

The `vsp --catalog . dev validate` command checks the canonical catalog, packs, dependency graph, scope rules, and extension policy. The `vsp --catalog . dev docs --check` command verifies both the `README.md` matrix block and the `docs/rust.md` §12 policy blocks. Use `vsp dev dist` to render release bundles. For changes to composition or export, compare two distribution runs in separate directories and review `SHA256SUMS`. Also run validation with the embedded catalog for changes to the snapshot or its loader.

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
2. If `dev docs --check` reports drift, regenerate the marked sections with `./target/release/vsp --catalog . dev docs --write` (or `cargo run --quiet --bin vsp -- --catalog . dev docs --write`) and re-run `--check`.
3. Include relevant test results and any change to a CLI flag, generated bundle, or security policy in the pull request.

The optional `bash scripts/install-hooks.sh` command sets `core.hooksPath` to `scripts/git-hooks` when the setting is unset. The hook validates the repository and rejects staged generated profile artifacts.

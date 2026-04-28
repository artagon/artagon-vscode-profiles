#!/usr/bin/env bash
# install-hooks.sh - idempotent installer for the project's git hooks.
#
# Behaviour:
#   - If core.hooksPath is unset, set it to scripts/git-hooks (relative to repo root).
#   - If core.hooksPath is already scripts/git-hooks, exit 0 with "already installed".
#   - If core.hooksPath is set to anything else, exit non-zero with a clear message.
#     The user is presumed to be using husky/pre-commit/etc. and must opt in.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="scripts/git-hooks"

current="$(git -C "$ROOT" config --get core.hooksPath || true)"

if [ -z "$current" ]; then
  git -C "$ROOT" config core.hooksPath "$TARGET"
  echo "install-hooks: set core.hooksPath = $TARGET"
  exit 0
fi

if [ "$current" = "$TARGET" ]; then
  echo "install-hooks: already installed (core.hooksPath = $TARGET)"
  exit 0
fi

cat >&2 <<EOF
install-hooks: refusing to overwrite existing core.hooksPath = $current

This usually means another tool (husky, pre-commit, lefthook, etc.) is managing
your hooks. Either:
  - integrate this project's hooks into your existing setup, or
  - explicitly run: git config core.hooksPath $TARGET
EOF
exit 1

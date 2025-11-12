#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-}"
if [[ -z "$PROFILE" ]]; then
  echo "Usage: $(basename "$0") <cpp-profile>" >&2
  echo "Profiles: cpp-clangd, cpp-intellisense" >&2
  exit 1
fi
SRC_DIR="$ROOT/profiles/$PROFILE"
KITS_SRC="$SRC_DIR/cmake-kits.example.json"
TARGET_DIR="${ROOT}/../.vscode"
KITS_DST="$TARGET_DIR/cmake-kits.json"
if [[ ! -f "$KITS_SRC" ]]; then
  echo "Template not found: $KITS_SRC" >&2
  exit 1
fi
mkdir -p "$TARGET_DIR"
cp "$KITS_SRC" "$KITS_DST"
echo "Copied $KITS_SRC -> $KITS_DST" >&2
TOOLCHAIN_SRC="$ROOT/cmake/toolchains/llvm-homebrew.cmake"
TOOLCHAIN_DST="${ROOT}/../cmake/toolchains/llvm-homebrew.cmake"
if [[ -f "$TOOLCHAIN_SRC" ]]; then
  mkdir -p "$(dirname "$TOOLCHAIN_DST")"
  cp "$TOOLCHAIN_SRC" "$TOOLCHAIN_DST"
  echo "Copied $TOOLCHAIN_SRC -> $TOOLCHAIN_DST" >&2
fi
cat <<MSG
Setup complete. Next steps:
1. Open VS Code via 'vsp $PROFILE <path>'
2. Run the command 'CMake: Select Kit' and pick one from .vscode/cmake-kits.json
3. (Optional) Edit .vscode/cmake-kits.json or cmake/toolchains/*.cmake to reference your custom compilers.
MSG

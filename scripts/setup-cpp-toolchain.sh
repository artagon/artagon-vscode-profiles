#!/usr/bin/env bash
# setup-cpp-toolchain.sh — Copy CMake toolchain / kits templates into an explicit workspace.
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
PROFILE="${1:-}"
WORKSPACE="${2:-.}"

if [[ -z "${PROFILE}" ]]; then
  echo "Usage: $(basename -- "${0}") <cpp-profile> [workspace-dir]" >&2
  echo "Profiles: cpp-clangd, cpp-intellisense" >&2
  exit 1
fi

SRC_DIR="${ROOT}/profiles/${PROFILE}"
if [[ ! -d "${SRC_DIR}" ]]; then
  echo "Unknown C/C++ profile: ${PROFILE}" >&2
  echo "Profiles: cpp-clangd, cpp-intellisense" >&2
  exit 1
fi

if [[ ! -d "${WORKSPACE}" ]]; then
  echo "Workspace directory does not exist: ${WORKSPACE}" >&2
  exit 1
fi

WS_ABS="$(CDPATH='' cd -- "${WORKSPACE}" && pwd -P)"
KITS_SRC="${SRC_DIR}/cmake-kits.example.json"
if [[ -f "${KITS_SRC}" ]]; then
  mkdir -p "${WS_ABS}/.vscode"
  cp "${KITS_SRC}" "${WS_ABS}/.vscode/cmake-kits.json"
  echo "Copied ${KITS_SRC} -> ${WS_ABS}/.vscode/cmake-kits.json" >&2
fi

TOOLCHAIN_SRC="${ROOT}/cmake/toolchains/llvm-homebrew.cmake"
TOOLCHAIN_DST="${WS_ABS}/cmake/toolchains/llvm-homebrew.cmake"
if [[ -f "${TOOLCHAIN_SRC}" ]]; then
  mkdir -p "$(dirname -- "${TOOLCHAIN_DST}")"
  cp "${TOOLCHAIN_SRC}" "${TOOLCHAIN_DST}"
  echo "Copied ${TOOLCHAIN_SRC} -> ${TOOLCHAIN_DST}" >&2
fi

cat <<MSG
Setup complete for '${PROFILE}' in ${WS_ABS}.
Next steps:
1. Apply workspace config via 'vspcli apply --target=workspace --toolchain ${PROFILE} ${WS_ABS}'
2. Open VS Code via 'vspcli open ${PROFILE} ${WS_ABS}'
3. Run 'CMake: Select Kit' in VS Code if you want a custom LLVM/GCC kit.
MSG

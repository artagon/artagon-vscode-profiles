#!/usr/bin/env bash
# setup-rust-toolchain.sh — Ensure rustup components and sync Rust profile extensions.
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
PROFILE="${1:-rust}"
COMPONENTS=(rust-src rustfmt clippy)

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup is required to manage Rust components" >&2
  exit 1
fi
for comp in "${COMPONENTS[@]}"; do
  echo "Ensuring rustup component: ${comp}"
  rustup component add "${comp}" >/dev/null || true
done

INSTALL_SCRIPT="${SCRIPT_ROOT}/scripts/install-extensions.sh"
if [[ -f "${INSTALL_SCRIPT}" ]]; then
  echo "Ensuring VS Code extensions for ${PROFILE}"
  if ! VSPCLI_ROOT="${ROOT}" "${BASH}" "${INSTALL_SCRIPT}" "${PROFILE}"; then
    echo "Warning: extension install failed for ${PROFILE}" >&2
  fi
fi

cat <<MSG
Rust toolchain ready.
- Components checked: ${COMPONENTS[*]}
- VS Code profile synced: ${PROFILE}
Next steps:
1. Open your workspace via 'vspcli open ${PROFILE} <path>'
2. Or apply workspace config via 'vspcli apply --target=workspace <path>'
MSG

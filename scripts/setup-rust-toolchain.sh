#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-rust-profile-crisp}"
COMPONENTS=(rust-src rustfmt clippy)
if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup is required to manage Rust components" >&2
  exit 1
fi
for comp in "${COMPONENTS[@]}"; do
  echo "Ensuring rustup component: $comp"
  rustup component add "$comp" >/dev/null || true
done
INSTALL_SCRIPT="$ROOT/scripts/install-extensions.sh"
if [[ -x "$INSTALL_SCRIPT" ]]; then
  echo "Ensuring VS Code extensions for $PROFILE"
  if ! bash "$INSTALL_SCRIPT" "$PROFILE"; then
    echo "Warning: extension install failed for $PROFILE" >&2
  fi
fi
cat <<MSG
Rust toolchain ready.
- Components checked: ${COMPONENTS[*]}
- VS Code profile synced: $PROFILE
Next steps:
1. Open your workspace via 'vsp $PROFILE <path>'
2. Copy profiles/$PROFILE/tasks.example.json to .vscode/tasks.json if you want cargo build/test shortcuts.
MSG

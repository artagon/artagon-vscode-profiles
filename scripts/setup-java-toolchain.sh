#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-java-profile-crisp}"
WORKSPACE="${2:-}"
PROFILE_DIR="$ROOT/profiles/$PROFILE"
INSTALL_SCRIPT="$ROOT/scripts/install-extensions.sh"
if [[ ! -d "$PROFILE_DIR" ]]; then
  echo "Unknown profile: $PROFILE" >&2
  echo "Available: $(cd "$ROOT/profiles" && ls -1 java-* 2>/dev/null | paste -sd ' ')" >&2
  exit 1
fi

if command -v jenv >/dev/null 2>&1; then
  if ! jenv doctor >/dev/null 2>&1; then
    echo "Warning: jenv detected issues. Run 'jenv doctor' for details." >&2
  fi
  if [[ ! -d "$HOME/.jenv/plugins/export" ]]; then
    echo "Tip: enable jenv export plugin with 'jenv enable-plugin export' so JAVA_HOME stays in sync." >&2
  fi
else
  echo "Warning: jenv not found. Ensure JAVA_HOME is set before using Java profiles." >&2
fi

if [[ -z "${JAVA_HOME:-}" ]]; then
  echo "Warning: JAVA_HOME is not set. Java tooling won't have a JDK reference until you set it (or enable jenv export)." >&2
fi

echo "Ensuring VS Code extensions for $PROFILE"
if [[ -x "$INSTALL_SCRIPT" ]]; then
  bash "$INSTALL_SCRIPT" "$PROFILE" || echo "Warning: extension install failed for $PROFILE" >&2
else
  echo "Warning: install script missing at $INSTALL_SCRIPT" >&2
fi

TASKS_SRC="$PROFILE_DIR/tasks.example.json"
if [[ -n "$WORKSPACE" && -f "$TASKS_SRC" ]]; then
  mkdir -p "$WORKSPACE/.vscode"
  cp "$TASKS_SRC" "$WORKSPACE/.vscode/tasks.json"
  echo "Copied $TASKS_SRC -> $WORKSPACE/.vscode/tasks.json" >&2
fi

echo "Java toolchain ready."
echo "Active JAVA_HOME: ${JAVA_HOME:-<not set>}"
cat <<MSG
Next steps:
1. Open your workspace via 'vsp $PROFILE <path>'.
2. Copy tasks template manually (if not auto-copied): $TASKS_SRC -> <workspace>/.vscode/tasks.json
3. Use Gradle/Maven commands from the Tasks pane or VS Code Java commands.
MSG

#!/usr/bin/env bash
# extract-rust-policy.sh — extract §12 "Docs Policy for AI Agents" body
# from docs/rust.md (the markdown block between the section and `---`)
# and emit it on stdout as the agent-doc Documentation Policy.
# Used by §6.3 (writes the block) and §6.3a (CI staleness gate).
#
# Usage:
#   extract-rust-policy.sh <path/to/rust.md>

set -euo pipefail

SRC="${1:-docs/rust.md}"
[[ -f "$SRC" ]] || { echo "error: not found: $SRC" >&2; exit 1; }

# Pull the inner ```md ... ``` block from §12 — that's the canonical
# policy text agent docs embed (skipping the meta "Add to: AGENTS.md..."
# instructional preamble).
awk '
  /^# 12\. Docs Policy for AI Agents/ { found=1; next }
  found && /^```md$/ { in_block=1; next }
  in_block && /^```[a-z]+$/ { inner=1; print; next }
  in_block && /^```$/ {
    if (inner) { inner=0; print; next }
    exit
  }
  in_block { print }
' "$SRC"

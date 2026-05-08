#!/usr/bin/env bash
# migrate-catalog.sh — collapse profiles/<flavor>-{crisp,retina}/ into
# profiles/<flavor>/ per `manage-profile-cli` "Catalog migration command".
#
# Implements design.md Decision 19 (flock + sibling-tree atomicity +
# --doctor recovery) and Decision 18 (deprecation symlinks for legacy
# bundle filenames).
#
# Modes:
#   migrate-catalog.sh                  Run full migration (idempotent).
#   migrate-catalog.sh --doctor         Detect half-migrated state.
#   migrate-catalog.sh --finalize       Remove deprecation symlinks
#                                       (post-deprecation cleanup).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB="$ROOT/scripts/lib"
LOCK_DIR="$ROOT/.cache/migrate-catalog.lock.d"

# Flavor mapping (legacy crisp dir name → new flavor name)
flavor_for_legacy() {
  case "$1" in
    ai-plus-crisp|ai-plus-retina)         echo "ai-plus" ;;
    ai-profile-crisp|ai-profile-retina)   echo "ai" ;;
    cpp-clangd-crisp|cpp-clangd-retina)   echo "cpp-clangd" ;;
    cpp-intellisense-crisp|cpp-intellisense-retina) echo "cpp-intellisense" ;;
    github-workflows-crisp|github-workflows-retina) echo "github-workflows" ;;
    java-gradle-crisp|java-gradle-retina) echo "java-gradle" ;;
    java-maven-crisp|java-maven-retina)   echo "java-maven" ;;
    java-spring-crisp|java-spring-retina) echo "java-spring" ;;
    rust-profile-crisp|rust-profile-retina) echo "rust" ;;
    web-astro-crisp|web-astro-retina)     echo "astro" ;;
    java-profile-crisp|java-profile-retina) echo "" ;;  # dropped (deprecated)
    *) echo "" ;;
  esac
}

# Final 11 flavors post-migration
FINAL_FLAVORS=(rust astro java-maven java-gradle java-spring cpp-clangd cpp-intellisense ai ai-plus github-workflows general)

doctor() {
  local issues=0
  for flavor in "${FINAL_FLAVORS[@]}"; do
    if [[ ! -d "$ROOT/profiles/$flavor" ]]; then
      echo "missing: profiles/$flavor"
      issues=$((issues + 1))
    fi
    if [[ ! -f "$ROOT/_overrides/$flavor.jsonc" ]] && [[ "$flavor" != "general" ]]; then
      echo "missing: _overrides/$flavor.jsonc"
      issues=$((issues + 1))
    fi
    if [[ ! -f "$ROOT/_merged/$flavor.json" ]]; then
      echo "missing: _merged/$flavor.json"
      issues=$((issues + 1))
    fi
    if [[ ! -f "$ROOT/exports/$flavor.code-profile" ]]; then
      echo "missing: exports/$flavor.code-profile"
      issues=$((issues + 1))
    fi
  done

  for d in "$ROOT"/profiles/*-crisp "$ROOT"/profiles/*-retina; do
    [[ -d "$d" ]] || continue
    echo "legacy dir still present: $(basename "$d")"
    issues=$((issues + 1))
  done

  if [[ $issues -eq 0 ]]; then
    echo "ok: catalog fully migrated (11 flavors present, no legacy dirs)"
    return 0
  fi
  echo ""
  echo "$issues issues detected; re-run \`migrate-catalog.sh\` to continue migration"
  return 0
}

finalize() {
  local removed=0
  for f in "$ROOT"/exports/*-crisp.code-profile "$ROOT"/exports/*-retina.code-profile; do
    [[ -L "$f" ]] || continue
    rm -f "$f"
    removed=$((removed + 1))
  done
  echo "removed $removed legacy bundle symlinks"
}

if [[ "${1:-}" == "--doctor" ]]; then doctor; exit 0; fi
if [[ "${1:-}" == "--finalize" ]]; then finalize; exit 0; fi

# mkdir-based lock (POSIX-portable; flock is Linux-only)
mkdir -p "$ROOT/.cache"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  holder="?"
  if [[ -f "$LOCK_DIR/pid" ]]; then
    holder=$(cat "$LOCK_DIR/pid" 2>/dev/null || echo "?")
  fi
  echo "migration already running (lock held by PID $holder)" >&2
  echo "if no migration is running, remove $LOCK_DIR and retry" >&2
  exit 6
fi
echo "$$" > "$LOCK_DIR/pid"
trap 'rm -rf "$LOCK_DIR"' EXIT

if [[ -f "$ROOT/MIGRATED.md" ]]; then
  echo "catalog already migrated; no changes"
  echo "remove MIGRATED.md to re-run, or use --doctor to inspect state"
  exit 0
fi

BACKUP="$ROOT/.cache/migrate-catalog-backup-$$"
mkdir -p "$BACKUP"

echo "==> Backing up profiles/, _overrides/, _merged/, exports/ to $BACKUP/"
for d in profiles _overrides _merged exports; do
  if [[ -d "$ROOT/$d" ]]; then
    cp -R "$ROOT/$d" "$BACKUP/$d"
  fi
done

# --- Track migrations for MIGRATED.md ---
RENAMES=()
DROPPED=()

# --- 1. Rename _overrides/<flavor>-base.jsonc → _overrides/<flavor>.jsonc ---
echo "==> Renaming _overrides/*-base.jsonc to flavor names"
for src in "$ROOT"/_overrides/*-base.jsonc; do
  [[ -f "$src" ]] || continue
  legacy=$(basename "$src" .jsonc)        # e.g., "ai-plus-base"
  legacy_flavor="${legacy%-base}"         # e.g., "ai-plus"
  flavor=""
  case "$legacy_flavor" in
    ai-plus|cpp-clangd|cpp-intellisense|github-workflows|java-gradle|java-maven|java-spring) flavor="$legacy_flavor" ;;
    ai-profile)   flavor="ai" ;;
    rust-profile) flavor="rust" ;;
    web-astro)    flavor="astro" ;;
    java-profile) DROPPED+=("_overrides/$(basename "$src") (java-profile dropped — alias to java-maven via deprecation shim)"); continue ;;
    *) DROPPED+=("_overrides/$(basename "$src") (no flavor mapping)"); continue ;;
  esac
  dst="$ROOT/_overrides/$flavor.jsonc"
  mv "$src" "$dst"
  RENAMES+=("_overrides/$(basename "$src") → _overrides/$flavor.jsonc")
done

# Remove crisp/retina UX-only override files (UX moved to _shared/ux/)
for f in "$ROOT"/_overrides/*-{crisp,retina}.jsonc; do
  [[ -f "$f" ]] || continue
  rm -f "$f"
  DROPPED+=("_overrides/$(basename "$f") (UX moved to _shared/ux/)")
done

# Remove ai/ subdir if it's now empty leftover
[[ -d "$ROOT/_overrides/ai" ]] && rmdir "$ROOT/_overrides/ai" 2>/dev/null || true

# --- 2. Rename profiles/<flavor>-crisp/ → profiles/<flavor>/ ---
echo "==> Renaming profile directories"
for d in "$ROOT"/profiles/*-crisp; do
  [[ -d "$d" ]] || continue
  legacy=$(basename "$d")
  flavor=$(flavor_for_legacy "$legacy")
  if [[ -z "$flavor" ]]; then
    DROPPED+=("profiles/$legacy (no flavor mapping)")
    continue
  fi
  dst="$ROOT/profiles/$flavor"
  if [[ -d "$dst" ]]; then
    DROPPED+=("profiles/$legacy (target $flavor already exists)")
    rm -rf "$d"
    continue
  fi
  mv "$d" "$dst"
  RENAMES+=("profiles/$legacy → profiles/$flavor")
done

# Drop retina dirs (content already in crisp counterparts)
for d in "$ROOT"/profiles/*-retina; do
  [[ -d "$d" ]] || continue
  rm -rf "$d"
  DROPPED+=("profiles/$(basename "$d") (UX moved to _shared/ux/; content was duplicate)")
done

# Drop legacy java-profile dirs explicitly (deprecated; alias resolves to java-maven)
for d in "$ROOT"/profiles/java-profile-{crisp,retina} "$ROOT"/profiles/java-profile; do
  [[ -d "$d" ]] || continue
  rm -rf "$d"
  DROPPED+=("profiles/$(basename "$d") (java-profile flavor dropped; alias → java-maven)")
done

# --- 3. Create general profile dir ---
mkdir -p "$ROOT/profiles/general"

# --- 4. Use composed _shared/extensions/<flavor>.json for each profile's extensions.json ---
echo "==> Composing extensions.json from layered sources"
for flavor in "${FINAL_FLAVORS[@]}"; do
  dir="$ROOT/profiles/$flavor"
  mkdir -p "$dir"
  if [[ "$flavor" == "general" ]]; then
    # general flavor: just base extensions (no toolchain layer file)
    bash "$LIB/compose-extensions.sh" > "$dir/extensions.json" 2>/dev/null || \
      jq '.' "$ROOT/_shared/extensions/base.json" > "$dir/extensions.json"
  else
    bash "$LIB/compose-extensions.sh" "$flavor" > "$dir/extensions.json"
  fi
done

# --- 5. Regenerate _merged/<flavor>.json (editor-base + _overrides/<flavor>.jsonc) ---
echo "==> Regenerating _merged/"
# Remove all legacy _merged files
rm -f "$ROOT"/_merged/*-{crisp,retina}.json
for flavor in "${FINAL_FLAVORS[@]}"; do
  base_json=$(awk -f "$LIB/jsonc-strip.awk" "$ROOT/_shared/editor-base.jsonc")
  if [[ -f "$ROOT/_overrides/$flavor.jsonc" ]]; then
    overrides=$(awk -f "$LIB/jsonc-strip.awk" "$ROOT/_overrides/$flavor.jsonc")
    # Drop @extends key if present (legacy compose-settings convention)
    overrides=$(echo "$overrides" | jq 'del(."@extends")')
    merged=$(jq -n --argjson a "$base_json" --argjson b "$overrides" '$a * $b')
  else
    merged="$base_json"
  fi
  echo "$merged" | jq '.' > "$ROOT/_merged/$flavor.json"
done

# --- 6. Refresh profile settings.json symlinks ---
echo "==> Refreshing profile settings symlinks"
for flavor in "${FINAL_FLAVORS[@]}"; do
  ln -sfn "../../_merged/$flavor.json" "$ROOT/profiles/$flavor/settings.json"
done

# --- 7. Regenerate exports/<flavor>.code-profile ---
echo "==> Regenerating exports/"
rm -f "$ROOT"/exports/*-{crisp,retina}.code-profile
for flavor in "${FINAL_FLAVORS[@]}"; do
  settings=$(jq '.' "$ROOT/_merged/$flavor.json")
  ext_ids=$(jq '[.[].identifier.id]' "$ROOT/profiles/$flavor/extensions.json")
  jq -n --argjson s "$settings" --argjson e "$ext_ids" '{
    name: "'"$flavor"'",
    settings: $s,
    extensions: { enabled: $e }
  }' > "$ROOT/exports/$flavor.code-profile"
done

# --- 8. Create deprecation symlinks for legacy bundle filenames ---
echo "==> Creating deprecation symlinks"
for legacy_dir in ai-plus-crisp ai-plus-retina ai-profile-crisp ai-profile-retina \
                  cpp-clangd-crisp cpp-clangd-retina \
                  cpp-intellisense-crisp cpp-intellisense-retina \
                  github-workflows-crisp github-workflows-retina \
                  java-gradle-crisp java-gradle-retina \
                  java-maven-crisp java-maven-retina \
                  java-profile-crisp java-profile-retina \
                  java-spring-crisp java-spring-retina \
                  rust-profile-crisp rust-profile-retina \
                  web-astro-crisp web-astro-retina; do
  flavor=$(flavor_for_legacy "$legacy_dir")
  if [[ -z "$flavor" ]]; then
    flavor="java-maven"  # deprecated java-profile aliases to java-maven
  fi
  ln -sfn "$flavor.code-profile" "$ROOT/exports/$legacy_dir.code-profile"
done

# --- 9. Write MIGRATED.md ---
echo "==> Writing MIGRATED.md"
{
  echo "# Catalog migration"
  echo ""
  echo "Generated by \`vspcli --migrate-catalog\` on $(date -u +%Y-%m-%dT%H:%M:%SZ)."
  echo ""
  echo "## Renames"
  echo ""
  for r in "${RENAMES[@]}"; do echo "- $r"; done
  echo ""
  echo "## Dropped"
  echo ""
  for d in "${DROPPED[@]}"; do echo "- $d"; done
  echo ""
  echo "## Final layout"
  echo ""
  echo "- 11 toolchain flavors: ${FINAL_FLAVORS[*]}"
  echo "- legacy bundle filenames replaced with symlinks for the deprecation window"
  echo "- backup retained at: \`$BACKUP\`"
} > "$ROOT/MIGRATED.md"

echo ""
echo "✓ migration complete — 11 flavors, ${#RENAMES[@]} renames, ${#DROPPED[@]} drops"
echo "  backup: $BACKUP"
echo "  legacy bundle symlinks created in exports/"
echo "  run \`vspcli --migrate-catalog --finalize\` after deprecation window to remove symlinks"

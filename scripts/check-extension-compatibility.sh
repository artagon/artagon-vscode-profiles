#!/usr/bin/env bash
set -euo pipefail

# check-extension-compatibility.sh
# Checks VS Code extensions for compatibility with the current VS Code version.
#
# Exit codes:
#   0  clean run, no findings
#   1  findings present (incompatible or unknown extensions)
#   2  CLI misuse (unknown flag, missing dependency, version detection failure)

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VSCODE_VERSION="${VSCODE_VERSION:-$(code --version 2>/dev/null | head -n1 || echo "unknown")}"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/vscode-extension-check"
CACHE_TTL=3600  # 1 hour cache

log() { printf '%s\n' "$*"; }
warn() { printf '\033[33mWARNING:\033[0m %s\n' "$*" >&2; }
error() { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; }
success() { printf '\033[32m✓\033[0m %s\n' "$*"; }

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [PROFILE...]

Check VS Code extension compatibility with current VS Code version.

OPTIONS:
  --all                 Check all profiles
  --json                Output results as JSON
  --verbose, -v         Show detailed information
  --no-cache            Skip cache, fetch fresh data
  --marketplace-only    Only check Marketplace API (skip installed extensions)
  --help, -h            Show this help message

EXAMPLES:
  $(basename "$0") --all
  $(basename "$0") java-profile-crisp web-astro-crisp
  $(basename "$0") --json github-workflows-crisp

REQUIREMENTS:
  - VS Code CLI: code --version
  - jq: For JSON processing
  - curl: For Marketplace API queries
EOF
}

# Parse arguments
PROFILES=()
CHECK_ALL=false
JSON_OUTPUT=false
VERBOSE=false
USE_CACHE=true
MARKETPLACE_ONLY=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --all) CHECK_ALL=true; shift ;;
    --json) JSON_OUTPUT=true; shift ;;
    --verbose|-v) VERBOSE=true; shift ;;
    --no-cache) USE_CACHE=false; shift ;;
    --marketplace-only) MARKETPLACE_ONLY=true; shift ;;
    --help|-h) usage; exit 0 ;;
    -*) error "Unknown option: $1"; usage >&2; exit 2 ;;
    *) PROFILES+=("$1"); shift ;;
  esac
done

# Validate dependencies
for cmd in code jq curl; do
  if ! command -v "$cmd" &>/dev/null; then
    error "Required command not found: $cmd"
    exit 2
  fi
done

# Get VS Code version info
if [[ "$VSCODE_VERSION" == "unknown" ]]; then
  error "Cannot detect VS Code version. Ensure 'code' is in PATH."
  exit 2
fi

# Parse semantic version
VSCODE_MAJOR=$(echo "$VSCODE_VERSION" | cut -d. -f1)
VSCODE_MINOR=$(echo "$VSCODE_VERSION" | cut -d. -f2)

[[ "$VERBOSE" == true ]] && log "VS Code version: $VSCODE_VERSION (${VSCODE_MAJOR}.${VSCODE_MINOR})"

# Setup cache directory
mkdir -p "$CACHE_DIR"

# Get extension metadata from Marketplace API
get_extension_metadata() {
  local ext_id="$1"
  local publisher="${ext_id%%.*}"
  local name="${ext_id#*.}"
  local cache_file="$CACHE_DIR/${ext_id}.json"

  # Check cache
  if [[ "$USE_CACHE" == true ]] && [[ -f "$cache_file" ]]; then
    local cache_age=$(($(date +%s) - $(stat -f %m "$cache_file" 2>/dev/null || stat -c %Y "$cache_file" 2>/dev/null || echo 0)))
    if [[ $cache_age -lt $CACHE_TTL ]]; then
      cat "$cache_file"
      return 0
    fi
  fi

  # Fetch from Marketplace API
  local api_url="https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery"
  local request_body=$(cat <<EOF
{
  "filters": [{
    "criteria": [
      {"filterType": 7, "value": "$publisher.$name"}
    ],
    "pageSize": 1
  }],
  "flags": 914
}
EOF
)

  local response
  response=$(curl -s -X POST "$api_url" \
    -H "Content-Type: application/json" \
    -H "Accept: application/json;api-version=3.0-preview.1" \
    -d "$request_body")

  if [[ -z "$response" ]] || ! echo "$response" | jq -e '.results[0].extensions[0]' &>/dev/null; then
    echo "{\"error\":\"Extension not found\"}"
    return 1
  fi

  # Cache the response
  echo "$response" | jq '.results[0].extensions[0]' > "$cache_file"
  cat "$cache_file"
}

# Check if extension is compatible with current VS Code version
check_compatibility() {
  local ext_id="$1"
  local metadata

  metadata=$(get_extension_metadata "$ext_id")

  if echo "$metadata" | jq -e '.error' &>/dev/null; then
    echo "unknown|Extension not found in Marketplace"
    return 2
  fi

  # Extract engine compatibility
  local engine=$(echo "$metadata" | jq -r '.versions[0].properties[] | select(.key=="Microsoft.VisualStudio.Code.Engine") | .value' 2>/dev/null || echo "")

  if [[ -z "$engine" ]]; then
    echo "unknown|No engine compatibility info"
    return 2
  fi

  # Get latest version info
  local latest_version=$(echo "$metadata" | jq -r '.versions[0].version' 2>/dev/null || echo "unknown")
  local last_updated=$(echo "$metadata" | jq -r '.versions[0].lastUpdated' 2>/dev/null || echo "unknown")

  # Parse engine requirement (e.g., "^1.95.0", ">=1.90.0", "*")
  if [[ "$engine" == "*" ]]; then
    echo "compatible|$latest_version|Any version|$last_updated"
    return 0
  fi

  # Extract minimum version from engine string
  local min_version=$(echo "$engine" | sed -E 's/[\^>=~]//g')
  local min_major=$(echo "$min_version" | cut -d. -f1)
  local min_minor=$(echo "$min_version" | cut -d. -f2 2>/dev/null || echo "0")

  # Compare versions
  if [[ $VSCODE_MAJOR -gt $min_major ]] || \
     [[ $VSCODE_MAJOR -eq $min_major && $VSCODE_MINOR -ge $min_minor ]]; then
    echo "compatible|$latest_version|$engine|$last_updated"
    return 0
  else
    echo "incompatible|$latest_version|$engine|$last_updated"
    return 1
  fi
}

# Check installed extensions for the named profile.
# Uses `code --list-extensions --show-versions --profile <name>` so results are
# scoped to the profile under audit (no cross-profile false positives).
check_installed_extensions() {
  local ext_id="$1"
  local profile="$2"
  local listing
  listing=$(code --profile "$profile" --list-extensions --show-versions 2>/dev/null) || listing=""
  # Marketplace IDs are case-insensitive. Match each "id@version..." line by
  # splitting only on the FIRST '@' so version strings containing additional
  # '@' chars (e.g. "publisher.name@1.2.3@beta") are returned intact.
  # Compare the id literally — avoids the bug where regex '.' in the id
  # matches any character.
  local needle_lower
  needle_lower=$(printf '%s' "$ext_id" | tr '[:upper:]' '[:lower:]')
  local version
  version=$(printf '%s\n' "$listing" | awk -v n="$needle_lower" '
    {
      i = index($0, "@")
      if (i == 0) next
      lid = tolower(substr($0, 1, i - 1))
      if (lid == n) { print substr($0, i + 1); exit }
    }')
  if [[ -z "$version" ]]; then
    return 1
  fi
  printf '%s\n' "$version"
}

# Get all profiles
get_profiles() {
  if [[ "$CHECK_ALL" == true ]]; then
    find "$ROOT/profiles" -mindepth 1 -maxdepth 1 -type d -exec basename {} \; | sort
  else
    printf '%s\n' "${PROFILES[@]}"
  fi
}

# Main checking logic
TOTAL_EXTENSIONS=0
COMPATIBLE_COUNT=0
INCOMPATIBLE_COUNT=0
UNKNOWN_COUNT=0
RESULTS=()

while IFS= read -r profile; do
  ext_file="$ROOT/profiles/$profile/extensions.json"

  if [[ ! -f "$ext_file" ]]; then
    warn "Profile not found: $profile (expected: $ext_file)"
    continue
  fi

  [[ "$JSON_OUTPUT" == false ]] && printf '\n📦 Checking profile: %s\n' "$profile"
  [[ "$JSON_OUTPUT" == false ]] && log "$(printf '%.0s─' {1..60})"

  # Extract extension IDs
  extensions=$(jq -r '.[].identifier.id' "$ext_file")

  while IFS= read -r ext_id; do
    [[ -z "$ext_id" ]] && continue

    TOTAL_EXTENSIONS=$((TOTAL_EXTENSIONS + 1))

    # Capture per-extension compat result without aborting the loop under set -e.
    # check_compatibility returns 0 (compatible), 1 (incompatible), 2 (unknown).
    if compat_info=$(check_compatibility "$ext_id"); then
      compat_status=0
    else
      compat_status=$?
    fi

    IFS='|' read -r status version engine updated <<< "$compat_info"

    # Check if installed (unless marketplace-only mode), scoped to this profile.
    installed_version="not installed"
    if [[ "$MARKETPLACE_ONLY" == false ]]; then
      installed_version=$(check_installed_extensions "$ext_id" "$profile" || echo "not installed")
    fi

    # Track stats
    case "$status" in
      compatible) COMPATIBLE_COUNT=$((COMPATIBLE_COUNT + 1)) ;;
      incompatible) INCOMPATIBLE_COUNT=$((INCOMPATIBLE_COUNT + 1)) ;;
      *) UNKNOWN_COUNT=$((UNKNOWN_COUNT + 1)) ;;
    esac

    # Store result
    RESULTS+=("{\"profile\":\"$profile\",\"extension\":\"$ext_id\",\"status\":\"$status\",\"latestVersion\":\"$version\",\"engine\":\"$engine\",\"installedVersion\":\"$installed_version\",\"lastUpdated\":\"$updated\"}")

    # Output based on format
    if [[ "$JSON_OUTPUT" == false ]]; then
      case "$status" in
        compatible)
          if [[ "$VERBOSE" == true ]]; then
            success "$ext_id ($version) - Requires: $engine"
          fi
          ;;
        incompatible)
          error "$ext_id ($version) - Requires: $engine (VS Code $VSCODE_VERSION incompatible!)"
          ;;
        unknown)
          warn "$ext_id - $version"
          ;;
      esac
    fi

  done <<< "$extensions"

done < <(get_profiles)

# Output summary
if [[ "$JSON_OUTPUT" == true ]]; then
  # JSON output
  jq -n --argjson results "[$(IFS=,; echo "${RESULTS[*]}")]" \
    --arg vscode "$VSCODE_VERSION" \
    --argjson total "$TOTAL_EXTENSIONS" \
    --argjson compatible "$COMPATIBLE_COUNT" \
    --argjson incompatible "$INCOMPATIBLE_COUNT" \
    --argjson unknown "$UNKNOWN_COUNT" \
    '{
      vscodeVersion: $vscode,
      summary: {
        total: $total,
        compatible: $compatible,
        incompatible: $incompatible,
        unknown: $unknown
      },
      extensions: $results
    }'
else
  # Human-readable summary
  printf '\n%s\n' "$(printf '%.0s═' {1..60})"
  log "📊 SUMMARY"
  log "$(printf '%.0s═' {1..60})"
  log "VS Code Version: $VSCODE_VERSION"
  log "Total Extensions: $TOTAL_EXTENSIONS"
  success "Compatible: $COMPATIBLE_COUNT"
  [[ $INCOMPATIBLE_COUNT -gt 0 ]] && error "Incompatible: $INCOMPATIBLE_COUNT" || log "Incompatible: 0"
  [[ $UNKNOWN_COUNT -gt 0 ]] && warn "Unknown: $UNKNOWN_COUNT" || log "Unknown: 0"
  log "$(printf '%.0s═' {1..60})"

  if [[ $INCOMPATIBLE_COUNT -gt 0 ]] || [[ $UNKNOWN_COUNT -gt 0 ]]; then
    [[ $INCOMPATIBLE_COUNT -gt 0 ]] && printf '\n⚠️  Action required: %d incompatible extension(s) found!\n' "$INCOMPATIBLE_COUNT"
    [[ $UNKNOWN_COUNT -gt 0 ]] && warn "$UNKNOWN_COUNT extension(s) returned unknown — see notes above."
    exit 1
  else
    printf '\n✅ All extensions are compatible with VS Code %s\n' "$VSCODE_VERSION"
  fi
fi

# JSON path also returns 1 on findings so automation can branch on exit code.
if [[ "$JSON_OUTPUT" == true ]] && { [[ $INCOMPATIBLE_COUNT -gt 0 ]] || [[ $UNKNOWN_COUNT -gt 0 ]]; }; then
  exit 1
fi

exit 0

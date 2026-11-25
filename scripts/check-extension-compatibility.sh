#!/usr/bin/env bash
# check-extension-compatibility.sh — Marketplace & VS Code engine compatibility audit.
# Pure Bash 3.2 + jq + curl.
#
# Exit codes:
#   0 — all audited extensions are compatible
#   1 — one or more extensions are incompatible or NOT_FOUND
#   2 — usage error, missing dependency, failed VS Code version discovery, or verification unavailable (Marketplace/parser failure)
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"

API_URL="https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery"
JSON_MODE=0
MARKETPLACE_ONLY=0
CHECK_ALL=0
EXTENSIONS_FILE=""
VSCODE_VER_OVERRIDE=""
PROFILES=()

usage() {
  cat <<'EOF'
Usage: check-extension-compatibility.sh [options] [profile...]
Options:
  --all                      Audit all profiles under profiles/
  --extensions-file <path>   Audit extension IDs from a file (newline-separated or JSON array)
  --marketplace-only         Query Marketplace engines without requiring installed local extensions
  --vscode-version <ver>     Override detected local VS Code version (e.g. 1.147.0)
  --json                     Emit structured JSON report on stdout
  -h, --help                 Show help
EOF
}

while [[ $# -gt 0 ]]; do
  case "${1}" in
    -h|--help)
      usage
      exit 0
      ;;
    --all)
      CHECK_ALL=1
      shift
      ;;
    --marketplace-only)
      MARKETPLACE_ONLY=1
      shift
      ;;
    --json)
      JSON_MODE=1
      shift
      ;;
    --extensions-file)
      [[ $# -ge 2 && -n "${2}" ]] || { echo "check-compat: --extensions-file requires a path" >&2; exit 2; }
      EXTENSIONS_FILE="${2}"
      shift 2
      ;;
    --extensions-file=*)
      EXTENSIONS_FILE="${1#*=}"
      [[ -n "${EXTENSIONS_FILE}" ]] || { echo "check-compat: --extensions-file requires a path" >&2; exit 2; }
      shift
      ;;
    --vscode-version)
      [[ $# -ge 2 && -n "${2}" ]] || { echo "check-compat: --vscode-version requires a value" >&2; exit 2; }
      VSCODE_VER_OVERRIDE="${2}"
      shift 2
      ;;
    --vscode-version=*)
      VSCODE_VER_OVERRIDE="${1#*=}"
      shift
      ;;
    -*)
      printf 'check-compat: unknown option %q\n' "${1}" >&2
      exit 2
      ;;
    *)
      PROFILES+=("${1}")
      shift
      ;;
  esac
done

if ! command -v jq >/dev/null 2>&1 || ! command -v curl >/dev/null 2>&1; then
  echo "check-compat: requires jq and curl on PATH" >&2
  exit 2
fi

if [[ "${CHECK_ALL}" -eq 0 && -z "${EXTENSIONS_FILE}" && "${#PROFILES[@]}" -eq 0 ]]; then
  echo "check-compat: specify --all, --extensions-file <path>, or one or more profile names" >&2
  exit 2
fi

# Detect local VS Code version.
VSCODE_VER="${VSCODE_VER_OVERRIDE}"
if [[ -z "${VSCODE_VER}" ]]; then
  if ! command -v code >/dev/null 2>&1; then
    echo "check-compat: 'code' CLI not found on PATH (pass --vscode-version to override)" >&2
    exit 2
  fi
  VSCODE_VER="$(code --version 2>/dev/null | awk 'NR == 1 { print; exit }')" || true
fi

if [[ ! "${VSCODE_VER}" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  printf 'check-compat: unable to determine valid VS Code semver (saw: %q)\n' "${VSCODE_VER}" >&2
  exit 2
fi
VSCODE_VER="${VSCODE_VER%%-*}"

# Collect unique lowercased extension IDs to audit.
AUDIT_IDS=()
AUDIT_IDS_LC_SEEN="|"
add_id_unique() {
  local id="${1}"
  [[ -n "${id}" ]] || return 0
  local lc
  lc="$(to_lower "${id}")"
  case "${AUDIT_IDS_LC_SEEN}" in
    *"|${lc}|"*) return 0 ;;
  esac
  AUDIT_IDS_LC_SEEN="${AUDIT_IDS_LC_SEEN}${lc}|"
  AUDIT_IDS+=("${id}")
}

if [[ -n "${EXTENSIONS_FILE}" ]]; then
  if [[ ! -f "${EXTENSIONS_FILE}" ]]; then
    printf 'check-compat: extensions file not found: %s\n' "${EXTENSIONS_FILE}" >&2
    exit 2
  fi
  if jq -e -s 'length == 1 and (.[0] | type == "array")' "${EXTENSIONS_FILE}" >/dev/null 2>&1; then
    ext_ids_raw=""
    if ! ext_ids_raw="$(jq -r -s '.[0][] | if type == "string" then . else .identifier.id // empty end' "${EXTENSIONS_FILE}")"; then
      printf 'check-compat: failed to parse extensions JSON file: %s\n' "${EXTENSIONS_FILE}" >&2
      exit 2
    fi
    while IFS= read -r id; do
      add_id_unique "${id}"
    done <<< "${ext_ids_raw}"
  else
    while IFS= read -r id; do
      id="${id%%#*}"
      id="$(printf '%s' "${id}" | tr -d '[:space:]')"
      add_id_unique "${id}"
    done < "${EXTENSIONS_FILE}"
  fi
fi

if [[ "${CHECK_ALL}" -eq 1 ]]; then
  all_profs=""
  if ! all_profs="$(discover_profiles "${ROOT}")"; then
    echo "check-compat: failed to discover profiles" >&2
    exit 2
  fi
  while IFS= read -r p; do
    [[ -n "${p}" ]] && PROFILES+=("${p}")
  done <<< "${all_profs}"
fi

for p in "${PROFILES[@]+"${PROFILES[@]}"}"; do
  ef="${ROOT}/profiles/${p}/extensions.json"
  tmp_ef=""
  if [[ ! -f "${ef}" ]]; then
    if [[ -f "${ROOT}/profiles/${p}/profile.json" ]]; then
      tmp_ef="$(mktemp "${TMPDIR:-/tmp}/vsp-compat-ext.XXXXXX")"
      if ! compiler_compile_profile_extensions_file "${ROOT}" "${p}" "${tmp_ef}"; then
        rm -f "${tmp_ef}"
        printf 'check-compat: failed to compile extensions on-demand for profile %q\n' "${p}" >&2
        exit 2
      fi
      ef="${tmp_ef}"
    else
      printf 'check-compat: missing extensions file for profile %q: %s\n' "${p}" "${ef}" >&2
      exit 2
    fi
  fi
  if ! jq -e -s '
    length == 1
    and (.[0] | type == "array")
    and (all(.[0][]; type == "object" and (.identifier | type == "object") and (.identifier.id | type == "string") and ((.identifier.id | length) > 0)))
  ' "${ef}" >/dev/null; then
    printf 'check-compat: invalid or malformed extensions JSON for profile %q: %s\n' "${p}" "${ef}" >&2
    [[ -n "${tmp_ef}" ]] && rm -f "${tmp_ef}"
    exit 2
  fi
  prof_ids_raw=""
  if ! prof_ids_raw="$(jq -r -s '.[0][].identifier.id' "${ef}")"; then
    printf 'check-compat: failed to parse extensions JSON for profile %q: %s\n' "${p}" "${ef}" >&2
    [[ -n "${tmp_ef}" ]] && rm -f "${tmp_ef}"
    exit 2
  fi
  [[ -n "${tmp_ef}" ]] && rm -f "${tmp_ef}"
  while IFS= read -r id; do
    add_id_unique "${id}"
  done <<< "${prof_ids_raw}"
done

# Pure Bash 3.2 full semver comparison (major.minor.patch).
# Prints -1 if a < b, 0 if a == b, 1 if a > b.
semver_cmp() {
  local a="${1%%-*}" b="${2%%-*}"
  local a1 a2 a3 b1 b2 b3
  IFS='.' read -r a1 a2 a3 <<< "${a}"
  IFS='.' read -r b1 b2 b3 <<< "${b}"
  a1="${a1:-0}"; a2="${a2:-0}"; a3="${a3:-0}"
  b1="${b1:-0}"; b2="${b2:-0}"; b3="${b3:-0}"
  # Strip non-digits (e.g., x/*) safely.
  [[ "${a1}" =~ ^[0-9]+$ ]] || a1=0
  [[ "${a2}" =~ ^[0-9]+$ ]] || a2=0
  [[ "${a3}" =~ ^[0-9]+$ ]] || a3=0
  [[ "${b1}" =~ ^[0-9]+$ ]] || b1=0
  [[ "${b2}" =~ ^[0-9]+$ ]] || b2=0
  [[ "${b3}" =~ ^[0-9]+$ ]] || b3=0
  if [[ $((10#${a1})) -ne $((10#${b1})) ]]; then
    [[ $((10#${a1})) -lt $((10#${b1})) ]] && echo -1 || echo 1
    return 0
  fi
  if [[ $((10#${a2})) -ne $((10#${b2})) ]]; then
    [[ $((10#${a2})) -lt $((10#${b2})) ]] && echo -1 || echo 1
    return 0
  fi
  if [[ $((10#${a3})) -ne $((10#${b3})) ]]; then
    [[ $((10#${a3})) -lt $((10#${b3})) ]] && echo -1 || echo 1
    return 0
  fi
  echo 0
}

# Evaluate a single comparator token (^, ~, >=, >, <=, <, =, *, exact) against VSCODE_VER.
semver_clause_matches() {
  local ver="${1}"
  local clause="${2}"
  clause="${clause#v}"
  [[ -z "${clause}" || "${clause}" == "*" || "${clause}" == "x" || "${clause}" == "X" ]] && return 0

  local op="" target=""
  case "${clause}" in
    ">="*) op=">="; target="${clause#>=}" ;;
    "<="*) op="<="; target="${clause#<=}" ;;
    ">"*)  op=">";  target="${clause#>}" ;;
    "<"*)  op="<";  target="${clause#<}" ;;
    "^"*)  op="^";  target="${clause#^}" ;;
    "~"*)  op="~";  target="${clause#~}" ;;
    "="*)  op="=";  target="${clause#=}" ;;
    *)     op="=";  target="${clause}" ;;
  esac
  target="${target#v}"
  # Strip optional pre-release suffix on engine comparator (e.g. ^1.110.0-20260204).
  target="${target%%-*}"

  # Parse major.minor.patch and detect wildcard segments (1.x, 1.*, 1.80.x, 1.80.*).
  local t1="" t2="" t3="" extra=""
  IFS='.' read -r t1 t2 t3 extra <<< "${target}"
  [[ -z "${extra}" ]] || return 1

  local wild_minor=0 wild_patch=0
  if [[ "${t1}" == "x" || "${t1}" == "X" || "${t1}" == "*" ]]; then
    return 0
  fi
  if [[ "${t2}" == "x" || "${t2}" == "X" || "${t2}" == "*" ]]; then
    wild_minor=1
    t2="0"
    t3="0"
  elif [[ "${t3}" == "x" || "${t3}" == "X" || "${t3}" == "*" ]]; then
    wild_patch=1
    t3="0"
  fi

  # Security (CRIT): strictly validate numeric digits BEFORE any Bash arithmetic expansion!
  [[ "${t1}" =~ ^[0-9]+$ ]] || return 1
  [[ -z "${t2}" || "${t2}" =~ ^[0-9]+$ ]] || return 1
  [[ -z "${t3}" || "${t3}" =~ ^[0-9]+$ ]] || return 1

  local n1=$((10#${t1}))
  local n2=$((10#${t2:-0}))
  local n3=$((10#${t3:-0}))
  local norm_target="${n1}.${n2}.${n3}"

  local cmp
  cmp="$(semver_cmp "${ver}" "${norm_target}")"

  # Bare wildcard ranges like `1.x` (>=1.0.0 <2.0.0) or `1.80.x` (>=1.80.0 <1.81.0).
  if [[ "${op}" == "=" && "${wild_minor}" -eq 1 ]]; then
    [[ "${cmp}" -ge 0 ]] || return 1
    [[ "$(semver_cmp "${ver}" "$((n1 + 1)).0.0")" -lt 0 ]]
    return $?
  fi
  if [[ "${op}" == "=" && "${wild_patch}" -eq 1 ]]; then
    [[ "${cmp}" -ge 0 ]] || return 1
    [[ "$(semver_cmp "${ver}" "${n1}.$((n2 + 1)).0")" -lt 0 ]]
    return $?
  fi

  case "${op}" in
    ">=") [[ "${cmp}" -ge 0 ]] ;;
    ">")  [[ "${cmp}" -gt 0 ]] ;;
    "<=") [[ "${cmp}" -le 0 ]] ;;
    "<")  [[ "${cmp}" -lt 0 ]] ;;
    "=")  [[ "${cmp}" -eq 0 ]] ;;
    "^")
      # VS Code engine caret ^1.X.Y means >=1.X.Y and <2.0.0 (or <0.(X+1).0 if major==0).
      [[ "${cmp}" -ge 0 ]] || return 1
      local upper=""
      if [[ "${n1}" -gt 0 ]]; then
        upper="$((n1 + 1)).0.0"
      else
        upper="0.$((n2 + 1)).0"
      fi
      [[ "$(semver_cmp "${ver}" "${upper}")" -lt 0 ]]
      ;;
    "~")
      [[ "${cmp}" -ge 0 ]] || return 1
      local upper="${n1}.$((n2 + 1)).0"
      [[ "$(semver_cmp "${ver}" "${upper}")" -lt 0 ]]
      ;;
    *) return 1 ;;
  esac
}

# Evaluate a compound semver range (supports || OR groups and space-separated AND clauses).
semver_satisfies() {
  local ver="${1}"
  local range="${2}"
  [[ -z "${range}" || "${range}" == "*" ]] && return 0

  # Normalize spaces after comparator operators (e.g. ">= 1.80.0" -> ">=1.80.0").
  local norm_range
  norm_range="$(printf '%s' "${range}" | sed -E 's/(>=|<=|>|<|\^|~|=)[[:space:]]+/\1/g')"

  local rest="${norm_range}"
  while true; do
    local branch=""
    if [[ "${rest}" == *"||"* ]]; then
      branch="${rest%%||*}"
      rest="${rest#*||}"
    else
      branch="${rest}"
      rest=""
    fi
    local ok=1
    # Disable pathname expansion so `*` or `1.*` in branch never globs CWD filenames.
    local tokens=()
    local had_noglob=0
    case "$-" in *f*) had_noglob=1 ;; esac
    set -f
    local token
    for token in ${branch}; do
      tokens+=("${token}")
    done
    [[ "${had_noglob}" -eq 0 ]] && set +f

    for token in "${tokens[@]+"${tokens[@]}"}"; do
      if ! semver_clause_matches "${ver}" "${token}"; then
        ok=0
        break
      fi
    done
    [[ "${ok}" -eq 1 ]] && return 0
    [[ -z "${rest}" ]] && break
  done
  return 1
}

WORK_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vsp-compat.XXXXXX")"
trap 'rm -rf "${WORK_TMP}"' EXIT
JSONL_FILE="${WORK_TMP}/results.jsonl"
: > "${JSONL_FILE}"

# Query Marketplace in small chunks via jq-escaped JSON payload.
# Default flags=529 (0x211: IncludeVersions | IncludeVersionProperties | IncludeLatestVersionOnly).
# Optional third arg overrides flags (e.g. 17 = IncludeVersions | IncludeVersionProperties without IncludeFiles).
query_batch() {
  local chunk_file="${1}"
  local out_file="${2}"
  local flags="${3:-529}"
  local payload
  payload="$(jq -R . "${chunk_file}" | jq -s --argjson fl "${flags}" '{
    filters: [{
      criteria: map({ filterType: 7, value: . }),
      pageNumber: 1,
      pageSize: 100
    }],
    assetTypes: [],
    flags: $fl
  }')"
  curl -fsS --compressed --max-time 30 \
    -H "Content-Type: application/json" \
    -H "Accept: application/json;api-version=3.0-preview.1" \
    -d "${payload}" "${API_URL}" -o "${out_file}"
}

BATCH_DIR="${WORK_TMP}/batches"
mkdir -p "${BATCH_DIR}"
chunk_idx=0
line_cnt=0
cur_chunk="${BATCH_DIR}/chunk-0.txt"
: > "${cur_chunk}"

for id in "${AUDIT_IDS[@]+"${AUDIT_IDS[@]}"}"; do
  printf '%s\n' "${id}" >> "${cur_chunk}"
  line_cnt=$((line_cnt + 1))
  if [[ "${line_cnt}" -ge 40 ]]; then
    chunk_idx=$((chunk_idx + 1))
    line_cnt=0
    cur_chunk="${BATCH_DIR}/chunk-${chunk_idx}.txt"
    : > "${cur_chunk}"
  fi
done

MERGED_MP="${WORK_TMP}/marketplace.json"
RESP_FILES=()
for cf in "${BATCH_DIR}"/chunk-*.txt; do
  [[ -s "${cf}" ]] || continue
  rf="${cf%.txt}.resp.json"
  if ! query_batch "${cf}" "${rf}"; then
    echo "check-compat: Marketplace API request failed" >&2
    exit 2
  fi
  RESP_FILES+=("${rf}")
done

if [[ "${#RESP_FILES[@]}" -gt 0 ]]; then
  if ! jq -e -s 'all(.[]; type == "object" and (.results | type == "array"))' "${RESP_FILES[@]}" >/dev/null 2>&1; then
    echo "check-compat: Marketplace API returned malformed JSON response" >&2
    exit 2
  fi
  if ! jq -s '[ .[] | (.results[0].extensions // [])[] ]' "${RESP_FILES[@]}" > "${MERGED_MP}"; then
    echo "check-compat: failed to merge Marketplace API response" >&2
    exit 2
  fi
else
  printf '[]\n' > "${MERGED_MP}"
fi

COMPAT_COUNT=0
INCOMPAT_COUNT=0
NOT_FOUND_COUNT=0

for id in "${AUDIT_IDS[@]+"${AUDIT_IDS[@]}"}"; do
  id_lc="$(to_lower "${id}")"
  # Prefer latest stable version; fall back to latest pre-release ONLY when pre-release-only.
  ext_meta=""
  if ! ext_meta="$(jq -c --arg id_lc "${id_lc}" '
    map(select(((.publisher.publisherName + "." + .extensionName) | ascii_downcase) == $id_lc))
    | .[0] as $ext
    | if $ext == null then null
      else
        ($ext.versions // []) as $vers
        | ($vers | map(select(((.properties // []) | any(.key == "Microsoft.VisualStudio.Code.PreRelease" and .value == "true")) | not))) as $stable
        | (if ($stable | length) > 0 then { v: $stable[0], pre: false }
           elif ($vers | length) > 0 then { v: $vers[0], pre: true }
           else null end) as $pick
        | if $pick == null then null
          else {
            version: ($pick.v.version // ""),
            prerelease: $pick.pre,
            engine: ((($pick.v.properties // []) | map(select(.key == "Microsoft.VisualStudio.Code.Engine")) | .[0].value) // "*")
          }
          end
      end
  ' "${MERGED_MP}")"; then
    printf 'check-compat: verification unavailable for %s (malformed Marketplace extension metadata)\n' "${id}" >&2
    exit 2
  fi

  if [[ "${ext_meta}" == "null" || -z "${ext_meta}" ]]; then
    NOT_FOUND_COUNT=$((NOT_FOUND_COUNT + 1))
    jq -n --arg id "${id}" --arg status "NOT_FOUND" \
      '{ id: $id, status: $status, version: null, engine: null, prerelease: false }' >> "${JSONL_FILE}"
    continue
  fi

  ver="$(printf '%s' "${ext_meta}" | jq -r '.version')"
  eng="$(printf '%s' "${ext_meta}" | jq -r '.engine')"
  pre="$(printf '%s' "${ext_meta}" | jq -r '.prerelease')"

  # When IncludeLatestVersionOnly returned only a pre-release version, query full
  # version history (flags=17, no IncludeFiles) for this single extension so
  # stable-first selection picks the latest stable release if one exists.
  # Fail closed (exit 2, verification unavailable) on request failure or malformed payload
  # instead of falling back to a prerelease false pass.
  if [[ "${pre}" == "true" ]]; then
    fb_chunk="${WORK_TMP}/fb-id.txt"
    fb_resp="${WORK_TMP}/fb-resp.json"
    printf '%s\n' "${id}" > "${fb_chunk}"
    if ! query_batch "${fb_chunk}" "${fb_resp}" 17; then
      printf 'check-compat: verification unavailable for %s (stable-history Marketplace request failed)\n' "${id}" >&2
      exit 2
    fi
    if ! jq -e -s '
      length == 1
      and (.[0] | type == "object")
      and (.[0].results | type == "array")
      and ((.[0].results | length) > 0)
      and (.[0].results[0] | type == "object")
      and (.[0].results[0].extensions | type == "array")
    ' "${fb_resp}" >/dev/null 2>&1; then
      printf 'check-compat: verification unavailable for %s (malformed stable-history Marketplace response)\n' "${id}" >&2
      exit 2
    fi
    fb_meta=""
    if ! fb_meta="$(jq -c -s --arg id_lc "${id_lc}" '
      .[0].results[0].extensions
      | map(select(((.publisher.publisherName + "." + .extensionName) | ascii_downcase) == $id_lc))
      | .[0] as $ext
      | if $ext == null or (($ext.versions | type) != "array") or (($ext.versions | length) == 0) then
          { state: "unavailable" }
        else
          ($ext.versions | map(select(((.properties // []) | any(.key == "Microsoft.VisualStudio.Code.PreRelease" and .value == "true")) | not))) as $stable
          | if ($stable | length) > 0 then {
              state: "stable",
              version: ($stable[0].version // ""),
              prerelease: false,
              engine: ((($stable[0].properties // []) | map(select(.key == "Microsoft.VisualStudio.Code.Engine")) | .[0].value) // "*")
            } else {
              state: "prerelease_only"
            } end
        end
    ' "${fb_resp}")"; then
      printf 'check-compat: verification unavailable for %s (failed to parse stable-history Marketplace response)\n' "${id}" >&2
      exit 2
    fi
    fb_state="$(printf '%s' "${fb_meta}" | jq -r '.state // "unavailable"')"
    if [[ "${fb_state}" == "stable" ]]; then
      ver="$(printf '%s' "${fb_meta}" | jq -r '.version')"
      eng="$(printf '%s' "${fb_meta}" | jq -r '.engine')"
      pre="false"
    elif [[ "${fb_state}" != "prerelease_only" ]]; then
      printf 'check-compat: verification unavailable for %s (missing version history in stable-history response)\n' "${id}" >&2
      exit 2
    fi
  fi

  if semver_satisfies "${VSCODE_VER}" "${eng}"; then
    COMPAT_COUNT=$((COMPAT_COUNT + 1))
    status="compatible"
  else
    INCOMPAT_COUNT=$((INCOMPAT_COUNT + 1))
    status="incompatible"
  fi

  jq -n \
    --arg id "${id}" \
    --arg status "${status}" \
    --arg version "${ver}" \
    --arg engine "${eng}" \
    --argjson prerelease "${pre}" \
    '{ id: $id, status: $status, version: $version, engine: $engine, prerelease: $prerelease }' >> "${JSONL_FILE}"
done

TOTAL_COUNT="${#AUDIT_IDS[@]}"

if [[ "${JSON_MODE}" -eq 1 ]]; then
  jq -s \
    --arg vscode "${VSCODE_VER}" \
    --argjson total "${TOTAL_COUNT}" \
    --argjson compatible "${COMPAT_COUNT}" \
    --argjson incompatible "${INCOMPAT_COUNT}" \
    --argjson not_found "${NOT_FOUND_COUNT}" \
    '{
      vscode_version: $vscode,
      summary: {
        total: $total,
        compatible: $compatible,
        incompatible: $incompatible,
        not_found: $not_found
      },
      extensions: .
    }' "${JSONL_FILE}"
else
  printf 'VS Code version: %s | total=%d compatible=%d incompatible=%d not_found=%d\n' \
    "${VSCODE_VER}" "${TOTAL_COUNT}" "${COMPAT_COUNT}" "${INCOMPAT_COUNT}" "${NOT_FOUND_COUNT}"
  if [[ "${INCOMPAT_COUNT}" -gt 0 || "${NOT_FOUND_COUNT}" -gt 0 ]]; then
    jq -r 'select(.status != "compatible") | "  \(.status): \(.id) (version=\(.version // "none"), engine=\(.engine // "none"))"' "${JSONL_FILE}" >&2
  fi
fi

# Silence unused variable warning if MARKETPLACE_ONLY was passed.
: "${MARKETPLACE_ONLY}"

if [[ "${INCOMPAT_COUNT}" -gt 0 || "${NOT_FOUND_COUNT}" -gt 0 ]]; then
  exit 1
fi
exit 0

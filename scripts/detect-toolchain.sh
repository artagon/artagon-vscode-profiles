#!/usr/bin/env bash
# detect-toolchain.sh — 100% data-driven workspace toolchain & capability detector.
# Scans all modules/*/*/module.json with non-null .detect in ascending priority order.
# Pure Bash 3.2 + jq + POSIX/BSD find + draining awk (zero SIGPIPE 141).
#
# Exit codes:
#   0 — one or more toolchains/capabilities detected (or --toolchain override succeeded)
#   2 — target path missing or not a directory
#   3 — no toolchain detected
#   4 — usage error / unknown --toolchain / --no-detect without --toolchain
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"

JSON_MODE=0
NO_DETECT=0
OVERRIDE_TOOLCHAINS=()
TARGET_PATH="."

usage() {
  cat <<'EOF'
Usage: detect-toolchain.sh [--json] [--toolchain <id>] [--no-detect] [path]
EOF
}

while [[ $# -gt 0 ]]; do
  case "${1}" in
    --json)
      JSON_MODE=1
      shift
      ;;
    --no-detect)
      NO_DETECT=1
      shift
      ;;
    --toolchain)
      if [[ $# -lt 2 || -z "${2}" ]]; then
        echo "detect-toolchain: --toolchain requires a value" >&2
        exit 4
      fi
      OVERRIDE_TOOLCHAINS+=("${2}")
      shift 2
      ;;
    --toolchain=*)
      val="${1#*=}"
      if [[ -z "${val}" ]]; then
        echo "detect-toolchain: --toolchain requires a value" >&2
        exit 4
      fi
      OVERRIDE_TOOLCHAINS+=("${val}")
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      printf 'detect-toolchain: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      TARGET_PATH="${1}"
      shift
      ;;
  esac
done

if [[ "${NO_DETECT}" -eq 1 && "${#OVERRIDE_TOOLCHAINS[@]}" -eq 0 ]]; then
  echo "detect-toolchain: --no-detect requires --toolchain <id>" >&2
  exit 4
fi

if [[ ! -d "${TARGET_PATH}" ]]; then
  printf 'detect-toolchain: path does not exist or is not a directory: %s\n' "${TARGET_PATH}" >&2
  exit 2
fi

WS_ROOT="$(canonical_path "${TARGET_PATH}")" || exit 2

# Validate any explicit --toolchain overrides against discovered modules or profiles.
if [[ "${#OVERRIDE_TOOLCHAINS[@]}" -gt 0 ]]; then
  expanded=()
  for entry in "${OVERRIDE_TOOLCHAINS[@]}"; do
    while IFS= read -r item; do
      [[ -n "${item}" ]] || continue
      if ! module_dir_for_id "${ROOT}" "${item}" >/dev/null 2>&1 && [[ ! -f "${ROOT}/profiles/${item}/profile.json" ]]; then
        printf 'detect-toolchain: unknown toolchain %q\n' "${item}" >&2
        exit 4
      fi
      expanded+=("${item}")
    done < <(printf '%s\n' "${entry}" | tr ',' '\n')
  done
  if [[ "${#expanded[@]}" -eq 0 ]]; then
    echo "detect-toolchain: --toolchain requires at least one valid module/profile id" >&2
    exit 4
  fi
  if [[ "${JSON_MODE}" -eq 1 ]]; then
    jq -n --argjson tc "$(printf '%s\n' "${expanded[@]}" | jq -R . | jq -s .)" \
      '{ toolchains: $tc, signals: {}, override: true }'
  else
    printf '%s\n' "${expanded[*]}"
  fi
  exit 0
fi

# BSD/GNU safe, SIGPIPE-free regular-file finder returning first relative match.
# Separate directory-prune vs regular-file branches so `-type f` never suppresses `-prune`.
find_first_regular_file() {
  local base_dir="${1}"
  local pattern="${2}"
  local max_depth="${3:-3}"
  [[ -d "${base_dir}" ]] || return 1

  local tmp_err
  tmp_err="$(mktemp "${TMPDIR:-/tmp}/vsp-find-err.XXXXXX")" || return 1
  local match=""
  local find_rc=0
  local awk_rc=0

  set +e
  match="$(
    find "${base_dir}" -mindepth 1 -maxdepth "${max_depth}" \
      \( -type d \( -name .git -o -name node_modules -o -name target -o -name build -o -name .venv -o -name .cache \) -prune \) \
      -o \( -type f -name "${pattern}" -print \) 2>"${tmp_err}" \
    | LC_ALL=C awk 'NR == 1 { print; found = 1 } END { exit (found ? 0 : 1) }'
    printf '\037%s:%s' "${PIPESTATUS[0]}" "${PIPESTATUS[1]}"
  )"
  set -e

  local status_pair="${match##*$'\037'}"
  local content="${match%$'\037'*}"
  find_rc="${status_pair%%:*}"
  awk_rc="${status_pair#*:}"

  if [[ "${find_rc}" -ne 0 ]]; then
    cat "${tmp_err}" >&2 || true
    rm -f "${tmp_err}"
    return 2
  fi
  rm -f "${tmp_err}"

  if [[ "${awk_rc}" -eq 0 && -n "${content}" ]]; then
    printf '%s\n' "${content}"
    return 0
  fi
  return 1
}

# Evaluate a single detect rule JSON object against WS_ROOT.
# Prints matching signal string on stdout if matched; returns 0 on match, 1 on no match, 2 on error.
eval_rule() {
  local rule_json="${1}"
  local rtype
  rtype="$(printf '%s' "${rule_json}" | jq -r '.type // empty')"

  case "${rtype}" in
    file)
      local p
      p="$(printf '%s' "${rule_json}" | jq -r '.path // empty')"
      if [[ -n "${p}" && -f "${WS_ROOT}/${p}" ]]; then
        printf '%s\n' "${p}"
        return 0
      fi
      return 1
      ;;
    any_file)
      local p
      while IFS= read -r p; do
        [[ -n "${p}" ]] || continue
        if [[ -f "${WS_ROOT}/${p}" ]]; then
          printf '%s\n' "${p}"
          return 0
        fi
      done < <(printf '%s' "${rule_json}" | jq -r '.paths[]?')
      return 1
      ;;
    all_files)
      local matched=()
      local p
      while IFS= read -r p; do
        [[ -n "${p}" ]] || continue
        if [[ ! -f "${WS_ROOT}/${p}" ]]; then
          return 1
        fi
        matched+=("${p}")
      done < <(printf '%s' "${rule_json}" | jq -r '.paths[]?')
      [[ "${#matched[@]}" -gt 0 ]] || return 1

      local extra_count
      extra_count="$(printf '%s' "${rule_json}" | jq -r '(.any_extra // []) | length')"
      if [[ "${extra_count}" -gt 0 ]]; then
        local found_extra=""
        while IFS= read -r p; do
          [[ -n "${p}" ]] || continue
          if [[ -f "${WS_ROOT}/${p}" ]]; then
            found_extra="${p}"
            break
          fi
        done < <(printf '%s' "${rule_json}" | jq -r '.any_extra[]?')
        [[ -n "${found_extra}" ]] || return 1
        matched+=("${found_extra}")
      fi
      printf '%s\n' "${matched[*]}"
      return 0
      ;;
    glob)
      local pat dir max_d search_dir hit rc=0
      pat="$(printf '%s' "${rule_json}" | jq -r '.pattern // empty')"
      dir="$(printf '%s' "${rule_json}" | jq -r '.dir // "."')"
      max_d="$(printf '%s' "${rule_json}" | jq -r '.max_depth // 3')"
      [[ -n "${pat}" ]] || return 1
      if [[ "${dir}" == "." ]]; then
        search_dir="${WS_ROOT}"
      else
        search_dir="${WS_ROOT}/${dir}"
      fi
      [[ -d "${search_dir}" ]] || return 1
      if hit="$(find_first_regular_file "${search_dir}" "${pat}" "${max_d}")"; then
        printf '%s\n' "${hit#"${WS_ROOT}/"}"
        return 0
      else
        rc=$?
        return "${rc}"
      fi
      ;;
    file_and_absent)
      local pres abs
      pres="$(printf '%s' "${rule_json}" | jq -r '.present // empty')"
      abs="$(printf '%s' "${rule_json}" | jq -r '.absent // empty')"
      if [[ -n "${pres}" && -f "${WS_ROOT}/${pres}" && ( -z "${abs}" || ! -e "${WS_ROOT}/${abs}" ) ]]; then
        printf '%s\n' "${pres}"
        return 0
      fi
      return 1
      ;;
    file_and_grep)
      local found_manifest=""
      local p
      while IFS= read -r p; do
        [[ -n "${p}" ]] || continue
        if [[ -f "${WS_ROOT}/${p}" ]]; then
          found_manifest="${p}"
          break
        fi
      done < <(printf '%s' "${rule_json}" | jq -r '.paths[]?')
      [[ -n "${found_manifest}" ]] || return 1

      local sdir glob_pat grep_pat search_base
      sdir="$(printf '%s' "${rule_json}" | jq -r '.search_dir // "."')"
      glob_pat="$(printf '%s' "${rule_json}" | jq -r '.glob // "*"')"
      grep_pat="$(printf '%s' "${rule_json}" | jq -r '.pattern // empty')"
      [[ -n "${grep_pat}" ]] || return 1
      if [[ "${sdir}" == "." ]]; then
        search_base="${WS_ROOT}"
      else
        search_base="${WS_ROOT}/${sdir}"
      fi
      [[ -d "${search_base}" ]] || return 1

      local hit_file=""
      local tmp_err
      tmp_err="$(mktemp "${TMPDIR:-/tmp}/vsp-fgrep-err.XXXXXX")" || return 2
      local cand
      while IFS= read -r cand; do
        [[ -n "${cand}" ]] || continue
        if grep -qF -- "${grep_pat}" "${cand}" 2>/dev/null; then
          hit_file="${cand#"${WS_ROOT}/"}"
          break
        fi
      done < <(
        find "${search_base}" -mindepth 1 -maxdepth 8 \
          \( -type d \( -name .git -o -name node_modules -o -name target -o -name build -o -name .venv \) -prune \) \
          -o \( -type f -name "${glob_pat}" -print \) 2>"${tmp_err}"
      )
      rm -f "${tmp_err}"
      if [[ -n "${hit_file}" ]]; then
        printf '%s:%s\n' "${found_manifest}" "${hit_file}"
        return 0
      fi
      return 1
      ;;
    *)
      printf 'detect-toolchain: unknown rule type %q\n' "${rtype}" >&2
      return 2
      ;;
  esac
}

# Collect all modules with non-null .detect sorted by (priority, id).
CANDIDATES=()
while IFS= read -r line; do
  [[ -n "${line}" ]] && CANDIDATES+=("${line}")
done < <(
  while IFS=$'\t' read -r mod_id _mod_kind mod_dir; do
    [[ -n "${mod_id}" ]] || continue
    if jq -e '.detect != null' "${mod_dir}/module.json" >/dev/null 2>&1; then
      prio="$(jq -r '.detect.priority // 50' "${mod_dir}/module.json")"
      fb="$(jq -r 'if .detect.fallback == true then "1" else "0" end' "${mod_dir}/module.json")"
      printf '%03d\t%s\t%s\t%s\n' "${prio}" "${fb}" "${mod_id}" "${mod_dir}"
    fi
  done < <(discover_modules "${ROOT}") | LC_ALL=C sort
)

PRIMARY_MATCHES=()
FALLBACK_MATCHES=()
PRIMARY_SIGNALS_JSON='{}'
FALLBACK_SIGNALS_JSON='{}'
PRIMARY_SUPERSEDED=""
FALLBACK_SUPERSEDED=""

for entry in "${CANDIDATES[@]+"${CANDIDATES[@]}"}"; do
  IFS=$'\t' read -r _prio fb mod_id mod_dir <<< "${entry}"
  manifest="${mod_dir}/module.json"
  match_mode="$(jq -r '.detect.match // "any"' "${manifest}")"

  mod_signals=()
  matched_any=0
  matched_all=1
  rule_count=0

  while IFS= read -r rule_json; do
    [[ -n "${rule_json}" ]] || continue
    rule_count=$((rule_count + 1))
    sig=""
    rc=0
    if sig="$(eval_rule "${rule_json}")"; then
      matched_any=1
      mod_signals+=("${sig}")
    else
      rc=$?
      if [[ "${rc}" -eq 2 ]]; then
        exit 2
      fi
      matched_all=0
    fi
  done < <(jq -c '.detect.rules[]?' "${manifest}")

  is_match=0
  if [[ "${rule_count}" -gt 0 ]]; then
    if [[ "${match_mode}" == "all" && "${matched_all}" -eq 1 ]]; then
      is_match=1
    elif [[ "${match_mode}" != "all" && "${matched_any}" -eq 1 ]]; then
      is_match=1
    fi
  fi

  if [[ "${is_match}" -eq 1 ]]; then
    sig_arr="$(printf '%s\n' "${mod_signals[@]}" | jq -R . | jq -s .)"
    if [[ "${fb}" == "1" ]]; then
      FALLBACK_MATCHES+=("${mod_id}")
      FALLBACK_SIGNALS_JSON="$(printf '%s' "${FALLBACK_SIGNALS_JSON}" | jq --arg k "${mod_id}" --argjson v "${sig_arr}" '.[$k] = $v')"
      while IFS= read -r sup; do
        [[ -n "${sup}" ]] || continue
        FALLBACK_SUPERSEDED="${FALLBACK_SUPERSEDED:+${FALLBACK_SUPERSEDED}$'\n'}${mod_id}:${sup}"
      done < <(jq -r '.detect.supersedes[]?' "${manifest}")
    else
      PRIMARY_MATCHES+=("${mod_id}")
      PRIMARY_SIGNALS_JSON="$(printf '%s' "${PRIMARY_SIGNALS_JSON}" | jq --arg k "${mod_id}" --argjson v "${sig_arr}" '.[$k] = $v')"
      while IFS= read -r sup; do
        [[ -n "${sup}" ]] || continue
        PRIMARY_SUPERSEDED="${PRIMARY_SUPERSEDED:+${PRIMARY_SUPERSEDED}$'\n'}${mod_id}:${sup}"
      done < <(jq -r '.detect.supersedes[]?' "${manifest}")
    fi
  fi
done

ACTIVE=()
SIGNALS_JSON='{}'
SUPERSEDED_LIST=""
if [[ "${#PRIMARY_MATCHES[@]}" -gt 0 ]]; then
  ACTIVE=("${PRIMARY_MATCHES[@]}")
  SIGNALS_JSON="${PRIMARY_SIGNALS_JSON}"
  SUPERSEDED_LIST="${PRIMARY_SUPERSEDED}"
elif [[ "${#FALLBACK_MATCHES[@]}" -gt 0 ]]; then
  ACTIVE=("${FALLBACK_MATCHES[@]}")
  SIGNALS_JSON="${FALLBACK_SIGNALS_JSON}"
  SUPERSEDED_LIST="${FALLBACK_SUPERSEDED}"
fi

# Apply detect.supersedes[] cleanly (emitting informative stderr note when superseding a matched module).
FINAL=()
for mod_id in "${ACTIVE[@]+"${ACTIVE[@]}"}"; do
  sup_by=""
  if [[ -n "${SUPERSEDED_LIST}" ]]; then
    while IFS= read -r pair; do
      [[ -n "${pair}" ]] || continue
      winner="${pair%%:*}"
      loser="${pair#*:}"
      if [[ "${loser}" == "${mod_id}" ]]; then
        sup_by="${winner}"
        break
      fi
    done <<< "${SUPERSEDED_LIST}"
  fi
  if [[ -n "${sup_by}" ]]; then
    printf 'detect-toolchain: %s superseded by %s\n' "${mod_id}" "${sup_by}" >&2
    SIGNALS_JSON="$(printf '%s' "${SIGNALS_JSON}" | jq --arg k "${mod_id}" 'del(.[$k])')"
    continue
  fi
  FINAL+=("${mod_id}")
done

if [[ "${#FINAL[@]}" -eq 0 ]]; then
  echo "detect-toolchain: no toolchain detected in ${TARGET_PATH}" >&2
  if [[ "${JSON_MODE}" -eq 1 ]]; then
    printf '{"toolchains":[],"signals":{},"override":false}\n'
  fi
  exit 3
fi

if [[ "${JSON_MODE}" -eq 1 ]]; then
  tc_json="$(printf '%s\n' "${FINAL[@]}" | jq -R . | jq -s .)"
  jq -n --argjson tc "${tc_json}" --argjson sig "${SIGNALS_JSON}" \
    '{ toolchains: $tc, signals: $sig, override: false }'
else
  printf '%s\n' "${FINAL[*]}"
fi
exit 0

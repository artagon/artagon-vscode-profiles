#!/usr/bin/env bash
# compose-settings.sh — Compile profile recipes under profiles/*/profile.json
# into UX-agnostic _merged/<name>.json, profiles/<name>/settings.json symlinks,
# and profiles/<name>/extensions.json via scripts/lib/compiler.sh.
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
# shellcheck source=scripts/lib/legacy-profile-name.sh
source "${LIB_DIR}/legacy-profile-name.sh"

if ! command -v jq >/dev/null 2>&1; then
  echo "compose-settings: jq is required" >&2
  exit 1
fi

NO_LOCK=0
BREAK_STALE=""
REQUESTED=()

while [[ $# -gt 0 ]]; do
  case "${1}" in
    --no-lock)          NO_LOCK=1; shift ;;
    --break-stale-lock) BREAK_STALE="--break-stale-lock"; shift ;;
    -*)
      printf 'compose-settings: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      REQUESTED+=("${1}")
      shift
      ;;
  esac
done

LOCK_DIR="${ROOT}/.cache/.build.lock"
if [[ "${NO_LOCK}" -eq 0 ]]; then
  acquire_lock "${LOCK_DIR}" 15 ${BREAK_STALE:+"${BREAK_STALE}"} || exit 1
  trap 'release_lock "${LOCK_DIR}"' EXIT
fi

PROFILES=()
if [[ "${#REQUESTED[@]}" -gt 0 ]]; then
  for req in "${REQUESTED[@]}"; do
    legacy="$(resolve_legacy_profile_name "${req}")"
    if [[ -n "${legacy}" ]]; then
      PROFILES+=("${legacy%% *}")
    else
      PROFILES+=("${req}")
    fi
  done
else
  while IFS= read -r p; do
    [[ -n "${p}" ]] && PROFILES+=("${p}")
  done < <(discover_profiles "${ROOT}")
fi

FAILED=()
for profile in "${PROFILES[@]+"${PROFILES[@]}"}"; do
  if compiler_build_profile "${ROOT}" "${profile}"; then
    printf 'Built %s -> %s/_merged/%s.json (+ profiles/%s/extensions.json)\n' \
      "${profile}" "${ROOT}" "${profile}" "${profile}"
  else
    FAILED+=("${profile}")
  fi
done

if [[ "${#FAILED[@]}" -gt 0 ]]; then
  printf 'compose-settings: failed for %d profile(s):\n' "${#FAILED[@]}" >&2
  printf '  - %s\n' "${FAILED[@]}" >&2
  exit 1
fi

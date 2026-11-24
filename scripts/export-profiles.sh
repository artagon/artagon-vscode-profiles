#!/usr/bin/env bash
# export-profiles.sh — Export UX-agnostic .code-profile bundles (0644),
# refresh authoritative legacy symlinks, and atomically write exports/SHA256SUMS (0644).
# Holds build/export lock (unless --no-lock under vspcli build) and stages all bundles
# and SHA256SUMS together before publishing.
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
  echo "export-profiles: jq is required" >&2
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
      printf 'export-profiles: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      REQUESTED+=("${1}")
      shift
      ;;
  esac
done

LOCK_DIR="${ROOT}/.cache/.build.lock"
STAGE_DIR=""
cleanup() {
  local rc=$?
  [[ -n "${STAGE_DIR}" ]] && rm -rf "${STAGE_DIR}"
  if [[ "${NO_LOCK}" -eq 0 ]]; then
    release_lock "${LOCK_DIR}"
  fi
  exit "${rc}"
}
trap cleanup EXIT

if [[ "${NO_LOCK}" -eq 0 ]]; then
  acquire_lock "${LOCK_DIR}" 15 ${BREAK_STALE:+"${BREAK_STALE}"} || exit 1
fi

EXPORTS_DIR="${ROOT}/exports"
mkdir -p "${EXPORTS_DIR}"

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

UX_KEYS_JSON="$(compiler_ux_managed_keys_json "${ROOT}")" || exit 1
STAGE_DIR="$(mktemp -d "${EXPORTS_DIR}/.stage.XXXXXX")" || exit 1

STAGED_NAMES=()
for profile in "${PROFILES[@]+"${PROFILES[@]}"}"; do
  merged="${ROOT}/_merged/${profile}.json"
  ext_file="${ROOT}/profiles/${profile}/extensions.json"
  if [[ ! -f "${merged}" ]]; then
    printf 'Warning: skipping %s — missing %s\n' "${profile}" "${merged}" >&2
    continue
  fi
  if [[ ! -f "${ext_file}" ]]; then
    printf 'Warning: skipping %s — missing %s\n' "${profile}" "${ext_file}" >&2
    continue
  fi

  out_stage="${STAGE_DIR}/${profile}.code-profile"
  jq -n \
    --arg name "${profile}" \
    --slurpfile settings "${merged}" \
    --slurpfile exts "${ext_file}" \
    --argjson ux_keys "${UX_KEYS_JSON}" '
    {
      name: $name,
      settings: ($settings[0] | reduce $ux_keys[] as $k (.; del(.[$k]))),
      extensions: {
        enabled: [($exts[0] // [])[] | .identifier.id // empty]
      }
    }
  ' > "${out_stage}"
  chmod 0644 "${out_stage}"
  STAGED_NAMES+=("${profile}.code-profile")
done

# Publish staged bundles atomically.
for bname in "${STAGED_NAMES[@]+"${STAGED_NAMES[@]}"}"; do
  mv -f "${STAGE_DIR}/${bname}" "${EXPORTS_DIR}/${bname}"
  printf 'Exported %s\n' "${EXPORTS_DIR}/${bname}"
done

# Refresh all 22 legacy symlinks from single authoritative table (ai-profile-* -> ai-copilot).
refresh_legacy_bundle_links "${EXPORTS_DIR}"

# Build sorted SHA256SUMS over all regular canonical *.code-profile bundles (not symlinks).
SUMS_TMP="${STAGE_DIR}/SHA256SUMS"
: > "${SUMS_TMP}"
while IFS= read -r bundle_path; do
  [[ -n "${bundle_path}" && -f "${bundle_path}" && ! -L "${bundle_path}" ]] || continue
  bbase="$(basename -- "${bundle_path}")"
  digest="$(sha256_for_file "${bundle_path}")" || exit 1
  printf '%s  %s\n' "${digest}" "${bbase}" >> "${SUMS_TMP}"
done < <(LC_ALL=C find "${EXPORTS_DIR}" -maxdepth 1 -type f -name '*.code-profile' -print | LC_ALL=C sort)

LC_ALL=C sort -k2,2 "${SUMS_TMP}" > "${SUMS_TMP}.sorted"
chmod 0644 "${SUMS_TMP}.sorted"
mv -f "${SUMS_TMP}.sorted" "${EXPORTS_DIR}/SHA256SUMS"
printf 'Updated %s\n' "${EXPORTS_DIR}/SHA256SUMS"

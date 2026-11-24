#!/usr/bin/env bash
# import-profile.sh — Hardened .code-profile importer with:
#   - canonical-first single private 0600 staging copy (zero TOCTOU)
#   - symlink-aware SHA256SUMS verification (managed strict, third-party warn-and-proceed)
#   - exact jq -e -s single-object & optional-extensions schema validation
#   - parsed-config autonomy/exec deny-list check over bundle .settings (pre-mutation)
#   - graceful skip for all 4 non-active catalog statuses (removed/compromised/superseded/retired)
#   - --no-ai / --without-group AI (skips AI extensions and strips ai-agents coexistence keys)
#   - pre-mutation extension ID & active publisher validation
#   - exclusive locked storage.json registration with running-VS-Code & CAS guards (--force)
#   - consistent --user-data-dir single-store verification
#   - case-insensitive theme/icon fallback stripping both colorTheme & preferredDarkColorTheme
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/catalog.sh
source "${LIB_DIR}/catalog.sh"
# shellcheck source=scripts/lib/extension-id.sh
source "${LIB_DIR}/extension-id.sh"
# shellcheck source=scripts/lib/vsix-pin.sh
source "${LIB_DIR}/vsix-pin.sh"
# shellcheck source=scripts/lib/vscode-storage.sh
source "${LIB_DIR}/vscode-storage.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"

usage() {
  cat <<'EOF'
Usage: import-profile.sh <profile-name> <bundle.code-profile> [options]
Options:
  --no-ai                  Skip AI-group extensions and strip ai-agents coexistence settings
  --without-group <group>  Skip extensions in catalog group (repeatable)
  --force                  Allow storage.json mutation while VS Code is running
  --allow-unsafe-settings  Allow bundle settings matching the autonomy/exec deny-list
  --break-stale-lock       Allow breaking a confirmed-dead storage.json lock
  --no-batch               Install extensions one-by-one instead of batched
  -h, --help               Show help
EOF
}

PROFILE_NAME=""
BUNDLE_ARG=""
NO_AI=0
FORCE=0
ALLOW_UNSAFE=0
BREAK_STALE=""
BATCH_INSTALL="${VSCODE_BATCH_INSTALL:-1}"
EXCLUDE_GROUPS=()

while [[ $# -gt 0 ]]; do
  case "${1}" in
    -h|--help)
      usage
      exit 0
      ;;
    --no-ai)
      NO_AI=1
      EXCLUDE_GROUPS+=("ai")
      shift
      ;;
    --without-group)
      [[ $# -ge 2 ]] || { echo "import-profile: --without-group requires a value" >&2; exit 4; }
      g_lc="$(to_lower "${2}")"
      EXCLUDE_GROUPS+=("${g_lc}")
      [[ "${g_lc}" == "ai" ]] && NO_AI=1
      shift 2
      ;;
    --without-group=*)
      g_lc="$(to_lower "${1#*=}")"
      EXCLUDE_GROUPS+=("${g_lc}")
      [[ "${g_lc}" == "ai" ]] && NO_AI=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --allow-unsafe-settings)
      ALLOW_UNSAFE=1
      shift
      ;;
    --break-stale-lock)
      BREAK_STALE="--break-stale-lock"
      shift
      ;;
    --no-batch)
      BATCH_INSTALL=0
      shift
      ;;
    -*)
      printf 'import-profile: unknown option %q\n' "${1}" >&2
      exit 4
      ;;
    *)
      if [[ -z "${PROFILE_NAME}" ]]; then
        PROFILE_NAME="${1}"
      elif [[ -z "${BUNDLE_ARG}" ]]; then
        BUNDLE_ARG="${1}"
      else
        printf 'import-profile: unexpected argument %q\n' "${1}" >&2
        exit 4
      fi
      shift
      ;;
  esac
done

if [[ -z "${PROFILE_NAME}" || -z "${BUNDLE_ARG}" ]]; then
  usage >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "import-profile: jq is required" >&2
  exit 1
fi
if ! command -v code >/dev/null 2>&1; then
  echo "import-profile: VS Code 'code' CLI is required on PATH" >&2
  exit 1
fi
if [[ ! -f "${BUNDLE_ARG}" ]]; then
  printf 'import-profile: bundle file not found: %s\n' "${BUNDLE_ARG}" >&2
  exit 1
fi

# 1. Bind canonical path FIRST and stage ONCE into private 0600 temp file (zero TOCTOU).
CANON_BUNDLE="$(canonical_path "${BUNDLE_ARG}")" || exit 1
CANON_DIR="$(dirname -- "${CANON_BUNDLE}")"
CANON_BASE="$(basename -- "${CANON_BUNDLE}")"
MANAGED_EXPORTS_DIR="$(canonical_path "${ROOT}/exports" 2>/dev/null || printf '%s/exports' "${ROOT}")"

WORK_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vsp-import.XXXXXX")"
chmod 0700 "${WORK_TMP}"
trap 'rm -rf "${WORK_TMP}"' EXIT

STAGED_BUNDLE="${WORK_TMP}/bundle.code-profile"
cp "${CANON_BUNDLE}" "${STAGED_BUNDLE}"
chmod 0600 "${STAGED_BUNDLE}"

# 2. Verify SHA256SUMS against staged bytes BEFORE any inspection or mutation.
SUMS_FILE="${CANON_DIR}/SHA256SUMS"
IS_MANAGED=0
if [[ "${CANON_DIR}" == "${MANAGED_EXPORTS_DIR}" ]]; then
  IS_MANAGED=1
fi

if [[ -f "${SUMS_FILE}" ]]; then
  expected_matches=()
  while IFS= read -r line; do
    [[ -n "${line}" ]] || continue
    h="${line%% *}"
    f="${line#* }"
    f="${f# }"
    f="${f#\*}"
    if [[ "${f}" == "${CANON_BASE}" ]]; then
      expected_matches+=("${h}")
    fi
  done < "${SUMS_FILE}"

  if [[ "${#expected_matches[@]}" -gt 1 ]]; then
    printf 'Error: duplicate SHA256SUMS entries for %s in %s\n' "${CANON_BASE}" "${SUMS_FILE}" >&2
    exit 1
  elif [[ "${#expected_matches[@]}" -eq 1 ]]; then
    actual_sha="$(sha256_for_file "${STAGED_BUNDLE}")" || exit 1
    expected_sha="$(to_lower "${expected_matches[0]}")"
    if [[ "${actual_sha}" != "${expected_sha}" ]]; then
      printf 'Error: SHA256 mismatch for %s\n  expected: %s\n  actual:   %s\n' \
        "${CANON_BASE}" "${expected_sha}" "${actual_sha}" >&2
      exit 1
    fi
  else
    if [[ "${IS_MANAGED}" -eq 1 ]]; then
      printf 'Error: managed export %s is not listed in %s\n' "${CANON_BASE}" "${SUMS_FILE}" >&2
      exit 1
    fi
    printf 'Warning: %s not listed in SHA256SUMS (%s); proceeding without checksum verification\n' "${CANON_BASE}" "${SUMS_FILE}" >&2
  fi
else
  if [[ "${IS_MANAGED}" -eq 1 ]]; then
    printf 'Error: SHA256SUMS missing in managed exports directory %s\n' "${CANON_DIR}" >&2
    exit 1
  fi
  printf 'Warning: SHA256SUMS not found next to %s; proceeding without checksum verification\n' "${CANON_BASE}" >&2
fi

# 3. Exact single-object & optional-extensions schema validation (pre-mutation).
if ! jq -e -s '
  length == 1
  and (.[0] | type == "object")
  and (.[0].settings == null or (.[0].settings | type == "object"))
  and (
    .[0].extensions == null
    or (
      (.[0].extensions | type == "object")
      and (
        .[0].extensions.enabled == null
        or (
          (.[0].extensions.enabled | type == "array")
          and (.[0].extensions.enabled | all(type == "string" and length > 0))
        )
      )
    )
  )
' "${STAGED_BUNDLE}" >/dev/null 2>&1; then
  printf 'Error: invalid .code-profile schema in %s (expected single JSON object with optional object .settings and optional non-empty string array .extensions.enabled)\n' "${BUNDLE_ARG}" >&2
  exit 1
fi

# 4. Run parsed-config autonomy/exec deny-list over bundle .settings BEFORE mutation.
SETTINGS_JSON="$(jq -c '.settings // {}' "${STAGED_BUNDLE}")"
if [[ "${ALLOW_UNSAFE}" -eq 0 ]]; then
  if ! printf '%s\n' "${SETTINGS_JSON}" | check_autonomy_denylist_json "${BUNDLE_ARG}"; then
    echo "hint: pass --allow-unsafe-settings only if you explicitly trust this bundle's settings" >&2
    exit 1
  fi
fi

# 5. Pre-mutation extension ID syntax, non-active catalog skip, --no-ai filter, and allowlist gate.
RAW_IDS=()
while IFS= read -r eid; do
  [[ -n "${eid}" ]] && RAW_IDS+=("${eid}")
done < <(jq -r '(.extensions.enabled // [])[] | if type == "string" then . else error("non-string extension id") end' "${STAGED_BUNDLE}" 2>/dev/null) || {
  echo "Error: .extensions.enabled contains a non-string entry" >&2
  exit 1
}

INSTALL_IDS=()
for eid in "${RAW_IDS[@]+"${RAW_IDS[@]}"}"; do
  # Gate 1: syntax always checked first.
  validate_extension_id_syntax "${eid}" || exit 1

  # Gate 2: classify all 4 non-active catalog statuses before active publisher allowlist.
  cstatus="$(catalog_extension_status "${eid}" "${ROOT}" 2>/dev/null || true)"
  case "${cstatus}" in
    compromised)
      printf 'SECURITY: skipping compromised extension %s from bundle (marked compromised in catalog)\n' "${eid}" >&2
      continue
      ;;
    removed|superseded|retired)
      printf 'Warning: skipping catalog-%s extension %s from legacy bundle\n' "${cstatus}" "${eid}" >&2
      continue
      ;;
  esac

  # Gate 3: --no-ai / --without-group filter.
  egrp_lc="$(to_lower "$(catalog_extension_group "${eid}" "${ROOT}")")"
  skip_grp=0
  for exg in "${EXCLUDE_GROUPS[@]+"${EXCLUDE_GROUPS[@]}"}"; do
    if [[ "${exg}" == "${egrp_lc}" ]]; then
      skip_grp=1
      break
    fi
  done
  if [[ "${skip_grp}" -eq 1 ]]; then
    printf 'Info: skipping %s (group %s excluded)\n' "${eid}" "${egrp_lc}" >&2
    continue
  fi

  # Gate 4: active publisher allowlist (pre-mutation!).
  validate_extension_id "${eid}" || exit 1
  INSTALL_IDS+=("${eid}")
done

# Strip AI vendor, chat/inlineChat, and AI-only inlineSuggest keys while preserving shared
# core-editor settings (restoring editor.quickSuggestions.other="on" if set to offWhenInlineCompletions).
if [[ "${NO_AI}" -eq 1 ]]; then
  ai_keys_json='[]'
  if [[ -f "${ROOT}/modules/capabilities/ai-agents/settings.profile.jsonc" ]]; then
    ai_keys_json="$(compiler_ai_agents_keys_json "${ROOT}")"
  fi
  SETTINGS_JSON="$(printf '%s\n' "${SETTINGS_JSON}" | jq -c --argjson ak "${ai_keys_json}" '
    ($ak - ["editor.quickSuggestions", "editor.inlineSuggest.enabled"]) as $strip_ak
    | (reduce $strip_ak[] as $k (.; del(.[$k])))
    | with_entries(select(
        (.key | test("^(antigravity|claudeCode|chatgpt|github\\.copilot|geminicodeassist|chat|inlineChat)\\."))
        or (.key | test("^editor\\.inlineSuggest\\.(fontFamily|edits\\.)"))
        | not
      ))
    | (if (."editor.quickSuggestions" | type) == "object"
          and ."editor.quickSuggestions".other == "offWhenInlineCompletions"
       then ."editor.quickSuggestions".other = "on"
       else . end)
    | .["chat.disableAIFeatures"] = true
  ')"
fi

# 6. Resolve single consistent VS Code store BEFORE mutating storage.json.
CODE_STORE_USER_DIR=""
CODE_STORE_DATA_ROOT=""
resolve_code_store --require-data-root || exit $?
CODE_USER_DIR="${CODE_STORE_USER_DIR}"

# 7. Exclusive locked profile registration in storage.json.
ensure_args=()
[[ "${FORCE}" -eq 1 ]] && ensure_args+=(--force)
[[ -n "${BREAK_STALE}" ]] && ensure_args+=("${BREAK_STALE}")
PROFILE_ID="$(vscode_storage_ensure_profile "${ensure_args[@]+"${ensure_args[@]}"}" "${CODE_USER_DIR}" "${PROFILE_NAME}")" || exit 1

# 8. Write settings.json (0600, bounded backup if overwriting).
PROFILE_SETTINGS="${CODE_USER_DIR}/profiles/${PROFILE_ID}/settings.json"
if [[ -f "${PROFILE_SETTINGS}" ]]; then
  backup_user_file "${PROFILE_SETTINGS}" 5 || exit 1
fi
printf '%s\n' "${SETTINGS_JSON}" | jq . > "${WORK_TMP}/settings.json"
atomic_write_file "${PROFILE_SETTINGS}" "${WORK_TMP}/settings.json" 0600 || exit 1

# 9. Install validated extensions via VSIX pin path + consistent --user-data-dir.
DELAY="${VSCODE_EXTENSION_INSTALL_DELAY:-1}"
PINS_FILE="$(pins_file_path "${ROOT}")"
export VSCODE_VSIX_CACHE_DIR="${VSCODE_VSIX_CACHE_DIR-${ROOT}/.cache/vsix}"
FAILED_EXTS=()
BATCH_IDS=()
BATCH_TARGETS=()

run_code_install() {
  local args=()
  if [[ -n "${CODE_STORE_DATA_ROOT}" ]]; then
    args+=(--user-data-dir "${CODE_STORE_DATA_ROOT}")
  fi
  args+=(--profile "${PROFILE_NAME}")
  local target
  for target in "${@+"${@}"}"; do
    args+=(--install-extension "${target}")
  done
  code "${args[@]}"
}

for eid in "${INSTALL_IDS[@]+"${INSTALL_IDS[@]}"}"; do
  install_target="${eid}"
  if ! bypass_pins_active; then
    pin_out=""
    if ! pin_out="$(lookup_pin "${eid}" "${PINS_FILE}")"; then
      FAILED_EXTS+=("${eid}")
      continue
    fi
    if [[ -n "${pin_out}" ]]; then
      pin_sha="$(printf '%s\n' "${pin_out}" | sed -n '2p')"
      pin_url="$(printf '%s\n' "${pin_out}" | sed -n '3p')"
      vsix_path="${WORK_TMP}/${eid}.vsix"
      if ! fetch_and_verify_vsix "${eid}" "${pin_url}" "${pin_sha}" "${vsix_path}"; then
        FAILED_EXTS+=("${eid}")
        continue
      fi
      install_target="${vsix_path}"
    fi
  fi
  BATCH_IDS+=("${eid}")
  BATCH_TARGETS+=("${install_target}")
done

if [[ "${#BATCH_TARGETS[@]}" -gt 0 ]]; then
  if [[ "${BATCH_INSTALL}" == "1" && "${#BATCH_TARGETS[@]}" -gt 1 ]]; then
    if ! run_code_install "${BATCH_TARGETS[@]}"; then
      # Fall back to one-by-one install so FAILED_EXTS attribution & graceful theme fallback stay exact.
      idx=0
      while [[ "${idx}" -lt "${#BATCH_TARGETS[@]}" ]]; do
        if ! run_code_install "${BATCH_TARGETS[${idx}]}"; then
          FAILED_EXTS+=("${BATCH_IDS[${idx}]}")
        fi
        if [[ "${DELAY}" != "0" ]]; then
          sleep "${DELAY}"
        fi
        idx=$((idx + 1))
      done
    fi
  else
    idx=0
    while [[ "${idx}" -lt "${#BATCH_TARGETS[@]}" ]]; do
      if ! run_code_install "${BATCH_TARGETS[${idx}]}"; then
        FAILED_EXTS+=("${BATCH_IDS[${idx}]}")
      fi
      if [[ "${DELAY}" != "0" ]]; then
        sleep "${DELAY}"
      fi
      idx=$((idx + 1))
    done
  fi
fi

# 10. Graceful theme/icon-theme/product-icon-theme fallback on failed installs (case-insensitive via catalog).
if [[ "${#FAILED_EXTS[@]}" -gt 0 ]]; then
  cur_color="$(jq -r '."workbench.colorTheme" // empty' "${PROFILE_SETTINGS}")"
  cur_dark="$(jq -r '."workbench.preferredDarkColorTheme" // empty' "${PROFILE_SETTINGS}")"
  cur_icon="$(jq -r '."workbench.iconTheme" // empty' "${PROFILE_SETTINGS}")"
  cur_prod="$(jq -r '."workbench.productIconTheme" // empty' "${PROFILE_SETTINGS}")"
  cur_color_lc="$(to_lower "${cur_color}")"
  cur_dark_lc="$(to_lower "${cur_dark}")"
  cur_icon_lc="$(to_lower "${cur_icon}")"
  cur_prod_lc="$(to_lower "${cur_prod}")"

  strip_color=0
  strip_dark=0
  strip_icon=0
  strip_prod=0

  for fid in "${FAILED_EXTS[@]}"; do
    fid_lc="$(to_lower "${fid}")"
    [[ -n "${cur_color_lc}" && "${cur_color_lc}" == "${fid_lc}" ]] && strip_color=1
    [[ -n "${cur_dark_lc}" && "${cur_dark_lc}" == "${fid_lc}" ]] && strip_dark=1
    [[ -n "${cur_icon_lc}" && "${cur_icon_lc}" == "${fid_lc}" ]] && strip_icon=1
    [[ -n "${cur_prod_lc}" && "${cur_prod_lc}" == "${fid_lc}" ]] && strip_prod=1

    while IFS= read -r lbl; do
      [[ -n "${lbl}" ]] || continue
      lbl_lc="$(to_lower "${lbl}")"
      if [[ -n "${cur_color_lc}" && "${cur_color_lc}" == "${lbl_lc}" ]]; then
        strip_color=1
        strip_dark=1
      fi
      if [[ -n "${cur_dark_lc}" && "${cur_dark_lc}" == "${lbl_lc}" ]]; then
        strip_dark=1
      fi
      if [[ -n "${cur_icon_lc}" && "${cur_icon_lc}" == "${lbl_lc}" ]]; then
        strip_icon=1
      fi
      if [[ -n "${cur_prod_lc}" && "${cur_prod_lc}" == "${lbl_lc}" ]]; then
        strip_prod=1
      fi
    done < <(catalog_theme_labels "${fid}" "${ROOT}")
  done

  if [[ "${strip_color}" -eq 1 || "${strip_dark}" -eq 1 || "${strip_icon}" -eq 1 || "${strip_prod}" -eq 1 ]]; then
    del_arr='[]'
    [[ "${strip_color}" -eq 1 ]] && del_arr="$(printf '%s' "${del_arr}" | jq '. + ["workbench.colorTheme"]')"
    [[ "${strip_dark}" -eq 1 ]] && del_arr="$(printf '%s' "${del_arr}" | jq '. + ["workbench.preferredDarkColorTheme"]')"
    [[ "${strip_icon}" -eq 1 ]] && del_arr="$(printf '%s' "${del_arr}" | jq '. + ["workbench.iconTheme"]')"
    [[ "${strip_prod}" -eq 1 ]] && del_arr="$(printf '%s' "${del_arr}" | jq '. + ["workbench.productIconTheme"]')"
    jq --argjson dels "${del_arr}" 'reduce $dels[] as $k (.; del(.[$k]))' "${PROFILE_SETTINGS}" > "${WORK_TMP}/settings-fallback.json"
    atomic_write_file "${PROFILE_SETTINGS}" "${WORK_TMP}/settings-fallback.json" 0600
  fi

  printf 'import-profile: failed to install %d extension(s): %s\n' "${#FAILED_EXTS[@]}" "${FAILED_EXTS[*]}" >&2
  exit 1
fi

printf 'Imported profile %s (%s)\n' "${PROFILE_NAME}" "${PROFILE_ID}"
exit 0

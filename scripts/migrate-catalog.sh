#!/usr/bin/env bash
# migrate-catalog.sh — Catalog layout diagnostics, legacy bundle symlink
# management, and self-contained legacy `_overrides/`/`_shared/` migration.
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB}/common.sh"
# shellcheck source=scripts/lib/legacy-profile-name.sh
source "${LIB}/legacy-profile-name.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB}/module-resolver.sh"

LOCK_DIR="${ROOT}/.cache/migrate-catalog.lock.d"
BUILD_LOCK_DIR="${ROOT}/.cache/.build.lock"
SWAP_STARTED=0

FINAL_LEGACY_FLAVORS=(
  rust astro java-maven java-gradle java-spring cpp-clangd
  cpp-intellisense ai ai-copilot ai-plus github-workflows general
)

is_legacy_final_flavor() {
  case "${1}" in
    rust|astro|java-maven|java-gradle|java-spring|cpp-clangd|cpp-intellisense|ai|ai-copilot|ai-plus|github-workflows|general)
      return 0 ;;
    *) return 1 ;;
  esac
}

legacy_stem_for_flavor() {
  case "${1}" in
    rust)       printf 'rust-profile\n' ;;
    astro)      printf 'web-astro\n' ;;
    ai-copilot) printf 'ai-profile\n' ;;
    java-maven) printf 'java-maven\n' ;;
    java-gradle|java-spring|cpp-clangd|cpp-intellisense|github-workflows|ai-plus)
      printf '%s\n' "${1}" ;;
    *) printf '\n' ;;
  esac
}

refresh_symlinks() {
  if [[ -z "${ARTAGON_VSCODE_BUILD_LOCK_HELD:-}" ]]; then
    acquire_lock "${BUILD_LOCK_DIR}" 30 || exit 1
    trap 'release_lock "${BUILD_LOCK_DIR}"' EXIT
  fi
  refresh_legacy_bundle_links "${ROOT}/exports"
  echo "refreshed legacy bundle symlinks"
}

finalize() {
  if [[ -z "${ARTAGON_VSCODE_BUILD_LOCK_HELD:-}" ]]; then
    acquire_lock "${BUILD_LOCK_DIR}" 30 || exit 1
    trap 'release_lock "${BUILD_LOCK_DIR}"' EXIT
  fi
  local removed=0
  local bundle
  for bundle in "${ROOT}"/exports/*-crisp.code-profile "${ROOT}"/exports/*-retina.code-profile; do
    [[ -L "${bundle}" ]] || continue
    rm -f "${bundle}"
    removed=$((removed + 1))
  done
  echo "removed ${removed} legacy bundle symlinks"
}

doctor() {
  local issues=0
  local tree
  for tree in profiles _overrides _merged exports catalog modules; do
    if [[ -e "${ROOT}/${tree}.new" ]] || [[ -e "${ROOT}/${tree}.old" ]]; then
      echo "partial migration state: ${tree}.new/.old present"
      issues=$((issues + 1))
    fi
  done

  if [[ -d "${ROOT}/catalog" && -d "${ROOT}/modules" ]]; then
    # v2 vertical capability-module layout checks
    local cat_file
    for cat_file in extensions.json publishers.json defaults.json setting-scopes.json; do
      if [[ ! -f "${ROOT}/catalog/${cat_file}" ]]; then
        echo "missing: catalog/${cat_file}"
        issues=$((issues + 1))
      fi
    done

    local count=0
    local prof
    while IFS= read -r prof; do
      [[ -n "${prof}" ]] || continue
      count=$((count + 1))
      [[ -f "${ROOT}/profiles/${prof}/profile.json" ]] || { echo "missing: profiles/${prof}/profile.json"; issues=$((issues + 1)); }
      [[ -f "${ROOT}/_merged/${prof}.json" ]] || { echo "missing: _merged/${prof}.json"; issues=$((issues + 1)); }
      [[ -f "${ROOT}/exports/${prof}.code-profile" ]] || { echo "missing: exports/${prof}.code-profile"; issues=$((issues + 1)); }
    done < <(discover_profiles "${ROOT}")

    local legacy_dir
    for legacy_dir in "${ROOT}"/profiles/*-crisp "${ROOT}"/profiles/*-retina; do
      [[ -d "${legacy_dir}" ]] || continue
      echo "legacy dir still present: $(basename -- "${legacy_dir}")"
      issues=$((issues + 1))
    done

    if [[ "${issues}" -eq 0 ]]; then
      echo "ok: catalog fully migrated (${count} profiles present, no partial state)"
    else
      echo "${issues} migration issue(s) detected; no files were modified"
    fi
    return 0
  fi

  # Legacy fixture layout check
  local flavor
  for flavor in rust astro java-maven java-gradle java-spring cpp-clangd cpp-intellisense ai ai-plus github-workflows general; do
    [[ -d "${ROOT}/profiles/${flavor}" ]] || { echo "missing: profiles/${flavor}"; issues=$((issues + 1)); }
    [[ -f "${ROOT}/_merged/${flavor}.json" ]] || { echo "missing: _merged/${flavor}.json"; issues=$((issues + 1)); }
    [[ -f "${ROOT}/exports/${flavor}.code-profile" ]] || { echo "missing: exports/${flavor}.code-profile"; issues=$((issues + 1)); }
  done

  local legacy_dir
  for legacy_dir in "${ROOT}"/profiles/*-crisp "${ROOT}"/profiles/*-retina; do
    [[ -d "${legacy_dir}" ]] || continue
    echo "legacy dir still present: $(basename -- "${legacy_dir}")"
    issues=$((issues + 1))
  done

  if [[ "${issues}" -eq 0 ]]; then
    echo "ok: catalog fully migrated (legacy layout, no partial state)"
  else
    echo "${issues} migration issue(s) detected; no files were modified"
  fi
  return 0
}

cleanup_uncommitted_stage() {
  if [[ "${SWAP_STARTED}" -eq 0 ]]; then
    rm -rf "${ROOT}/profiles.new" "${ROOT}/_overrides.new" "${ROOT}/_merged.new" "${ROOT}/exports.new"
  fi
  release_lock "${LOCK_DIR}"
}

jsonc_file() {
  local file="${1}"
  if [[ -f "${file}" ]]; then
    LC_ALL=C awk -f "${LIB}/jsonc-strip.awk" "${file}" | jq 'del(.["@extends"])'
  else
    printf '{}\n'
  fi
}

write_legacy_override() {
  local flavor="${1}"
  local destination="${ROOT}/_overrides.new/${flavor}.jsonc"
  local stem
  stem="$(legacy_stem_for_flavor "${flavor}")"
  local canonical="${ROOT}/_overrides/${flavor}.jsonc"
  if [[ -z "${stem}" ]]; then
    if [[ -f "${canonical}" ]]; then
      jsonc_file "${canonical}" | jq . > "${destination}"
    fi
    return 0
  fi

  local base="${ROOT}/_overrides/${stem}-base.jsonc"
  local crisp="${ROOT}/_overrides/${stem}-crisp.jsonc"
  local retina="${ROOT}/_overrides/${stem}-retina.jsonc"
  local crisp_json="" retina_json=""

  [[ -f "${crisp}" ]] && crisp_json="$(jsonc_file "${crisp}")"
  [[ -f "${retina}" ]] && retina_json="$(jsonc_file "${retina}")"
  if [[ -z "${crisp_json}" && -z "${retina_json}" && -f "${canonical}" ]]; then
    jsonc_file "${canonical}" | jq . > "${destination}"
    return 0
  fi
  [[ -z "${crisp_json}" && -f "${base}" ]] && crisp_json="$(jsonc_file "${base}")"
  [[ -z "${retina_json}" && -f "${base}" ]] && retina_json="$(jsonc_file "${base}")"
  [[ -z "${crisp_json}" ]] && crisp_json="${retina_json:-{}}"
  [[ -z "${retina_json}" ]] && retina_json="${crisp_json}"

  local common different ux_keys ux_crisp_json ux_retina_json unexpected
  common="$(jq -n --argjson crisp "${crisp_json}" --argjson retina "${retina_json}" '
    reduce ($crisp | keys_unsorted[]) as $key ({};
      if ($retina | has($key)) and ($crisp[$key] == $retina[$key])
      then .[$key] = $crisp[$key] else . end)')"
  different="$(jq -n --argjson crisp "${crisp_json}" --argjson retina "${retina_json}" '
    [([($crisp | keys_unsorted[]), ($retina | keys_unsorted[])] | unique[]) as $key
      | select((($crisp | has($key)) | not) or (($retina | has($key)) | not) or ($crisp[$key] != $retina[$key]))
      | $key]')"
  ux_crisp_json="$(jsonc_file "${ROOT}/_shared/ux/crisp.jsonc")"
  ux_retina_json="$(jsonc_file "${ROOT}/_shared/ux/retina.jsonc")"
  ux_keys="$(jq -n --argjson crisp "${ux_crisp_json}" --argjson retina "${ux_retina_json}" '[($crisp | keys_unsorted[]), ($retina | keys_unsorted[])] | unique')"
  unexpected="$(jq -n --argjson different "${different}" --argjson ux "${ux_keys}" '$different | map(. as $key | select(($ux | index($key)) == null))')"
  if [[ "$(jq 'length' <<<"${unexpected}")" -gt 0 ]]; then
    echo "migration: ${flavor} has non-UX crisp/retina differences: ${unexpected}" >&2
    return 1
  fi
  printf '%s\n' "${common}" | jq . > "${destination}"
}

build_stage() {
  local tree
  for tree in profiles _overrides _merged exports; do
    mkdir -p "${ROOT}/${tree}.new"
  done

  local flavor
  for flavor in "${MIGRATION_FLAVORS[@]}"; do
    write_legacy_override "${flavor}"
  done

  local base_json override_json merged_json source_profile profile_dir extension_json stem
  for flavor in "${MIGRATION_FLAVORS[@]}"; do
    base_json="$(jsonc_file "${ROOT}/_shared/editor-base.jsonc")"
    if [[ -f "${ROOT}/_overrides.new/${flavor}.jsonc" ]]; then
      override_json="$(jsonc_file "${ROOT}/_overrides.new/${flavor}.jsonc")"
    else
      override_json='{}'
    fi
    merged_json="$(jq -n --argjson base "${base_json}" --argjson override "${override_json}" '$base * $override')"
    printf '%s\n' "${merged_json}" | jq . > "${ROOT}/_merged.new/${flavor}.json"

    source_profile="${ROOT}/profiles/${flavor}"
    if [[ "${flavor}" != "general" ]]; then
      stem="$(legacy_stem_for_flavor "${flavor}")"
      if [[ -n "${stem}" && -d "${ROOT}/profiles/${stem}-crisp" ]]; then
        source_profile="${ROOT}/profiles/${stem}-crisp"
      elif [[ -n "${stem}" && -d "${ROOT}/profiles/${stem}-retina" ]]; then
        source_profile="${ROOT}/profiles/${stem}-retina"
      fi
    fi
    profile_dir="${ROOT}/profiles.new/${flavor}"
    mkdir -p "${profile_dir}"
    if [[ -d "${source_profile}" ]]; then
      cp -R "${source_profile}/." "${profile_dir}/"
    fi
    rm -f "${profile_dir}/extensions.json" "${profile_dir}/settings.json"
    if [[ -f "${source_profile}/extensions.json" ]]; then
      extension_json="$(jq '.' "${source_profile}/extensions.json")"
    elif [[ -f "${ROOT}/_shared/extensions/${flavor}.json" ]]; then
      extension_json="$(jq '.' "${ROOT}/_shared/extensions/${flavor}.json")"
    elif [[ -f "${ROOT}/_shared/extensions/base.json" ]]; then
      extension_json="$(jq '.' "${ROOT}/_shared/extensions/base.json")"
    else
      extension_json='[]'
    fi
    printf '%s\n' "${extension_json}" | jq . > "${profile_dir}/extensions.json"
    # Always point at final `_merged/<flavor>.json`, NEVER `_merged.new`.
    ln -s "../../_merged/${flavor}.json" "${profile_dir}/settings.json"
  done

  local settings extension_ids
  for flavor in "${MIGRATION_FLAVORS[@]}"; do
    settings="$(jq '.' "${ROOT}/_merged.new/${flavor}.json")"
    extension_ids="$(jq '[.[].identifier.id]' "${ROOT}/profiles.new/${flavor}/extensions.json")"
    jq -n --arg name "${flavor}" --argjson settings "${settings}" --argjson extensions "${extension_ids}" \
      '{name: $name, settings: ($settings | del(."editor.fontFamily", ."editor.fontSize", ."workbench.colorTheme", ."workbench.iconTheme")), extensions: {enabled: $extensions}}' \
      > "${ROOT}/exports.new/${flavor}.code-profile"
  done
}

swap_stage() {
  local tree
  # Mark swap started BEFORE the first mv so EXIT trap never wipes partial swap state.
  SWAP_STARTED=1
  for tree in profiles _overrides _merged exports; do
    [[ -e "${ROOT}/${tree}.new" ]] || continue
    if [[ -e "${ROOT}/${tree}" ]]; then
      mv "${ROOT}/${tree}" "${ROOT}/${tree}.old"
    fi
    mv "${ROOT}/${tree}.new" "${ROOT}/${tree}"
  done
  for tree in profiles _overrides _merged exports; do
    [[ -e "${ROOT}/${tree}.old" ]] || continue
    rm -rf "${ROOT}/${tree}.old"
  done
}

mode="${1:-}"
case "${mode}" in
  --doctor)           doctor; exit 0 ;;
  --finalize)         finalize; exit 0 ;;
  --refresh-symlinks) refresh_symlinks; exit 0 ;;
  "") ;;
  *) echo "migrate-catalog: unknown option ${mode}" >&2; exit 4 ;;
esac

# Check partial state BEFORE acquiring staging trap so pre-existing .new/.old
# is NEVER deleted on early exit!
for tree in profiles _overrides _merged exports; do
  if [[ -e "${ROOT}/${tree}.new" ]] || [[ -e "${ROOT}/${tree}.old" ]]; then
    echo "migration: partial state detected (${tree}.new/.old); run --doctor before retrying" >&2
    exit 6
  fi
done

# If already on v2 catalog/modules layout, migration is already complete.
if [[ -d "${ROOT}/catalog" && -d "${ROOT}/modules" ]]; then
  echo "catalog already migrated to v2 capability-module layout; no changes"
  exit 0
fi

if ! acquire_lock "${LOCK_DIR}" 1; then
  echo "migration already running (lock held)" >&2
  exit 6
fi
# Only release lock on early exit until stage build begins.
trap 'release_lock "${LOCK_DIR}"' EXIT

if [[ -f "${ROOT}/MIGRATED.md" ]]; then
  echo "catalog already migrated; no changes"
  exit 0
fi

# Re-verify no partial state after acquiring lock, then arm stage cleanup trap.
for tree in profiles _overrides _merged exports; do
  if [[ -e "${ROOT}/${tree}.new" ]] || [[ -e "${ROOT}/${tree}.old" ]]; then
    echo "migration: partial state detected; run --doctor before retrying" >&2
    exit 6
  fi
done
trap cleanup_uncommitted_stage EXIT

EXTRA_FLAVORS=()
for profile_dir in "${ROOT}/profiles"/*; do
  [[ -d "${profile_dir}" ]] || continue
  flavor="$(basename -- "${profile_dir}")"
  if is_legacy_final_flavor "${flavor}"; then
    continue
  fi
  case "${flavor}" in
    *-crisp|*-retina) continue ;;
  esac
  EXTRA_FLAVORS+=("${flavor}")
done
MIGRATION_FLAVORS=("${FINAL_LEGACY_FLAVORS[@]}" "${EXTRA_FLAVORS[@]+"${EXTRA_FLAVORS[@]}"}")

BACKUP="${ROOT}/.cache/migrate-catalog-backup-$$"
mkdir -p "${BACKUP}"
for tree in profiles _overrides _merged exports; do
  if [[ -d "${ROOT}/${tree}" ]]; then
    cp -R "${ROOT}/${tree}" "${BACKUP}/${tree}"
  fi
done

build_stage
swap_stage
refresh_legacy_bundle_links "${ROOT}/exports"

{
  echo "# Catalog migration"
  echo ""
  echo "Generated by vspcli --migrate-catalog on $(date -u +%Y-%m-%dT%H:%M:%SZ)."
  echo ""
  echo "## Final layout"
  echo ""
  echo "- flavors: ${MIGRATION_FLAVORS[*]}"
  echo "- legacy bundle filenames are symlinks to canonical bundles"
  echo "- backup retained at: ${BACKUP}"
} > "${ROOT}/MIGRATED.md"

echo "migration complete — atomic sibling-tree swap finished"

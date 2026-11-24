#!/usr/bin/env bash
# compose-extensions.sh — Backwards-compatible wrapper over scripts/lib/module-resolver.sh
# and scripts/lib/compiler.sh.
# Usage:
#   compose_extensions <flavor_or_module> [<flavor_or_module>...]
# Outputs VS Code extensions.json array: [ { "identifier": { "id": "<id>" } }, ... ]

# shellcheck disable=SC2148

compose_extensions() {
  local lib_dir root
  lib_dir="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
  root="${VSPCLI_ROOT:-$(CDPATH='' cd -- "${lib_dir}/../.." && pwd -P)}"
  # shellcheck source=scripts/lib/common.sh
  source "${lib_dir}/common.sh"
  # shellcheck source=scripts/lib/module-resolver.sh
  source "${lib_dir}/module-resolver.sh"
  # shellcheck source=scripts/lib/compiler.sh
  source "${lib_dir}/compiler.sh"

  if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    return 1
  fi

  local roots=""
  local excludes=""
  local arg
  local used_defaults=0
  if [[ $# -eq 0 ]]; then
    if [[ -f "${root}/catalog/defaults.json" ]]; then
      roots="$(jq -r '.capabilities[]?' "${root}/catalog/defaults.json")"
      used_defaults=1
    else
      roots="core-editor"
    fi
  fi
  for arg in "${@+"${@}"}"; do
    if [[ -f "${root}/profiles/${arg}/profile.json" ]]; then
      local use_def
      use_def="$(jq -r 'if .useDefaults == false then "false" else "true" end' "${root}/profiles/${arg}/profile.json")"
      if [[ "${use_def}" == "true" && "${used_defaults}" -eq 0 && -f "${root}/catalog/defaults.json" ]]; then
        roots="${roots:+${roots}$'\n'}$(jq -r '.capabilities[]?' "${root}/catalog/defaults.json")"
        used_defaults=1
      fi
      local rm_mods rm_ex
      rm_mods="$(jq -r '.modules[]?' "${root}/profiles/${arg}/profile.json")"
      rm_ex="$(jq -r '.excludeModules[]?' "${root}/profiles/${arg}/profile.json")"
      [[ -n "${rm_mods}" ]] && roots="${roots:+${roots}$'\n'}${rm_mods}"
      [[ -n "${rm_ex}" ]] && excludes="${excludes:+${excludes}$'\n'}${rm_ex}"
    elif module_dir_for_id "${root}" "${arg}" >/dev/null 2>&1; then
      if [[ "${used_defaults}" -eq 0 && -f "${root}/catalog/defaults.json" ]]; then
        roots="${roots:+${roots}$'\n'}$(jq -r '.capabilities[]?' "${root}/catalog/defaults.json")"
        used_defaults=1
      fi
      roots="${roots:+${roots}$'\n'}${arg}"
    else
      printf 'error: missing layer/module/profile for %q\n' "${arg}" >&2
      return 1
    fi
  done

  local resolved_str
  resolved_str="$(resolve_modules "${root}" --roots "${roots}" --exclude "${excludes}")" || return $?
  local mods=()
  local m
  while IFS= read -r m; do
    [[ -n "${m}" ]] && mods+=("${m}")
  done <<< "${resolved_str}"

  compiler_compile_extensions "${root}" profile "${mods[@]+"${mods[@]}"}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail
  compose_extensions "${@+"${@}"}"
fi

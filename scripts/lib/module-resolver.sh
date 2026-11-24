# shellcheck shell=bash
# module-resolver.sh — Auto-discovery & pure Bash 3.2 topological DAG resolver.
# Sourced library: NEVER mutates caller options, NEVER calls `exit` (always returns 4 on error).

MODULE_ID_RE='^[a-z0-9][a-z0-9-]*$'

_mr_list_contains() {
  local needle="${1}"
  local haystack="${2}"
  [[ -n "${haystack}" ]] || return 1
  local item
  while IFS= read -r item; do
    [[ "${item}" == "${needle}" ]] && return 0
  done <<< "${haystack}"
  return 1
}

# Print `<id>\t<kind>\t<dir>` for every discovered module under modules/{capabilities,toolchains,ux}/.
# Fails (return 4) on invalid IDs, dir/id mismatch, or duplicate IDs across categories.
discover_modules() {
  local root="${1:-$(vsp_repo_root)}"
  local seen=""
  local kind dir base manifest id declared_kind
  for kind in capabilities toolchains ux; do
    [[ -d "${root}/modules/${kind}" ]] || continue
    while IFS= read -r dir; do
      [[ -n "${dir}" && -d "${dir}" ]] || continue
      base="$(basename -- "${dir}")"
      manifest="${dir}/module.json"
      if [[ ! -f "${manifest}" ]]; then
        printf 'error: missing module.json in %s\n' "${dir}" >&2
        return 4
      fi
      if [[ ! "${base}" =~ ${MODULE_ID_RE} ]]; then
        printf 'error: invalid module directory name %q (must match %s)\n' "${base}" "${MODULE_ID_RE}" >&2
        return 4
      fi
      id="$(jq -r '.id // empty' "${manifest}" 2>/dev/null || true)"
      declared_kind="$(jq -r '.kind // empty' "${manifest}" 2>/dev/null || true)"
      if [[ "${id}" != "${base}" ]]; then
        printf 'error: module.json id %q does not match directory name %q in %s\n' "${id}" "${base}" "${manifest}" >&2
        return 4
      fi
      if [[ -z "${declared_kind}" ]]; then
        printf 'error: module.json missing kind in %s\n' "${manifest}" >&2
        return 4
      fi
      if _mr_list_contains "${id}" "${seen}"; then
        printf 'error: duplicate module id %q across modules/*/\n' "${id}" >&2
        return 4
      fi
      seen="${seen:+${seen}$'\n'}${id}"
      printf '%s\t%s\t%s\n' "${id}" "${declared_kind}" "${dir}"
    done < <(LC_ALL=C find "${root}/modules/${kind}" -mindepth 1 -maxdepth 1 -type d -print | LC_ALL=C sort)
  done
  return 0
}

# Print module directory path for a given module ID (returns 4 if unknown or invalid).
module_dir_for_id() {
  local root="${1}"
  local id="${2}"
  if [[ ! "${id}" =~ ${MODULE_ID_RE} ]]; then
    printf 'error: invalid module id %q (must match %s)\n' "${id}" "${MODULE_ID_RE}" >&2
    return 4
  fi
  local found=""
  local kind candidate
  for kind in capabilities toolchains ux; do
    candidate="${root}/modules/${kind}/${id}"
    if [[ -d "${candidate}" && -f "${candidate}/module.json" ]]; then
      if [[ -n "${found}" ]]; then
        printf 'error: duplicate module id %q found in %s and %s\n' "${id}" "${found}" "${candidate}" >&2
        return 4
      fi
      found="${candidate}"
    fi
  done
  if [[ -z "${found}" ]]; then
    printf 'error: unknown module %q\n' "${id}" >&2
    return 4
  fi
  printf '%s\n' "${found}"
  return 0
}

# List canonical profile names (directories under profiles/ containing profile.json).
discover_profiles() {
  local root="${1:-$(vsp_repo_root)}"
  [[ -d "${root}/profiles" ]] || return 0
  local dir
  while IFS= read -r dir; do
    [[ -n "${dir}" && -f "${dir}/profile.json" ]] || continue
    basename -- "${dir}"
  done < <(LC_ALL=C find "${root}/profiles" -mindepth 1 -maxdepth 1 -type d -print | LC_ALL=C sort)
}

# Check if module A conflicts with module B (symmetric check).
_mr_pair_conflicts() {
  local root="${1}"
  local a="${2}"
  local b="${3}"
  local dir_a dir_b
  dir_a="$(module_dir_for_id "${root}" "${a}")" || return 1
  dir_b="$(module_dir_for_id "${root}" "${b}")" || return 1
  if jq -e --arg b "${b}" '(.conflicts // []) | index($b) != null' "${dir_a}/module.json" >/dev/null 2>&1; then
    return 0
  fi
  if jq -e --arg a "${a}" '(.conflicts // []) | index($a) != null' "${dir_b}/module.json" >/dev/null 2>&1; then
    return 0
  fi
  return 1
}

# Recursive Bash 3.2 DFS expanding `requires[]` in parent-first post-order.
# Uses caller-scoped variables `_MR_ROOT`, `_MR_EXCLUDED`, `_MR_VISITED`.
_mr_dfs_visit() {
  local id="${1}"
  local visiting="${2}"
  local requester="${3}"

  if [[ ! "${id}" =~ ${MODULE_ID_RE} ]]; then
    printf 'error: invalid module id %q\n' "${id}" >&2
    return 4
  fi
  if _mr_list_contains "${id}" "${_MR_EXCLUDED}"; then
    if [[ -n "${requester}" ]]; then
      printf 'error: module %q requires excluded module %q\n' "${requester}" "${id}" >&2
      return 4
    fi
    return 0
  fi
  if _mr_list_contains "${id}" "${visiting}"; then
    printf 'error: dependency cycle detected involving %q\n' "${id}" >&2
    return 4
  fi
  if _mr_list_contains "${id}" "${_MR_VISITED}"; then
    return 0
  fi

  local dir
  dir="$(module_dir_for_id "${_MR_ROOT}" "${id}")" || return 4
  local next_visiting="${visiting:+${visiting}$'\n'}${id}"

  local req
  while IFS= read -r req; do
    [[ -n "${req}" ]] || continue
    _mr_dfs_visit "${req}" "${next_visiting}" "${id}" || return $?
  done < <(jq -r '.requires[]?' "${dir}/module.json")

  _MR_VISITED="${_MR_VISITED:+${_MR_VISITED}$'\n'}${id}"
  return 0
}

# Check if candidate module D (or any of its transitive requires) is excluded or conflicts
# with any currently selected module. Used for conditional `defaultWith[]` expansion.
_mr_can_add_default_companion() {
  local root="${1}"
  local cand="${2}"
  local current_set="${3}"
  local excluded="${4}"

  _mr_list_contains "${cand}" "${excluded}" && return 1
  _mr_list_contains "${cand}" "${current_set}" && return 1

  # Gather candidate + its transitive requires into a scratch check.
  local saved_visited="${_MR_VISITED}"
  local saved_excluded="${_MR_EXCLUDED}"
  local saved_root="${_MR_ROOT}"
  _MR_ROOT="${root}"
  _MR_EXCLUDED="${excluded}"
  _MR_VISITED=""
  if ! _mr_dfs_visit "${cand}" "" ""; then
    _MR_VISITED="${saved_visited}"
    _MR_EXCLUDED="${saved_excluded}"
    _MR_ROOT="${saved_root}"
    return 1
  fi
  local closure="${_MR_VISITED}"
  _MR_VISITED="${saved_visited}"
  _MR_EXCLUDED="${saved_excluded}"
  _MR_ROOT="${saved_root}"

  local m c
  while IFS= read -r c; do
    [[ -n "${c}" ]] || continue
    while IFS= read -r m; do
      [[ -n "${m}" ]] || continue
      if _mr_pair_conflicts "${root}" "${c}" "${m}"; then
        return 1
      fi
    done <<< "${current_set}"
  done <<< "${closure}"
  return 0
}

# Print module kind + priority sort key for deterministic total ordering:
#   00:<default_idx> core-editor & default capabilities (defaults.json order)
#   10:<id>          base toolchains (detect == null, non-conflict bases)
#   20:<prio>:<id>   concrete toolchains (ascending detect.priority, then LC_ALL=C id)
#   30:<id>          extra capabilities
#   40:<id>          UX preset (always last)
_mr_sort_key() {
  local root="${1}"
  local id="${2}"
  local defaults_list="${3}"
  local dir kind prio is_detect
  dir="$(module_dir_for_id "${root}" "${id}")" || return 4
  kind="$(jq -r '.kind // "capability"' "${dir}/module.json")"

  if [[ "${id}" == "core-editor" ]]; then
    printf '00:000:%s\n' "${id}"
    return 0
  fi
  if [[ "${kind}" == "ux" ]]; then
    printf '40:000:%s\n' "${id}"
    return 0
  fi

  local idx=1
  local d
  while IFS= read -r d; do
    [[ -n "${d}" ]] || continue
    if [[ "${d}" == "${id}" ]]; then
      printf '01:%03d:%s\n' "${idx}" "${id}"
      return 0
    fi
    idx=$((idx + 1))
  done <<< "${defaults_list}"

  if [[ "${kind}" == "toolchain" ]]; then
    is_detect="$(jq -r 'if .detect == null then "0" else "1" end' "${dir}/module.json")"
    if [[ "${is_detect}" == "0" ]]; then
      # Base/non-detected toolchains: distinguish bases (python-base, java-base, cpp-base, ts-base)
      # from provider companions (ts-eslint-prettier, python-ty).
      case "${id}" in
        *-base) printf '10:000:%s\n' "${id}" ;;
        *)      printf '25:050:%s\n' "${id}" ;;
      esac
    else
      prio="$(jq -r '.detect.priority // 50' "${dir}/module.json")"
      printf '20:%03d:%s\n' "${prio}" "${id}"
    fi
    return 0
  fi

  printf '30:000:%s\n' "${id}"
  return 0
}

# Resolve a module selection into a deterministic parent-first ordered list (one ID per line).
# Usage:
#   resolve_modules <root> --roots <csv-or-nl> [--exclude <csv-or-nl>] [--cli-with <csv>] [--cli-without <csv>]
resolve_modules() {
  local root="${1}"; shift
  local roots_raw=""
  local exclude_raw=""
  local cli_with_raw=""
  local cli_without_raw=""

  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --roots)       roots_raw="${2}"; shift 2 ;;
      --exclude)     exclude_raw="${2}"; shift 2 ;;
      --cli-with)    cli_with_raw="${2}"; shift 2 ;;
      --cli-without) cli_without_raw="${2}"; shift 2 ;;
      *)             printf 'error: resolve_modules unknown option %q\n' "${1}" >&2; return 4 ;;
    esac
  done

  # Normalize CSV / newline inputs.
  local roots="" excludes="" cli_with="" cli_without="" item
  while IFS= read -r item; do
    [[ -n "${item}" ]] && roots="${roots:+${roots}$'\n'}${item}"
  done < <(printf '%s\n' "${roots_raw}" | tr ',' '\n')

  while IFS= read -r item; do
    [[ -n "${item}" ]] && excludes="${excludes:+${excludes}$'\n'}${item}"
  done < <(printf '%s\n' "${exclude_raw}" | tr ',' '\n')

  while IFS= read -r item; do
    [[ -n "${item}" ]] && cli_with="${cli_with:+${cli_with}$'\n'}${item}"
  done < <(printf '%s\n' "${cli_with_raw}" | tr ',' '\n')

  while IFS= read -r item; do
    [[ -n "${item}" ]] && cli_without="${cli_without:+${cli_without}$'\n'}${item}"
  done < <(printf '%s\n' "${cli_without_raw}" | tr ',' '\n')

  # Reject contradictory CLI --with X --without X.
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    if _mr_list_contains "${item}" "${cli_without}"; then
      printf 'error: contradictory selection: %q passed in both --with and --without\n' "${item}" >&2
      return 4
    fi
  done <<< "${cli_with}"

  # Build ONE unified effective exclusion set:
  # (recipe excludes + CLI --without) minus explicit CLI --with (which overrides recipe exclude).
  local eff_excluded=""
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    if _mr_list_contains "${item}" "${cli_with}"; then
      printf 'info: --with %s overrides recipe excludeModules\n' "${item}" >&2
      continue
    fi
    _mr_list_contains "${item}" "${eff_excluded}" || eff_excluded="${eff_excluded:+${eff_excluded}$'\n'}${item}"
  done <<< "${excludes}"
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    _mr_list_contains "${item}" "${eff_excluded}" || eff_excluded="${eff_excluded:+${eff_excluded}$'\n'}${item}"
  done <<< "${cli_without}"

  # Validate all excluded IDs syntax/existence cleanly.
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    if [[ ! "${item}" =~ ${MODULE_ID_RE} ]]; then
      printf 'error: invalid excluded module id %q\n' "${item}" >&2
      return 4
    fi
  done <<< "${eff_excluded}"

  # Combine roots + cli_with (removing excluded roots).
  local initial=""
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    _mr_list_contains "${item}" "${eff_excluded}" && continue
    _mr_list_contains "${item}" "${initial}" || initial="${initial:+${initial}$'\n'}${item}"
  done <<< "${roots}"
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    _mr_list_contains "${item}" "${initial}" || initial="${initial:+${initial}$'\n'}${item}"
  done <<< "${cli_with}"

  # Phase 1: Transitive requires DFS.
  _MR_ROOT="${root}"
  _MR_EXCLUDED="${eff_excluded}"
  _MR_VISITED=""
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    _mr_dfs_visit "${item}" "" "" || return $?
  done <<< "${initial}"

  # Phase 2: Conditional defaultWith[] expansion (suppressed if in eff_excluded or conflicting).
  local added=1
  while [[ "${added}" -eq 1 ]]; do
    added=0
    local snapshot="${_MR_VISITED}"
    local mod dir comp
    while IFS= read -r mod; do
      [[ -n "${mod}" ]] || continue
      dir="$(module_dir_for_id "${root}" "${mod}")" || return 4
      while IFS= read -r comp; do
        [[ -n "${comp}" ]] || continue
        if _mr_can_add_default_companion "${root}" "${comp}" "${_MR_VISITED}" "${eff_excluded}"; then
          _mr_dfs_visit "${comp}" "" "${mod}" || return $?
          added=1
        fi
      done < <(jq -r '.defaultWith[]?' "${dir}/module.json")
    done <<< "${snapshot}"
  done

  # Phase 3: Pairwise symmetric conflict check.
  local m1 m2
  while IFS= read -r m1; do
    [[ -n "${m1}" ]] || continue
    local d1
    d1="$(module_dir_for_id "${root}" "${m1}")" || return 4
    while IFS= read -r m2; do
      [[ -n "${m2}" ]] || continue
      if _mr_list_contains "${m2}" "${_MR_VISITED}"; then
        printf 'error: conflicting modules selected: %q conflicts with %q (resolve with --without <id>)\n' "${m1}" "${m2}" >&2
        return 4
      fi
    done < <(jq -r '.conflicts[]?' "${d1}/module.json")
  done <<< "${_MR_VISITED}"

  # Phase 4: Deterministic total order preserving parent-first invariant.
  local defaults_list=""
  if [[ -f "${root}/catalog/defaults.json" ]]; then
    defaults_list="$(jq -r '.capabilities[]?' "${root}/catalog/defaults.json")"
  fi

  local sorted_raw=""
  while IFS= read -r m1; do
    [[ -n "${m1}" ]] || continue
    local sk
    sk="$(_mr_sort_key "${root}" "${m1}" "${defaults_list}")" || return 4
    sorted_raw="${sorted_raw:+${sorted_raw}$'\n'}${sk}"
  done <<< "${_MR_VISITED}"

  # Re-run DFS in sorted priority order so parent-first guarantees hold strictly.
  local ordered_seeds
  ordered_seeds="$(printf '%s\n' "${sorted_raw}" | LC_ALL=C sort | awk -F: '{print $3}')"
  _MR_VISITED=""
  while IFS= read -r item; do
    [[ -n "${item}" ]] || continue
    _mr_dfs_visit "${item}" "" "" || return $?
  done <<< "${ordered_seeds}"

  printf '%s\n' "${_MR_VISITED}"
  return 0
}

# Resolve a profile recipe (profiles/<name>/profile.json) plus optional CLI --with/--without/--ux.
resolve_profile_recipe() {
  local root="${1}"
  local profile_name="${2}"
  local cli_with="${3:-}"
  local cli_without="${4:-}"
  local ux_override="${5:-}"

  local recipe="${root}/profiles/${profile_name}/profile.json"
  if [[ ! -f "${recipe}" ]]; then
    printf 'error: missing profile recipe %s\n' "${recipe}" >&2
    return 4
  fi

  local use_defaults
  use_defaults="$(jq -r 'if .useDefaults == false then "false" else "true" end' "${recipe}")" || return 4
  local roots=""
  if [[ "${use_defaults}" == "true" && -f "${root}/catalog/defaults.json" ]]; then
    roots="$(jq -r '.capabilities[]?' "${root}/catalog/defaults.json")"
  fi
  local recipe_mods
  recipe_mods="$(jq -r '.modules[]?' "${recipe}")"
  if [[ -n "${recipe_mods}" ]]; then
    roots="${roots:+${roots}$'\n'}${recipe_mods}"
  fi

  local ux="${ux_override}"
  if [[ -z "${ux}" ]]; then
    ux="$(jq -r '.ux // "default"' "${recipe}")"
  fi
  if [[ -n "${ux}" && "${ux}" != "default" ]]; then
    roots="${roots:+${roots}$'\n'}${ux}"
  fi

  local recipe_excludes
  recipe_excludes="$(jq -r '.excludeModules[]?' "${recipe}")"

  resolve_modules "${root}" \
    --roots "${roots}" \
    --exclude "${recipe_excludes}" \
    --cli-with "${cli_with}" \
    --cli-without "${cli_without}"
}

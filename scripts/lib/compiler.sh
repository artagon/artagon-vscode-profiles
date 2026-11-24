# shellcheck shell=bash
# compiler.sh — Unified pure Bash 3.2 + jq compiler for profiles, workspaces, tasks, and RTK.
# Sourced library: NEVER mutates caller shell options or traps.

# Set-like array keys merged by order-preserving set union instead of overwrite.
COMPILER_UNION_KEYS_JSON='["shellcheck.exclude","java.import.exclusions","java.project.resourceFilters","eslint.validate","recommendations","unwantedRecommendations"]'

# Strip JSONC comments/trailing commas, assert single JSON object, strip $schema & @extends.
compiler_parse_jsonc_object() {
  local _root="${1}"
  local file="${2}"
  local lib_dir
  lib_dir="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
  local strip="${lib_dir}/jsonc-strip.awk"
  : "${_root}"
  local stripped
  stripped="$(LC_ALL=C awk -f "${strip}" "${file}")" || return 1
  if ! printf '%s\n' "${stripped}" | jq -e -s 'length == 1 and (.[0] | type == "object")' >/dev/null 2>&1; then
    printf 'error: %s is not a single valid JSON object\n' "${file}" >&2
    return 1
  fi
  printf '%s\n' "${stripped}" | jq 'del(.["$schema"], .["@extends"])'
}

# Print JSON array of the 15 symmetric managed UX keys dynamically from modules/ux/crisp/.
compiler_ux_managed_keys_json() {
  local root="${1}"
  local crisp="${root}/modules/ux/crisp/settings.profile.jsonc"
  compiler_parse_jsonc_object "${root}" "${crisp}" | jq -S -c 'keys'
}

# Print JSON array of ai-agents settings keys dynamically from modules/capabilities/ai-agents/.
compiler_ai_agents_keys_json() {
  local root="${1}"
  local ai_cfg="${root}/modules/capabilities/ai-agents/settings.profile.jsonc"
  compiler_parse_jsonc_object "${root}" "${ai_cfg}" | jq -S -c 'keys'
}

# Deterministic jq deep-merge across a list of JSON object files:
#   - Objects merge recursively
#   - Declared set-like top-level array keys merge by order-preserving union
#   - Other arrays and scalars use right-hand precedence
compiler_merge_json_files() {
  if [[ $# -eq 0 ]]; then
    printf '{}\n'
    return 0
  fi
  jq -s --argjson union_keys "${COMPILER_UNION_KEYS_JSON}" '
    def merge_vals($k; $a; $b):
      if ($a | type) == "object" and ($b | type) == "object" then
        reduce ($b | keys_unsorted[]) as $sub ($a;
          .[$sub] = (if has($sub) then merge_vals($sub; .[$sub]; $b[$sub]) else $b[$sub] end)
        )
      elif ($a | type) == "array" and ($b | type) == "array" and ($union_keys | index($k) != null) then
        reduce $b[] as $el ($a; if index($el) != null then . else . + [$el] end)
      else
        $b
      end;
    reduce .[] as $doc ({};
      reduce ($doc | keys_unsorted[]) as $k (.;
        .[$k] = (if has($k) then merge_vals($k; .[$k]; $doc[$k]) else $doc[$k] end)
      )
    )
  ' "${@+"${@}"}"
}

# Compile settings across resolved module IDs for a target scope: workspace | profile | both.
# Optional `--override-json '<json-object>'` applied last.
compiler_compile_settings() {
  local root="${1}"; shift
  local scope="${1}"; shift
  local override_json="{}"
  local modules=()

  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --override-json) override_json="${2}"; shift 2 ;;
      --)              shift; while [[ $# -gt 0 ]]; do modules+=("${1}"); shift; done ;;
      *)               modules+=("${1}"); shift ;;
    esac
  done

  local tmp_dir
  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vsp-compile.XXXXXX")" || return 1
  local inputs=()
  local idx=0
  local mod dir ws_file prof_file out_f

  for mod in "${modules[@]+"${modules[@]}"}"; do
    [[ -n "${mod}" ]] || continue
    dir="$(module_dir_for_id "${root}" "${mod}")" || { rm -rf "${tmp_dir}"; return 4; }
    ws_file="$(jq -r '.settings.workspace // empty' "${dir}/module.json")"
    prof_file="$(jq -r '.settings.profile // empty' "${dir}/module.json")"

    if [[ "${scope}" == "workspace" || "${scope}" == "both" ]]; then
      if [[ -n "${ws_file}" ]]; then
        idx=$((idx + 1))
        out_f="${tmp_dir}/$(printf '%03d' "${idx}")-${mod}-ws.json"
        if ! compiler_parse_jsonc_object "${root}" "${dir}/${ws_file}" > "${out_f}"; then
          rm -rf "${tmp_dir}"
          return 1
        fi
        inputs+=("${out_f}")
      fi
    fi

    if [[ "${scope}" == "profile" || "${scope}" == "both" ]]; then
      if [[ -n "${prof_file}" ]]; then
        idx=$((idx + 1))
        out_f="${tmp_dir}/$(printf '%03d' "${idx}")-${mod}-prof.json"
        if ! compiler_parse_jsonc_object "${root}" "${dir}/${prof_file}" > "${out_f}"; then
          rm -rf "${tmp_dir}"
          return 1
        fi
        inputs+=("${out_f}")
      fi
    fi
  done

  if [[ "${override_json}" != "{}" && -n "${override_json}" ]]; then
    idx=$((idx + 1))
    out_f="${tmp_dir}/$(printf '%03d' "${idx}")-override.json"
    printf '%s\n' "${override_json}" | jq 'del(.["$schema"], .["@extends"])' > "${out_f}" || { rm -rf "${tmp_dir}"; return 1; }
    inputs+=("${out_f}")
  fi

  local rc=0
  if [[ "${#inputs[@]}" -eq 0 ]]; then
    printf '{}\n'
  else
    compiler_merge_json_files "${inputs[@]}" || rc=$?
  fi
  rm -rf "${tmp_dir}"
  return "${rc}"
}

# Compile extensions across resolved modules.
# scope:
#   "profile"   -> VS Code extensions.json array: [ { "identifier": { "id": "..." } } ]
#                  (ordered case-insensitively deduplicated union of workspace[] + profile[])
#   "workspace" -> { "recommendations": [...], "unwantedRecommendations": [...] }
#                  (workspace[] ONLY; unwanted[] ONLY from workspace-active modules)
compiler_compile_extensions() {
  local root="${1}"; shift
  local scope="${1}"; shift
  local modules=("${@+"${@}"}")

  local manifests=()
  local mod dir
  for mod in "${modules[@]+"${modules[@]}"}"; do
    [[ -n "${mod}" ]] || continue
    dir="$(module_dir_for_id "${root}" "${mod}")" || return 4
    manifests+=("${dir}/module.json")
  done

  if [[ "${#manifests[@]}" -eq 0 ]]; then
    if [[ "${scope}" == "workspace" ]]; then
      printf '{"recommendations":[],"unwantedRecommendations":[]}\n'
    else
      printf '[]\n'
    fi
    return 0
  fi

  if [[ "${scope}" == "workspace" ]]; then
    jq -s '
      def dedup_ci:
        reduce .[] as $id ({seen: [], out: []};
          ($id | ascii_downcase) as $lc
          | if (.seen | index($lc)) != null then .
            else {seen: (.seen + [$lc]), out: (.out + [$id])}
            end
        ) | .out;
      def is_ws_active:
        ((.extensions.workspace // []) | length > 0) or (.settings.workspace != null);
      ([.[] | (.extensions.workspace // [])[]] | dedup_ci) as $recs
      | ([.[] | select(is_ws_active) | (.extensions.unwanted // [])[]] | dedup_ci) as $unwanted
      | ($unwanted | map(ascii_downcase)) as $unwanted_lc
      | {
          recommendations: ($recs | map(. as $r | select(($unwanted_lc | index($r | ascii_downcase)) == null))),
          unwantedRecommendations: $unwanted
        }
    ' "${manifests[@]}"
  else
    jq -s '
      def dedup_ci:
        reduce .[] as $id ({seen: [], out: []};
          ($id | ascii_downcase) as $lc
          | if (.seen | index($lc)) != null then .
            else {seen: (.seen + [$lc]), out: (.out + [$id])}
            end
        ) | .out;
      [
        .[] | ((.extensions.workspace // []) + (.extensions.profile // []))[]
      ]
      | dedup_ci
      | map({identifier: {id: .}})
    ' "${manifests[@]}"
  fi
}

# Compile tasks.json across resolved modules (merging by .label).
# rtk_mode: "plain" (default portable workspace) | "rtk" (--rtk-in-workspace or profile)
compiler_compile_tasks() {
  local root="${1}"; shift
  local rtk_mode="${1}"; shift
  local modules=("${@+"${@}"}")

  local task_files=()
  local mod dir tf
  for mod in "${modules[@]+"${modules[@]}"}"; do
    [[ -n "${mod}" ]] || continue
    dir="$(module_dir_for_id "${root}" "${mod}")" || return 4
    tf="$(jq -r '.tasks // empty' "${dir}/module.json")"
    if [[ -n "${tf}" && -f "${dir}/${tf}" ]]; then
      task_files+=("${dir}/${tf}")
    fi
  done

  if [[ "${#task_files[@]}" -eq 0 ]]; then
    return 0
  fi

  jq -s --arg rtk_mode "${rtk_mode}" '
    {
      version: "2.0.0",
      tasks: (
        reduce (.[] | (.tasks // [])[]) as $t ([];
          (map(.label) | index($t.label)) as $idx
          | if $idx != null then .[$idx] = $t else . + [$t] end
        )
        | if $rtk_mode == "rtk" then
            map(
              if (.command | type) == "string" and (.command | startswith("rtk ") | not) then
                .command = "rtk " + .command
              else . end
            )
          else . end
      )
    }
  ' "${task_files[@]}"
}

# Install portable XDG RTK wrappers using --no-follow atomic writes (0755 dirs, 0644 files).
# Prints installed XDG RTK directory on stdout.
compiler_install_rtk_xdg() {
  local root="${1}"
  local src_dir="${root}/modules/capabilities/rtk-terminal/scripts"
  local xdg_base
  xdg_base="$(resolve_xdg_config_home)"
  local dest_dir="${xdg_base}/artagon-vscode/rtk"
  mkdir -p "${dest_dir}/zsh" || return 1
  chmod 0755 "${dest_dir}" "${dest_dir}/zsh" 2>/dev/null || true

  local f
  for f in rtk-init.bash rtk-init.fish rtk-init.zsh; do
    atomic_write_file --no-follow "${dest_dir}/${f}" "${src_dir}/${f}" 0644 || return 1
  done
  for f in .zshenv .zprofile .zshrc .zlogin; do
    atomic_write_file --no-follow "${dest_dir}/zsh/${f}" "${src_dir}/zsh/${f}" 0644 || return 1
  done
  printf '%s\n' "${dest_dir}"
}

# Emit OS-accurate RTK terminal profiles JSON referencing installed XDG RTK dir.
# Never sets terminal.integrated.automationProfile.*; selects an actually installed Linux shell.
compiler_rtk_terminal_settings_json() {
  local xdg_rtk_dir="${1}"
  local linux_default="rtk-bash"
  if command -v zsh >/dev/null 2>&1; then
    linux_default="rtk-zsh"
  elif command -v fish >/dev/null 2>&1; then
    linux_default="rtk-fish"
  fi
  local osx_default="rtk-zsh"
  if command -v fish >/dev/null 2>&1 && [[ "${SHELL:-}" == *fish ]]; then
    osx_default="rtk-fish"
  fi

  local orig_zdotdir_set="0"
  local orig_zdotdir=""
  if [[ -n "${ZDOTDIR+x}" ]]; then
    orig_zdotdir_set="1"
    orig_zdotdir="${ZDOTDIR}"
  fi

  jq -n \
    --arg rtk_dir "${xdg_rtk_dir}" \
    --arg linux_def "${linux_default}" \
    --arg osx_def "${osx_default}" \
    --arg zset "${orig_zdotdir_set}" \
    --arg zorig "${orig_zdotdir}" '
    {
      "terminal.integrated.profiles.osx": {
        "rtk-zsh": {
          "path": "/bin/zsh",
          "args": ["-l"],
          "env": {
            "ARTAGON_ORIG_ZDOTDIR_SET": $zset,
            "ARTAGON_ORIG_ZDOTDIR": $zorig,
            "ZDOTDIR": ($rtk_dir + "/zsh")
          }
        },
        "rtk-fish": {
          "path": "fish",
          "args": ["--init-command", "source \"$ARTAGON_RTK_DIR/rtk-init.fish\""],
          "env": {
            "ARTAGON_RTK_DIR": $rtk_dir
          }
        },
        "rtk-bash": {
          "path": "/bin/bash",
          "args": ["--rcfile", ($rtk_dir + "/rtk-init.bash")]
        }
      },
      "terminal.integrated.defaultProfile.osx": $osx_def,
      "terminal.integrated.profiles.linux": {
        "rtk-zsh": {
          "path": "/usr/bin/zsh",
          "args": ["-l"],
          "env": {
            "ARTAGON_ORIG_ZDOTDIR_SET": $zset,
            "ARTAGON_ORIG_ZDOTDIR": $zorig,
            "ZDOTDIR": ($rtk_dir + "/zsh")
          }
        },
        "rtk-fish": {
          "path": "/usr/bin/fish",
          "args": ["--init-command", "source \"$ARTAGON_RTK_DIR/rtk-init.fish\""],
          "env": {
            "ARTAGON_RTK_DIR": $rtk_dir
          }
        },
        "rtk-bash": {
          "path": "/bin/bash",
          "args": ["--rcfile", ($rtk_dir + "/rtk-init.bash")]
        }
      },
      "terminal.integrated.defaultProfile.linux": $linux_def
    }
  '
}

# Build a canonical profile recipe into:
#   - _merged/<name>.json (0644, UX-agnostic: all 15 symmetric UX keys stripped)
#   - profiles/<name>/settings.json -> ../../_merged/<name>.json
#   - profiles/<name>/extensions.json (0644, ordered deduplicated VS Code format)
compiler_build_profile() {
  local root="${1}"
  local profile_name="${2}"
  local recipe="${root}/profiles/${profile_name}/profile.json"
  if [[ ! -f "${recipe}" ]]; then
    printf 'error: missing profile recipe: %s\n' "${recipe}" >&2
    return 1
  fi

  local resolved_str
  resolved_str="$(resolve_profile_recipe "${root}" "${profile_name}" "" "" "default")" || return $?
  local mods=()
  local m
  while IFS= read -r m; do
    [[ -n "${m}" ]] && mods+=("${m}")
  done <<< "${resolved_str}"

  local ov_ws ov_prof combined_ov
  ov_ws="$(jq -c 'if (.overrides | type) == "object" and (.overrides | has("workspace") or has("profile")) then (.overrides.workspace // {}) else {} end' "${recipe}")"
  ov_prof="$(jq -c 'if (.overrides | type) == "object" and (.overrides | has("workspace") or has("profile")) then (.overrides.profile // {}) else (.overrides // {}) end' "${recipe}")"
  combined_ov="$(jq -n --argjson a "${ov_ws}" --argjson b "${ov_prof}" '$a * $b')"

  local ux_keys
  ux_keys="$(compiler_ux_managed_keys_json "${root}")" || return 1

  local tmp_dir
  tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/vsp-build-prof.XXXXXX")" || return 1

  if ! compiler_compile_settings "${root}" both --override-json "${combined_ov}" -- "${mods[@]+"${mods[@]}"}" \
      | jq -S --argjson ux "${ux_keys}" 'reduce $ux[] as $k (.; del(.[$k]))' > "${tmp_dir}/merged.json"; then
    rm -rf "${tmp_dir}"
    return 1
  fi

  if ! compiler_compile_extensions "${root}" profile "${mods[@]+"${mods[@]}"}" | jq . > "${tmp_dir}/extensions.json"; then
    rm -rf "${tmp_dir}"
    return 1
  fi

  mkdir -p "${root}/_merged" "${root}/profiles/${profile_name}"
  if ! atomic_write_file --no-follow "${root}/_merged/${profile_name}.json" "${tmp_dir}/merged.json" 0644; then
    rm -rf "${tmp_dir}"
    return 1
  fi
  if ! atomic_write_file --no-follow "${root}/profiles/${profile_name}/extensions.json" "${tmp_dir}/extensions.json" 0644; then
    rm -rf "${tmp_dir}"
    return 1
  fi
  ln -sfn "../../_merged/${profile_name}.json" "${root}/profiles/${profile_name}/settings.json"
  rm -rf "${tmp_dir}"
  return 0
}

# Workspace provenance path under XDG_STATE_HOME for a canonical workspace path.
compiler_workspace_state_file() {
  local ws_canon="${1}"
  local state_base
  state_base="$(resolve_xdg_state_home)"
  local hash
  hash="$(printf '%s' "${ws_canon}" | sha256_for_file -)" || return 1
  printf '%s/artagon-vscode/workspaces/%s.json\n' "${state_base}" "${hash}"
}

# Compute workspace generation fingerprint (canonical path + directory inode)
# so deleting and recreating a checkout at the same path invalidates stale provenance
# while switching git branches preserves managed-key retirement cleanly.
compiler_workspace_fingerprint() {
  local ws_canon="${1}"
  local inode="0"
  local candidate=""
  if candidate="$(stat -c '%i' "${ws_canon}" 2>/dev/null)" && [[ "${candidate}" =~ ^[0-9]+$ ]]; then
    inode="${candidate}"
  elif candidate="$(stat -f '%i' "${ws_canon}" 2>/dev/null)" && [[ "${candidate}" =~ ^[0-9]+$ ]]; then
    inode="${candidate}"
  fi
  printf '%s|%s' "${ws_canon}" "${inode}" | sha256_for_file -
}

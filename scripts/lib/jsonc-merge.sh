# shellcheck shell=bash
# jsonc-merge.sh — Hardened key-level JSONC merger.
# Sourced library: NEVER sets top-level `set -Eeuo pipefail` or mutates caller traps.
#
# Features:
#   - Enforces single JSON object cardinality (`jq -e -s 'length == 1 and (.[0] | type == "object")'`)
#   - Accurate string-aware interior comment detection (never confused by `https://` in strings)
#   - Optional `--workspace-root <dir>` containment check (refuses symlink escape with exit 4)
#   - Optional `--user-root <dir>` mode: refuses symlinks outside <dir> (exit 4), writes bounded
#     timestamped 0600 `.bak`, and fails closed (exit 4) on unpreserveable interior comments unless
#     `--allow-comment-loss` is passed
#   - Preserves existing file permissions without widening

_jsonc_has_interior_comments() {
  local file="$1"
  LC_ALL=C awk '
    BEGIN { in_str=0; esc=0; header=1; found=0; in_block=0 }
    {
      line=$0
      sub(/\r$/, "", line)
      if (header) {
        if (line ~ /^[[:space:]]*\/\// || line ~ /^[[:space:]]*$/) {
          next
        }
        header=0
      }
      n=length(line)
      for (i=1; i<=n; i++) {
        c=substr(line, i, 1)
        nxt=(i<n) ? substr(line, i+1, 1) : ""
        if (in_block) {
          if (c == "*" && nxt == "/") { in_block=0; i++ }
          continue
        }
        if (in_str) {
          if (esc) { esc=0 }
          else if (c == "\\") { esc=1 }
          else if (c == "\"") { in_str=0 }
          continue
        }
        if (c == "\"") { in_str=1; continue }
        if (c == "/" && nxt == "/") { found=1; exit 0 }
        if (c == "/" && nxt == "*") { found=1; in_block=1; exit 0 }
      }
    }
    END { exit (found ? 0 : 1) }
  ' "${file}"
}

# Usage:
#   jsonc_merge [--workspace-root <dir>] [--user-root <dir>] [--allow-comment-loss] \
#               [--delete-keys-json '<json-array>'] [--delete-paths-json '<json-array-of-paths>'] \
#               <target-path> <new-json-string-or-file>
jsonc_merge() {
  local workspace_root=""
  local user_root=""
  local allow_comment_loss=0
  local delete_keys_json="[]"
  local delete_paths_json="[]"

  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --workspace-root)    workspace_root="${2}"; shift 2 ;;
      --user-root)         user_root="${2}"; shift 2 ;;
      --allow-comment-loss) allow_comment_loss=1; shift ;;
      --delete-keys-json)  delete_keys_json="${2}"; shift 2 ;;
      --delete-paths-json) delete_paths_json="${2}"; shift 2 ;;
      --)                  shift; break ;;
      -*)                  echo "error: jsonc_merge: unknown option ${1}" >&2; return 4 ;;
      *)                   break ;;
    esac
  done

  local target="${1:-}"
  local new_doc_arg="${2:-}"
  if [[ -z "${target}" || -z "${new_doc_arg}" ]]; then
    echo "error: jsonc_merge requires <target> <new-json>" >&2
    return 4
  fi

  local lib_dir
  lib_dir="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
  # shellcheck source=scripts/lib/common.sh
  source "${lib_dir}/common.sh"
  local strip="${lib_dir}/jsonc-strip.awk"

  if ! command -v jq >/dev/null 2>&1; then
    echo "error: jq required" >&2
    return 1
  fi
  if [[ ! -f "${strip}" ]]; then
    echo "error: missing ${strip}" >&2
    return 1
  fi

  # Boundary checks BEFORE reading or writing.
  if [[ -n "${workspace_root}" ]]; then
    local ws_canon
    ws_canon="$(canonical_path "${workspace_root}")" || return 4
    if ! check_path_contained "${ws_canon}" "${target}"; then
      return 4
    fi
  fi
  if [[ -n "${user_root}" ]]; then
    local ur_canon
    ur_canon="$(canonical_path "${user_root}")" || return 4
    if ! check_path_contained "${ur_canon}" "${target}"; then
      echo "hint: ${target} points outside ${ur_canon}; edit your dotfile source directly instead" >&2
      return 4
    fi
  fi

  local existing_json="{}"
  local leading_comment=""
  local had_interior_comments=0

  if [[ -f "${target}" ]]; then
    leading_comment="$(LC_ALL=C awk '
      /^[[:space:]]*\/\// { print; next }
      /^[[:space:]]*$/ && header_done == 0 { print; next }
      { header_done=1; exit }
    ' "${target}")"

    if _jsonc_has_interior_comments "${target}"; then
      had_interior_comments=1
    fi

    if [[ -n "${user_root}" && "${had_interior_comments}" -eq 1 && "${allow_comment_loss}" -eq 0 ]]; then
      printf 'error: %s contains interior comments that would be lost by JSON merge (rerun with --allow-comment-loss to proceed after backup)\n' "${target}" >&2
      return 4
    fi

    existing_json="$(LC_ALL=C awk -f "${strip}" "${target}")" || {
      echo "error: failed to strip JSONC from ${target}" >&2
      return 1
    }
    if ! printf '%s\n' "${existing_json}" | jq -e -s 'length == 1 and (.[0] | type == "object")' >/dev/null 2>&1; then
      echo "error: existing ${target} is not a single valid JSON object after comment-strip" >&2
      return 1
    fi
  fi

  local new_doc
  if [[ "${new_doc_arg}" == "-" ]]; then
    new_doc="$(cat)"
  elif [[ -f "${new_doc_arg}" ]]; then
    new_doc="$(LC_ALL=C awk -f "${strip}" "${new_doc_arg}")" || return 1
  else
    new_doc="${new_doc_arg}"
  fi

  if ! printf '%s\n' "${new_doc}" | jq -e -s 'length == 1 and (.[0] | type == "object")' >/dev/null 2>&1; then
    echo "error: new document is not a single valid JSON object" >&2
    return 1
  fi

  local tmp_work
  tmp_work="$(mktemp -d "${TMPDIR:-/tmp}/vsp-jmerge.XXXXXX")" || return 1
  printf '%s\n' "${existing_json}" > "${tmp_work}/existing.json"
  printf '%s\n' "${new_doc}" > "${tmp_work}/new.json"

  local merged
  if ! merged="$(jq -s --argjson del "${delete_keys_json}" --argjson delp "${delete_paths_json}" '
    (.[0]
      | reduce $del[] as $k (.; del(.[$k]))
      | reduce $delp[] as $p (.; delpaths([$p]))
    ) * .[1]
  ' "${tmp_work}/existing.json" "${tmp_work}/new.json")"; then
    rm -rf "${tmp_work}"
    echo "error: jq merge failed" >&2
    return 1
  fi

  # Backup user-root files before every mutation (bounded last 5, 0600),
  # and backup comment-bearing workspace files on first lossy merge.
  if [[ -f "${target}" ]]; then
    if [[ -n "${user_root}" ]]; then
      if ! backup_user_file "${target}" 5; then
        rm -rf "${tmp_work}"
        return 1
      fi
    elif [[ "${had_interior_comments}" -eq 1 && ! -e "${target}.bak" && ! -L "${target}.bak" ]]; then
      if [[ -n "${workspace_root}" ]] && ! check_path_contained "${ws_canon}" "${target}.bak"; then
        rm -rf "${tmp_work}"
        return 4
      fi
      atomic_write_file --no-follow "${target}.bak" "${target}" 0600 || { rm -rf "${tmp_work}"; return 1; }
    elif [[ "${had_interior_comments}" -eq 1 && -L "${target}.bak" ]]; then
      if [[ -n "${workspace_root}" ]] && ! check_path_contained "${ws_canon}" "${target}.bak"; then
        rm -rf "${tmp_work}"
        return 4
      fi
    fi
  fi

  local staged="${tmp_work}/out.json"
  {
    if [[ -n "${leading_comment}" ]]; then
      printf '%s\n' "${leading_comment}"
    fi
    printf '%s\n' "${merged}" | jq .
  } > "${staged}"

  local rc=0
  if [[ -n "${user_root}" ]]; then
    atomic_write_file "${target}" "${staged}" 0600 || rc=$?
  else
    atomic_write_file --preserve-mode "${target}" "${staged}" 0644 || rc=$?
  fi
  rm -rf "${tmp_work}"
  return "${rc}"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  set -euo pipefail
  jsonc_merge "${@+"${@}"}"
fi

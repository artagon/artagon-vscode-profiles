# shellcheck shell=bash
# common.sh — Pure Bash 3.2 + POSIX foundation helpers for artagon-vscode-profiles.
# Sourced library: NEVER mutates caller shell options (set/shopt/IFS/trap).

# Resolve repository root reliably even when sourced from any working directory.
vsp_repo_root() {
  if [[ -n "${VSPCLI_ROOT:-}" ]]; then
    printf '%s\n' "${VSPCLI_ROOT}"
    return 0
  fi
  local dir
  dir="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  printf '%s\n' "${dir}"
}

# Lowercase ASCII string (Bash 3.2 safe; avoids Bash 4 lowercase expansion).
to_lower() {
  printf '%s' "${1}" | LC_ALL=C tr '[:upper:]' '[:lower:]'
}

# Portable SHA-256 hex digest of a file (or stdin when arg is "-").
sha256_for_file() {
  local target="${1}"
  local raw="" digest=""

  if [[ "${target}" != "-" ]]; then
    if [[ ! -r "${target}" || -d "${target}" ]]; then
      printf 'error: sha256_for_file: cannot read file: %s\n' "${target}" >&2
      return 1
    fi
  fi

  if command -v sha256sum >/dev/null 2>&1; then
    if [[ "${target}" == "-" ]]; then
      raw="$(sha256sum)" || return 1
    else
      raw="$(sha256sum < "${target}")" || return 1
    fi
  elif command -v shasum >/dev/null 2>&1; then
    if [[ "${target}" == "-" ]]; then
      raw="$(shasum -a 256)" || return 1
    else
      raw="$(shasum -a 256 < "${target}")" || return 1
    fi
  else
    echo "error: sha256sum or shasum is required" >&2
    return 1
  fi

  raw="${raw#"${raw%%[![:space:]]*}"}"
  digest="${raw%%[[:space:]]*}"
  digest="$(printf '%s' "${digest}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'error: sha256_for_file: invalid digest output for %s\n' "${target}" >&2
    return 1
  fi
  printf '%s\n' "${digest}"
  return 0
}

# Portable 8-hex-character random ID (zero Python dependency, zero SIGPIPE head).
rand_hex_8() {
  local id=""
  if command -v openssl >/dev/null 2>&1; then
    id="$(openssl rand -hex 4 2>/dev/null || true)"
  fi
  if [[ ! "${id}" =~ ^[0-9a-f]{8}$ ]]; then
    id="$(dd if=/dev/urandom bs=1 count=4 2>/dev/null | od -An -tx1 | tr -d '[:space:]')"
  fi
  if [[ ! "${id}" =~ ^[0-9a-f]{8}$ ]]; then
    echo "error: unable to generate 8-hex random ID" >&2
    return 1
  fi
  printf '%s\n' "${id}"
}

# Portable octal file mode reader (BSD macOS stat -f %Lp vs GNU stat -c %a).
file_mode() {
  local target="${1}"
  [[ -e "${target}" ]] || return 1
  local mode=""
  if mode="$(stat -f '%Lp' "${target}" 2>/dev/null)" && [[ "${mode}" =~ ^[0-7]{3,4}$ ]]; then
    printf '%s\n' "${mode}"
    return 0
  fi
  if mode="$(stat -c '%a' "${target}" 2>/dev/null)" && [[ "${mode}" =~ ^[0-7]{3,4}$ ]]; then
    printf '%s\n' "${mode}"
    return 0
  fi
  return 1
}

# Lexically collapse "." and ".." segments on an absolute path (pure Bash 3.2).
_normalize_lexical_path() {
  local raw="${1}"
  local rest="${raw#/}"
  local stack=()
  local seg
  while [[ -n "${rest}" ]]; do
    if [[ "${rest}" == */* ]]; then
      seg="${rest%%/*}"
      rest="${rest#*/}"
    else
      seg="${rest}"
      rest=""
    fi
    case "${seg}" in
      ""|".") continue ;;
      "..")
        if [[ "${#stack[@]}" -gt 0 ]]; then
          unset "stack[$(( ${#stack[@]} - 1 ))]"
          if [[ "${#stack[@]}" -gt 0 ]]; then
            stack=("${stack[@]}")
          else
            stack=()
          fi
        fi
        ;;
      *)
        stack+=("${seg}")
        ;;
    esac
  done
  if [[ "${#stack[@]}" -eq 0 ]]; then
    printf '/\n'
  else
    local out=""
    for seg in "${stack[@]}"; do
      out="${out}/${seg}"
    done
    printf '%s\n' "${out}"
  fi
}

# Pure Bash 3.2 + POSIX canonical path resolution with bounded 40-hop symlink loop.
# Resolves both existing and dangling intermediate symlinks AND collapses lexical `..`
# across non-existent path tails so symlink + `x/../../escape` can NEVER bypass checks.
canonical_path() {
  local p="${1}"
  local hops="${2:-0}"
  if [[ -z "${p}" ]]; then
    return 1
  fi
  # Make relative paths absolute against physical PWD.
  case "${p}" in
    /*) ;;
    *)  p="$(pwd -P)/${p}" ;;
  esac

  while [[ "${p}" != "/" && "${p}" == */ ]]; do
    p="${p%/}"
  done

  local dir base link
  while [[ -L "${p}" ]]; do
    hops=$((hops + 1))
    if [[ "${hops}" -gt 40 ]]; then
      echo "error: symlink loop detected (exceeded 40 hops): ${1}" >&2
      return 1
    fi
    dir="$(dirname -- "${p}")"
    if [[ -d "${dir}" ]]; then
      dir="$(CDPATH='' cd -P -- "${dir}" 2>/dev/null && pwd -P)" || return 1
    else
      dir="$(canonical_path "${dir}" "${hops}")" || return 1
    fi
    link="$(readlink "${p}")" || return 1
    case "${link}" in
      /*) p="${link}" ;;
      *)  p="${dir}/${link}" ;;
    esac
    while [[ "${p}" != "/" && "${p}" == */ ]]; do
      p="${p%/}"
    done
  done

  if [[ -d "${p}" ]]; then
    CDPATH='' cd -P -- "${p}" 2>/dev/null && pwd -P
    return $?
  fi

  dir="$(dirname -- "${p}")"
  base="$(basename -- "${p}")"
  if [[ -d "${dir}" ]]; then
    dir="$(CDPATH='' cd -P -- "${dir}" 2>/dev/null && pwd -P)" || return 1
    if [[ "${dir}" == "/" ]]; then
      _normalize_lexical_path "/${base}"
    else
      _normalize_lexical_path "${dir}/${base}"
    fi
    return 0
  fi

  # Parent does not exist yet: walk up to deepest existing dir OR dangling symlink,
  # then join and lexically normalize every `.` and `..` component!
  local tail="${base}"
  local cur="${dir}"
  while [[ "${cur}" != "/" && "${cur}" != "." && ! -d "${cur}" ]]; do
    if [[ -L "${cur}" ]]; then
      local resolved_cur
      resolved_cur="$(canonical_path "${cur}" "$((hops + 1))")" || return 1
      canonical_path "$(_normalize_lexical_path "${resolved_cur}/${tail}")" "$((hops + 1))"
      return $?
    fi
    tail="$(basename -- "${cur}")/${tail}"
    cur="$(dirname -- "${cur}")"
  done
  if [[ -d "${cur}" ]]; then
    cur="$(CDPATH='' cd -P -- "${cur}" 2>/dev/null && pwd -P)" || return 1
    local joined
    if [[ "${cur}" == "/" ]]; then
      joined="/${tail}"
    else
      joined="${cur}/${tail}"
    fi
    local norm
    norm="$(_normalize_lexical_path "${joined}")"
    # If normalization jumped back onto an existing path or symlink (e.g. x/../real), re-resolve!
    if [[ "${norm}" != "${p}" && ( -L "${norm}" || -e "${norm}" ) ]]; then
      canonical_path "${norm}" "$((hops + 1))"
      return $?
    fi
    printf '%s\n' "${norm}"
    return 0
  fi
  _normalize_lexical_path "${p}"
}

# Check that target (after canonical + lexical resolution) stays strictly inside root.
check_path_contained() {
  local root_canon="${1}"
  local target="${2}"
  local target_canon
  target_canon="$(canonical_path "${target}")" || return 1
  case "/${target_canon}/" in
    *"/../"*)
      echo "error: unnormalized parent traversal in resolved path: ${target} -> ${target_canon}" >&2
      return 1
      ;;
  esac
  if [[ "${root_canon}" == "/" ]]; then
    [[ "${target_canon}" == /* ]] && return 0
  elif [[ "${target_canon}" == "${root_canon}" || "${target_canon}" == "${root_canon}/"* ]]; then
    return 0
  fi
  echo "error: path escapes boundary '${root_canon}': ${target} -> ${target_canon}" >&2
  return 1
}

# XDG Base Directory Spec compliant resolvers (ignore relative paths).
resolve_xdg_config_home() {
  if [[ -n "${XDG_CONFIG_HOME:-}" && "${XDG_CONFIG_HOME}" == /* ]]; then
    printf '%s\n' "${XDG_CONFIG_HOME%/}"
  else
    printf '%s/.config\n' "${HOME%/}"
  fi
}

resolve_xdg_state_home() {
  if [[ -n "${XDG_STATE_HOME:-}" && "${XDG_STATE_HOME}" == /* ]]; then
    printf '%s\n' "${XDG_STATE_HOME%/}"
  else
    printf '%s/.local/state\n' "${HOME%/}"
  fi
}

# Resolve VS Code User directory (honors VSCODE_USER_DATA_DIR or VSCODE_USER_DIR).
detect_code_user_dir() {
  if [[ -n "${VSCODE_USER_DIR:-}" ]]; then
    printf '%s\n' "${VSCODE_USER_DIR%/}"
    return 0
  fi
  if [[ -n "${VSCODE_USER_DATA_DIR:-}" ]]; then
    printf '%s/User\n' "${VSCODE_USER_DATA_DIR%/}"
    return 0
  fi
  case "$(uname -s)" in
    Darwin)
      printf '%s/Library/Application Support/Code/User\n' "${HOME%/}"
      ;;
    Linux)
      printf '%s/Code/User\n' "$(resolve_xdg_config_home)"
      ;;
    *)
      printf '%s/.config/Code/User\n' "${HOME%/}"
      ;;
  esac
}

# Verify and emit `--user-data-dir <root>` (newline-separated tokens) when a custom
# VS Code store override is active, guaranteeing that `<root>/User` is the exact
# same physical directory as `detect_code_user_dir`.
# Sets globals: CODE_STORE_USER_DIR, CODE_STORE_DATA_ROOT.
resolve_code_store() {
  CODE_STORE_USER_DIR="$(detect_code_user_dir)"
  CODE_STORE_DATA_ROOT=""

  if [[ -n "${VSCODE_USER_DATA_DIR:-}" ]]; then
    CODE_STORE_DATA_ROOT="${VSCODE_USER_DATA_DIR%/}"
    local expected_user="${CODE_STORE_DATA_ROOT}/User"
    local c_expected c_actual
    c_expected="$(canonical_path "${expected_user}")" || return 1
    c_actual="$(canonical_path "${CODE_STORE_USER_DIR}")" || return 1
    if [[ "${c_expected}" != "${c_actual}" ]]; then
      echo "error: VSCODE_USER_DIR (${CODE_STORE_USER_DIR}) disagrees with VSCODE_USER_DATA_DIR/User (${expected_user})" >&2
      return 4
    fi
    return 0
  fi

  if [[ -n "${VSCODE_USER_DIR:-}" ]]; then
    local base parent
    base="$(basename -- "${CODE_STORE_USER_DIR}")"
    parent="$(dirname -- "${CODE_STORE_USER_DIR}")"
    if [[ "${base}" == "User" ]]; then
      CODE_STORE_DATA_ROOT="${parent}"
      return 0
    fi
    # Legacy VSCODE_USER_DIR without trailing /User: warn and refuse silent split-brain
    # when CLI mutation explicitly requires a data root unless caller permits direct mode.
    if [[ "${1:-}" == "--require-data-root" ]]; then
      echo "error: VSCODE_USER_DIR='${CODE_STORE_USER_DIR}' does not end in '/User'; set VSCODE_USER_DIR='<root>/User' or VSCODE_USER_DATA_DIR='<root>' so VS Code CLI and storage.json target the same store" >&2
      return 4
    fi
  fi
  return 0
}

# Prune timestamped backups `<file>.bak.*`, retaining the newest N (default 5).
prune_backups() {
  local target="${1}"
  local keep="${2:-5}"
  local dir base
  dir="$(dirname -- "${target}")"
  base="$(basename -- "${target}")"
  [[ -d "${dir}" ]] || return 0
  local backups=()
  local f
  while IFS= read -r f; do
    [[ -n "${f}" ]] && backups+=("${f}")
  done < <(LC_ALL=C find "${dir}" -maxdepth 1 -type f -name "${base}.bak.*" -print 2>/dev/null | LC_ALL=C sort)
  local total="${#backups[@]}"
  if [[ "${total}" -le "${keep}" ]]; then
    return 0
  fi
  local drop_count=$((total - keep))
  local idx=0
  while [[ "${idx}" -lt "${drop_count}" ]]; do
    rm -f "${backups[${idx}]}"
    idx=$((idx + 1))
  done
}

# Create a bounded timestamped 0600 backup before modifying a user-owned file.
backup_user_file() {
  local target="${1}"
  local keep="${2:-5}"
  [[ -f "${target}" ]] || return 0
  local ts bak
  ts="$(date -u '+%Y%m%dT%H%M%SZ')"
  bak="${target}.bak.${ts}.$$"
  cp -p "${target}" "${bak}" || return 1
  chmod 0600 "${bak}" 2>/dev/null || true
  prune_backups "${target}" "${keep}"
  return 0
}

# Atomic write helper.
# Usage: atomic_write_file [--no-follow] [--preserve-mode] <dest> <src_file> <default_mode>
atomic_write_file() {
  local no_follow=0
  local preserve_mode=0
  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --no-follow)     no_follow=1; shift ;;
      --preserve-mode) preserve_mode=1; shift ;;
      --)              shift; break ;;
      *)               break ;;
    esac
  done
  local dest="${1}"
  local src="${2}"
  local mode="${3:-0644}"

  local write_path="${dest}"
  if [[ "${no_follow}" -eq 0 && -L "${dest}" ]]; then
    write_path="$(canonical_path "${dest}")" || return 1
  fi

  local effective_mode="${mode}"
  if [[ "${preserve_mode}" -eq 1 && -f "${write_path}" ]]; then
    local existing_mode
    if existing_mode="$(file_mode "${write_path}" 2>/dev/null)"; then
      # Never widen permissions compared to existing file; preserve existing mode.
      effective_mode="${existing_mode}"
    fi
  fi

  local dir
  dir="$(dirname -- "${write_path}")"
  mkdir -p "${dir}" || return 1
  local tmp
  tmp="$(mktemp "${dir}/.tmp.atomic.XXXXXX")" || return 1
  if ! cp "${src}" "${tmp}"; then
    rm -f "${tmp}"
    return 1
  fi
  if ! chmod "${effective_mode}" "${tmp}"; then
    rm -f "${tmp}"
    return 1
  fi
  if ! mv -f "${tmp}" "${write_path}"; then
    rm -f "${tmp}"
    return 1
  fi
  return 0
}

# Populate lock metadata atomically (write host + started_at before atomic pid rename).
_write_lock_metadata() {
  local lock_dir="${1}"
  hostname 2>/dev/null > "${lock_dir}/host" || printf 'unknown\n' > "${lock_dir}/host" || return 1
  date -u '+%Y-%m-%dT%H:%M:%SZ' > "${lock_dir}/started_at" || return 1
  printf '%s\n' "$$" > "${lock_dir}/.pid.$$" || return 1
  mv -f "${lock_dir}/.pid.$$" "${lock_dir}/pid" || { rm -f "${lock_dir}/.pid.$$"; return 1; }
}

# ABA-free fail-closed directory lock (NEVER silently auto-evicts stale locks).
# Usage: acquire_lock <lock_dir> [timeout_secs] [--break-stale-lock]
acquire_lock() {
  local lock_dir="${1}"
  local timeout="${2:-10}"
  local break_stale=0
  if [[ "${3:-}" == "--break-stale-lock" || "${ARTAGON_VSCODE_BREAK_STALE_LOCK:-0}" == "1" ]]; then
    break_stale=1
  fi

  local parent
  parent="$(dirname -- "${lock_dir}")"
  mkdir -p "${parent}" || return 1

  local reclaim_guard="${lock_dir}.reclaim"
  local waited=0
  while true; do
    if mkdir "${lock_dir}" 2>/dev/null; then
      if ! _write_lock_metadata "${lock_dir}"; then
        rm -rf "${lock_dir}"
        return 1
      fi
      return 0
    fi

    if [[ "${break_stale}" -eq 1 && -f "${lock_dir}/pid" && -f "${lock_dir}/host" ]]; then
      # Serialize stale-lock reclamation atomically under a secondary mkdir reclaim guard
      # so two concurrent reclaimers can never race and move a newly acquired owner's lock.
      if mkdir "${reclaim_guard}" 2>/dev/null; then
        local acquired_after_reap=0
        local owner_pid owner_host owner_ts my_host
        if [[ -f "${lock_dir}/pid" && -f "${lock_dir}/host" ]]; then
          owner_pid="$(cat "${lock_dir}/pid" 2>/dev/null || true)"
          owner_host="$(cat "${lock_dir}/host" 2>/dev/null || true)"
          owner_ts="$(cat "${lock_dir}/started_at" 2>/dev/null || true)"
          my_host="$(hostname 2>/dev/null || printf 'unknown')"
          # Only reap confirmed-dead local-host PID; verify `ps -p $$` works so a broken `ps`
          # never causes a false-dead reclamation.
          if [[ -n "${owner_host}" && "${owner_host}" == "${my_host}" && "${owner_pid}" =~ ^[0-9]+$ ]] \
             && ps -p "$$" >/dev/null 2>&1 \
             && ! ps -p "${owner_pid}" >/dev/null 2>&1; then
            # Re-verify lock ownership AFTER confirming owner_pid is dead. A dead PID cannot
            # release the lock, so if metadata still matches, no live owner replaced it between
            # our initial read and the ps check.
            local recheck_pid recheck_host recheck_ts
            recheck_pid="$(cat "${lock_dir}/pid" 2>/dev/null || true)"
            recheck_host="$(cat "${lock_dir}/host" 2>/dev/null || true)"
            recheck_ts="$(cat "${lock_dir}/started_at" 2>/dev/null || true)"
            if [[ "${recheck_pid}" == "${owner_pid}" && "${recheck_host}" == "${owner_host}" && "${recheck_ts}" == "${owner_ts}" ]]; then
              local reap_dir="${lock_dir}.reap.$$"
              if mv "${lock_dir}" "${reap_dir}" 2>/dev/null; then
                local reaped_pid
                reaped_pid="$(cat "${reap_dir}/pid" 2>/dev/null || true)"
                if [[ "${reaped_pid}" == "${owner_pid}" ]]; then
                  echo "lock: removing confirmed-dead stale lock (pid=${owner_pid}): ${lock_dir}" >&2
                  rm -rf "${reap_dir}"
                  # Claim the lock and write live metadata BEFORE releasing reclaim_guard
                  # so no waiting reclaimer can ever observe or disturb an uninitialized lock.
                  if mkdir "${lock_dir}" 2>/dev/null; then
                    if _write_lock_metadata "${lock_dir}"; then
                      acquired_after_reap=1
                    else
                      rm -rf "${lock_dir}"
                    fi
                  fi
                else
                  # Unexpected owner raced via external removal; restore cleanly without deleting.
                  mv "${reap_dir}" "${lock_dir}" 2>/dev/null || true
                fi
              fi
            fi
          fi
        fi
        rmdir "${reclaim_guard}" 2>/dev/null || true
        if [[ "${acquired_after_reap}" -eq 1 ]]; then
          return 0
        fi
        break_stale=0
      fi
    fi

    if [[ "${waited}" -ge "${timeout}" ]]; then
      local lpid lhost lts
      lpid="$(cat "${lock_dir}/pid" 2>/dev/null || printf 'unknown')"
      lhost="$(cat "${lock_dir}/host" 2>/dev/null || printf 'unknown')"
      lts="$(cat "${lock_dir}/started_at" 2>/dev/null || printf 'unknown')"
      printf 'error: lock busy after %ss: %s (pid=%s host=%s started=%s)\n' \
        "${timeout}" "${lock_dir}" "${lpid}" "${lhost}" "${lts}" >&2
      printf 'hint: if pid %s is dead, rerun with --break-stale-lock or remove %s manually\n' \
        "${lpid}" "${lock_dir}" >&2
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
}

# Verify PID ownership before removing lock directory.
release_lock() {
  local lock_dir="${1}"
  [[ -d "${lock_dir}" ]] || return 0
  local owner_pid
  owner_pid="$(cat "${lock_dir}/pid" 2>/dev/null || true)"
  if [[ "${owner_pid}" == "$$" ]]; then
    rm -rf "${lock_dir}"
  fi
}

# Detect whether VS Code desktop app is currently running (skip when tests opt out).
is_vscode_running() {
  if [[ "${ARTAGON_VSCODE_TEST_RUNNING:-}" == "1" ]]; then
    return 0
  fi
  if [[ "${ARTAGON_VSCODE_TEST_RUNNING:-}" == "0" ]]; then
    return 1
  fi
  case "$(uname -s)" in
    Darwin)
      local ps_out=""
      if ! ps_out="$(ps -eo args= 2>/dev/null)"; then
        echo "warning: ps process scan failed; treating VS Code as running (fail-closed)" >&2
        return 0
      fi
      # Drain ps output via awk so large process tables never trigger SIGPIPE (141) under pipefail.
      # VS Code 1.147+ on macOS runs as Contents/MacOS/Code (older releases used Contents/MacOS/Electron);
      # match absolute paths including spaces in parent directories while rejecting unrelated commands
      # referencing the path in argv[1+], and exclude transient CLI out/cli.js invocations.
      printf '%s\n' "${ps_out}" | LC_ALL=C awk '
        {
          line = $0
          sub(/^[[:space:]]+/, "", line)
          if (line ~ /out\/cli\.js/) next
          if (match(line, /\/Visual Studio Code(\.app)?\/Contents\/MacOS\/(Code|Electron)([[:space:]]|$)/)) {
            prefix = substr(line, 1, RSTART - 1)
            if (prefix == "" || (prefix ~ /^\// && prefix !~ /[[:space:]]\//)) {
              found = 1
            }
          }
        }
        END { exit (found ? 0 : 1) }
      ' && return 0
      return 1
      ;;
    Linux)
      local ps_out=""
      if ! ps_out="$(ps -eo args= 2>/dev/null)"; then
        echo "warning: ps process scan failed; treating VS Code as running (fail-closed)" >&2
        return 0
      fi
      # Anchor argv[0] to .../code and exclude transient CLI (out/cli.js) and remote-cli invocations.
      printf '%s\n' "${ps_out}" | LC_ALL=C awk '
        /^[[:space:]]*[^[:space:]]*\/code([[:space:]]|$)/ && !/out\/cli\.js/ && !/\/remote-cli\// { found = 1 }
        END { exit (found ? 0 : 1) }
      ' && return 0
      return 1
      ;;
    *)
      return 1
      ;;
  esac
}

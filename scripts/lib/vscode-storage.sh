# shellcheck shell=bash
# vscode-storage.sh — Safe, locked VS Code globalStorage/storage.json inspection & registration.
# Sourced library: pure Bash 3.2 + jq, NEVER mutates caller shell options.

VSCODE_PROFILE_LOC_RE='^[A-Za-z0-9_-]+$'

# Read-only lookup: print the single location ID for profile display name, or empty if absent.
# Returns 1 if storage.json is malformed, contains duplicate names, or has an invalid location.
vscode_storage_find_profile_id() {
  local user_dir="${1}"
  local profile_name="${2}"
  local storage="${user_dir}/globalStorage/storage.json"
  if [[ ! -f "${storage}" ]]; then
    return 0
  fi

  if ! jq -e -s 'length == 1 and (.[0] | type == "object")' "${storage}" >/dev/null 2>&1; then
    printf 'error: malformed storage.json: %s\n' "${storage}" >&2
    return 1
  fi

  local count
  count="$(jq -r --arg name "${profile_name}" '
    [(.userDataProfiles // [])[] | select(.name == $name)] | length
  ' "${storage}")" || return 1

  if [[ "${count}" -gt 1 ]]; then
    printf 'error: duplicate userDataProfiles entries named %q in %s\n' "${profile_name}" "${storage}" >&2
    return 1
  fi
  if [[ "${count}" -eq 0 ]]; then
    return 0
  fi

  local loc
  loc="$(jq -r --arg name "${profile_name}" '
    (.userDataProfiles // [])[] | select(.name == $name) | .location // empty
  ' "${storage}")" || return 1

  if [[ ! "${loc}" =~ ${VSCODE_PROFILE_LOC_RE} ]]; then
    printf 'error: invalid profile location %q for %q in %s\n' "${loc}" "${profile_name}" "${storage}" >&2
    return 1
  fi
  printf '%s\n' "${loc}"
  return 0
}

# List all registered profile display names in storage.json (sorted, one per line).
vscode_storage_list_profiles() {
  local user_dir="${1}"
  local storage="${user_dir}/globalStorage/storage.json"
  [[ -f "${storage}" ]] || return 0
  jq -e -s 'length == 1 and (.[0] | type == "object")' "${storage}" >/dev/null 2>&1 || return 1
  jq -r '(.userDataProfiles // [])[] | .name // empty' "${storage}" | LC_ALL=C sort -u
}

# Register (or reuse) a profile in storage.json safely under an ABA-free lock with:
#   - double-checked VS Code running guard (fails closed unless --force)
#   - pre-read & pre-publish SHA-256 CAS check against independent writers
#   - duplicate name & invalid location rejection
#   - unused 8-hex ID reservation checking profiles/<id> absence
#   - bounded timestamped 0600 backup & atomic 0600 write
# Prints the resolved profile location ID on stdout.
vscode_storage_ensure_profile() {
  local force=0
  local break_stale=""
  while [[ $# -gt 0 ]]; do
    case "${1}" in
      --force)            force=1; shift ;;
      --break-stale-lock) break_stale="--break-stale-lock"; shift ;;
      --)                 shift; break ;;
      -*)                 printf 'error: vscode_storage_ensure_profile unknown option %q\n' "${1}" >&2; return 4 ;;
      *)                  break ;;
    esac
  done
  local user_dir="${1}"
  local profile_name="${2}"

  if [[ "${force}" -eq 0 ]] && is_vscode_running; then
    echo "error: VS Code appears to be running; mutating storage.json while VS Code is open risks lost updates (close VS Code or pass --force to accept the risk)" >&2
    return 1
  fi

  local storage_dir="${user_dir}/globalStorage"
  local storage="${storage_dir}/storage.json"
  local lock_dir="${storage_dir}/.storage.json.lock"
  mkdir -p "${storage_dir}" || return 1

  acquire_lock "${lock_dir}" 10 ${break_stale:+"${break_stale}"} || return 1

  # Re-check VS Code process under lock.
  if [[ "${force}" -eq 0 ]] && is_vscode_running; then
    release_lock "${lock_dir}"
    echo "error: VS Code started while waiting for storage.json lock; aborting to prevent lost update (close VS Code or pass --force)" >&2
    return 1
  fi

  local pre_hash="ABSENT"
  if [[ -f "${storage}" ]]; then
    pre_hash="$(sha256_for_file "${storage}")" || { release_lock "${lock_dir}"; return 1; }
  fi

  local existing_id=""
  if ! existing_id="$(vscode_storage_find_profile_id "${user_dir}" "${profile_name}")"; then
    release_lock "${lock_dir}"
    return 1
  fi

  if [[ -n "${existing_id}" ]]; then
    if ! mkdir -p "${user_dir}/profiles/${existing_id}"; then
      release_lock "${lock_dir}"
      return 1
    fi
    chmod 0700 "${user_dir}/profiles/${existing_id}" 2>/dev/null || true
    release_lock "${lock_dir}"
    printf '%s\n' "${existing_id}"
    return 0
  fi

  # Reserve an unused 8-hex ID not present in storage.json or on disk.
  local new_id=""
  local tries=0
  while [[ "${tries}" -lt 20 ]]; do
    tries=$((tries + 1))
    local cand
    cand="$(rand_hex_8)" || { release_lock "${lock_dir}"; return 1; }
    if [[ -e "${user_dir}/profiles/${cand}" ]]; then
      continue
    fi
    if [[ -f "${storage}" ]] && jq -e --arg id "${cand}" '(.userDataProfiles // []) | any(.location == $id)' "${storage}" >/dev/null 2>&1; then
      continue
    fi
    new_id="${cand}"
    break
  done

  if [[ -z "${new_id}" ]]; then
    release_lock "${lock_dir}"
    echo "error: unable to reserve unused profile ID" >&2
    return 1
  fi

  local tmp_file
  tmp_file="$(mktemp "${storage_dir}/.storage.tmp.XXXXXX")" || { release_lock "${lock_dir}"; return 1; }
  if [[ -f "${storage}" ]]; then
    jq --arg name "${profile_name}" --arg id "${new_id}" '
      .userDataProfiles = ((.userDataProfiles // []) + [{name: $name, location: $id}])
    ' "${storage}" > "${tmp_file}" || { rm -f "${tmp_file}"; release_lock "${lock_dir}"; return 1; }
  else
    jq -n --arg name "${profile_name}" --arg id "${new_id}" '
      {userDataProfiles: [{name: $name, location: $id}]}
    ' > "${tmp_file}" || { rm -f "${tmp_file}"; release_lock "${lock_dir}"; return 1; }
  fi

  if [[ -f "${storage}" ]]; then
    backup_user_file "${storage}" 5 || { rm -f "${tmp_file}"; release_lock "${lock_dir}"; return 1; }
  fi

  if ! mkdir -p "${user_dir}/profiles/${new_id}"; then
    rm -f "${tmp_file}"
    release_lock "${lock_dir}"
    return 1
  fi
  chmod 0700 "${user_dir}/profiles/${new_id}" 2>/dev/null || true

  # Pre-publish CAS check immediately before atomic publish (guards against independent writers even under --force).
  local now_hash="ABSENT"
  if [[ -f "${storage}" ]]; then
    now_hash="$(sha256_for_file "${storage}")" || { rm -f "${tmp_file}"; release_lock "${lock_dir}"; return 1; }
  fi
  if [[ "${now_hash}" != "${pre_hash}" ]]; then
    rm -f "${tmp_file}"
    release_lock "${lock_dir}"
    echo "error: storage.json changed concurrently by an independent writer; aborting to avoid lost update" >&2
    return 1
  fi

  local rc=0
  atomic_write_file "${storage}" "${tmp_file}" 0600 || rc=$?
  rm -f "${tmp_file}"
  release_lock "${lock_dir}"
  if [[ "${rc}" -ne 0 ]]; then
    return "${rc}"
  fi
  printf '%s\n' "${new_id}"
  return 0
}

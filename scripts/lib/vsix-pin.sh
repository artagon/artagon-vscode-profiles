# shellcheck shell=bash
# Pin manifest helper backed by catalog/extension-pins.json.
# Sourced library: pure Bash 3.2 + jq + curl, NEVER mutates caller shell options.
#
# Case-insensitive lookup, strict official https:// Marketplace host + redirect validation,
# fail-closed SHA-256 verification without live Marketplace fallback.
# Relax URL host/protocol checks ONLY when ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=1
# (for offline localhost/file BATS test fixtures).

_vsix_pin_sha256() {
  local path="${1}"
  local raw="" digest=""
  if [[ ! -r "${path}" || -d "${path}" ]]; then
    printf 'vsix-pin: cannot read file for sha256 verification: %s\n' "${path}" >&2
    return 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    raw="$(sha256sum < "${path}")" || return 1
  elif command -v shasum >/dev/null 2>&1; then
    raw="$(shasum -a 256 < "${path}")" || return 1
  else
    echo "vsix-pin: need sha256sum or shasum for VSIX verification" >&2
    return 1
  fi
  raw="${raw#"${raw%%[![:space:]]*}"}"
  digest="${raw%%[[:space:]]*}"
  digest="$(printf '%s' "${digest}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'vsix-pin: invalid sha256 digest output for %s\n' "${path}" >&2
    return 1
  fi
  printf '%s\n' "${digest}"
  return 0
}

pins_file_path() {
  local repo_root="${1}"
  if [[ -n "${ARTAGON_VSCODE_PINS_FILE:-}" ]]; then
    printf '%s\n' "${ARTAGON_VSCODE_PINS_FILE}"
    return 0
  fi
  printf '%s\n' "${repo_root}/catalog/extension-pins.json"
}

bypass_pins_active() {
  if [[ "${ARTAGON_VSCODE_BYPASS_PINS:-0}" != "1" ]]; then
    return 1
  fi
  if [[ "${_artagon_pins_bypass_warned:-0}" != "1" ]]; then
    echo "vsix-pin: bypassing pin manifest (ARTAGON_VSCODE_BYPASS_PINS=1)" >&2
    _artagon_pins_bypass_warned=1
  fi
  return 0
}

# Validate that a VSIX URL uses https:// and an official Marketplace host with NO
# userinfo (`@`), query/fragment authority spoofing (`?`, `#`), or invalid labels.
_vsix_pin_validate_url() {
  local url="${1}"
  if [[ "${ARTAGON_VSCODE_ALLOW_TEST_PIN_URL:-0}" == "1" ]]; then
    case "${url}" in
      https://*|http://127.0.0.1*|http://localhost*|file://*) return 0 ;;
      *)
        printf 'vsix-pin: disallowed test URL scheme: %s\n' "${url}" >&2
        return 1
        ;;
    esac
  fi

  local url_re='^https://([A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+)(:[0-9]+)?(/.*)?$'
  if [[ ! "${url}" =~ ${url_re} ]]; then
    printf 'vsix-pin: rejected malformed or non-https vsix_url: %s\n' "${url}" >&2
    return 1
  fi

  local host_lc
  host_lc="$(printf '%s' "${BASH_REMATCH[1]}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  local mp_re='^(marketplace\.visualstudio\.com|[a-z0-9-]+(\.[a-z0-9-]+)*\.gallery(cdn)?\.vsassets\.io)$'
  if [[ "${host_lc}" =~ ${mp_re} ]]; then
    return 0
  fi
  printf 'vsix-pin: rejected non-Marketplace host %q in %s\n' "${host_lc}" "${url}" >&2
  return 1
}

# lookup_pin <id> <pins_file>
# Case-insensitive lookup over .pins; prints version, sha256, vsix_url on 3 lines.
# Fails closed on:
#   - missing or unreadable pins manifest (unless ARTAGON_VSCODE_ALLOW_UNLOCKED=1)
#   - duplicate case-colliding keys in .pins
#   - malformed pin entry missing sha256 or vsix_url
#   - pinRequired=true when id is absent
lookup_pin() {
  local id="${1}"
  local pins_file="${2}"
  if [[ ! -f "${pins_file}" || ! -r "${pins_file}" ]]; then
    if [[ "${ARTAGON_VSCODE_ALLOW_UNLOCKED:-0}" == "1" ]]; then
      printf 'vsix-pin: WARNING: pins manifest missing or unreadable (%s); proceeding unlocked (ARTAGON_VSCODE_ALLOW_UNLOCKED=1)\n' "${pins_file}" >&2
      return 0
    fi
    printf 'vsix-pin: pins manifest missing or unreadable: %s (set ARTAGON_VSCODE_ALLOW_UNLOCKED=1 to bypass)\n' "${pins_file}" >&2
    return 1
  fi

  local pin_required
  pin_required="$(jq -r '.pinRequired // false' "${pins_file}")" || return 1

  local match_count
  match_count="$(jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | (.pins // {}) | to_entries | map(select((.key | ascii_downcase) == $k)) | length
  ' "${pins_file}")" || return 1

  if [[ "${match_count}" -gt 1 ]]; then
    printf 'vsix-pin: duplicate case-colliding pin entries for %s in %s\n' "${id}" "${pins_file}" >&2
    return 1
  fi

  if [[ "${match_count}" -eq 0 ]]; then
    if [[ "${pin_required}" == "true" ]]; then
      printf 'vsix-pin: pinRequired=true in %s but no pin exists for %s\n' "${pins_file}" "${id}" >&2
      return 1
    fi
    return 0
  fi

  local ver sha url
  ver="$(jq -r --arg id "${id}" '($id | ascii_downcase) as $k | (.pins // {}) | to_entries | map(select((.key | ascii_downcase) == $k))[0].value.version // ""' "${pins_file}")" || return 1
  sha="$(jq -r --arg id "${id}" '($id | ascii_downcase) as $k | (.pins // {}) | to_entries | map(select((.key | ascii_downcase) == $k))[0].value.sha256 // ""' "${pins_file}")" || return 1
  url="$(jq -r --arg id "${id}" '($id | ascii_downcase) as $k | (.pins // {}) | to_entries | map(select((.key | ascii_downcase) == $k))[0].value.vsix_url // ""' "${pins_file}")" || return 1

  if [[ -z "${sha}" || -z "${url}" ]]; then
    printf 'vsix-pin: malformed pin entry for %s in %s (missing sha256 or vsix_url)\n' "${id}" "${pins_file}" >&2
    return 1
  fi

  printf '%s\n%s\n%s\n' "${ver}" "${sha}" "${url}"
}

# fetch_and_verify_vsix <id> <vsix_url> <expected_sha256> <out_path>
# When VSCODE_VSIX_CACHE_DIR is set (non-empty), copies cached VSIX into private 0600 temp
# file BEFORE computing SHA-256 (zero TOCTOU), falls through to curl on miss/eviction/mismatch,
# and atomically populates the 0700 content-addressed cache on verified download.
fetch_and_verify_vsix() {
  local id="${1}"
  local url="${2}"
  local expected
  expected="$(printf '%s' "${3}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  local out_path="${4}"

  local id_lc
  id_lc="$(printf '%s' "${id}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  local id_re='^[a-z0-9][a-z0-9._-]*\.[a-z0-9][a-z0-9._-]*$'
  if [[ ! "${id_lc}" =~ ${id_re} || "${id_lc}" == *..* ]]; then
    printf 'vsix-pin: invalid extension id: %q\n' "${id}" >&2
    return 1
  fi

  if [[ ! "${expected}" =~ ^[0-9a-f]{64}$ ]]; then
    printf 'vsix-pin: invalid sha256 digest for %s: %q\n' "${id}" "${expected}" >&2
    return 1
  fi

  _vsix_pin_validate_url "${url}" || return 1

  local out_dir
  out_dir="$(dirname -- "${out_path}")"
  mkdir -p -- "${out_dir}" || return 1

  # Content-addressed local cache check: copy to private 0600 temp FIRST, then hash private copy.
  local cache_dir="${VSCODE_VSIX_CACHE_DIR:-}"
  local cache_file=""
  if [[ -n "${cache_dir}" && ! -L "${cache_dir}" ]]; then
    cache_file="${cache_dir}/${id_lc}-${expected}.vsix"
    if [[ -f "${cache_file}" && ! -L "${cache_file}" ]]; then
      local ctmp=""
      ctmp="$(mktemp "${out_dir}/.vsix-cache.XXXXXX" 2>/dev/null || true)"
      if [[ -n "${ctmp}" ]]; then
        chmod -- 0600 "${ctmp}" 2>/dev/null || true
        if cp -- "${cache_file}" "${ctmp}" 2>/dev/null && chmod -- 0600 "${ctmp}" 2>/dev/null; then
          local cached_sha=""
          cached_sha="$(_vsix_pin_sha256 "${ctmp}" 2>/dev/null || true)"
          if [[ "${cached_sha}" == "${expected}" ]]; then
            if [[ ! -d "${out_path}" ]] && mv -f -- "${ctmp}" "${out_path}"; then
              return 0
            fi
            rm -f -- "${ctmp}"
            printf 'vsix-pin: failed to publish verified cached VSIX for %s to %s\n' "${id}" "${out_path}" >&2
            return 1
          else
            # Completed read mismatched expected SHA-256: evict non-symlink corrupt cache file and re-fetch.
            if [[ -f "${cache_file}" && ! -L "${cache_file}" ]]; then
              rm -f -- "${cache_file}"
            fi
          fi
        fi
        rm -f -- "${ctmp}"
      fi
    fi
  fi

  if ! command -v curl >/dev/null 2>&1; then
    echo "vsix-pin: curl required to verify pinned VSIXs (set ARTAGON_VSCODE_BYPASS_PINS=1 to disable pinning)" >&2
    return 1
  fi

  local tmp
  tmp="$(mktemp "${out_dir}/.vsix-pin.XXXXXX")" || return 1
  chmod -- 0600 "${tmp}" 2>/dev/null || true

  local curl_rc=0
  local eff_url=""
  if [[ "${ARTAGON_VSCODE_ALLOW_TEST_PIN_URL:-0}" == "1" ]]; then
    eff_url="$(curl -fsSL --max-time 60 -w '%{url_effective}' -o "${tmp}" -- "${url}")" || curl_rc=$?
  else
    eff_url="$(curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --max-time 60 -w '%{url_effective}' -o "${tmp}" -- "${url}")" || curl_rc=$?
  fi
  if [[ "${curl_rc}" -ne 0 ]]; then
    rm -f -- "${tmp}"
    printf 'vsix-pin: failed to fetch %s\n  url: %s\n' "${id}" "${url}" >&2
    return 1
  fi

  # Re-verify final effective URL after any 3xx redirects so off-host redirects cannot bypass allowlist.
  if [[ -n "${eff_url}" && "${ARTAGON_VSCODE_ALLOW_TEST_PIN_URL:-0}" != "1" ]]; then
    if ! _vsix_pin_validate_url "${eff_url}"; then
      rm -f -- "${tmp}"
      printf 'vsix-pin: rejected off-host redirect for %s -> %s\n' "${id}" "${eff_url}" >&2
      return 1
    fi
  fi

  local actual
  actual="$(_vsix_pin_sha256 "${tmp}")" || { rm -f -- "${tmp}"; return 1; }
  if [[ "${actual}" != "${expected}" ]]; then
    rm -f -- "${tmp}"
    printf 'vsix-pin: sha256 mismatch for %s\n  expected: %s\n  actual:   %s\n  url: %s\n' \
      "${id}" "${expected}" "${actual}" "${url}" >&2
    return 1
  fi

  chmod -- 0600 "${tmp}" 2>/dev/null || true
  if [[ -n "${cache_file}" && ! -L "${cache_dir}" ]]; then
    mkdir -p -- "${cache_dir}" 2>/dev/null || true
    chmod -- 0700 "${cache_dir}" 2>/dev/null || true
    if [[ -d "${cache_dir}" && ! -L "${cache_dir}" && ! -L "${cache_file}" && ! -d "${cache_file}" ]]; then
      local csave
      csave="$(mktemp "${cache_dir}/.vsix-save.XXXXXX" 2>/dev/null || true)"
      if [[ -n "${csave}" ]]; then
        if cp -- "${tmp}" "${csave}" 2>/dev/null && chmod -- 0600 "${csave}" 2>/dev/null; then
          mv -f -- "${csave}" "${cache_file}" 2>/dev/null || rm -f -- "${csave}"
        else
          rm -f -- "${csave}"
        fi
      fi
    fi
  fi
  if [[ -d "${out_path}" ]] || ! mv -f -- "${tmp}" "${out_path}"; then
    rm -f -- "${tmp}"
    printf 'vsix-pin: failed to publish verified VSIX for %s to %s\n' "${id}" "${out_path}" >&2
    return 1
  fi
  return 0
}

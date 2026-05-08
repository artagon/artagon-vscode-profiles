# shellcheck shell=bash
# Pin manifest helper. Source from scripts that hand IDs to
# `code --install-extension`. Looks up `_catalog/extension-pins.json`
# (or override via ARTAGON_VSCODE_PINS_FILE) and, when an ID is pinned,
# fetches its VSIX from the recorded URL and verifies the recorded
# sha256 before install.
#
# Schema: { "version": 1, "pins": { "<publisher>.<name>": { "version": "...", "sha256": "<hex>", "vsix_url": "https://..." }, ... } }
#
# Bypass: ARTAGON_VSCODE_BYPASS_PINS=1 disables pin lookup entirely
# (live marketplace path for every install). Emits a one-time stderr
# notice per process.
#
# Failure mode is closed: on hash mismatch or fetch failure, the
# install is REFUSED — no live-marketplace fallback. That fallback
# would defeat the entire point of pinning.

## _vsix_pin_hasher
## Echoes the available sha256 hasher command-line ("sha256sum" or
## "shasum -a 256"); returns non-zero with a stderr error if neither
## is on PATH.
_vsix_pin_hasher() {
  if command -v sha256sum >/dev/null 2>&1; then
    printf '%s\n' 'sha256sum'
    return 0
  fi
  if command -v shasum >/dev/null 2>&1; then
    printf '%s\n' 'shasum -a 256'
    return 0
  fi
  echo "vsix-pin: need sha256sum or shasum for VSIX verification" >&2
  return 1
}

## _vsix_pin_sha256 <path>
## Echoes the lowercase hex sha256 of <path>. Wraps the available hasher.
_vsix_pin_sha256() {
  local path="$1"
  local hasher
  hasher=$(_vsix_pin_hasher) || return 1
  # word-split intentional: hasher is a 1-or-2-token command line.
  # shellcheck disable=SC2086
  ${hasher} "${path}" | awk '{print $1}'
}

## pins_file_path
## Resolves the active pin manifest path: ARTAGON_VSCODE_PINS_FILE
## override or the repo-default _catalog/extension-pins.json.
## Caller passes the script's resolved repo root as $1 (so the helper
## doesn't need to walk up from its own location, which is brittle
## when sourced).
pins_file_path() {
  local repo_root="$1"
  if [ -n "${ARTAGON_VSCODE_PINS_FILE:-}" ]; then
    printf '%s\n' "${ARTAGON_VSCODE_PINS_FILE}"
    return 0
  fi
  printf '%s\n' "${repo_root}/_catalog/extension-pins.json"
}

## bypass_pins_active
## Returns 0 if ARTAGON_VSCODE_BYPASS_PINS=1, 1 otherwise. Emits a
## one-time stderr notice per process when bypass is active.
bypass_pins_active() {
  if [ "${ARTAGON_VSCODE_BYPASS_PINS:-0}" != "1" ]; then
    return 1
  fi
  if [ "${_artagon_pins_bypass_warned:-0}" != "1" ]; then
    echo "vsix-pin: bypassing pin manifest (ARTAGON_VSCODE_BYPASS_PINS=1)" >&2
    _artagon_pins_bypass_warned=1
  fi
  return 0
}

## lookup_pin <id> <pins_file>
## Prints the pin's version, sha256, and vsix_url to stdout
## (newline-separated, in that order) when <id> is listed and
## <pins_file> exists; prints nothing and returns 0 otherwise.
## Returns non-zero only on jq parse failure (manifest is malformed).
lookup_pin() {
  local id="$1"
  local pins_file="$2"
  if [ ! -f "${pins_file}" ]; then
    return 0
  fi
  local entry
  entry=$(jq -r --arg id "${id}" '.pins[$id] // empty | "\(.version // "")\n\(.sha256 // "")\n\(.vsix_url // "")"' "${pins_file}") || return 1
  if [ -z "${entry}" ] || [ "${entry}" = $'\n\n' ]; then
    return 0
  fi
  printf '%s\n' "${entry}"
}

## fetch_and_verify_vsix <id> <vsix_url> <expected_sha256> <out_path>
## Fetches the VSIX from <vsix_url> via curl (-fsSL), verifies the
## sha256 matches <expected_sha256>, and on success leaves the
## verified file at <out_path>. Atomic write via a sibling temp file.
## Returns non-zero with a stderr error on any failure.
fetch_and_verify_vsix() {
  local id="$1"
  local url="$2"
  local expected="$3"
  local out_path="$4"

  if ! command -v curl >/dev/null 2>&1; then
    echo "vsix-pin: curl required to verify pinned VSIXs (set ARTAGON_VSCODE_BYPASS_PINS=1 to disable pinning)" >&2
    return 1
  fi

  local out_dir
  out_dir=$(dirname "${out_path}")
  mkdir -p "${out_dir}"

  local tmp="${out_path}.tmp.$$"
  if ! curl -fsSL --max-time 60 -o "${tmp}" "${url}"; then
    rm -f "${tmp}"
    printf 'vsix-pin: failed to fetch %s\n  url: %s\n' "${id}" "${url}" >&2
    return 1
  fi

  local actual
  actual=$(_vsix_pin_sha256 "${tmp}") || { rm -f "${tmp}"; return 1; }
  if [ "${actual}" != "${expected}" ]; then
    rm -f "${tmp}"
    printf 'vsix-pin: sha256 mismatch for %s\n  expected: %s\n  actual:   %s\n  url: %s\n' \
      "${id}" "${expected}" "${actual}" "${url}" >&2
    return 1
  fi

  mv -f "${tmp}" "${out_path}"
  return 0
}

# shellcheck shell=bash
# Shared marketplace-extension-id validation backed by catalog/publishers.json.
# Sourced library: pure Bash 3.2 + jq, NEVER mutates caller shell options.
#
# Two gates:
#   1. EXTENSION_ID_REGEX — shape (publisher.extension), denies shell
#      metacharacters, path separators, whitespace, quote characters.
#   2. is_allowed_publisher — supply-chain gate backed dynamically by
#      catalog/publishers.json. Override:
#      ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 (warns, accepts).
EXTENSION_ID_REGEX='^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$'

_ext_id_publishers_file() {
  if [[ -n "${ARTAGON_VSCODE_PUBLISHERS_FILE:-}" ]]; then
    printf '%s\n' "${ARTAGON_VSCODE_PUBLISHERS_FILE}"
    return 0
  fi
  local root
  if [[ -n "${VSPCLI_ROOT:-}" ]]; then
    root="${VSPCLI_ROOT}"
  else
    root="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)"
  fi
  printf '%s/catalog/publishers.json\n' "${root}"
}

validate_extension_id_syntax() {
  local id="$1"
  if [[ ! "${id}" =~ ${EXTENSION_ID_REGEX} ]]; then
    printf 'Error: rejected extension id %q (must match %s)\n' "${id}" "${EXTENSION_ID_REGEX}" >&2
    return 1
  fi
  return 0
}

is_allowed_publisher() {
  local publisher="$1"
  local lc
  lc="$(printf '%s' "${publisher}" | LC_ALL=C tr '[:upper:]' '[:lower:]')"
  local pub_file
  pub_file="$(_ext_id_publishers_file)"
  if [[ ! -f "${pub_file}" ]]; then
    printf 'Error: missing publisher allowlist: %s\n' "${pub_file}" >&2
    return 1
  fi
  jq -e --arg p "${lc}" '(.publishers // []) | map(ascii_downcase) | index($p) != null' "${pub_file}" >/dev/null 2>&1
}

validate_extension_id() {
  local id="$1"
  validate_extension_id_syntax "${id}" || return 1
  local publisher="${id%%.*}"
  if ! is_allowed_publisher "${publisher}"; then
    if [[ "${ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER:-0}" == "1" ]]; then
      if [[ "${_artagon_unknown_pub_warned:-0}" != "1" ]]; then
        printf 'Warning: accepting unknown publisher(s) (ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1)\n' >&2
        _artagon_unknown_pub_warned=1
      fi
      printf 'Warning: unknown publisher %q in extension id %q\n' "${publisher}" "${id}" >&2
      return 0
    fi
    printf 'Error: rejected extension id %q: publisher %q not in allowlist (set ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 to override; update catalog/publishers.json to add)\n' "${id}" "${publisher}" >&2
    return 1
  fi
  return 0
}

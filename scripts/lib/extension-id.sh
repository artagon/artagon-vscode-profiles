# shellcheck shell=bash
# Shared marketplace-extension-id validation. Source from any script that hands
# user-controlled identifiers to `code --install-extension`.
#
# Two gates:
#   1. EXTENSION_ID_REGEX — shape (publisher.extension), denies shell
#      metacharacters, path separators, whitespace, quote characters.
#   2. is_allowed_publisher — supply-chain gate. Refuses any publisher not
#      in the curated allowlist. Defends against typosquatted publishers
#      (e.g. microsfot.python). Override:
#      ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 (warns, accepts).
EXTENSION_ID_REGEX='^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$'

# Publisher allowlist: every publisher referenced by this repo's
# profiles/*/extensions.json. Audit with:
#   jq -r '[.[].identifier.id | split(".")[0]] | unique[]' profiles/*/extensions.json | sort -u
# Add a publisher here in the SAME PR that introduces an extension from it.
# Compared case-insensitively (marketplace publishers are case-insensitive;
# `github` and `GitHub` both resolve to the same publisher).
is_allowed_publisher() {
  local publisher="$1"
  local lc
  lc=$(printf '%s' "${publisher}" | tr '[:upper:]' '[:lower:]')
  # Single flat allowlist (deduplicated). Comments describe which
  # category each publisher belongs to, but the case statement itself
  # stays flat to avoid SC2221/SC2222 (duplicate patterns across
  # branches; first match wins, later branches silently die).
  case "${lc}" in
    # themes / icon themes
    akamud|catppuccin|dracula-theme|enkia|monokai|pkief|sdras|vscode-icons-team|zhuangtongfa) return 0 ;;
    # web/astro stack
    astro-build|bradlc|christian-kohler|clinyong|dbaeumer|ecmel|esbenp|naumovs|pranaygp|stylelint) return 0 ;;
    # java/jvm stack
    fawwazfirdaus|gabrielbb|pivotal|pmd|redhat|richardwillis|shengchen|teabyii|vscjava) return 0 ;;
    # rust stack
    dustypomerleau|fill-labs|jscearcy|rust-lang) return 0 ;;
    # c/c++ stack
    llvm-vs-code-extensions|ms-vscode|notskm|vadimcn) return 0 ;;
    # cross-stack: tamasfe (TOML), twxs (CMake)
    tamasfe|twxs) return 0 ;;
    # AI / Copilot / GitHub (one publisher segment, both casings)
    continue|github|sourcegraph) return 0 ;;
    # general productivity (cross-stack)
    barbosshack|eamodio|editorconfig|eg2|foxundermoon|jeff-hykin|liviuschera|miguelsolorio|mikestead|ms-azuretools|ms-vsliveshare|mutantdino|mvllow|naco-siren|oderwat|ryanluker|shirdows|sonarsource|streetsidesoftware|timonwong|usernamehw) return 0 ;;
  esac
  return 1
}

validate_extension_id() {
  local id="$1"
  if [[ ! "$id" =~ $EXTENSION_ID_REGEX ]]; then
    printf 'Error: rejected extension id %q (must match %s)\n' "$id" "$EXTENSION_ID_REGEX" >&2
    return 1
  fi
  local publisher="${id%%.*}"
  if ! is_allowed_publisher "${publisher}"; then
    if [ "${ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER:-0}" = "1" ]; then
      if [ "${_artagon_unknown_pub_warned:-0}" != "1" ]; then
        printf 'Warning: accepting unknown publisher(s) (ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1)\n' >&2
        _artagon_unknown_pub_warned=1
      fi
      printf 'Warning: unknown publisher %q in extension id %q\n' "${publisher}" "${id}" >&2
      return 0
    fi
    printf 'Error: rejected extension id %q: publisher %q not in allowlist (set ARTAGON_VSCODE_TRUST_UNKNOWN_PUBLISHER=1 to override; edit scripts/lib/extension-id.sh to add)\n' "$id" "${publisher}" >&2
    return 1
  fi
  return 0
}

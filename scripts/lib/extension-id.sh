# Shared marketplace-extension-id validation. Source from any script that hands
# user-controlled identifiers to `code --install-extension`.
#
# Pattern matches publisher.extension where each segment starts with [A-Za-z0-9]
# and otherwise contains [A-Za-z0-9._-]. Verified against all 64 unique IDs in
# this repo at proposal time. Denies shell metacharacters, path separators,
# whitespace, and quote characters.
EXTENSION_ID_REGEX='^[a-zA-Z0-9][a-zA-Z0-9._-]*\.[a-zA-Z0-9][a-zA-Z0-9._-]*$'

validate_extension_id() {
  local id="$1"
  if [[ ! "$id" =~ $EXTENSION_ID_REGEX ]]; then
    printf 'Error: rejected extension id %q (must match %s)\n' "$id" "$EXTENSION_ID_REGEX" >&2
    return 1
  fi
  return 0
}

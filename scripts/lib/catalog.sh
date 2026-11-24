# shellcheck shell=bash
# catalog.sh — Dynamic queries over catalog/{extensions,publishers,defaults,setting-scopes}.json
# plus shared parsed-config autonomy deny-list checker.
# Sourced library: pure Bash 3.2 + jq, NEVER mutates caller shell options.

catalog_dir() {
  local root="${1:-$(vsp_repo_root)}"
  printf '%s/catalog\n' "${root}"
}

catalog_extensions_file() {
  printf '%s/extensions.json\n' "$(catalog_dir "${1:-}")"
}

catalog_publishers_file() {
  printf '%s/publishers.json\n' "$(catalog_dir "${1:-}")"
}

catalog_defaults_file() {
  printf '%s/defaults.json\n' "$(catalog_dir "${1:-}")"
}

catalog_setting_scopes_file() {
  printf '%s/setting-scopes.json\n' "$(catalog_dir "${1:-}")"
}

# Print status of extension ID (active|superseded|removed|compromised|retired), or empty if unknown.
catalog_extension_status() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 1
  jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | .extensions[$k].status // empty
  ' "${ext_file}"
}

# Print replacedBy target for a superseded extension ID (or empty).
catalog_extension_replaced_by() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 1
  jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | .extensions[$k].replacedBy // empty
  ' "${ext_file}"
}

# Print extension group from catalog/extensions.json, defaulting to "General".
catalog_extension_group() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  if [[ ! -f "${ext_file}" ]]; then
    printf 'General\n'
    return 0
  fi
  jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | .extensions[$k].group // "General"
  ' "${ext_file}"
}

# Return 0 if ID is either a top-level key in catalog/extensions.json OR a declared packMember.
catalog_is_known_for_uninstall() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 1
  jq -e --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | (.extensions | has($k))
      or ([.extensions[].packMembers[]? | ascii_downcase] | index($k) != null)
  ' "${ext_file}" >/dev/null 2>&1
}

# Print newline-separated packMembers[] for an extension ID.
catalog_pack_members() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 0
  jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | .extensions[$k].packMembers[]?
  ' "${ext_file}"
}

# Print newline-separated extension IDs with a given status.
catalog_ids_by_status() {
  local status="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 1
  jq -r --arg s "${status}" '
    .extensions | to_entries[] | select(.value.status == $s) | .key
  ' "${ext_file}" | LC_ALL=C sort
}

# Print newline-separated themeLabels[] for an extension ID (case-insensitive ID match).
catalog_theme_labels() {
  local id="$1"
  local root="${2:-$(vsp_repo_root)}"
  local ext_file
  ext_file="$(catalog_extensions_file "${root}")"
  [[ -f "${ext_file}" ]] || return 0
  jq -r --arg id "${id}" '
    ($id | ascii_downcase) as $k
    | .extensions[$k].themeLabels[]?
  ' "${ext_file}"
}

# Print default capability IDs (one per line) from catalog/defaults.json.
catalog_default_capabilities() {
  local root="${1:-$(vsp_repo_root)}"
  local def_file
  def_file="$(catalog_defaults_file "${root}")"
  [[ -f "${def_file}" ]] || return 1
  jq -r '.capabilities[]?' "${def_file}"
}

# Print default UX preset name from catalog/defaults.json.
catalog_default_ux() {
  local root="${1:-$(vsp_repo_root)}"
  local def_file
  def_file="$(catalog_defaults_file "${root}")"
  [[ -f "${def_file}" ]] || { printf 'crisp\n'; return 0; }
  jq -r '.ux // "crisp"' "${def_file}"
}

# Shared parsed-config autonomy/exec deny-list check over a JSON object on stdin.
# Prints matching violation descriptions (one per line) and returns 1 if ANY violation exists.
# Used by BOTH scripts/validate-json.sh and scripts/import-profile.sh.
check_autonomy_denylist_json() {
  local label="${1:-stdin}"
  local hits
  hits="$(jq -r '
    def inspect($p):
      if type == "object" then
        to_entries[] as $e
        | ($p + [$e.key]) as $np
        | ($np | join(".")) as $path
        | (
            if ($e.key | test("^chat\\.tools\\..*autoapprov"; "i")) then
              "forbidden autonomy key: \($path)"
            elif ($e.key == "claudeCode.allowDangerouslySkipPermissions" and $e.value == true) then
              "forbidden autonomy setting: \($path)=true"
            elif ($e.key == "claudeCode.initialPermissionMode" and $e.value == "bypassPermissions") then
              "forbidden autonomy setting: \($path)=bypassPermissions"
            elif ($e.key == "geminicodeassist.agentYoloMode" and $e.value == true) then
              "forbidden autonomy setting: \($path)=true"
            elif ($e.key == "claudeCode.claudeProcessWrapper" or $e.key == "claudeCode.environmentVariables" or $e.key == "antigravity.serverArgs" or $e.key == "antigravity.releaseBaseUrl" or $e.key == "antigravity.caCertificates" or $e.key == "chatgpt.cliExecutable") then
              "forbidden exec-override key: \($path)"
            else empty end
          ),
          ($e.value | inspect($np))
      elif type == "array" then
        .[] | inspect($p)
      elif type == "string" then
        if test("bypassPermissions|--dangerously-skip-permissions") then
          "forbidden bypassPermissions value at: \($p | join("."))"
        else empty end
      else empty end;
    inspect([])
  ')" || return 2

  if [[ -n "${hits}" ]]; then
    local line
    while IFS= read -r line; do
      [[ -n "${line}" ]] || continue
      printf 'Error: autonomy deny-list violation in %s: %s\n' "${label}" "${line}" >&2
    done <<< "${hits}"
    return 1
  fi
  return 0
}

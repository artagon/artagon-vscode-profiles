#!/usr/bin/env bash
# validate-json.sh — Comprehensive pure Bash 3.2 + jq repository & security validator.
# Enforces:
#   1. Single-document strict .json/.code-profile and JSONC validation with expected root types
#   2. Catalog integrity: lowercase keys, publisher==prefix, 5 lifecycle statuses, active-module
#      parity, exact LC_ALL=C sorted catalog/publishers.json, pre-v2 bundle completeness
#   3. Module DAG acyclicity, unique IDs, valid symmetric conflicts[], existing sibling files
#   4. Apple jq 1.7.1-safe recursive scalar_leaves + object-vs-leaf prefix collision check across
#      jointly selectable modules within the same scope (excluding mutually exclusive UX presets)
#   5. Workspace scope lints + catalog/setting-scopes.json type/enum/scope enforcement
#   6. Tracked forbidden paths (agents, .agents/settings.local.json) + parsed-config autonomy
#      deny-list (^chat\.tools\..*autoApprove$, bypassPermissions, claudeProcessWrapper, etc.)
#   7. Protected compiled profile baseline invariants + zero $schema/@extends
#   8. exports/SHA256SUMS digest verification + broken repo symlink check
#   9. Rust docs policy sync check (apply-rust-policy.sh --check)
set -euo pipefail

: "${TMPDIR:=/tmp}"

if ! command -v jq >/dev/null 2>&1; then
  echo "validate-json: jq is required but not found in PATH" >&2
  exit 1
fi

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB_DIR="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB_DIR}/common.sh"
# shellcheck source=scripts/lib/catalog.sh
source "${LIB_DIR}/catalog.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB_DIR}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB_DIR}/compiler.sh"
# shellcheck source=scripts/lib/vsix-pin.sh
source "${LIB_DIR}/vsix-pin.sh"

STRIP_AWK="${LIB_DIR}/jsonc-strip.awk"
FAILED=0
CHECKED=0
TMP_WORK="$(mktemp -d "${TMPDIR}/vsp-validate.XXXXXX")"
trap 'rm -rf "${TMP_WORK}"' EXIT

err() {
  printf 'validate-json: FAILED: %s\n' "${*}" >&2
  FAILED=1
}

# ---------------------------------------------------------------------------
# 1. Single-document JSON & JSONC syntax + expected root type validation
# ---------------------------------------------------------------------------
validate_file_doc() {
  local file="${1}"
  local expected_type="${2}" # object | array
  CHECKED=$((CHECKED + 1))

  case "${file}" in
    *.jsonc)
      local stripped=""
      if ! stripped="$(LC_ALL=C awk -f "${STRIP_AWK}" "${file}" 2>"${TMP_WORK}/awk.err")"; then
        err "${file} (JSONC strip failed: $(cat "${TMP_WORK}/awk.err"))"
        return 1
      fi
      if ! printf '%s\n' "${stripped}" | jq -e -s --arg t "${expected_type}" 'length == 1 and (.[0] | type == $t)' >/dev/null 2>"${TMP_WORK}/jq.err"; then
        err "${file} (expected single JSON ${expected_type}: $(cat "${TMP_WORK}/jq.err"))"
        return 1
      fi
      ;;
    *)
      if ! jq -e -s --arg t "${expected_type}" 'length == 1 and (.[0] | type == $t)' "${file}" >/dev/null 2>"${TMP_WORK}/jq.err"; then
        err "${file} (expected strict single JSON ${expected_type}: $(cat "${TMP_WORK}/jq.err"))"
        return 1
      fi
      ;;
  esac
  return 0
}

for dir in catalog modules profiles _merged exports; do
  [[ -d "${ROOT}/${dir}" ]] || continue
  while IFS= read -r f; do
    [[ -n "${f}" ]] || continue
    case "${f}" in
      */extensions.json)
        if [[ "${f}" == "${ROOT}/catalog/extensions.json" ]]; then
          validate_file_doc "${f}" "object" || true
        else
          validate_file_doc "${f}" "array" || true
        fi
        ;;
      *.json|*.jsonc|*.code-profile)
        validate_file_doc "${f}" "object" || true
        ;;
    esac
  done < <(LC_ALL=C find "${ROOT}/${dir}" -type f \( -name '*.json' -o -name '*.jsonc' -o -name '*.code-profile' \) -print | LC_ALL=C sort)
done

if [[ "${CHECKED}" -eq 0 ]]; then
  echo "validate-json: no JSON/JSONC files found to validate" >&2
  exit 0
fi

# Bail early if core catalog files failed syntax checks.
if [[ "${FAILED}" -ne 0 ]]; then
  echo "validate-json: syntax validation failed" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# 2. Discover modules, verify manifests, sibling files, symmetric conflicts & DAG
# ---------------------------------------------------------------------------
MOD_IDS=()
MOD_KINDS=()
MOD_DIRS=()

if ! mod_table="$(discover_modules "${ROOT}")"; then
  err "module discovery failed"
else
  while IFS=$'\t' read -r mid mkind mdir; do
    [[ -n "${mid}" ]] || continue
    MOD_IDS+=("${mid}")
    MOD_KINDS+=("${mkind}")
    MOD_DIRS+=("${mdir}")

    manifest="${mdir}/module.json"
    for sf_key in '.settings.workspace' '.settings.profile' '.tasks'; do
      sf="$(jq -r "${sf_key} // empty" "${manifest}")"
      if [[ -n "${sf}" && ! -f "${mdir}/${sf}" ]]; then
        err "${manifest}: references missing file ${sf}"
      fi
    done
  done <<< "${mod_table}"
fi

# Verify every requires/defaultWith/conflicts target exists and conflicts are symmetric.
if [[ "${#MOD_IDS[@]}" -gt 0 ]]; then
  for idx in "${!MOD_IDS[@]}"; do
    mid="${MOD_IDS[${idx}]}"
    mdir="${MOD_DIRS[${idx}]}"
    manifest="${mdir}/module.json"

    while IFS= read -r ref; do
      [[ -n "${ref}" ]] || continue
      module_dir_for_id "${ROOT}" "${ref}" >/dev/null 2>&1 || err "${manifest}: unknown module reference '${ref}'"
    done < <(jq -r '((.requires // []) + (.defaultWith // []) + (.conflicts // []))[]' "${manifest}")

    while IFS= read -r conf; do
      [[ -n "${conf}" ]] || continue
      if cdir="$(module_dir_for_id "${ROOT}" "${conf}" 2>/dev/null)"; then
        if ! jq -e --arg me "${mid}" '(.conflicts // []) | index($me) != null' "${cdir}/module.json" >/dev/null 2>&1; then
          err "${manifest}: asymmetric conflict ('${mid}' conflicts with '${conf}', but '${conf}' does not declare '${mid}')"
        fi
      fi
    done < <(jq -r '.conflicts[]?' "${manifest}")

    # Check single-module DAG resolution (catches cycles immediately).
    if ! resolve_modules "${ROOT}" --roots "${mid}" >/dev/null 2>"${TMP_WORK}/dag.err"; then
      err "module '${mid}' DAG resolution failed: $(cat "${TMP_WORK}/dag.err")"
    fi
  done
fi

# ---------------------------------------------------------------------------
# 3. Catalog integrity & exact active-module publisher allowlist parity
# ---------------------------------------------------------------------------
CAT_EXT="${ROOT}/catalog/extensions.json"
CAT_PUB="${ROOT}/catalog/publishers.json"
CAT_PINS="${ROOT}/catalog/extension-pins.json"
CAT_SCOPES="${ROOT}/catalog/setting-scopes.json"

if [[ -f "${CAT_EXT}" && -f "${CAT_PUB}" ]]; then
  # Check every catalog key is lowercase, publisher == prefix, valid status, replacedBy active.
  cat_issues="$(jq -r '
    .extensions as $all
    | $all | to_entries[]
    | .key as $k | .value as $v
    | ($k | split(".")[0]) as $prefix
    | (if $k != ($k | ascii_downcase) then "non-lowercase catalog key: \($k)" else empty end),
      (if $v.publisher != $prefix then "publisher prefix mismatch for \($k): publisher=\($v.publisher) expected=\($prefix)" else empty end),
      (if (["active","superseded","removed","compromised","retired"] | index($v.status)) == null then "invalid status for \($k): \($v.status)" else empty end),
      (if $v.status == "superseded" and (($v.replacedBy // "") == "" or ($all[$v.replacedBy].status // "") != "active") then "superseded \($k) missing active replacedBy" else empty end)
  ' "${CAT_EXT}")"
  if [[ -n "${cat_issues}" ]]; then
    while IFS= read -r line; do
      [[ -n "${line}" ]] && err "catalog/extensions.json: ${line}"
    done <<< "${cat_issues}"
  fi

  # Collect all extension IDs referenced in module workspace[] + profile[].
  MOD_MANIFESTS=()
  for mdir in "${MOD_DIRS[@]+"${MOD_DIRS[@]}"}"; do
    MOD_MANIFESTS+=("${mdir}/module.json")
  done

  ACTIVE_MOD_IDS_JSON='[]'
  if [[ "${#MOD_MANIFESTS[@]}" -gt 0 ]]; then
    ACTIVE_MOD_IDS_JSON="$(jq -s '[ .[] | ((.extensions.workspace // []) + (.extensions.profile // []))[] | ascii_downcase ] | unique | sort' "${MOD_MANIFESTS[@]}")"
  fi

  # Check: every active module ext exists with status=="active", every active catalog row is used,
  # AND every unwanted[] extension declared by modules is cataloged.
  UNWANTED_MOD_IDS_JSON='[]'
  if [[ "${#MOD_MANIFESTS[@]}" -gt 0 ]]; then
    UNWANTED_MOD_IDS_JSON="$(jq -s '[ .[] | (.extensions.unwanted // [])[] | ascii_downcase ] | unique | sort' "${MOD_MANIFESTS[@]}")"
  fi

  parity_err="$(jq -r --argjson used "${ACTIVE_MOD_IDS_JSON}" --argjson unw "${UNWANTED_MOD_IDS_JSON}" '
    .extensions as $all
    | ([ $used[] | select(($all[.].status // "") != "active") | "module extension not active in catalog: \(.)" ])
      + ([ $all | to_entries[] | select(.value.status == "active") | .key as $k | select(($used | index($k)) == null) | "orphan active catalog entry unreferenced by any module: \($k)" ])
      + ([ $unw[] | . as $u | select(($all | has($u)) | not) | "module unwanted extension missing from catalog: \($u)" ])
    | .[]
  ' "${CAT_EXT}")"
  if [[ -n "${parity_err}" ]]; then
    while IFS= read -r line; do
      [[ -n "${line}" ]] && err "${line}"
    done <<< "${parity_err}"
  fi

  # Check catalog/publishers.json equals exact LC_ALL=C sorted unique lowercase active module publishers.
  expected_pubs="$(printf '%s\n' "${ACTIVE_MOD_IDS_JSON}" | jq -r '.[] | split(".")[0]' | LC_ALL=C sort -u)"
  actual_pubs="$(jq -r '.publishers[]' "${CAT_PUB}")"
  actual_pubs_sorted="$(printf '%s\n' "${actual_pubs}" | LC_ALL=C tr '[:upper:]' '[:lower:]' | LC_ALL=C sort -u)"
  if [[ "${actual_pubs}" != "${actual_pubs_sorted}" ]]; then
    err "catalog/publishers.json is not strictly lowercase, unique, and LC_ALL=C sorted"
  fi
  if [[ "${actual_pubs_sorted}" != "${expected_pubs}" ]]; then
    diff_out="$(diff -u <(printf '%s\n' "${expected_pubs}") <(printf '%s\n' "${actual_pubs_sorted}") || true)"
    err "catalog/publishers.json drifted from active module publishers:\n${diff_out}"
  fi

  # Check pins file has lowercase unique keys, 64-hex sha256, and strict official Marketplace https URLs.
  if [[ -f "${CAT_PINS}" ]]; then
    pin_errs="$(jq -r '
      (.pins // {}) | to_entries[]
      | .key as $k | .value as $v
      | (if $k != ($k | ascii_downcase) then "non-lowercase pin key: \($k)" else empty end),
        (if (($v.sha256 // "") | test("^[0-9a-f]{64}$") | not) then "invalid sha256 for pin \($k)" else empty end)
    ' "${CAT_PINS}")"
    if [[ -n "${pin_errs}" ]]; then
      while IFS= read -r line; do
        [[ -n "${line}" ]] && err "catalog/extension-pins.json: ${line}"
      done <<< "${pin_errs}"
    fi
    while IFS=$'\t' read -r pk pu; do
      [[ -n "${pk}" ]] || continue
      if ! ARTAGON_VSCODE_ALLOW_TEST_PIN_URL=0 _vsix_pin_validate_url "${pu}" >/dev/null 2>&1; then
        err "catalog/extension-pins.json: invalid or non-Marketplace vsix_url for pin ${pk}: ${pu}"
      fi
    done < <(jq -r '(.pins // {}) | to_entries[] | "\(.key)\t\(.value.vsix_url // "")"' "${CAT_PINS}")
  fi
fi

# ---------------------------------------------------------------------------
# 4. Workspace scope lints + catalog/setting-scopes.json type/enum/scope check
#    + parsed-config autonomy deny-list across all modules/profiles/bundles
# ---------------------------------------------------------------------------
UX_KEYS_JSON='[]'
if [[ -f "${ROOT}/modules/ux/crisp/settings.profile.jsonc" ]]; then
  UX_KEYS_JSON="$(compiler_ux_managed_keys_json "${ROOT}")"
fi

check_settings_fragment() {
  local file="${1}"
  local scope="${2}" # workspace | profile
  local mod_kind="${3}"

  local parsed
  parsed="$(compiler_parse_jsonc_object "${ROOT}" "${file}")" || { err "${file}: failed parse"; return 1; }

  # Autonomy deny-list on parsed JSON.
  if ! printf '%s\n' "${parsed}" | check_autonomy_denylist_json "${file}"; then
    FAILED=1
  fi

  if [[ "${scope}" == "workspace" ]]; then
    # Workspace-specific forbidden rules:
    local ws_errs
    ws_errs="$(printf '%s\n' "${parsed}" | jq -r --argjson ux "${UX_KEYS_JSON}" '
      (if has("editor.defaultFormatter") then "top-level editor.defaultFormatter forbidden in workspace settings (scope inside [lang])" else empty end),
      (keys[] | select(. as $k | $ux | index($k) != null) | "personal UX key forbidden in workspace settings: \(.)"),
      (."files.watcherExclude" // {} | keys[] | select(endswith("/**") | not) | "files.watcherExclude missing trailing /** : \(.)"),
      (tostring | if test("/opt/homebrew/|/usr/local/opt/|/Users/|\\$\\{command:jenv\\.javaHome\\}") then "hardcoded host path or broken jenv command found" else empty end)
    ')"
    if [[ -n "${ws_errs}" ]]; then
      while IFS= read -r line; do
        [[ -n "${line}" ]] && err "${file}: ${line}"
      done <<< "${ws_errs}"
    fi
  fi

  # Validate against catalog/setting-scopes.json (forbiddenKeys + type/enum/scope).
  if [[ -f "${CAT_SCOPES}" ]]; then
    local sc_errs
    sc_errs="$(printf '%s\n' "${parsed}" | jq -r \
      --slurpfile reg "${CAT_SCOPES}" \
      --arg scope "${scope}" \
      --arg mkind "${mod_kind}" '
      $reg[0] as $r
      | to_entries[] as $e
      | (if (($r.forbiddenKeys // []) | index($e.key) != null) then
           "forbidden/deprecated VS Code 1.147.0 key: \($e.key)"
         else empty end),
        (
          ($r.settings[$e.key]) as $spec
          | if $spec == null then empty
            else
              (if $scope == "workspace" and ($spec.scope == "machine" or $spec.scope == "profile" or $spec.scope == "application") then
                 "key \($e.key) has scope=\($spec.scope) and is forbidden in settings.workspace.jsonc"
               elif $spec.scope == "application" and ($spec.allowInCompiledProfile == false) then
                 "key \($e.key) is application-scoped and disallowed in compiled modules"
               else empty end),
              (
                ($e.value | type) as $vt
                | if $spec.type == "integer" then
                    if $vt != "number" or (($e.value | floor) != $e.value) then
                      "key \($e.key) expected integer, got \($vt)"
                    else empty end
                  elif ($spec.allowBoolean == true and $vt == "boolean") then empty
                  elif $vt != $spec.type then
                    "key \($e.key) expected \($spec.type), got \($vt)"
                  else empty end
              ),
              (if ($spec.enum // null) != null and (($e.value | type) == "string") and (($spec.enum | index($e.value)) == null) then
                 "key \($e.key) value \($e.value | tojson) not in allowed enum \($spec.enum | tojson)"
               else empty end),
              (if ($spec.valueType // "") == "integer" and (($e.value | type) == "object") then
                 ($e.value | to_entries[] | select((.value | type) != "number" or ((.value | floor) != .value)) | "key \($e.key).\(.key) must be integer")
               else empty end)
            end
        )
    ')"
    if [[ -n "${sc_errs}" ]]; then
      while IFS= read -r line; do
        [[ -n "${line}" ]] && err "${file}: ${line}"
      done <<< "${sc_errs}"
    fi
  fi
}

if [[ "${#MOD_IDS[@]}" -gt 0 ]]; then
  for idx in "${!MOD_IDS[@]}"; do
    mid="${MOD_IDS[${idx}]}"
    mkind="${MOD_KINDS[${idx}]}"
    mdir="${MOD_DIRS[${idx}]}"
    ws_f="$(jq -r '.settings.workspace // empty' "${mdir}/module.json")"
    pr_f="$(jq -r '.settings.profile // empty' "${mdir}/module.json")"
    [[ -n "${ws_f}" && -f "${mdir}/${ws_f}" ]] && check_settings_fragment "${mdir}/${ws_f}" "workspace" "${mkind}"
    [[ -n "${pr_f}" && -f "${mdir}/${pr_f}" ]] && check_settings_fragment "${mdir}/${pr_f}" "profile" "${mkind}"
  done
fi

# ---------------------------------------------------------------------------
# 5. Apple jq 1.7.1-safe recursive scalar_leaves + object-vs-leaf collision check
#    across jointly selectable modules within the same scope (excludes UX pairs).
# ---------------------------------------------------------------------------
MOD_SETTINGS_BUNDLE="${TMP_WORK}/mod-settings.json"
{
  printf '[\n'
  first_item=1
  if [[ "${#MOD_IDS[@]}" -gt 0 ]]; then
    for idx in "${!MOD_IDS[@]}"; do
      mid="${MOD_IDS[${idx}]}"
      mkind="${MOD_KINDS[${idx}]}"
      mdir="${MOD_DIRS[${idx}]}"
      ws_f="$(jq -r '.settings.workspace // empty' "${mdir}/module.json")"
      pr_f="$(jq -r '.settings.profile // empty' "${mdir}/module.json")"
      ws_obj='{}'
      pr_obj='{}'
      [[ -n "${ws_f}" && -f "${mdir}/${ws_f}" ]] && ws_obj="$(compiler_parse_jsonc_object "${ROOT}" "${mdir}/${ws_f}")"
      [[ -n "${pr_f}" && -f "${mdir}/${pr_f}" ]] && pr_obj="$(compiler_parse_jsonc_object "${ROOT}" "${mdir}/${pr_f}")"
      reqs="$(jq -c '.requires // []' "${mdir}/module.json")"
      confs="$(jq -c '.conflicts // []' "${mdir}/module.json")"
      [[ "${first_item}" -eq 0 ]] && printf ',\n'
      first_item=0
      jq -n \
        --arg id "${mid}" \
        --arg kind "${mkind}" \
        --argjson reqs "${reqs}" \
        --argjson confs "${confs}" \
        --argjson ws "${ws_obj}" \
        --argjson pr "${pr_obj}" \
        '{ id: $id, kind: $kind, requires: $reqs, conflicts: $confs, ws: $ws, pr: $pr }'
    done
  fi
  printf '\n]\n'
} > "${MOD_SETTINGS_BUNDLE}"

collision_errs="$(jq -r --argjson union_keys "${COMPILER_UNION_KEYS_JSON}" '
  def scalar_leaves:
    def walk($p):
      if type == "object" then
        to_entries[] | .key as $k | .value | walk($p + [$k])
      elif type == "array" then
        if ($p | length == 1) and ($union_keys | index($p[0]) != null) then empty
        else {path: $p, value: .}
        end
      else
        {path: $p, value: .}
      end;
    [walk([])];

  def obj_paths:
    def walk($p):
      if type == "object" then
        (if ($p | length) > 0 then $p else empty end),
        (to_entries[] | .key as $k | .value | walk($p + [$k]))
      else empty end;
    [walk([])];

  # Build transitive requires map for every module id.
  (reduce .[] as $m ({}; .[$m.id] = $m.requires)) as $direct
  | def ancestors($id):
      def step($frontier; $seen):
        ([ $frontier[] | ($direct[.] // [])[] | select(. as $n | $seen | index($n) == null) ] | unique) as $next
        | if ($next | length) == 0 then $seen
          else step($next; ($seen + $next) | unique) end;
      step([$id]; []);

  . as $mods
  | [ range(0; length) as $i
      | range($i + 1; length) as $j
      | $mods[$i] as $a
      | $mods[$j] as $b
      # Skip mutually exclusive UX presets and explicitly conflicting pairs.
      | select(($a.kind == "ux" and $b.kind == "ux") | not)
      | select((($a.conflicts | index($b.id)) != null) or (($b.conflicts | index($a.id)) != null) | not)
      # Skip pairs where either transitively requires the other.
      | (ancestors($a.id)) as $anc_a
      | (ancestors($b.id)) as $anc_b
      | select((($anc_a | index($b.id)) != null) or (($anc_b | index($a.id)) != null) | not)
      # Compare within each scope independently (ws vs ws, pr vs pr).
      | ("ws", "pr") as $sc
      | ($a[$sc]) as $da
      | ($b[$sc]) as $db
      | ($da | scalar_leaves) as $la
      | ($db | scalar_leaves) as $lb
      | ($da | obj_paths) as $oa
      | ($db | obj_paths) as $ob
      | (
          # Exact leaf path value/type mismatch
          [ $la[] as $xa
            | $lb[] as $xb
            | select($xa.path == $xb.path and ($xa.value != $xb.value or ($xa.value | type) != ($xb.value | type)))
            | "sibling collision (\($sc)) between \($a.id) and \($b.id) at \($xa.path | join(".")): \($xa.value | tojson) vs \($xb.value | tojson)"
          ]
          +
          # Object-vs-leaf prefix conflict (use any(. == path), NOT index(array)!)
          [ $la[] as $xa
            | select($ob | any(. == $xa.path))
            | "object-vs-leaf collision (\($sc)) between \($a.id) (leaf) and \($b.id) (object) at \($xa.path | join("."))"
          ]
          +
          [ $lb[] as $xb
            | select($oa | any(. == $xb.path))
            | "object-vs-leaf collision (\($sc)) between \($a.id) (object) and \($b.id) (leaf) at \($xb.path | join("."))"
          ]
        )[]
    ] | .[]
' "${MOD_SETTINGS_BUNDLE}")"

if [[ -n "${collision_errs}" ]]; then
  while IFS= read -r line; do
    [[ -n "${line}" ]] && err "${line}"
  done <<< "${collision_errs}"
fi

# ---------------------------------------------------------------------------
# 6. Check profile recipes, _merged/*.json, exports/*.code-profile, & tracked security
# ---------------------------------------------------------------------------
while IFS= read -r prof; do
  [[ -n "${prof}" ]] || continue
  recipe="${ROOT}/profiles/${prof}/profile.json"
  if ! jq '.' "${recipe}" | check_autonomy_denylist_json "${recipe}"; then
    FAILED=1
  fi
  if ! resolve_profile_recipe "${ROOT}" "${prof}" >/dev/null 2>"${TMP_WORK}/prof.err"; then
    err "profile '${prof}' recipe failed resolution: $(cat "${TMP_WORK}/prof.err")"
  fi
done < <(discover_profiles "${ROOT}")

for artifact in "${ROOT}"/_merged/*.json; do
  [[ -f "${artifact}" ]] || continue
  if ! jq '.' "${artifact}" | check_autonomy_denylist_json "${artifact}"; then
    FAILED=1
  fi
  inv_err="$(jq -r '
    (if ."workbench.sideBar.location" != "left" then "workbench.sideBar.location != left" else empty end),
    (if ."security.workspace.trust.untrustedFiles" != "prompt" then "security.workspace.trust.untrustedFiles != prompt" else empty end),
    (if ."telemetry.telemetryLevel" != "off" then "telemetry.telemetryLevel != off" else empty end),
    (if ."editor.hover.enabled" != "on" then "editor.hover.enabled != \"on\" (must be string)" else empty end),
    (if has("$schema") or has("@extends") then "contains forbidden $schema or @extends metadata key" else empty end)
  ' "${artifact}")"
  if [[ -n "${inv_err}" ]]; then
    while IFS= read -r line; do
      [[ -n "${line}" ]] && err "${artifact}: ${line}"
    done <<< "${inv_err}"
  fi
done

for bundle in "${ROOT}"/exports/*.code-profile; do
  [[ -f "${bundle}" && ! -L "${bundle}" ]] || continue
  if ! jq '.settings // {}' "${bundle}" | check_autonomy_denylist_json "${bundle}"; then
    FAILED=1
  fi
  inv_err="$(jq -r --argjson ux "${UX_KEYS_JSON}" '
    (.settings // {}) as $s
    | (if $s."workbench.sideBar.location" != "left" then "workbench.sideBar.location != left" else empty end),
      (if $s."security.workspace.trust.untrustedFiles" != "prompt" then "security.workspace.trust.untrustedFiles != prompt" else empty end),
      (if $s."telemetry.telemetryLevel" != "off" then "telemetry.telemetryLevel != off" else empty end),
      (if $s."editor.hover.enabled" != "on" then "editor.hover.enabled != \"on\"" else empty end),
      (if ($s | has("$schema")) or ($s | has("@extends")) then "contains $schema or @extends" else empty end),
      ($s | keys[] | select(. as $k | $ux | index($k) != null) | "exported bundle leaks UX key: \(.)")
  ' "${bundle}")"
  if [[ -n "${inv_err}" ]]; then
    while IFS= read -r line; do
      [[ -n "${line}" ]] && err "${bundle}: ${line}"
    done <<< "${inv_err}"
  fi
done

# Verify exports/SHA256SUMS if present (both entry digests and full coverage).
if [[ -f "${ROOT}/exports/SHA256SUMS" ]]; then
  sums_names_file="${TMP_WORK}/sums-names.txt"
  : > "${sums_names_file}"
  while IFS= read -r line; do
    [[ -n "${line}" ]] || continue
    exp="${line%% *}"
    fname="${line#* }"
    fname="${fname# }"
    printf '%s\n' "${fname}" >> "${sums_names_file}"
    fpath="${ROOT}/exports/${fname}"
    if [[ ! -f "${fpath}" ]]; then
      err "exports/SHA256SUMS references missing file: ${fname}"
      continue
    fi
    act="$(sha256_for_file "${fpath}")"
    if [[ "${act}" != "${exp}" ]]; then
      err "exports/SHA256SUMS mismatch for ${fname} (expected ${exp}, got ${act})"
    fi
  done < "${ROOT}/exports/SHA256SUMS"
  for bundle in "${ROOT}"/exports/*.code-profile; do
    [[ -f "${bundle}" && ! -L "${bundle}" ]] || continue
    bname="$(basename -- "${bundle}")"
    if ! grep -Fxq "${bname}" "${sums_names_file}"; then
      err "exports/${bname} is missing from exports/SHA256SUMS"
    fi
  done
fi

# Check forbidden security-bypass paths unconditionally (works in git worktrees & non-git copies).
for bad_path in agents .agents/settings.local.json; do
  if [[ -e "${ROOT}/${bad_path}" ]]; then
    err "forbidden security-bypass file exists in repo: ${bad_path}"
  fi
done

# Parsed-config autonomy scan on tracked .agents/.vscode/.claude JSON/JSONC (supports git worktrees where .git is a file).
if [[ -e "${ROOT}/.git" ]] && command -v git >/dev/null 2>&1; then
  while IFS= read -r cfg; do
    [[ -n "${cfg}" && -f "${ROOT}/${cfg}" ]] || continue
    case "${cfg}" in
      .agents/*.json|.agents/*.jsonc|.vscode/*.json|.vscode/*.jsonc|.claude/*.json|.claude/*.jsonc)
        if parsed_cfg="$(compiler_parse_jsonc_object "${ROOT}" "${ROOT}/${cfg}" 2>"${TMP_WORK}/cfg.err")"; then
          printf '%s\n' "${parsed_cfg}" | check_autonomy_denylist_json "${cfg}" || FAILED=1
        else
          err "${cfg}: failed JSON/JSONC parse during autonomy scan: $(cat "${TMP_WORK}/cfg.err")"
        fi
        ;;
    esac
  done < <(git -C "${ROOT}" ls-files)
fi

# Check for broken symlinks anywhere in repo (excluding .git and .cache).
while IFS= read -r link; do
  [[ -n "${link}" ]] || continue
  if [[ ! -e "${link}" ]]; then
    err "broken symlink: ${link}"
  fi
done < <(find "${ROOT}" \( -path "${ROOT}/.git" -o -path "${ROOT}/.cache" \) -prune -o -type l -print 2>/dev/null)

# Run Rust docs policy sync check if docs/rust.md and AGENTS.md exist in ROOT.
if [[ -f "${ROOT}/docs/rust.md" && -f "${ROOT}/AGENTS.md" && -f "${LIB_DIR}/apply-rust-policy.sh" ]]; then
  if ! VSPCLI_ROOT="${ROOT}" "${BASH}" "${LIB_DIR}/apply-rust-policy.sh" --check >/dev/null 2>"${TMP_WORK}/rust-pol.err"; then
    err "rust-docs policy out of sync: $(cat "${TMP_WORK}/rust-pol.err")"
  fi
fi

# Verify every active catalog extension group is present in install-extensions.sh GROUP_ORDER.
if [[ -f "${SCRIPT_ROOT}/scripts/install-extensions.sh" && -f "${ROOT}/catalog/extensions.json" ]]; then
  group_order_line="$(grep -E '^GROUP_ORDER=\(' "${SCRIPT_ROOT}/scripts/install-extensions.sh" | head -n 1 || true)"
  group_order_vals=" ${group_order_line#GROUP_ORDER=(}"
  group_order_vals="${group_order_vals%)} "
  while IFS= read -r cg; do
    [[ -n "${cg}" ]] || continue
    case "${group_order_vals}" in
      *" ${cg} "*) ;;
      *) err "catalog group '${cg}' is missing from scripts/install-extensions.sh GROUP_ORDER" ;;
    esac
  done < <(jq -r '.extensions | to_entries[] | select(.value.status == "active") | .value.group // empty' "${ROOT}/catalog/extensions.json" | LC_ALL=C sort -u)
fi

if [[ "${FAILED}" -ne 0 ]]; then
  echo "validate-json: one or more checks failed" >&2
  exit 1
fi

echo "validate-json: validated ${CHECKED} files and all catalog/module/security invariants successfully"
exit 0

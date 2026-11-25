#!/usr/bin/env bash
# generate-readme-matrix.sh — Deterministically render the Profile & Module Matrix
# Markdown section from modules/*/*/module.json and profiles/*/profile.json.
#
# Modes:
#   (default) / --stdout : print rendered block to stdout
#   --write              : replace sentinel block in README.md atomically
#   --check              : exit 1 if README.md sentinel block is out of sync
set -euo pipefail

SCRIPT_ROOT="$(CDPATH='' cd -- "$(dirname -- "${0}")/.." && pwd -P)"
ROOT="${VSPCLI_ROOT:-${SCRIPT_ROOT}}"
LIB="${SCRIPT_ROOT}/scripts/lib"
# shellcheck source=scripts/lib/common.sh
source "${LIB}/common.sh"
# shellcheck source=scripts/lib/module-resolver.sh
source "${LIB}/module-resolver.sh"
# shellcheck source=scripts/lib/compiler.sh
source "${LIB}/compiler.sh"

MODE="stdout"
case "${1:-}" in
  ""|--stdout) MODE="stdout" ;;
  --write)     MODE="write" ;;
  --check)     MODE="check" ;;
  -h|--help)
    echo "Usage: $(basename -- "${0}") [--stdout|--write|--check]"
    exit 0 ;;
  *)
    echo "generate-readme-matrix: unknown option '${1}'" >&2
    exit 2 ;;
esac

render_matrix() {
  cat <<'HEADER'
### Profiles (15 Compiled Recipes)

| Profile | Description | Included Modules | Extensions | Export Bundle |
| --- | --- | --- | ---: | --- |
HEADER

  local prof recipe desc mods_str ext_count bundle_cell resolved_str mod_item
  while IFS= read -r prof; do
    [[ -n "${prof}" ]] || continue
    recipe="${ROOT}/profiles/${prof}/profile.json"
    desc="$(jq -r '.description // ""' "${recipe}")"
    resolved_str="$(resolve_profile_recipe "${ROOT}" "${prof}" "" "" "default")"
    mods_str="$(printf '%s\n' "${resolved_str}" | awk -F '\t' '{printf "%s%s", (NR > 1 ? ", " : ""), $1}')"
    ext_count=0
    if [[ -f "${ROOT}/profiles/${prof}/extensions.json" ]]; then
      ext_count="$(jq 'length' "${ROOT}/profiles/${prof}/extensions.json")"
    else
      local mods=()
      while IFS= read -r mod_item; do
        [[ -n "${mod_item}" ]] && mods+=("${mod_item}")
      done <<< "${resolved_str}"
      ext_count="$(compiler_compile_extensions "${ROOT}" profile "${mods[@]+"${mods[@]}"}" | jq 'length')"
    fi
    bundle_cell="[\`${prof}.code-profile\`](exports/${prof}.code-profile)"
    # shellcheck disable=SC2016
    printf '| `%s` | %s | `%s` | %s | %s |\n' \
      "${prof}" "${desc}" "${mods_str}" "${ext_count}" "${bundle_cell}"
  done < <(discover_profiles "${ROOT}")

  cat <<'MODHEADER'

### Modules (`modules/`)

| Kind | Module ID | Description | Requires | Conflicts |
| --- | --- | --- | --- | --- |
MODHEADER

  local mid mkind mdir manifest reqs confs
  while IFS=$'\t' read -r mid mkind mdir; do
    [[ -n "${mid}" ]] || continue
    manifest="${mdir}/module.json"
    desc="$(jq -r '.description // ""' "${manifest}")"
    reqs="$(jq -r '(.requires // []) | if length == 0 then "—" else join(", ") end' "${manifest}")"
    confs="$(jq -r '(.conflicts // []) | if length == 0 then "—" else join(", ") end' "${manifest}")"
    # shellcheck disable=SC2016
    printf '| `%s` | `%s` | %s | %s | %s |\n' \
      "${mkind}" "${mid}" "${desc}" "${reqs}" "${confs}"
  done < <(discover_modules "${ROOT}")
}

RENDERED="$(render_matrix)"
BLOCK="$(printf '<!-- BEGIN GENERATED-MATRIX -->\n%s\n<!-- END GENERATED-MATRIX -->' "${RENDERED}")"

if [[ "${MODE}" == "stdout" ]]; then
  printf '%s\n' "${BLOCK}"
  exit 0
fi

README="${ROOT}/README.md"
if [[ ! -f "${README}" ]]; then
  echo "generate-readme-matrix: missing ${README}" >&2
  exit 1
fi

if ! grep -q '<!-- BEGIN GENERATED-MATRIX -->' "${README}"; then
  echo "generate-readme-matrix: ${README} lacks BEGIN/END GENERATED-MATRIX sentinels" >&2
  exit 1
fi

if [[ "${MODE}" == "check" ]]; then
  current="$(awk '/<!-- BEGIN GENERATED-MATRIX -->/,/<!-- END GENERATED-MATRIX -->/' "${README}")"
  if [[ "${current}" != "${BLOCK}" ]]; then
    echo "generate-readme-matrix: README.md matrix block is out of sync; run 'bash scripts/generate-readme-matrix.sh --write'" >&2
    exit 1
  fi
  echo "ok: README.md matrix block is in sync"
  exit 0
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vsp-readme.XXXXXX")"
trap 'rm -rf "${TMP_DIR}"' EXIT
BLOCK_FILE="${TMP_DIR}/block.md"
TMP_OUT="${TMP_DIR}/README.md"
printf '%s\n' "${BLOCK}" > "${BLOCK_FILE}"
awk -v bfile="${BLOCK_FILE}" '
  /<!-- BEGIN GENERATED-MATRIX -->/ {
    in_block=1
    while ((getline bline < bfile) > 0) print bline
    close(bfile)
    next
  }
  /<!-- END GENERATED-MATRIX -->/ { in_block=0; next }
  !in_block { print }
' "${README}" > "${TMP_OUT}"
atomic_write_file --preserve-mode "${README}" "${TMP_OUT}" 0644
echo "updated ${README} matrix block"

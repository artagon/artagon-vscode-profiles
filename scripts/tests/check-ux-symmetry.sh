#!/usr/bin/env bash
# CI gate: assert UX presets carry the exact 15 symmetric keys, retina uses
# verified catppuccin-mocha iconTheme + fluent-icons productIconTheme, crisp
# uses material-product-icons, default preset is {}, and UX keys do not overlap
# with core-editor.
set -euo pipefail

ROOT="${VSPCLI_ROOT:-$(CDPATH='' cd -- "$(dirname -- "${0}")/../.." && pwd -P)}"
LIB="${ROOT}/scripts/lib"
CRISP="${ROOT}/modules/ux/crisp/settings.profile.jsonc"
RETINA="${ROOT}/modules/ux/retina/settings.profile.jsonc"
DEFAULT="${ROOT}/modules/ux/default/settings.profile.jsonc"
CORE="${ROOT}/modules/capabilities/core-editor/settings.profile.jsonc"
STRIP="${LIB}/jsonc-strip.awk"

for f in "${CRISP}" "${RETINA}" "${DEFAULT}" "${CORE}" "${STRIP}"; do
  [[ -f "${f}" ]] || { echo "missing: ${f}" >&2; exit 1; }
done

EXPECTED_15_KEYS='[
  "editor.fontFamily",
  "editor.fontLigatures",
  "editor.fontSize",
  "editor.fontVariations",
  "editor.letterSpacing",
  "editor.lineHeight",
  "terminal.integrated.fontFamily",
  "terminal.integrated.fontSize",
  "window.density.editorTabHeight",
  "workbench.colorTheme",
  "workbench.fontAliasing",
  "workbench.iconTheme",
  "workbench.preferredDarkColorTheme",
  "workbench.productIconTheme",
  "workbench.tree.indent"
]'

parse_jsonc() {
  LC_ALL=C awk -f "${STRIP}" "${1}" | jq -S '.'
}

crisp_doc="$(parse_jsonc "${CRISP}")"
retina_doc="$(parse_jsonc "${RETINA}")"
default_doc="$(parse_jsonc "${DEFAULT}")"
core_doc="$(parse_jsonc "${CORE}")"

crisp_keys="$(printf '%s\n' "${crisp_doc}" | jq -S 'keys')"
retina_keys="$(printf '%s\n' "${retina_doc}" | jq -S 'keys')"
default_keys="$(printf '%s\n' "${default_doc}" | jq -S 'keys')"
core_keys="$(printf '%s\n' "${core_doc}" | jq -S 'keys')"
expected_keys="$(printf '%s\n' "${EXPECTED_15_KEYS}" | jq -S '.')"

if [[ "${crisp_keys}" != "${retina_keys}" ]]; then
  echo "ERROR: crisp and retina UX presets have different key sets" >&2
  diff -u <(printf '%s\n' "${crisp_keys}") <(printf '%s\n' "${retina_keys}") >&2 || true
  exit 1
fi

if [[ "${crisp_keys}" != "${expected_keys}" ]]; then
  echo "ERROR: UX presets do not match the exact 15 symmetric keys" >&2
  diff -u <(printf '%s\n' "${expected_keys}") <(printf '%s\n' "${crisp_keys}") >&2 || true
  exit 1
fi

if [[ "${default_keys}" != "[]" ]]; then
  echo "ERROR: modules/ux/default/settings.profile.jsonc must be empty {}" >&2
  exit 1
fi

retina_icon="$(printf '%s\n' "${retina_doc}" | jq -r '."workbench.iconTheme"')"
if [[ "${retina_icon}" != "catppuccin-mocha" ]]; then
  echo "ERROR: retina workbench.iconTheme must be 'catppuccin-mocha', got '${retina_icon}'" >&2
  exit 1
fi

retina_prod="$(printf '%s\n' "${retina_doc}" | jq -r '."workbench.productIconTheme"')"
if [[ "${retina_prod}" != "fluent-icons" ]]; then
  echo "ERROR: retina workbench.productIconTheme must be 'fluent-icons', got '${retina_prod}'" >&2
  exit 1
fi

crisp_prod="$(printf '%s\n' "${crisp_doc}" | jq -r '."workbench.productIconTheme"')"
if [[ "${crisp_prod}" != "material-product-icons" ]]; then
  echo "ERROR: crisp workbench.productIconTheme must be 'material-product-icons', got '${crisp_prod}'" >&2
  exit 1
fi

overlap="$(jq -S -n --argjson a "${crisp_keys}" --argjson b "${core_keys}" '$a - ($a - $b)')"
if [[ "${overlap}" != "[]" ]]; then
  echo "ERROR: keys appear in both UX preset and core-editor:" >&2
  echo "${overlap}" >&2
  exit 1
fi

echo "ok: UX symmetry verified"
echo "  ux keys: $(printf '%s\n' "${crisp_keys}" | jq 'length') (exact 15 symmetric keys in crisp & retina; default={})"
echo "  core-editor profile keys: $(printf '%s\n' "${core_keys}" | jq 'length')"

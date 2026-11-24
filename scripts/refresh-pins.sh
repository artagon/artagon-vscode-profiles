#!/usr/bin/env bash
# refresh-pins.sh — Fetch official universal VSIX packages from VS Code Marketplace
# and update catalog/extension-pins.json with verified SHA-256 digests.
# Pure Bash 3.2 + jq + curl + shasum/sha256sum (no Python).
set -euo pipefail

ROOT="${VSPCLI_ROOT:-$(cd "$(dirname "${0}")/.." && pwd)}"
PINS_FILE="${ARTAGON_VSCODE_PINS_FILE:-${ROOT}/catalog/extension-pins.json}"
API_URL="https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery"

PIN_IDS=(
  "catppuccin.catppuccin-vsc"
  "catppuccin.catppuccin-vsc-icons"
  "editorconfig.editorconfig"
  "enkia.tokyo-night"
  "miguelsolorio.fluent-icons"
  "pkief.material-icon-theme"
  "pkief.material-product-icons"
  "tamasfe.even-better-toml"
)

if ! command -v jq >/dev/null 2>&1 || ! command -v curl >/dev/null 2>&1; then
  echo "refresh-pins: requires jq and curl on PATH" >&2
  exit 1
fi

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${1}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${1}" | awk '{print $1}'
  else
    echo "refresh-pins: requires shasum or sha256sum" >&2
    return 1
  fi
}

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vsp-refresh-pins.XXXXXX")"
trap 'rm -rf "${TMP_DIR}"' EXIT

pins_json='{}'
now_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

for ext_id in "${PIN_IDS[@]}"; do
  payload="$(jq -n --arg id "${ext_id}" '{
    filters: [{
      criteria: [{ filterType: 7, value: $id }],
      pageNumber: 1,
      pageSize: 1
    }],
    assetTypes: [],
    flags: 103
  }')"
  resp_file="${TMP_DIR}/${ext_id}.json"
  curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --max-time 30 \
    -H "Content-Type: application/json" \
    -H "Accept: application/json;api-version=3.0-preview.1" \
    -d "${payload}" "${API_URL}" -o "${resp_file}"

  # Pick latest non-prerelease universal version
  meta="$(jq -r '
    .results[0].extensions[0].versions
    | map(select(
        ((.properties // []) | any(.key == "Microsoft.VisualStudio.Code.PreRelease" and .value == "true") | not)
        and ((.targetPlatform // "") == "")
      ))
    | .[0]
    | {
        version: .version,
        vsix_url: ((.files // []) | map(select(.assetType == "Microsoft.VisualStudio.Services.VSIXPackage")) | .[0].source)
      }
  ' "${resp_file}")"

  version="$(printf '%s' "${meta}" | jq -r '.version // empty')"
  vsix_url="$(printf '%s' "${meta}" | jq -r '.vsix_url // empty')"

  if [[ -z "${version}" || -z "${vsix_url}" ]]; then
    echo "refresh-pins: failed to resolve universal stable VSIX for ${ext_id}" >&2
    exit 1
  fi

  case "${vsix_url}" in
    https://*) ;;
    *)
      echo "refresh-pins: non-HTTPS vsix_url rejected for ${ext_id}: ${vsix_url}" >&2
      exit 1
      ;;
  esac

  vsix_file="${TMP_DIR}/${ext_id}.vsix"
  curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --max-time 60 \
    "${vsix_url}" -o "${vsix_file}"
  digest="$(sha256_file "${vsix_file}")"

  if [[ ! "${digest}" =~ ^[0-9a-f]{64}$ ]]; then
    echo "refresh-pins: invalid digest for ${ext_id}: ${digest}" >&2
    exit 1
  fi

  pins_json="$(printf '%s' "${pins_json}" | jq \
    --arg id "${ext_id}" \
    --arg ver "${version}" \
    --arg sha "${digest}" \
    --arg url "${vsix_url}" \
    --arg ts "${now_utc}" \
    '.[$id] = { version: $ver, sha256: $sha, vsix_url: $url, verified_at: $ts }')"
  echo "pinned ${ext_id}@${version} (${digest})"
done

mkdir -p "$(dirname "${PINS_FILE}")"
out_tmp="$(mktemp "$(dirname "${PINS_FILE}")/.extension-pins.XXXXXX")"
jq -S -n --argjson pins "${pins_json}" '{
  version: 1,
  note: "Install-time verified SHA-256 VSIX pins for universal core/UX extensions. Refresh via scripts/refresh-pins.sh.",
  pins: $pins
}' > "${out_tmp}"
chmod 0644 "${out_tmp}"
mv -f "${out_tmp}" "${PINS_FILE}"
echo "Wrote ${PINS_FILE}"

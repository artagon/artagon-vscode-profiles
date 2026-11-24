# shellcheck shell=bash
# legacy-profile-name.sh — Single authoritative mapping for legacy
# `<flavor>-{crisp,retina}` profile names and `exports/*.code-profile` symlinks.
# Shared by vspcli, export-profiles.sh, migrate-catalog.sh --refresh-symlinks,
# compose-settings.sh, and install-extensions.sh.
#
# Security/Compatibility Rule:
#   ai-profile-{crisp,retina} -> ai-copilot (Copilot-only, NEVER silently upgraded to AI trio)

resolve_legacy_profile_name() {
  local name="$1"
  local flavor="" ux=""

  case "${name}" in
    *-crisp)  ux="crisp" ;;
    *-retina) ux="retina" ;;
    *) printf ''; return 0 ;;
  esac

  local stem="${name%-"${ux}"}"
  case "${stem}" in
    ai-profile)   flavor="ai-copilot" ;;
    rust-profile) flavor="rust" ;;
    web-astro)    flavor="astro" ;;
    java-profile) flavor="java-maven" ;;
    ai-plus|cpp-clangd|cpp-intellisense|github-workflows|java-gradle|java-maven|java-spring)
      flavor="${stem}" ;;
    *) printf ''; return 0 ;;
  esac

  _legacy_profile_warn "${name}" "${flavor}" "${ux}"
  printf '%s %s' "${flavor}" "${ux}"
}

# Authoritative 22 legacy export symlink pairs: `<legacy_link_name> <canonical_target_file>`
# Used by BOTH export-profiles.sh and migrate-catalog.sh --refresh-symlinks.
list_legacy_bundle_links() {
  cat <<'EOF'
ai-profile-crisp.code-profile ai-copilot.code-profile
ai-profile-retina.code-profile ai-copilot.code-profile
ai-plus-crisp.code-profile ai-plus.code-profile
ai-plus-retina.code-profile ai-plus.code-profile
cpp-clangd-crisp.code-profile cpp-clangd.code-profile
cpp-clangd-retina.code-profile cpp-clangd.code-profile
cpp-intellisense-crisp.code-profile cpp-intellisense.code-profile
cpp-intellisense-retina.code-profile cpp-intellisense.code-profile
github-workflows-crisp.code-profile github-workflows.code-profile
github-workflows-retina.code-profile github-workflows.code-profile
java-gradle-crisp.code-profile java-gradle.code-profile
java-gradle-retina.code-profile java-gradle.code-profile
java-maven-crisp.code-profile java-maven.code-profile
java-maven-retina.code-profile java-maven.code-profile
java-profile-crisp.code-profile java-maven.code-profile
java-profile-retina.code-profile java-maven.code-profile
java-spring-crisp.code-profile java-spring.code-profile
java-spring-retina.code-profile java-spring.code-profile
rust-profile-crisp.code-profile rust.code-profile
rust-profile-retina.code-profile rust.code-profile
web-astro-crisp.code-profile astro.code-profile
web-astro-retina.code-profile astro.code-profile
EOF
}

# Refresh all 22 legacy symlinks inside <exports_dir> when their canonical target exists.
refresh_legacy_bundle_links() {
  local exports_dir="$1"
  mkdir -p "${exports_dir}"
  local link target
  while read -r link target; do
    [[ -n "${link}" && -n "${target}" ]] || continue
    if [[ -f "${exports_dir}/${target}" ]]; then
      ln -sfn "${target}" "${exports_dir}/${link}"
    fi
  done < <(list_legacy_bundle_links)
}

_legacy_profile_warn() {
  local name="$1" flavor="$2" ux="$3"

  if [[ -n "${ARTAGON_VSCODE_DEPRECATION_ACK:-}" ]]; then
    return 0
  fi

  if [[ -n "${ARTAGON_VSCODE_DEPRECATION_SUMMARY:-}" ]]; then
    : "${ARTAGON_VSCODE_DEPRECATION_COUNT:=0}"
    ARTAGON_VSCODE_DEPRECATION_COUNT=$((ARTAGON_VSCODE_DEPRECATION_COUNT + 1))
    export ARTAGON_VSCODE_DEPRECATION_COUNT
    return 0
  fi

  if [[ -n "${ARTAGON_VSCODE_DEPRECATION_SEEN:-}" ]]; then
    return 0
  fi
  export ARTAGON_VSCODE_DEPRECATION_SEEN=1

  echo "deprecation: '${name}' resolves to '${flavor}' with --ux=${ux}; use --toolchain ${flavor} --ux=${ux} directly" >&2
}

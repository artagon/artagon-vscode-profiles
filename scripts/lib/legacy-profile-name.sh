#!/usr/bin/env bash
# legacy-profile-name.sh — resolve `<flavor>-{crisp,retina}` legacy profile
# names to `<flavor>` plus implied `--ux=<look>`. Sourced by all entry
# points (vspcli, install-extensions.sh, compose-settings.sh, etc.) per
# design.md Decision 21.
#
# Provides:
#   resolve_legacy_profile_name <name>
#     Prints "<flavor> <ux>" if <name> is a legacy form; empty if not.
#     Also emits a rate-limited deprecation warning to stderr per
#     design.md Decision 20.

# shellcheck shell=bash

resolve_legacy_profile_name() {
  local name="$1"
  local flavor="" ux=""

  case "$name" in
    *-crisp)  ux="crisp" ;;
    *-retina) ux="retina" ;;
    *) printf '' ; return 0 ;;
  esac

  local stem="${name%-"${ux}"}"
  case "$stem" in
    ai-profile)   flavor="ai" ;;
    rust-profile) flavor="rust" ;;
    web-astro)    flavor="astro" ;;
    java-profile) flavor="java-maven" ;;
    ai-plus|cpp-clangd|cpp-intellisense|github-workflows|java-gradle|java-maven|java-spring)
      flavor="$stem" ;;
    *) printf '' ; return 0 ;;
  esac

  _legacy_profile_warn "$name" "$flavor" "$ux"
  printf '%s %s' "$flavor" "$ux"
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

  echo "deprecation: '$name' resolves to '$flavor' with --ux=$ux; use --toolchain $flavor --ux=$ux directly" >&2
}

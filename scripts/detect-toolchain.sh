#!/usr/bin/env bash
# detect-toolchain.sh — detect workspace toolchain(s) by signal precedence.
# Implements the `detect-workspace-toolchain` capability spec.
#
# Exit codes:
#   0  detection succeeded
#   2  internal/path error
#   3  no toolchain detected and no --toolchain override
#   4  invalid arguments

set -euo pipefail

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS] [PATH]

Detect workspace toolchain from signals at PATH (default: \$PWD).

Options:
  --toolchain FLAVOR   Skip detection; emit FLAVOR. May be passed multiple times to stack.
  --no-detect          Disable detection; require --toolchain.
  --json               Emit JSON object instead of plain space-separated names.
  -h, --help           Show this help.

Exit codes: 0 ok, 2 path error, 3 no detection, 4 invalid args.
EOF
}

VALID_FLAVORS="ai ai-plus astro cpp-clangd cpp-intellisense general github-workflows java-gradle java-maven java-spring rust"

is_valid_flavor() {
  local f="$1"
  case " ${VALID_FLAVORS} " in
    *" ${f} "*) return 0 ;;
    *) return 1 ;;
  esac
}

WORKSPACE=""
JSON=0
NO_DETECT=0
OVERRIDES=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --toolchain)
      [[ $# -ge 2 ]] || { echo "error: --toolchain requires a flavor" >&2; exit 4; }
      if ! is_valid_flavor "$2"; then
        echo "unknown toolchain '$2'; valid: ${VALID_FLAVORS// /, }" >&2
        exit 4
      fi
      OVERRIDES+=("$2")
      shift 2
      ;;
    --no-detect)
      NO_DETECT=1
      shift
      ;;
    --json)
      JSON=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    -*)
      echo "error: unknown flag $1" >&2
      exit 4
      ;;
    *)
      if [[ -n "$WORKSPACE" ]]; then
        echo "error: multiple workspace paths" >&2
        exit 4
      fi
      WORKSPACE="$1"
      shift
      ;;
  esac
done

WORKSPACE="${WORKSPACE:-$PWD}"

if [[ ! -e "$WORKSPACE" ]]; then
  echo "error: workspace path $WORKSPACE does not exist" >&2
  exit 2
fi
if [[ ! -d "$WORKSPACE" ]]; then
  echo "error: workspace path $WORKSPACE is not a directory" >&2
  exit 2
fi

WORKSPACE="$(cd "$WORKSPACE" && pwd)"

if [[ $NO_DETECT -eq 1 ]] && [[ ${#OVERRIDES[@]} -eq 0 ]]; then
  echo "--no-detect requires --toolchain <flavor>" >&2
  exit 4
fi

# --- Override path: skip detection entirely ---
if [[ ${#OVERRIDES[@]} -gt 0 ]] || [[ $NO_DETECT -eq 1 ]]; then
  if [[ $JSON -eq 1 ]]; then
    jq -n --argjson tools "$(printf '%s\n' "${OVERRIDES[@]}" | jq -R . | jq -s .)" \
          --arg workspace "$WORKSPACE" \
          '{toolchains: $tools, signals: ($tools | map({(.): []}) | add // {}), workspace: $workspace, override: true}'
  else
    printf '%s\n' "${OVERRIDES[*]}"
  fi
  exit 0
fi

# --- Detection by signal precedence ---
TOOLCHAINS=()
declare -a SIGNAL_LINES=()

add_toolchain() {
  local flavor="$1"
  shift
  TOOLCHAINS+=("$flavor")
  local files_json
  files_json=$(printf '%s\n' "$@" | jq -R . | jq -s .)
  SIGNAL_LINES+=("$flavor:$files_json")
}

# 1. rust
if [[ -f "$WORKSPACE/Cargo.toml" ]]; then
  add_toolchain rust "Cargo.toml"
fi

# 2-3. cpp-clangd / cpp-intellisense
if [[ -f "$WORKSPACE/CMakeLists.txt" ]]; then
  if [[ -f "$WORKSPACE/.clangd" ]]; then
    add_toolchain cpp-clangd "CMakeLists.txt" ".clangd"
  else
    add_toolchain cpp-intellisense "CMakeLists.txt"
  fi
fi

# 4-5. java-spring / java-gradle
GRADLE_FILES=()
[[ -f "$WORKSPACE/build.gradle" ]] && GRADLE_FILES+=("build.gradle")
[[ -f "$WORKSPACE/build.gradle.kts" ]] && GRADLE_FILES+=("build.gradle.kts")

if [[ ${#GRADLE_FILES[@]} -gt 0 ]]; then
  # Spring promotion: look for @SpringBootApplication in src/main/java/**/*Application.java
  SPRING=0
  if [[ -d "$WORKSPACE/src/main/java" ]]; then
    if find "$WORKSPACE/src/main/java" -type f -name '*Application.java' \
        -exec grep -l '@SpringBootApplication' {} + 2>/dev/null | head -1 | grep -q . ; then
      SPRING=1
    fi
  fi
  if [[ $SPRING -eq 1 ]]; then
    add_toolchain java-spring "${GRADLE_FILES[@]}"
  else
    add_toolchain java-gradle "${GRADLE_FILES[@]}"
  fi
fi

# 6. java-maven
if [[ -f "$WORKSPACE/pom.xml" ]]; then
  add_toolchain java-maven "pom.xml"
fi

# 7. astro
if [[ -f "$WORKSPACE/package.json" ]]; then
  ASTRO_CFG=""
  for ext in mjs ts js cjs; do
    if [[ -f "$WORKSPACE/astro.config.$ext" ]]; then
      ASTRO_CFG="astro.config.$ext"
      break
    fi
  done
  if [[ -n "$ASTRO_CFG" ]]; then
    add_toolchain astro "package.json" "$ASTRO_CFG"
  fi
fi

# 8. github-workflows (only if NO other signal matched — gate per spec scenario)
if [[ ${#TOOLCHAINS[@]} -eq 0 ]]; then
  if compgen -G "$WORKSPACE/.github/workflows/*.yml" > /dev/null 2>&1 \
     || compgen -G "$WORKSPACE/.github/workflows/*.yaml" > /dev/null 2>&1; then
    add_toolchain github-workflows ".github/workflows/"
  fi
fi

if [[ ${#TOOLCHAINS[@]} -eq 0 ]]; then
  echo "no toolchain detected; pass --toolchain <flavor> or --no-detect" >&2
  if [[ $JSON -eq 1 ]]; then
    jq -n --arg workspace "$WORKSPACE" '{toolchains: [], signals: {}, workspace: $workspace}'
  fi
  exit 3
fi

if [[ $JSON -eq 1 ]]; then
  signals_json="{}"
  for line in "${SIGNAL_LINES[@]}"; do
    flavor="${line%%:*}"
    files="${line#*:}"
    signals_json=$(echo "$signals_json" | jq --arg f "$flavor" --argjson v "$files" '. + {($f): $v}')
  done
  jq -n --argjson tools "$(printf '%s\n' "${TOOLCHAINS[@]}" | jq -R . | jq -s .)" \
        --argjson signals "$signals_json" \
        --arg workspace "$WORKSPACE" \
        '{toolchains: $tools, signals: $signals, workspace: $workspace}'
else
  printf '%s\n' "${TOOLCHAINS[*]}"
fi

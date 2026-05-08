#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

# --- Single-toolchain detection ---

@test "detect: rust workspace emits 'rust'" {
  ws=$(make_workspace rust Cargo.toml)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "rust" ]
}

@test "detect: astro workspace emits 'astro' (package.json + astro.config.mjs)" {
  ws=$(make_workspace astro package.json astro.config.mjs)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "astro" ]
}

@test "detect: java-maven workspace emits 'java-maven'" {
  ws=$(make_workspace mvn pom.xml)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "java-maven" ]
}

@test "detect: gradle without Spring marker emits 'java-gradle'" {
  ws=$(make_workspace gradle build.gradle)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "java-gradle" ]
}

@test "detect: gradle.kts with @SpringBootApplication emits 'java-spring'" {
  ws=$(make_workspace spring build.gradle.kts)
  mkdir -p "$ws/src/main/java/example"
  cat > "$ws/src/main/java/example/MyApplication.java" <<EOF
package example;
import org.springframework.boot.autoconfigure.SpringBootApplication;
@SpringBootApplication
public class MyApplication {}
EOF
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "java-spring" ]
}

@test "detect: CMakeLists + .clangd emits 'cpp-clangd'" {
  ws=$(make_workspace cpp CMakeLists.txt .clangd)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "cpp-clangd" ]
}

@test "detect: CMakeLists alone emits 'cpp-intellisense'" {
  ws=$(make_workspace cpp CMakeLists.txt)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "cpp-intellisense" ]
}

@test "detect: .github/workflows only emits 'github-workflows'" {
  ws=$(make_workspace gha)
  mkdir -p "$ws/.github/workflows"
  : > "$ws/.github/workflows/ci.yml"
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "github-workflows" ]
}

# --- Polyglot ---

@test "detect: Tauri (rust + astro) stacks 'rust astro' in precedence order" {
  ws=$(make_workspace tauri Cargo.toml package.json astro.config.mjs)
  run detect "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "rust astro" ]
}

# --- No-detection path ---

@test "detect: empty workspace exits 3 with stderr message" {
  ws=$(make_workspace empty)
  run detect "$ws"
  [ "$status" -eq 3 ]
  [[ "$stderr" == *"no toolchain detected"* ]] \
    || [[ "$output" == *"no toolchain detected"* ]]  # bats merges streams under run
}

# --- Override path ---

@test "detect: --toolchain rust in empty dir succeeds" {
  ws=$(make_workspace empty)
  run detect --toolchain rust "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "rust" ]
}

@test "detect: --toolchain unknown exits 4" {
  ws=$(make_workspace empty)
  run detect --toolchain zzz "$ws"
  [ "$status" -eq 4 ]
}

@test "detect: --no-detect without --toolchain exits 4" {
  ws=$(make_workspace empty)
  run detect --no-detect "$ws"
  [ "$status" -eq 4 ]
}

@test "detect: --toolchain override beats stacked detection" {
  ws=$(make_workspace tauri Cargo.toml package.json astro.config.mjs)
  run detect --toolchain rust "$ws"
  [ "$status" -eq 0 ]
  [ "$output" = "rust" ]
}

# --- Path errors ---

@test "detect: missing path exits 2" {
  run detect /no/such/path
  [ "$status" -eq 2 ]
}

# --- JSON mode ---

@test "detect: --json polyglot emits structured object" {
  require_bin jq
  ws=$(make_workspace tauri Cargo.toml package.json astro.config.mjs)
  run detect --json "$ws"
  [ "$status" -eq 0 ]
  toolchains=$(echo "$output" | jq -r '.toolchains | join(",")')
  [ "$toolchains" = "rust,astro" ]
  rust_signals=$(echo "$output" | jq -r '.signals.rust[0]')
  [ "$rust_signals" = "Cargo.toml" ]
}

@test "detect: --json empty emits toolchains: []" {
  require_bin jq
  ws=$(make_workspace empty)
  run --separate-stderr detect --json "$ws"
  [ "$status" -eq 3 ]
  count=$(echo "$output" | jq -r '.toolchains | length')
  [ "$count" -eq 0 ]
}

@test "detect: --json with --toolchain override sets override:true" {
  require_bin jq
  ws=$(make_workspace empty)
  run detect --json --toolchain rust "$ws"
  [ "$status" -eq 0 ]
  override=$(echo "$output" | jq -r '.override')
  [ "$override" = "true" ]
}

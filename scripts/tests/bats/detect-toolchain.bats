#!/usr/bin/env bats

bats_require_minimum_version 1.5.0
load test_helper

@test "detect: rust workspace emits 'rust'" {
  ws="$(make_workspace rust Cargo.toml)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "rust" ]]
}

@test "detect: astro workspace emits 'astro' (package.json + astro.config.mjs)" {
  ws="$(make_workspace astro package.json astro.config.mjs)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "astro" ]]
}

@test "detect: plain TypeScript workspace emits 'ts-base' (tsconfig.json)" {
  ws="$(make_workspace ts tsconfig.json)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "ts-base" ]]
}

@test "detect: Astro + biome.json emits 'astro biome'" {
  ws="$(make_workspace astro-biome package.json astro.config.mjs biome.json)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "astro biome" ]]
}

@test "detect: java-maven without Spring marker emits 'java-maven'" {
  ws="$(make_workspace mvn pom.xml)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-maven" ]]
}

@test "detect: java-maven WITH @SpringBootApplication layers 'java-maven java-spring'" {
  ws="$(make_workspace mvn-spring pom.xml)"
  mkdir -p "${ws}/src/main/java/example"
  printf '@SpringBootApplication\npublic class App {}\n' > "${ws}/src/main/java/example/MyApplication.java"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-maven java-spring" ]]
}

@test "detect: java-gradle without Spring marker emits 'java-gradle'" {
  ws="$(make_workspace gradle build.gradle)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-gradle" ]]
}

@test "detect: java-gradle WITH @SpringBootApplication layers 'java-gradle java-spring'" {
  ws="$(make_workspace gradle-spring build.gradle.kts)"
  mkdir -p "${ws}/src/main/java/example"
  printf '@SpringBootApplication\npublic class App {}\n' > "${ws}/src/main/java/example/MyApplication.java"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-gradle java-spring" ]]
}

@test "detect: both pom.xml + build.gradle without Spring prefers java-gradle (supersedes java-maven)" {
  ws="$(make_workspace dual-java pom.xml build.gradle)"
  run --separate-stderr detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-gradle" ]]
  [[ "${stderr}" == *"java-maven superseded by java-gradle"* ]]
}

@test "detect: both pom.xml + build.gradle WITH @SpringBootApplication layers 'java-gradle java-spring'" {
  ws="$(make_workspace dual-spring pom.xml build.gradle)"
  mkdir -p "${ws}/src/main/java/example"
  printf '@SpringBootApplication\npublic class App {}\n' > "${ws}/src/main/java/example/AppApplication.java"
  run --separate-stderr detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-gradle java-spring" ]]
  [[ "${stderr}" == *"java-maven superseded by java-gradle"* ]]
}

@test "detect: CMakeLists + .clangd emits 'cpp-clangd'; CMakeLists alone emits 'cpp-intellisense'" {
  ws1="$(make_workspace cpp1 CMakeLists.txt .clangd)"
  run detect "${ws1}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "cpp-clangd" ]]

  ws2="$(make_workspace cpp2 CMakeLists.txt)"
  run detect "${ws2}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "cpp-intellisense" ]]
}

@test "detect: python (pyproject.toml) and shell (.shellcheckrc)" {
  ws1="$(make_workspace py pyproject.toml)"
  run detect "${ws1}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "python" ]]

  ws2="$(make_workspace sh .shellcheckrc)"
  run detect "${ws2}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "shell" ]]
}

@test "detect: Rust + .github/workflows stacks github-workflows additively; workflow-only emits github-workflows" {
  ws1="$(make_workspace rust-gha Cargo.toml)"
  mkdir -p "${ws1}/.github/workflows"
  : > "${ws1}/.github/workflows/ci.yml"
  run detect "${ws1}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "rust github-workflows" ]]

  ws2="$(make_workspace gha-only)"
  mkdir -p "${ws2}/.github/workflows"
  : > "${ws2}/.github/workflows/deploy.yaml"
  run detect "${ws2}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "github-workflows" ]]
}

@test "detect: Tauri (rust + astro) stacks 'rust astro' in priority order" {
  ws="$(make_workspace tauri Cargo.toml package.json astro.config.mjs)"
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "rust astro" ]]
}

@test "detect: large match count (>1000 *Application.java files) never fails with SIGPIPE 141" {
  ws="$(make_workspace big-spring build.gradle)"
  mkdir -p "${ws}/src/main/java"
  i=1
  while [[ "${i}" -le 300 ]]; do
    printf '@SpringBootApplication\n' > "${ws}/src/main/java/A${i}Application.java"
    i=$((i + 1))
  done
  run detect "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "java-gradle java-spring" ]]
}

@test "detect: handles workspace paths with spaces and symlinked roots" {
  real_ws="${TEST_TMP}/my spaced workspace"
  mkdir -p "${real_ws}/.github/workflows"
  : > "${real_ws}/Cargo.toml"
  : > "${real_ws}/.github/workflows/build.yml"
  link_ws="${TEST_TMP}/link to workspace"
  ln -s "${real_ws}" "${link_ws}"

  run detect "${link_ws}"
  [[ "${status}" -eq 0 ]]
  [[ "${output}" = "rust github-workflows" ]]
}

@test "detect: empty workspace exits 3; missing path exits 2; invalid/missing override exits 4" {
  ws="$(make_workspace empty)"
  run detect "${ws}"
  [[ "${status}" -eq 3 ]]
  [[ "${output}" == *"no toolchain detected"* ]]

  run detect "/no/such/path/ever"
  [[ "${status}" -eq 2 ]]

  run detect --toolchain zzz "${ws}"
  [[ "${status}" -eq 4 ]]

  run detect --no-detect "${ws}"
  [[ "${status}" -eq 4 ]]
}

@test "detect: --toolchain override and --json output contract" {
  ws="$(make_workspace tauri Cargo.toml package.json astro.config.mjs)"
  run detect --json "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.toolchains | join(",")')" = "rust,astro" ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.signals.rust[0]')" = "Cargo.toml" ]]

  run detect --json --toolchain rust "${ws}"
  [[ "${status}" -eq 0 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.override')" = "true" ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.toolchains | join(",")')" = "rust" ]]

  empty_ws="$(make_workspace empty2)"
  run --separate-stderr detect --json "${empty_ws}"
  [[ "${status}" -eq 3 ]]
  [[ "$(printf '%s\n' "${output}" | jq -r '.toolchains | length')" -eq 0 ]]
}

# Homebrew formula template for the `artagon/homebrew-aitools` tap.
#
# Installs the prebuilt cross-platform `vsp` release tarballs built by
# `.github/workflows/release.yml` (ADR D4) without requiring a Rust toolchain
# on stable installs. `--HEAD` builds from source with `cargo`.
#
# The release workflow substitutes `version` and each platform's `sha256` from
# `SHA256SUMS`, verifies `ruby -c`, uploads the rendered `Formula/vsp.rb` as a
# release asset and workflow artifact, and optionally publishes to the tap when
# `vars.HOMEBREW_TAP_REPO` and `secrets.HOMEBREW_TAP_TOKEN` are configured.
#
# The all-zero `sha256` digests below are fail-closed placeholders: `brew install`
# refuses to install until a tagged release populates real digests.
class Vsp < Formula
  desc "Unified CLI and deterministic profile compiler for Artagon VS Code profiles"
  homepage "https://github.com/trumpyla/artagon-vscode-profiles"
  version "3.0.0"
  # This value mirrors `license` in the `[workspace.package]` table of `Cargo.toml`, which
  # is the only license statement in the repository. The source tree has no `LICENSE`
  # file, so this formula installs none. Keep the two values the same.
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/trumpyla/artagon-vscode-profiles/releases/download/v#{version}/vsp-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end

    on_intel do
      url "https://github.com/trumpyla/artagon-vscode-profiles/releases/download/v#{version}/vsp-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/trumpyla/artagon-vscode-profiles/releases/download/v#{version}/vsp-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  head do
    url "https://github.com/trumpyla/artagon-vscode-profiles.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "crates/vsp")
    else
      bin.install "vsp"
    end

    # `vspcli` is the legacy command name. The binary starts its compatibility mode
    # when the basename of argv[0] is `vspcli`.
    bin.install_symlink "vsp" => "vspcli"

    generate_completions_from_executable(bin/"vsp", "completions")
    # The legacy command has its own completion script (`vspcli completions <shell>`).
    generate_completions_from_executable(bin/"vspcli", "completions", base_name: "vspcli")
  end

  test do
    assert_match "vsp #{version}", shell_output("#{bin}/vsp --version")

    # The catalog is embedded in the binary, so these commands need no checkout.
    system bin/"vsp", "dev", "validate"
    assert_match "rust", shell_output("#{bin}/vsp list")

    assert_match "vspcli #{version}", shell_output("#{bin}/vspcli --version")
    assert_match "rust", shell_output("#{bin}/vspcli list")

    # The compatibility mode parses a mutating verb and rejects it before any change:
    # `reconcile` without a target is a usage error (exit 4).
    assert_match "specify --profile <name> or --all",
                 shell_output("#{bin}/vspcli reconcile 2>&1", 4)

    assert_match "reconcile", shell_output("#{bin}/vspcli completions bash")
  end
end

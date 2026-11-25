# Homebrew formula template for the `artagon/homebrew-aitools` tap.
#
# The release workflow copies this file to the tap and replaces `url` and `sha256`
# with the values of the published release archive. The all-zero `sha256` below is a
# placeholder: `brew install` fails closed until a real digest is set.
class Vsp < Formula
  desc "Unified CLI and deterministic profile compiler for Artagon VS Code profiles"
  homepage "https://github.com/trumpyla/artagon-vscode-profiles"
  url "https://github.com/trumpyla/artagon-vscode-profiles/archive/refs/tags/v3.0.0.tar.gz"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"
  # This value mirrors `license` in the `[workspace.package]` table of `Cargo.toml`, which
  # is the only license statement in the repository. The source tree has no `LICENSE`
  # file, so this formula installs none. Keep the two values the same.
  license "MIT"
  head "https://github.com/trumpyla/artagon-vscode-profiles.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/vsp")

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

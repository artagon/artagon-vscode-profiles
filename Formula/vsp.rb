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
  license "MIT"
  head "https://github.com/trumpyla/artagon-vscode-profiles.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/vsp")

    # `vspcli` is the legacy command name. The binary starts its compatibility mode
    # when the basename of argv[0] is `vspcli`.
    bin.install_symlink "vsp" => "vspcli"

    generate_completions_from_executable(bin/"vsp", "completions")
  end

  test do
    assert_match "vsp #{version}", shell_output("#{bin}/vsp --version")

    # The catalog is embedded in the binary, so these commands need no checkout.
    system bin/"vsp", "dev", "validate"
    assert_match "rust", shell_output("#{bin}/vsp list")

    assert_match "vspcli #{version}", shell_output("#{bin}/vspcli --version")
    assert_match "rust", shell_output("#{bin}/vspcli list")
  end
end

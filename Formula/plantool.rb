# Rendered by the release workflow from scripts/render-formula.sh; no release yet.
class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  license "MIT"
  head "https://github.com/amritghimire/plantool.git", branch: "main"

  depends_on "node" => :build
  depends_on "rust" => :build

  def install
    system "npm", "ci", "--prefix", "web"
    system "npm", "run", "build", "--prefix", "web"
    system "cargo", "install", *std_cargo_args(path: "crates/cli")
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

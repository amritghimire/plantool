class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.1/plantool-darwin-arm64.tar.gz"
      sha256 "b069bc4ee2d9a5b34f86f1615b51a35394e05cc686541b1cc015ce4cb4e6ce05"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.1/plantool-darwin-x64.tar.gz"
      sha256 "fb785d9607b6fb44c9a9d87d4cfa23b341ae095c0ef4a502458d809c64de4c7f"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.1/plantool-linux-arm64.tar.gz"
      sha256 "885e67b53a5758953e2e4d8f9b76ca0bbd6fe4482f16b269b9cad2eaae44ec9d"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.1/plantool-linux-x64.tar.gz"
      sha256 "94ae2301ee3d7a22d3f6e3626af87ae3fcffdb8a9c2bc3ce7a07cc6c1af81985"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

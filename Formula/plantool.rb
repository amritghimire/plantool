class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.14"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.14/plantool-darwin-arm64.tar.gz"
      sha256 "7fa31c09c1490cfc71a39996ce742c60d68bbadac3886aba6e9c3416decec6bd"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.14/plantool-darwin-x64.tar.gz"
      sha256 "a1e989fffb011a1904791499c57f137fb0f8b6fa745bbb030679d3b59bad9b99"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.14/plantool-linux-arm64.tar.gz"
      sha256 "90da02c66d251cc1aa16af03e5fee3caee03895550e28ecda7df18227a0eae41"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.14/plantool-linux-x64.tar.gz"
      sha256 "6ebe62a879f59709c5db1ea76d88afb26089f700674931085b62bf82de80442b"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

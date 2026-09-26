class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.7"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.7/plantool-darwin-arm64.tar.gz"
      sha256 "0d640405eef6b9ed259746c662581602945e5e3c09ac77c367a85dc9fc32fec1"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.7/plantool-darwin-x64.tar.gz"
      sha256 "6b5a8fce37917effaa6217a086e729c7a1a938dcd56ea32509568eea541c56b7"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.7/plantool-linux-arm64.tar.gz"
      sha256 "839f41f6202df0ab7f1660c91f1a3baca321ec744e20fc98ce9394ffd2c58f45"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.7/plantool-linux-x64.tar.gz"
      sha256 "ecbb13f0b613f95c1cf2f87a136b950f731e98d6db0ac40a9f59439cafdb59d7"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

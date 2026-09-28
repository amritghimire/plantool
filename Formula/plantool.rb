class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.11"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.11/plantool-darwin-arm64.tar.gz"
      sha256 "0ab156296f8204307b18b171ab04c165e3dd0f3ae2276b7ecff2f5f8cbffcfb0"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.11/plantool-darwin-x64.tar.gz"
      sha256 "c80b6b09c95a5348d3663f0a8f88b16da7ffed15a4a0cd37a3f7007f3f2ccdd0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.11/plantool-linux-arm64.tar.gz"
      sha256 "a38bc7c2d3e9438a8c87503bc4ebe538bcf37c3f1e90a374537f1f605bed11e3"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.11/plantool-linux-x64.tar.gz"
      sha256 "502465698f2a4ab63bf86ae903ebb197c3a5715b3d002e1fd30e51ca1d88e109"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

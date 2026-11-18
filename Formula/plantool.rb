class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.1.11"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.11/plantool-darwin-arm64.tar.gz"
      sha256 "54250cae7f1e7bf06c8aea144eeda70c9a1a7ea5b053361dca372849e8b79a22"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.11/plantool-darwin-x64.tar.gz"
      sha256 "e4158db9bd5920a9d38b87c93818418faffff49185ccdbe4e283f564480f752d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.11/plantool-linux-arm64.tar.gz"
      sha256 "96869ec4f30ac966f9ae6735d38fa95d2c53b5025a665d5364dd8050f9b54ad6"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.11/plantool-linux-x64.tar.gz"
      sha256 "178d6e2630cb71fddf9ed7fb4e65fd980c3d81d3fb4577a684f3e28dbc84674e"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

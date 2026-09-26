class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.2/plantool-darwin-arm64.tar.gz"
      sha256 "023f92f12b70807d2a66a4e06d0811f261e30a557b3dc28714b0211587be935a"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.2/plantool-darwin-x64.tar.gz"
      sha256 "80b5e272a1ba7e64881d0b8c9f4a9c6d92b2290bc97da4c1c65c54c8a665797a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.2/plantool-linux-arm64.tar.gz"
      sha256 "280ace3713e6a086c9704bca8d2e216ef45466c5000e90758e307fe0982115e6"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.2/plantool-linux-x64.tar.gz"
      sha256 "39d2363827c3c3a2b7e76f6a23af10d81c70a1f608987675832623fd0cca4773"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

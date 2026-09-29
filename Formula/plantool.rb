class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.13"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.13/plantool-darwin-arm64.tar.gz"
      sha256 "8fa64e9e1e89a99a8f3cfe8f335233b11163ec2cafa9b0e19be530ef62d44efc"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.13/plantool-darwin-x64.tar.gz"
      sha256 "365e11d5f734808755a8be8eeb0c118de938f344afbdfbf56143874576a7fcb8"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.13/plantool-linux-arm64.tar.gz"
      sha256 "cb1e8b7abbed57430b6a333b8d938d765f10984ca2d74af68b65a7acde14b422"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.13/plantool-linux-x64.tar.gz"
      sha256 "b9e848e60d22c913123ffd53d0cfe2806047b125209a75d6c2a7bdaeff1d67d8"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

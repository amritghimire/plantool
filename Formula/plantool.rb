class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.16"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.16/plantool-darwin-arm64.tar.gz"
      sha256 "c3b73a24c3d40d7bcf995273e42170fbec56dc4c55b4816fdbde07d65070338f"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.16/plantool-darwin-x64.tar.gz"
      sha256 "2f3c6892bbe0d104b750268de8afb7521896c9e54a486836db491b88f36403e9"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.16/plantool-linux-arm64.tar.gz"
      sha256 "f36bf6ee837b99d41bfe7d1b4448f87e5212752873d9a0d2c610cf2a8f90d7a7"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.16/plantool-linux-x64.tar.gz"
      sha256 "fa60c9a8007bb67fe0b54447718e46d9f91452cadf1d051bc7aa41d5b1d97cf8"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

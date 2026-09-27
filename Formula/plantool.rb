class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.8"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.8/plantool-darwin-arm64.tar.gz"
      sha256 "3fa13e380378a344915a947dc3245e21d9e823b9db907b0770b0ba5301a4bd24"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.8/plantool-darwin-x64.tar.gz"
      sha256 "5c7ae19458f6801740e8042f238c3d4596b6c5256fb0e4c6817280c4b69ba52d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.8/plantool-linux-arm64.tar.gz"
      sha256 "459dc3870b034eca34fd1edfa91ab96fc79ceee463d7b55f5b8d60c0a69ac73c"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.8/plantool-linux-x64.tar.gz"
      sha256 "7878192c60f542196cd175134c87003c2247a3956e9411b4e10389b5cc1e806a"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

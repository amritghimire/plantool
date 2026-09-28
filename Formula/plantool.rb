class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.10"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.10/plantool-darwin-arm64.tar.gz"
      sha256 "fac76d6d38f271a147760075fdbf44825188793a38c877cc84ea7b3e4e84968d"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.10/plantool-darwin-x64.tar.gz"
      sha256 "0aec594377880524876a8a8e4920b21de647edbbf05a3bf21b934627f14cca14"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.10/plantool-linux-arm64.tar.gz"
      sha256 "7bb8465a0eaadf63ce5e2b63994970776cb2d821d9236611bed75efbfcd6ccbd"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.10/plantool-linux-x64.tar.gz"
      sha256 "7af30cae2344d4d27d5a712f5fcd636a8469d60e64ebef398fb08e79acac2cd8"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

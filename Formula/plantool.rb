class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.4/plantool-darwin-arm64.tar.gz"
      sha256 "66b23387737815acd4b31701b1760ae0e8cdcc810090856541db4e8ddc2e0fcb"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.4/plantool-darwin-x64.tar.gz"
      sha256 "e92d78e6f73e15d095793740b33bb669f06972f84f64622a4e8bf39548a70cd1"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.4/plantool-linux-arm64.tar.gz"
      sha256 "712976a97a59dec927a81c5b3f62a88dceec9ffb6a35c5fca976a26983fcd19d"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.4/plantool-linux-x64.tar.gz"
      sha256 "13c8f074f91eed46b883d0c845ab6504c540a7498466757dde392eff2f0c3b6e"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

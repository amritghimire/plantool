class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.1.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.6/plantool-darwin-arm64.tar.gz"
      sha256 "5eaa37e2742fd1dd7fa548a88ff16666e8589ebc92978d954439005c2ea08597"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.6/plantool-darwin-x64.tar.gz"
      sha256 "8572a5fb2d4641db409203609991e5511ce13fdc5302f94315f0d44d1425b798"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.6/plantool-linux-arm64.tar.gz"
      sha256 "c5b139e95194bbad24bade2b5ade6b01bd2bb08501941921b3372f30407c6839"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.6/plantool-linux-x64.tar.gz"
      sha256 "7f1f154edd94ec463b5a9fb0ccee048d46ba6148195f6f6e8bac76c72734ed8e"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

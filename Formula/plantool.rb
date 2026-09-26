class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.6"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.6/plantool-darwin-arm64.tar.gz"
      sha256 "c2e51f7e96c254a7d106a40163b362d3af578592161a72c1028e1170b1eb66b7"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.6/plantool-darwin-x64.tar.gz"
      sha256 "d42d6a859e4267a98c0d884f25258f10832467b7e0e9b09159af89d4081b7876"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.6/plantool-linux-arm64.tar.gz"
      sha256 "eed0e81dbeaaa1965a5ac2d30b25fe071c3624aefce39f0e05bccd9f19e63624"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.6/plantool-linux-x64.tar.gz"
      sha256 "33d9a322a76c8fb2d705db8ee126f24d12d65a56817ff74749d63a85a7257060"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

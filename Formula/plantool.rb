class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.9"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.9/plantool-darwin-arm64.tar.gz"
      sha256 "36e52ff1324a50e7bc57e94b28ff1277b40972c25fab6cd9d2ef53c0b9fd14b0"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.9/plantool-darwin-x64.tar.gz"
      sha256 "4f417700433dfa3889791de418ebb597a72d6d0ea9c1eb32d7bd1c9d9ff597d0"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.9/plantool-linux-arm64.tar.gz"
      sha256 "aa876a5dccd60cb2ffd6bb7ced3237177ffea7d5fea976d1f4fa91859932a669"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.9/plantool-linux-x64.tar.gz"
      sha256 "a4c6e4ef732c8c03934bbfc9d70d74f05b36a5f367df5154079ac45156a9af1f"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

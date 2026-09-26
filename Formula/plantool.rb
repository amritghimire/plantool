class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.3/plantool-darwin-arm64.tar.gz"
      sha256 "2352664124fdf504ee02f66c9a7b8cb536813892ade0d21420fcf0dfdab986d9"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.3/plantool-darwin-x64.tar.gz"
      sha256 "c186605a946fed02b072082b3257bc07b3a639a2c67ecc710eadf36430602e8e"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.3/plantool-linux-arm64.tar.gz"
      sha256 "c08fbd0fae3ed8943df241b1a1afd662d279eed030606566fdd396b13dfb7dfa"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.3/plantool-linux-x64.tar.gz"
      sha256 "99ddef9f05bbb3847da2798fd701f31cebb8ef4fb5b5f0a1cfca68c2e15dcf09"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

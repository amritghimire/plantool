class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.15"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.15/plantool-darwin-arm64.tar.gz"
      sha256 "5252740de39eed28d0f7e59ae6688aab3c70fa12806353ab848093742dfee88f"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.15/plantool-darwin-x64.tar.gz"
      sha256 "5195748aebd0d51217cadefeaa055cf93db5d66a05c29e5d1718b2fb9f997a30"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.15/plantool-linux-arm64.tar.gz"
      sha256 "a51376dc81cd38df31af01b3e871c65179703f2343f9c9e73b61449b8ae70e05"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.15/plantool-linux-x64.tar.gz"
      sha256 "3cdd83330103fa85e251637d56389d19fc9f37fd5d8fbf6e9e482282deb55083"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

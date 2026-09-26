class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.5/plantool-darwin-arm64.tar.gz"
      sha256 "1720e7f959aaeab76a38b635bded1d56a5dd151dc00a7907d7bab4e34a2d0096"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.5/plantool-darwin-x64.tar.gz"
      sha256 "bade6aed1d46885830619bd9754734fe6fd824ca8252bda8f625e9ea394c6126"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.5/plantool-linux-arm64.tar.gz"
      sha256 "e1cd7685847bc0d4788474986d4a10b6fcdeaec78e304d75cf887a00d6fc5d48"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.5/plantool-linux-x64.tar.gz"
      sha256 "9342adae3a000f8987182e5fddfe0d2f510cdbfb9aba8f0cd0fbb7592929f2e1"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

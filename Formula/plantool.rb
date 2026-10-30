class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.0/plantool-darwin-arm64.tar.gz"
      sha256 "82136ef5182b7d58e777b29ca047a62ed5005c956b5e2786b748f93f0419a010"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.0/plantool-darwin-x64.tar.gz"
      sha256 "5d8c5ae163c2c9941a97e2c40edf9f53d14824553f506b4d5f515f9b541cfa9d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.0/plantool-linux-arm64.tar.gz"
      sha256 "27792b7c95a097080c184b55cfc14f6e010bbcde995ffa4670d121e2235b2ebe"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/0.1.0/plantool-linux-x64.tar.gz"
      sha256 "ae3ebc6be706f52e83914f9f2c4098787f529c71f9833d3aed17b7df00ccea49"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/amritghimire/plantool"
  version "0.0.12"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.12/plantool-darwin-arm64.tar.gz"
      sha256 "c1d1f1b97bd17a05281cd03c2a918a28e780f9079c8d0188ca0cae8d05d401ab"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.12/plantool-darwin-x64.tar.gz"
      sha256 "33d515768279174471ceedfae9e9224e5b734b7f45a5c2b6cf41aeb706704b96"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.12/plantool-linux-arm64.tar.gz"
      sha256 "0db24b0ad623e871f8c0b164ca7c705c0de980474153bbe29554498b174582a7"
    end
    on_intel do
      url "https://github.com/amritghimire/plantool/releases/download/v0.0.12/plantool-linux-x64.tar.gz"
      sha256 "3658ccc02ef1e5f96df141cbfc7b029c059a31cb7ef10eac6f762b566f3821b4"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end

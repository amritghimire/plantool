#!/usr/bin/env bash
# Render Formula/plantool.rb from a release tag and its SHA256SUMS.txt.
#   scripts/render-formula.sh v0.1.0 dist/SHA256SUMS.txt > Formula/plantool.rb
set -euo pipefail
tag="$1"; sums="$2"
version="${tag#v}"
repo="amritghimire/plantool"
sum_for() { awk -v n="plantool-$1.tar.gz" '{ f=$2; sub(/^\*/, "", f); if (f == n) print $1 }' "$sums" | head -n 1; }
for p in darwin-arm64 darwin-x64 linux-x64 linux-arm64; do
  [ -n "$(sum_for "$p")" ] || { echo "missing checksum for $p in $sums" >&2; exit 1; }
done
cat <<RUBY
class Plantool < Formula
  desc "Research, plan and implement with your coding agent, reviewed in the browser"
  homepage "https://github.com/$repo"
  version "$version"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/$repo/releases/download/$tag/plantool-darwin-arm64.tar.gz"
      sha256 "$(sum_for darwin-arm64)"
    end
    on_intel do
      url "https://github.com/$repo/releases/download/$tag/plantool-darwin-x64.tar.gz"
      sha256 "$(sum_for darwin-x64)"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/$repo/releases/download/$tag/plantool-linux-arm64.tar.gz"
      sha256 "$(sum_for linux-arm64)"
    end
    on_intel do
      url "https://github.com/$repo/releases/download/$tag/plantool-linux-x64.tar.gz"
      sha256 "$(sum_for linux-x64)"
    end
  end

  def install
    bin.install "plantool"
  end

  test do
    assert_match "plantool", shell_output("#{bin}/plantool --version")
  end
end
RUBY

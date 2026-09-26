#!/bin/sh
# Install plantool: detect the platform, download the release archive and SHA256SUMS.txt,
# verify the checksum, and put the binary on your PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/amritghimire/plantool/main/install.sh | sh
#
# Environment:
#   PLANTOOL_VERSION       tag to install (default: the latest release)
#   PLANTOOL_INSTALL_DIR   where the binary goes (default: ~/.local/bin, or /usr/local/bin if writable and ~/.local/bin is not on PATH)
#   PLANTOOL_DOWNLOAD_BASE base URL holding the archives (default: the GitHub release for the tag)
set -eu

REPO="amritghimire/plantool"
BIN="plantool"

say() { printf '%s\n' "$*" >&2; }
die() { say "install: $*"; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }
need curl
need tar

os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
  Darwin) os=darwin ;;
  Linux) os=linux ;;
  MINGW*|MSYS*|CYGWIN*) die "on Windows run: irm https://raw.githubusercontent.com/$REPO/main/install.ps1 | iex" ;;
  *) die "unsupported OS $os" ;;
esac
case "$arch" in
  arm64|aarch64) arch=arm64 ;;
  x86_64|amd64) arch=x64 ;;
  *) die "unsupported architecture $arch" ;;
esac
plat="$os-$arch"
archive="plantool-$plat.tar.gz"

version="${PLANTOOL_VERSION:-}"
if [ -z "$version" ]; then
  tag_of() { sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1; }
  version="$(curl -fsSL -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null | tag_of || true)"
  if [ -z "$version" ]; then
    # No stable release yet: take the newest one, pre-releases included.
    version="$(curl -fsSL -H 'Accept: application/vnd.github+json' "https://api.github.com/repos/$REPO/releases?per_page=1" | tag_of)"
  fi
  [ -n "$version" ] || die "could not find a release of $REPO"
fi
base="${PLANTOOL_DOWNLOAD_BASE:-https://github.com/$REPO/releases/download/$version}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM
say "downloading plantool $version for $plat"
curl -fsSL "$base/$archive" -o "$tmp/$archive" || die "no archive for $plat at $base"
curl -fsSL "$base/SHA256SUMS.txt" -o "$tmp/SHA256SUMS.txt" || die "no SHA256SUMS.txt at $base"

expected="$(awk -v n="$archive" '{ f=$2; sub(/^\*/, "", f); if (f == n) print $1 }' "$tmp/SHA256SUMS.txt" | head -n 1)"
[ -n "$expected" ] || die "$archive is not listed in SHA256SUMS.txt"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$archive" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')"
else
  die "need sha256sum or shasum to verify the download"
fi
[ "$actual" = "$expected" ] || die "checksum mismatch for $archive (expected $expected, got $actual)"

tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/$BIN" ] || die "the archive does not contain $BIN"

dir="${PLANTOOL_INSTALL_DIR:-}"
if [ -z "$dir" ]; then
  case ":$PATH:" in
    *":$HOME/.local/bin:"*) dir="$HOME/.local/bin" ;;
    *) if [ -w /usr/local/bin ]; then dir=/usr/local/bin; else dir="$HOME/.local/bin"; fi ;;
  esac
fi
mkdir -p "$dir"
chmod +x "$tmp/$BIN"
if [ "$os" = darwin ] && command -v xattr >/dev/null 2>&1; then
  xattr -d com.apple.quarantine "$tmp/$BIN" 2>/dev/null || true
fi
mv -f "$tmp/$BIN" "$dir/$BIN.new" && mv -f "$dir/$BIN.new" "$dir/$BIN"
say "installed $("$dir/$BIN" --version) to $dir/$BIN"

case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    say ""
    say "$dir is not on your PATH. Add it, for example:"
    say "  echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac
say ""
say "next: cd into a repo and run   plantool new <slug>"

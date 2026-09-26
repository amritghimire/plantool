#!/usr/bin/env bash
# Build and package plantool for one or more targets. Each archive holds a single `plantool`
# binary (tar.gz keeps the exec bit; Windows gets plantool.exe in a zip). Writes dist/<archive>
# and appends to dist/SHA256SUMS.txt. Assumes web/dist is already built (npm run build in web/).
set -euo pipefail
cd "$(dirname "$0")/.."

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) targets=(aarch64-apple-darwin) ;;
    Darwin-x86_64) targets=(x86_64-apple-darwin) ;;
    Linux-x86_64) targets=(x86_64-unknown-linux-gnu) ;;
    Linux-aarch64) targets=(aarch64-unknown-linux-gnu) ;;
    *) echo "unknown host; pass a target triple" >&2; exit 1 ;;
  esac
fi

plat_for() {
  case "$1" in
    aarch64-apple-darwin) echo darwin-arm64 ;;
    x86_64-apple-darwin) echo darwin-x64 ;;
    x86_64-unknown-linux-gnu|x86_64-unknown-linux-musl) echo linux-x64 ;;
    aarch64-unknown-linux-gnu|aarch64-unknown-linux-musl) echo linux-arm64 ;;
    x86_64-pc-windows-msvc) echo windows-x64 ;;
    *) echo "unsupported target $1" >&2; exit 1 ;;
  esac
}

[ -f web/dist/index.html ] || { echo "web/dist is missing; run: (cd web && npm ci && npm run build)" >&2; exit 1; }
mkdir -p dist
for target in "${targets[@]}"; do
  plat="$(plat_for "$target")"
  bin=plantool
  case "$plat" in windows-*) bin=plantool.exe ;; esac
  if command -v cross >/dev/null 2>&1 && [[ "$target" == *linux* ]] && [[ "$(uname -s)" != Linux || "$(uname -m)" != "${target%%-*}" ]]; then
    cross build --release --target "$target" -p plantool
  else
    cargo build --release --target "$target" -p plantool
  fi
  mkdir -p "dist/pkg/$plat"
  cp "target/$target/release/$bin" "dist/pkg/$plat/$bin"
  chmod +x "dist/pkg/$plat/$bin"
  case "$plat" in
    windows-*) archive="plantool-$plat.zip"; (cd "dist/pkg/$plat" && zip -q "../../$archive" "$bin") ;;
    *) archive="plantool-$plat.tar.gz"; tar -czf "dist/$archive" -C "dist/pkg/$plat" "$bin" ;;
  esac
  echo "packaged dist/$archive"
done
rm -rf dist/pkg
(cd dist && { command -v sha256sum >/dev/null && sha256sum plantool-* || shasum -a 256 plantool-*; } > SHA256SUMS.txt)
cat dist/SHA256SUMS.txt

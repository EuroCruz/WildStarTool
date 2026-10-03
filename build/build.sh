#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

targets=("$@")
if [ ${#targets[@]} -eq 0 ]; then
  host=$(rustc -vV | sed -n 's/^host: //p')
  [[ "$host" == *linux-gnu ]] && host=${host%-gnu}-musl
  targets=("$host")
fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

rm -rf dist
for t in "${targets[@]}"; do
  echo "==> $t"
  cargo build --release --locked --target "$t"
  out="dist/wildstartool-$version-$t"
  mkdir -p "$out"
  exe=wildstartool
  [[ "$t" == *windows* ]] && exe=wildstartool.exe
  cp "target/$t/release/$exe" "$out/"
  cp -r licenses "$out/"
  (cd "$out" && find . -type f ! -name SHA256SUMS | sort | sed 's|^\./||' | xargs sha256sum > SHA256SUMS)
done
echo "==> dist"

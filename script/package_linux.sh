#!/usr/bin/env bash
# 构建 Linux 发布包。
#
# 用法：./script/package_linux.sh
# 产物：dist/RareBox-Desktop-<版本>-linux-x64.tar.gz
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
STAGE="stage/RareBox-Desktop-$VERSION-linux-x64"

cargo build --release --bin rarebox-desktop

rm -rf "$STAGE"
mkdir -p "$STAGE/assets" dist
cp "target/release/rarebox-desktop" "$STAGE/"
cp -R "assets/icons" "$STAGE/assets/icons"

# 若工作流/本机已构建 hdc，则一并打包；否则应用会查找外部 hdc。
if [[ -x "vendor/hdc/linux/hdc" ]]; then
    cp "vendor/hdc/linux/hdc" "$STAGE/"
fi

tar -C stage -czf "dist/RareBox-Desktop-$VERSION-linux-x64.tar.gz" \
    "RareBox-Desktop-$VERSION-linux-x64"

echo "已生成 dist/RareBox-Desktop-$VERSION-linux-x64.tar.gz"

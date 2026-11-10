#!/usr/bin/env bash
# 构建浏览器版（WebUSB）。
#
# 用法：./script/build_web.sh
# 产物：dist/web/（静态站点；需要 HTTPS 或 localhost，WebUSB 才可用）
#
# 依赖：
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.130
set -euo pipefail

cd "$(dirname "$0")/.."

# 某些非登录 shell（CI、Git Bash）不会加载 ~/.cargo/env。
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi

VERSION=$(grep -m1 '^version = ' Cargo.toml | cut -d'"' -f2)
OUT="dist/web"

cargo build --release --locked -p rarebox-web --target wasm32-unknown-unknown

rm -rf "$OUT"
mkdir -p "$OUT"

wasm-bindgen \
  --target web \
  --out-dir "$OUT" \
  --out-name rarebox_web \
  target/wasm32-unknown-unknown/release/rarebox_web.wasm

cp -R web/. "$OUT/"
mkdir -p "$OUT/icons"
cp assets/icons/*.svg "$OUT/icons/"

echo "已生成 $OUT (RareBox Web $VERSION)"
echo "本地预览：python3 -m http.server 8080 --directory $OUT"

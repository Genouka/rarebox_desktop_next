#!/usr/bin/env bash
# 在 macOS 上构建未签名的 iOS .ipa。
#
# 用法：./script/package_ios.sh [debug|release]
# 产物：dist/RareBox-Desktop-<版本>-ios-unsigned.ipa
#
# 依赖：Xcode 15+、XcodeGen（brew install xcodegen）、Rust 目标 aarch64-apple-ios。
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${1:-release}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
DIST="dist"
OUT="$DIST/RareBox-Desktop-$VERSION-ios-unsigned.ipa"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "iOS 只能在 macOS + Xcode 上构建。" >&2
    exit 1
fi

rustup target add aarch64-apple-ios >/dev/null 2>&1 || true

if [[ "$MODE" == "debug" ]]; then
    cargo build --lib --target aarch64-apple-ios
    XCODE_CONFIG="Debug"
else
    cargo build --lib --release --target aarch64-apple-ios
    XCODE_CONFIG="Release"
fi

command -v xcodegen >/dev/null 2>&1 || {
    echo "缺少 xcodegen，请先执行：brew install xcodegen" >&2
    exit 1
}

(
    cd ios
    xcodegen generate --spec project.yml
    xcodebuild \
        -project RareBoxDesktop.xcodeproj \
        -scheme RareBoxDesktop \
        -configuration "$XCODE_CONFIG" \
        -sdk iphoneos \
        -derivedDataPath build \
        CODE_SIGNING_ALLOWED=NO \
        CODE_SIGNING_REQUIRED=NO \
        CODE_SIGN_IDENTITY="" \
        build
)

APP="ios/build/Build/Products/$XCODE_CONFIG-iphoneos/RareBoxDesktop.app"
if [[ ! -d "$APP" ]]; then
    echo "未找到构建产物：$APP" >&2
    exit 1
fi

rm -rf "$DIST/ios"
mkdir -p "$DIST/ios/Payload"
cp -R "$APP" "$DIST/ios/Payload/"
rm -f "$OUT"
(
    cd "$DIST/ios"
    zip -qry "../../$OUT" Payload
)
echo "已生成 $OUT（未签名）"

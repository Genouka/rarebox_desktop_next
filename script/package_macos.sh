#!/usr/bin/env bash
# 在 macOS 上构建通用（Apple Silicon + Intel）应用包。
#
# 用法：./script/package_macos.sh
# 产物：dist/RareBox Desktop.app 与 dist/RareBox-Desktop-<版本>-macos-universal.zip
#
# 说明：macOS 版需要 Apple 平台工具链（Xcode SDK）：gpui 链接 Metal/AppKit，
# ring 与 openh264-sys2 需要编译 C/C++ 与汇编。普通 Windows/Linux Rust 工具链
# 无法提供这些框架与头文件，请在本机或 CI 的 macOS runner 上执行本脚本。
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="RareBox Desktop"
BUNDLE_ID="top.qiumingsanyu.rarebox"
BIN_NAME="rarebox-desktop"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
DIST_DIR="dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"

export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --target "$target"
done

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources/assets"

lipo -create \
    "target/aarch64-apple-darwin/release/$BIN_NAME" \
    "target/x86_64-apple-darwin/release/$BIN_NAME" \
    -output "$APP_DIR/Contents/MacOS/$BIN_NAME"

cp -R assets/icons "$APP_DIR/Contents/Resources/assets/icons"

# 内置 hdc：优先用 script/build_hdc.sh macos 产出的通用二进制；没有就跳过，
# 应用仍会回退到 DevEco Studio / OpenHarmony SDK 的 hdc。
if [[ -x "vendor/hdc/macos/hdc" ]]; then
    cp "vendor/hdc/macos/hdc" "$APP_DIR/Contents/MacOS/hdc"
    chmod +x "$APP_DIR/Contents/MacOS/hdc"
elif [[ -x "vendor/hdc/macos/aarch64/hdc" && -x "vendor/hdc/macos/x86_64/hdc" ]]; then
    lipo -create "vendor/hdc/macos/aarch64/hdc" "vendor/hdc/macos/x86_64/hdc" \
        -output "$APP_DIR/Contents/MacOS/hdc"
    chmod +x "$APP_DIR/Contents/MacOS/hdc"
else
    echo "提示：vendor/hdc/macos/hdc 不存在，macOS 包将依赖外部 hdc。" >&2
fi

cat > "$APP_DIR/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleExecutable</key><string>$BIN_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# 本地 ad-hoc 签名：未签名 .app 在 Apple Silicon 上无法启动。分发时请换成开发者证书签名与公证。
codesign --force --deep --sign - "$APP_DIR"

rm -f "$DIST_DIR/RareBox-Desktop-$VERSION-macos-universal.zip"
ditto -c -k --keepParent "$APP_DIR" "$DIST_DIR/RareBox-Desktop-$VERSION-macos-universal.zip"
echo "已生成 $DIST_DIR/RareBox-Desktop-$VERSION-macos-universal.zip"

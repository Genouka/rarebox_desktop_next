#!/usr/bin/env bash
# 构建 Android APK。
#
# 用法：./script/package_android.sh [debug|release]
# 产物：dist/RareBox-Desktop-<版本>-android.apk
#
# 依赖：
#   - Android SDK（platform 35、build-tools）与 NDK 27.x
#   - Rust 目标：aarch64-linux-android（手机）、x86_64-linux-android（模拟器）
#   - cargo-ndk：cargo install cargo-ndk
#
# 可用环境变量覆盖：
#   ANDROID_SDK_ROOT / ANDROID_NDK_HOME、RAREBOX_ANDROID_ABIS（默认 arm64-v8a,x86_64）、
#   RAREBOX_ANDROID_API（默认 26）。
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${1:-release}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
ABIS="${RAREBOX_ANDROID_ABIS:-arm64-v8a,x86_64}"
API="${RAREBOX_ANDROID_API:-26}"

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
        ANDROID_NDK_HOME="$(ls -d "$ANDROID_SDK_ROOT"/ndk/* 2>/dev/null | sort -V | tail -n 1)"
    elif [[ -n "${LOCALAPPDATA:-}" ]]; then
        ANDROID_SDK_ROOT="$LOCALAPPDATA/Android/Sdk"
        ANDROID_NDK_HOME="$(ls -d "$ANDROID_SDK_ROOT"/ndk/* 2>/dev/null | sort -V | tail -n 1)"
    fi
fi
# 优先使用项目锁定的 NDK 27.2.12479018（与 CI 一致；避免本机更新的半安装 NDK）。
if [[ -d "${ANDROID_SDK_ROOT:-}/ndk/27.2.12479018" ]]; then
    ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
fi
if [[ -z "${ANDROID_NDK_HOME:-}" || ! -d "$ANDROID_NDK_HOME" ]]; then
    echo "找不到 Android NDK，请设置 ANDROID_NDK_HOME（或 ANDROID_SDK_ROOT）。" >&2
    exit 1
fi
export ANDROID_NDK_HOME
if [[ -z "${ANDROID_SDK_ROOT:-}" ]]; then
    ANDROID_SDK_ROOT="${ANDROID_NDK_HOME%/ndk/*}"
    export ANDROID_SDK_ROOT
fi

echo "NDK: $ANDROID_NDK_HOME"
echo "ABIs: $ABIS (API $API)"

# cargo-ndk 把每个 ABI 的 .so 放进 android/app/src/main/jniLibs/<abi>/。
cargo ndk --platform "$API" -t "$ABIS" -o android/app/src/main/jniLibs build --lib --release

# openh264 等 C++ 依赖需要 libc++_shared.so；NDK 不会自动把它放进 APK。
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) HOST_TAG="windows-x86_64" ;;
    Darwin) HOST_TAG="darwin-x86_64" ;;
    *) HOST_TAG="linux-x86_64" ;;
esac
for abi in ${ABIS//,/ }; do
    case "$abi" in
        arm64-v8a) triple="aarch64-linux-android" ;;
        armeabi-v7a) triple="armv7a-linux-androideabi" ;;
        x86_64) triple="x86_64-linux-android" ;;
        x86) triple="i686-linux-android" ;;
        *) echo "未知 ABI: $abi" >&2; exit 1 ;;
    esac
    cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/sysroot/usr/lib/$triple/libc++_shared.so" \
        "android/app/src/main/jniLibs/$abi/"
done

# 内置 adb（libadb.so + 依赖库）：Android 端不再依赖 Termux。
# 需要联网抓 Termux 包；设置 RAREBOX_SKIP_ANDROID_TOOLS=1 可以跳过。
if [[ -z "${RAREBOX_SKIP_ANDROID_TOOLS:-}" ]]; then
    RAREBOX_ANDROID_ABIS="$ABIS" bash ./script/fetch_android_tools.sh
fi

# 内置 hdc（libhdc.so + OpenSSL/libusb）：从 vendor/hdc 源码用 NDK 交叉编译。
# 首次运行会下载 Termux 的 OpenSSL/libusb；设置 RAREBOX_SKIP_ANDROID_HDC=1 可跳过。
if [[ -z "${RAREBOX_SKIP_ANDROID_HDC:-}" ]]; then
    IFS=',' read -r -a ABI_LIST <<< "$ABIS"
    bash ./script/build_hdc.sh android "${ABI_LIST[@]}"
fi

(
    cd android
    if [[ ! -f local.properties && -n "${ANDROID_SDK_ROOT:-}" ]]; then
        # Gradle 需要 local.properties 定位 SDK；该文件已被 .gitignore 忽略。
        printf 'sdk.dir=%s\n' "${ANDROID_SDK_ROOT//\\/\\\\}" > local.properties
    fi
    sh ./gradlew "assemble${MODE^}" --no-daemon
)

APK="android/app/build/outputs/apk/$MODE/app-$MODE.apk"
mkdir -p dist
cp "$APK" "dist/RareBox-Desktop-$VERSION-android.apk"
echo "已生成 dist/RareBox-Desktop-$VERSION-android.apk"

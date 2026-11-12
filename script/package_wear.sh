#!/usr/bin/env bash
# 构建 Wear OS 客户端 APK（Compose for Wear OS + rarebox-ffi）。
#
# 用法：./script/package_wear.sh [debug|release]
# 产物：dist/RareBox-Wear-<版本>-<mode>.apk
#
# 依赖：
#   - Android SDK（platform 35 / 36、build-tools）与 NDK 27.x
#   - Rust 目标：aarch64-linux-android、x86_64-linux-android
#   - cargo-ndk：cargo install cargo-ndk
#   - JDK 17-21（Gradle/AGP 不支持 JDK 24）
#
# 可用环境变量覆盖：
#   ANDROID_SDK_ROOT / ANDROID_NDK_HOME、RAREBOX_WATCH_ABIS（默认 arm64-v8a,x86_64）、
#   RAREBOX_WATCH_API（默认 26）、JAVA_HOME。
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${1:-debug}"
case "$MODE" in
    debug|release) ;;
    *) echo "用法：$0 [debug|release]" >&2; exit 1 ;;
esac
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
ABIS="${RAREBOX_WATCH_ABIS:-arm64-v8a,x86_64}"
API="${RAREBOX_WATCH_API:-26}"

# 与 package_android.sh 相同的 SDK / NDK 定位逻辑。
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    if [[ -n "${ANDROID_SDK_ROOT:-}" ]]; then
        ANDROID_NDK_HOME="$(ls -d "$ANDROID_SDK_ROOT"/ndk/* 2>/dev/null | sort -V | tail -n 1)"
    elif [[ -n "${LOCALAPPDATA:-}" ]]; then
        ANDROID_SDK_ROOT="$LOCALAPPDATA/Android/Sdk"
        ANDROID_NDK_HOME="$(ls -d "$ANDROID_SDK_ROOT"/ndk/* 2>/dev/null | sort -V | tail -n 1)"
    fi
fi
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

# 1) 引擎库（dist/jniLibs/<abi>/librarebox_ffi.so + libc++_shared.so）。
RAREBOX_WATCH_ABIS="$ABIS" RAREBOX_ANDROID_API="$API" ./script/build_watch_lib.sh

# 2) 组装 Wear 使用的 jniLibs：引擎 + 内置 adb/hdc 及其依赖（排除 GPUI/桌面库）。
TARGET="dist/wear-jniLibs"
rm -rf "$TARGET"
for abi in ${ABIS//,/ }; do
    mkdir -p "$TARGET/$abi"
    cp dist/jniLibs/"$abi"/*.so "$TARGET/$abi"/
    if [[ -d android/app/src/main/jniLibs/$abi ]]; then
        cp android/app/src/main/jniLibs/"$abi"/*.so "$TARGET/$abi"/
    fi
    rm -f "$TARGET/$abi/librarebox_desktop.so" "$TARGET/$abi/libgpui_mobile.so"
done

# 3) Gradle。
(
    cd wear
    if [[ ! -f local.properties && -n "${ANDROID_SDK_ROOT:-}" ]]; then
        printf 'sdk.dir=%s\n' "${ANDROID_SDK_ROOT//\\/\\\\}" > local.properties
    fi
    if [[ -x ./gradlew ]] && ./gradlew --version >/dev/null 2>&1; then
        ./gradlew "assemble${MODE^}" --no-daemon
    else
        cmd //c "gradlew.bat assemble${MODE^} --no-daemon"
    fi
)

APK="wear/app/build/outputs/apk/$MODE/app-$MODE.apk"
mkdir -p dist
cp "$APK" "dist/RareBox-Wear-$VERSION-$MODE.apk"
echo "已生成 dist/RareBox-Wear-$VERSION-$MODE.apk"

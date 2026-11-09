#!/usr/bin/env bash
# 构建可嵌入 Wear OS / Android 客户端的引擎库（librarebox_ffi.so）。
set -euo pipefail
cd "$(dirname "$0")/.."

ABIS="${RAREBOX_WATCH_ABIS:-arm64-v8a,x86_64}"
API="${RAREBOX_ANDROID_API:-26}"
OUT="${RAREBOX_WATCH_OUT:-dist/jniLibs}"

if [ -z "$(command -v cargo-ndk)" ]; then
    echo "需要 cargo-ndk：cargo install cargo-ndk" >&2
    exit 1
fi
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    echo "请设置 ANDROID_NDK_HOME（NDK 27.x 推荐）" >&2
    exit 1
fi

cargo ndk --platform "$API" -t "$ABIS" -o "$OUT" build -p rarebox-ffi --release

# 引擎（OpenH264）依赖 libc++_shared.so；APK 必须随包分发。
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) HOST_TAG="windows-x86_64" ;;
    Darwin) HOST_TAG="darwin-x86_64" ;;
    *) HOST_TAG="linux-x86_64" ;;
esac
SYSROOT_LIB="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/sysroot/usr/lib"
for abi in ${ABIS//,/ }; do
    case "$abi" in
        arm64-v8a) triple="aarch64-linux-android" ;;
        armeabi-v7a) triple="arm-linux-androideabi" ;;
        x86_64) triple="x86_64-linux-android" ;;
        x86) triple="i686-linux-android" ;;
    esac
    cp "$SYSROOT_LIB/$triple/libc++_shared.so" "$OUT/$abi/"
done

echo "已生成 $OUT/<abi>/librarebox_ffi.so"
echo "Kotlin 封装：bindings/kotlin/com/rarebox/core/CoreEngine.kt"

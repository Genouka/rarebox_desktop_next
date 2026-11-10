#!/usr/bin/env bash
# 为 HAP 构建内置原生库（aarch64-unknown-linux-ohos）并放入 ohos/entry/libs/arm64-v8a/。
#
# 用法：bash script/build_ohos_libs.sh [--debug]
# 产物：ohos/entry/libs/arm64-v8a/librarebox_ohos.so、libgpui_ohos.so（crate 存在才构建）
#
# 说明：hvigor 会把 entry/libs/<abi>/*.so 原样打进 HAP 的 libs/<abi>/，
#       所以这里不需要 CMake，也不需要 DevEco 工程声明 externalNativeOptions。
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=aarch64-unknown-linux-ohos
PROFILE_DIR=release
build_args=(--release)
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE_DIR=debug
    build_args=()
fi

crates=()
[[ -f crates/rarebox-ohos/Cargo.toml ]] && crates+=(-p rarebox-ohos)
[[ -f crates/gpui-ohos/Cargo.toml ]] && crates+=(-p gpui-ohos)
if (( ${#crates[@]} == 0 )); then
    echo "没有可构建的 OHOS 原生库（rarebox-ohos / gpui-ohos 均不存在）。" >&2
    exit 1
fi

# release 配置是 codegen-units=1，单个 rustc 峰值内存很高；并发数默认压到 2，
# 否则在 8GB 机器上会直接 OOM（rustc-LLVM ERROR: out of memory / 页面文件太小 1455）。
# 机器内存足够时可用 OHOS_BUILD_JOBS=8 覆盖。
jobs="${OHOS_BUILD_JOBS:-2}"
bash script/ohos_cross.sh cargo build --target "$TARGET" "${build_args[@]}" -j "$jobs" "${crates[@]}"

out="ohos/entry/libs/arm64-v8a"
mkdir -p "$out"
found=0
for name in librarebox_ohos.so libgpui_ohos.so; do
    if [[ -f "target/$TARGET/$PROFILE_DIR/$name" ]]; then
        cp -f "target/$TARGET/$PROFILE_DIR/$name" "$out/"
        found=1
    fi
done
if (( ! found )); then
    echo "构建完成但 target/$TARGET/$PROFILE_DIR 下没有预期 .so（检查 crate-type）。" >&2
    exit 1
fi

echo "已拷贝原生库到 $out："
ls -l "$out"/*.so

#!/usr/bin/env bash
# 鸿蒙门禁：Rust 交叉 check → 原生库 → HAP 打包。CI 与本地共用。
# 用法：bash script/check_ohos.sh [--skip-hap]
set -euo pipefail
cd "$(dirname "$0")/.."

packages=(-p rarebox-core -p rarebox-ohos)
if [[ -f crates/gpui-ohos/Cargo.toml ]]; then
    packages+=(-p gpui-ohos)
fi

echo "==> [1/3] Rust cross check (aarch64-unknown-linux-ohos)"
bash script/ohos_cross.sh cargo check --target aarch64-unknown-linux-ohos "${packages[@]}"

if [[ "${1:-}" == "--skip-hap" ]]; then
    echo "==> [2/3] native libs skipped"
    echo "==> [3/3] HAP build skipped"
    exit 0
fi

echo "==> [2/3] Build native libs for HAP"
bash script/build_ohos_libs.sh

echo "==> [3/3] HAP assembleHap"
bash script/package_ohos.sh

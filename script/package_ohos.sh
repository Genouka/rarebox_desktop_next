#!/usr/bin/env bash
# 构建未签名的 HarmonyOS HAP（Stage 模型）。
#
# 用法：bash script/package_ohos.sh
# 产物：dist/ohos/*.hap（未签名）
#
# 依赖：本机 DevEco Studio 26（默认 C:/Program Files/Huawei/DevEco Studio，
#       可用 DEVECO_HOME 覆盖）。node / hvigor / ohpm 全部取自 DevEco 自带工具，
#       无需全局安装，也不需要联网下载 @ohos/hvigor：
#       ohos/hvigor/hvigor-config.json5 的 dependencies 为空时，hvigor 直接复用
#       DevEco 内置的 @ohos/hvigor 6.26.9 与 @ohos/hvigor-ohos-plugin 6.26.9
#       （modelVersion 26.0.0，与 HarmonyOS SDK 26.0.0 对齐）。
#
# 说明：同一脚本同时兼容 Git Bash / MSYS 和 WSL。WSL 下无法直接调用 Windows 版
#       node.exe 的 POSIX 路径参数，因此通过一份临时 .bat 在 Windows 侧执行 hvigorw。
set -euo pipefail

# 定位仓库根目录（只做字符串处理，不依赖相对路径 cd）。
SELF="$0"
if command -v realpath >/dev/null 2>&1; then
    SELF="$(realpath -- "$SELF")"
fi
case "$SELF" in
    /*|[A-Za-z]:[\\/]*) ;;
    *) SELF="$PWD/$SELF" ;;
esac
ROOT="${SELF%/*}"
ROOT="${ROOT%/*}"
cd "$ROOT"

# 两条运行路径：
#   1. CLI tools（GitHub Actions / 纯 Linux）：ErBWs/setup-ohos 导出 HOS_SDK_HOME，
#      布局是 <cli>/command-line-tools/{bin,tool/node,sdk}。
#   2. DevEco Studio（Windows / WSL）：node / hvigor / ohpm 都在 <DevEco>/tools 下。
cli_home="${OHOS_CLI_HOME:-}"
if [[ -z "$cli_home" && -n "${HOS_SDK_HOME:-}" ]]; then
    cli_home="${HOS_SDK_HOME%/sdk}"
fi

DEVECO_HOME_DEFAULT_WIN='C:\Program Files\Huawei\DevEco Studio'

if [[ -n "$cli_home" ]]; then
    cli_home="${cli_home%/}"
    # Windows 的 CLI tools 才有 hvigorw.bat；Linux 只有 hvigorw 脚本，
    # 因此 .bat 不在必检清单里，存在时在下方优先选用。
    for path in "$cli_home/bin/hvigorw" \
        "$cli_home/tool/node/bin" "$cli_home/sdk"; do
        if [[ ! -e "$path" ]]; then
            echo "缺少 CLI tools 组件：$path" >&2
            exit 1
        fi
    done
    export DEVECO_SDK_HOME="$cli_home/sdk"
    export PATH="$cli_home/bin:$cli_home/tool/node/bin:$PATH"
    # Windows 的 CLI tools 带 hvigorw.bat；Linux 是无扩展名脚本。
    if [[ -f "$cli_home/bin/hvigorw.bat" ]]; then
        HVIGORW="$cli_home/bin/hvigorw.bat"
    else
        HVIGORW="$cli_home/bin/hvigorw"
    fi
    (
        cd "$ROOT/ohos"
        "$HVIGORW" assembleHap --no-daemon
    )
else
    case "$(uname -s)" in
        Linux*) BUILD_HOST=wsl ;;
        Windows*|MINGW*|MSYS*|CYGWIN*) BUILD_HOST=windows ;;
        *)
            echo "仅支持 Windows / WSL，或设置 HOS_SDK_HOME 指向 CLI tools。" >&2
            exit 1
            ;;
    esac

    if [[ "$BUILD_HOST" == "wsl" ]]; then
        DEVECO_HOME_NATIVE="$(wslpath -u "${DEVECO_HOME:-$DEVECO_HOME_DEFAULT_WIN}")"
        DEVECO_HOME_WIN="$(wslpath -w "$DEVECO_HOME_NATIVE")"
    else
        DEVECO_HOME_WIN="${DEVECO_HOME:-C:/Program Files/Huawei/DevEco Studio}"
        DEVECO_HOME_WIN="${DEVECO_HOME_WIN%/}"
        if command -v cygpath >/dev/null 2>&1; then
            DEVECO_HOME_NATIVE="$(cygpath -u "$DEVECO_HOME_WIN")"
        else
            DEVECO_HOME_NATIVE="$DEVECO_HOME_WIN"
        fi
    fi

    for dir in "$DEVECO_HOME_NATIVE/sdk" "$DEVECO_HOME_NATIVE/tools/node" \
        "$DEVECO_HOME_NATIVE/tools/hvigor/bin" "$DEVECO_HOME_NATIVE/tools/ohpm/bin"; do
        if [[ ! -d "$dir" ]]; then
            echo "缺少 DevEco 组件目录：$dir" >&2
            exit 1
        fi
    done

    export NODE_HOME="$DEVECO_HOME_NATIVE/tools/node"
    export PATH="$DEVECO_HOME_NATIVE/tools/node:$DEVECO_HOME_NATIVE/tools/hvigor:$DEVECO_HOME_NATIVE/tools/hvigor/bin:$DEVECO_HOME_NATIVE/tools/ohpm/bin:$PATH"

    if [[ "$BUILD_HOST" == "wsl" ]]; then
        BUILD_BAT="$ROOT/ohos/.package_ohos_build.bat"
        trap 'rm -f "$BUILD_BAT"' EXIT
        {
            printf '@echo off\r\n'
            printf 'setlocal\r\n'
            printf 'set "DEVECO_SDK_HOME=%s"\r\n' "$DEVECO_HOME_WIN\\sdk"
            printf 'set "NODE_HOME=%s"\r\n' "$DEVECO_HOME_WIN\\tools\\node"
            printf 'set "PATH=%s;%s;%s;%%PATH%%"\r\n' \
                "$DEVECO_HOME_WIN\\tools\\node" \
                "$DEVECO_HOME_WIN\\tools\\hvigor\\bin" \
                "$DEVECO_HOME_WIN\\tools\\ohpm\\bin"
            printf 'cd /d "%s"\r\n' "$(wslpath -w "$ROOT/ohos")"
            printf 'call "%s" assembleHap --no-daemon\r\n' "$DEVECO_HOME_WIN\\tools\\hvigor\\bin\\hvigorw.bat"
            printf 'exit /b %%ERRORLEVEL%%\r\n'
        } > "$BUILD_BAT"
        cmd.exe /d /c "$(wslpath -w "$BUILD_BAT")"
    else
        export DEVECO_SDK_HOME="$DEVECO_HOME_WIN/sdk"
        (
            cd "$ROOT/ohos"
            hvigorw assembleHap --no-daemon
        )
    fi
fi

HAP_DIR="$ROOT/ohos/entry/build/default/outputs/default"

mkdir -p "$ROOT/dist/ohos"
rm -f "$ROOT/dist/ohos/"*.hap

shopt -s nullglob
haps=("$HAP_DIR"/*.hap)
shopt -u nullglob
if (( ${#haps[@]} == 0 )); then
    echo "未找到 HAP 产物：$HAP_DIR/*.hap" >&2
    exit 1
fi

for hap in "${haps[@]}"; do
    cp -f "$hap" "$ROOT/dist/ohos/"
done

echo "已生成未签名 HAP："
ls -l "$ROOT/dist/ohos/"*.hap

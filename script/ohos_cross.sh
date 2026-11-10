#!/usr/bin/env bash
# Exports the OpenHarmony (OHOS) NDK toolchain, then runs the given command.
#
# Usage:
#   bash script/ohos_cross.sh cargo check --target aarch64-unknown-linux-ohos -p rarebox-core \
#       --config 'patch.crates-io.openh264-sys2.path="vendor/openh264-sys2"'
#
# What it sets (per target triple; aarch64-unknown-linux-ohos and
# x86_64-unknown-linux-ohos are configured):
#   CC_<triple> / CXX_<triple> / AR_<triple>   NDK clang / clang++ / llvm-ar
#   CFLAGS_<triple> / CXXFLAGS_<triple>        --sysroot=<ndk>/sysroot -fPIC -O3
#   CXXSTDLIB_<triple>                         c++_shared (OHOS ships libc++_shared.so,
#                                              it has no plain -lc++ in the sysroot)
#   CARGO_TARGET_<TRIPLE>_LINKER               NDK clang driver
#
# Notes:
# - On Windows the clang.exe driver is used instead of the extension-less
#   `aarch64-unknown-linux-ohos-clang` shell wrapper, which is not a Win32
#   executable. cc-rs appends `--target=<triple>` itself.
# - cc-rs splits CFLAGS on ASCII whitespace, so the SDK path must not contain
#   spaces. The script resolves the 8.3 short path on Windows, e.g.
#   C:/PROGRA~1/Huawei/DEVECO~1/sdk/default/OPENHA~1/native.
# - Real links (not `cargo check`) additionally need
#   `-L <ndk>/llvm/lib/<triple>` to find libc++_shared.so.
# - When invoked from WSL the script re-executes itself under the Windows
#   Git Bash, because the cargo/clang toolchain lives on the Windows side.
#
# Environment overrides:
#   OHOS_SDK     OHOS SDK root; the NDK is expected at "$OHOS_SDK/native"
#   OHOS_NDK     NDK root directly (wins over OHOS_SDK)
#   POWERSHELL   PowerShell executable used for short-path resolution
#   GIT_BASH     Windows bash.exe used for the WSL hand-off

set -euo pipefail

DEFAULT_SDK="C:/Program Files/Huawei/DevEco Studio/sdk/default/openharmony"

kernel="$(uname -s)"
case "$kernel" in
    *MINGW* | *MSYS* | *CYGWIN* | Windows_NT) platform=windows ;;
    Linux)
        if grep -qi microsoft /proc/version 2>&1; then
            platform=wsl
        else
            platform=posix
        fi
        ;;
    *) platform=posix ;;
esac

# --- WSL: hand over to the Windows toolchain ------------------------------
if [ "$platform" = wsl ]; then
    win_bash="${GIT_BASH:-}"
    if [ -z "$win_bash" ]; then
        for candidate in \
            "/mnt/c/Program Files/Git/bin/bash.exe" \
            "/mnt/c/Program Files/Git/usr/bin/bash.exe" \
            "/mnt/c/Program Files (x86)/Git/bin/bash.exe"; do
            if [ -x "$candidate" ]; then
                win_bash="$candidate"
                break
            fi
        done
    fi
    if [ -z "$win_bash" ]; then
        echo "ohos_cross.sh: running under WSL but no Windows Git Bash was found." >&2
        echo "  Set GIT_BASH to its bash.exe and retry." >&2
        exit 1
    fi
    self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
    win_self="$(wslpath -w "$self")"
    exec "$win_bash" "$win_self" "$@"
fi

# --- path helpers ---------------------------------------------------------
to_win() { # POSIX path -> Windows path (no-op on POSIX hosts)
    if [ "$platform" = windows ]; then
        local cyg
        cyg="$(command -v cygpath || true)"
        if [ -n "$cyg" ]; then
            "$cyg" -w "$1"
            return
        fi
    fi
    printf '%s\n' "$1"
}

short_path() { # Windows path -> space-free path with forward slashes
    local p="$1"
    if [ "$platform" = windows ]; then
        local ps="" out
        if [ -n "${POWERSHELL:-}" ]; then
            ps="$POWERSHELL"
        else
            ps="$(command -v powershell || true)"
            if [ -z "$ps" ]; then
                ps="$(command -v pwsh || true)"
            fi
        fi
        if [ -n "$ps" ]; then
            # The 8.3 short name drops the spaces from "Program Files" etc.
            out="$("$ps" -NoProfile -Command \
                "(New-Object -ComObject Scripting.FileSystemObject).GetFolder('$p').ShortPath" \
                | tr -d '\r\n')"
            # A valid short path never contains spaces; anything else is error text.
            case "$out" in
                "" | *" "*) ;;
                *) p="$out" ;;
            esac
        fi
        case "$p" in
            *" "*)
                echo "ohos_cross.sh: NDK path contains spaces and no 8.3 short name was available:" >&2
                echo "  $p" >&2
                echo "  Point OHOS_NDK at a space-free path (e.g. a junction) and retry." >&2
                exit 1
                ;;
        esac
    fi
    printf '%s\n' "$p" | tr '\\' '/'
}

# --- locate the NDK -------------------------------------------------------
# 输入可以是 NDK 根目录，也可以是任意一层 SDK 根目录（DevEco、CLI tools、
# ErBWs/setup-ohos 的 HOS_SDK_HOME 都不同层），这里统一向下探测。
resolve_ndk() { # $1 = 候选根目录；找到含 llvm/ 与 sysroot/ 的目录就输出
    local base="$1" cand
    for cand in "$base" "$base/native" "$base/openharmony" \
        "$base/openharmony/native" "$base"/*/openharmony \
        "$base"/*/openharmony/native; do
        if [ -d "$cand/llvm" ] && [ -d "$cand/sysroot" ]; then
            printf '%s\n' "$cand"
            return 0
        fi
    done
    return 1
}

ndk=""
for candidate in "${OHOS_NDK:-}" "${OHOS_NDK_HOME:-}" "${OHOS_SDK:-}" \
    "${HOS_SDK_HOME:-}" "$DEFAULT_SDK"; do
    [ -n "$candidate" ] || continue
    if resolved="$(resolve_ndk "$candidate")"; then
        ndk="$resolved"
        break
    fi
done
if [ -z "$ndk" ]; then
    echo "ohos_cross.sh: 找不到 NDK（llvm/ + sysroot/）。" >&2
    echo "  可设置 OHOS_NDK、OHOS_NDK_HOME、OHOS_SDK 或 HOS_SDK_HOME。" >&2
    exit 1
fi
ndk="$(short_path "$(to_win "$ndk")")"

if [ -f "$ndk/llvm/bin/clang.exe" ]; then
    clang="$ndk/llvm/bin/clang.exe"
    clangxx="$ndk/llvm/bin/clang++.exe"
    ar="$ndk/llvm/bin/llvm-ar.exe"
elif [ -f "$ndk/llvm/bin/clang" ]; then
    clang="$ndk/llvm/bin/clang"
    clangxx="$ndk/llvm/bin/clang++"
    ar="$ndk/llvm/bin/llvm-ar"
else
    echo "ohos_cross.sh: NDK toolchain not found under '$ndk' (llvm/bin/clang)." >&2
    echo "  Set OHOS_SDK (SDK root) or OHOS_NDK (NDK root) and retry." >&2
    exit 1
fi

sysroot="$ndk/sysroot"
if [ ! -d "$sysroot/usr" ]; then
    echo "ohos_cross.sh: NDK sysroot missing at '$sysroot'." >&2
    exit 1
fi

# --- export the toolchain -------------------------------------------------
# Lets cc-rs honour quoted CFLAGS values, so a long SDK path stays usable as
# a fallback even when 8.3 names are unavailable.
export CC_SHELL_ESCAPED_FLAGS=1

for triple in aarch64-unknown-linux-ohos x86_64-unknown-linux-ohos; do
    u="${triple//-/_}"
    export "CC_$u=$clang"
    export "CXX_$u=$clangxx"
    export "AR_$u=$ar"
    export "CFLAGS_$u=--sysroot=$sysroot -fPIC -O3"
    export "CXXFLAGS_$u=--sysroot=$sysroot -fPIC -O3"
    export "CXXSTDLIB_$u=c++_shared"
done

export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$clang"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_LINKER="$clang"

# `napi-build-ohos` 在宿主侧构建脚本里要求这个变量（它不能依赖 cfg(target_env)）。
export OHOS_NDK_HOME="$ndk"
# 真实链接（cargo build 而非 check）时，rustc 调 `$clang` **不会**自动带 `--target`，
# 于是 clang 会退回宿主默认目标（Windows 上就是 MinGW 的 ld，直接报
# `unrecognized option '--eh-frame-hdr'`）。这里显式补上目标三元组、sysroot 与 lld，
# 再补 -lc++_shared 需要的库搜索路径。
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_RUSTFLAGS:-} -C link-arg=--target=aarch64-unknown-linux-ohos -C link-arg=--sysroot=$sysroot -C link-arg=-fuse-ld=lld -C link-arg=-L$ndk/llvm/lib/aarch64-linux-ohos"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_RUSTFLAGS="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_OHOS_RUSTFLAGS:-} -C link-arg=--target=x86_64-unknown-linux-ohos -C link-arg=--sysroot=$sysroot -C link-arg=-fuse-ld=lld -C link-arg=-L$ndk/llvm/lib/x86_64-linux-ohos"

if [ "$#" -eq 0 ]; then
    echo "usage: bash script/ohos_cross.sh <command> [args...]" >&2
    exit 2
fi

exec "$@"

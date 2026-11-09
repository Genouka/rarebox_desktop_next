#!/usr/bin/env bash
# 从 vendor/hdc 源码构建各平台的内置 hdc。
#
# 用法：
#   ./script/build_hdc.sh [windows|linux|macos|android] [abi...]
#
# 产物：
#   windows -> vendor/hdc/windows/hdc.exe（及 OpenSSL / libusb DLL）
#   linux   -> vendor/hdc/linux/hdc
#   macos   -> vendor/hdc/macos/hdc
#   android -> android/app/src/main/jniLibs/<abi>/{libhdc.so,libssl.3.so,libcrypto.3.so,libusb-1.0.so}
#
# 依赖：
#   - Windows：Git Bash、MinGW-w64（CXX/CC/AR，默认 C:/Strawberry/c/bin）、
#     CMake、MinGW Make、OpenSSL 开发包（OPENSSL_DIR，默认 C:/Strawberry/c）。
#   - Linux：g++/clang++、pkg-config、libssl-dev、libuv1-dev、libusb-1.0-0-dev、liblz4-dev。
#   - macOS：Xcode 命令行工具、Homebrew 的 openssl@3 / libuv / libusb / lz4。
#   - Android：Android NDK（ANDROID_NDK_HOME 或 SDK 下最新版）、CMake、Python3、
#     curl。首次运行会从 Termux 镜像下载 OpenSSL 与 libusb 的 bionic 预编译库。
#
# 说明：hdc 源码位于 vendor/hdc，构建只读该目录；中间产物在 target/hdc-build。
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"
SRC="$ROOT/vendor/hdc"
BUILD="$ROOT/target/hdc-build"
API="${RAREBOX_ANDROID_API:-26}"
JOBS="${JOBS:-8}"

PLATFORM="${1:-}"
if [[ -z "$PLATFORM" ]]; then
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) PLATFORM=windows ;;
        Darwin) PLATFORM=macos ;;
        Linux) PLATFORM=linux ;;
        *) echo "无法识别平台：$(uname -s)" >&2; exit 1 ;;
    esac
fi
shift || true

PYTHON="${PYTHON:-}"
if [[ -z "$PYTHON" ]]; then
    for candidate in python3 python; do
        command -v "$candidate" >/dev/null 2>&1 && { PYTHON="$candidate"; break; }
    done
fi
[[ -n "$PYTHON" ]] || { echo "需要 python3。" >&2; exit 1; }

# ---------- 公共部分 ----------

generate_hash() {
    local out="$BUILD/gen"
    mkdir -p "$out"
    "$PYTHON" "$SRC/scripts/hdc_hash_gen.py" -f hdc_hash_gen.h -i "$SRC" -o "$out/" >/dev/null
    [[ -f "$out/hdc_hash_gen.h" ]] || { echo "生成 hdc_hash_gen.h 失败" >&2; exit 1; }
}

# 参与构建的源文件：common + host（不含设备端与 OHOS 专有实现）。
hdc_sources() {
    local f
    for f in "$SRC"/src/common/*.cpp; do
        case "$f" in
            *command_event_report.cpp|*connect_validation.cpp|*hdc_huks.cpp|*password.cpp|*credential_message.cpp) continue ;;
        esac
        printf '%s\n' "$f"
    done
    for f in "$SRC"/src/host/*.cpp; do
        case "$f" in
            *system_depend.cpp) continue ;;
        esac
        printf '%s\n' "$f"
    done
    for f in "$SRC"/src/host/subserver/*.cpp; do
        printf '%s\n' "$f"
    done
}

build_securec() { # <cc> <ar> <outdir>
    local cc="$1" ar="$2" out="$3" f
    mkdir -p "$out"
    for f in "$SRC"/third_party/bounds_checking_function/src/*.c; do
        "$cc" -O2 -fPIC -w -D_INC_STDIO_S -D_INC_WCHAR_S -c "$f" \
            -I"$SRC/third_party/bounds_checking_function/include" \
            -o "$out/$(basename "$f" .c).o"
    done
    "$ar" rcs "$out/libsecurec.a" "$out"/*.o
}

build_lz4() { # <cc> <ar> <outdir>
    local cc="$1" ar="$2" out="$3"
    mkdir -p "$out"
    "$cc" -O2 -fPIC -w -c "$SRC/third_party/lz4/lib/lz4.c" -I"$SRC/third_party/lz4/lib" \
        -o "$out/lz4.o"
    "$ar" rcs "$out/liblz4.a" "$out/lz4.o"
}

HDC_OBJS=()
compile_hdc() { # <cxx> <objdir> <defines...> -- <includes...>；结果写入 HDC_OBJS
    local cxx="$1" objdir="$2"
    shift 2
    local defines=() includes=()
    while [[ $# -gt 0 && "$1" != "--" ]]; do defines+=("$1"); shift; done
    [[ ${1:-} == "--" ]] && shift
    includes=("$@")
    mkdir -p "$objdir"
    HDC_OBJS=()
    local f out
    while IFS= read -r f; do
        out="$objdir/$(echo "${f#$SRC/}" | tr '/' '_' | sed 's/\.cpp$/.o/')"
        "$cxx" -std=c++17 -O2 -w -c "$f" "${defines[@]}" "${includes[@]}" -o "$out"
        HDC_OBJS+=("$out")
    done < <(hdc_sources)
}

# ---------- Windows ----------

build_windows() {
    local cxx="${HDC_CXX:-C:/Strawberry/c/bin/g++.exe}"
    local cc="${HDC_CC:-C:/Strawberry/c/bin/gcc.exe}"
    local ar="${HDC_AR:-C:/Strawberry/c/bin/ar.exe}"
    local make="${HDC_MAKE:-C:/Strawberry/c/bin/mingw32-make.exe}"
    local openssl="${OPENSSL_DIR:-C:/Strawberry/c}"
    [[ -x "$cxx" ]] || { echo "找不到 MinGW g++：$cxx" >&2; exit 1; }
    [[ -f "$openssl/include/openssl/ssl.h" ]] || {
        echo "找不到 OpenSSL 头文件（OPENSSL_DIR=$openssl）。" >&2; exit 1; }

    local out="$BUILD/windows"
    local uv="$out/libuv" libusb="$out/libusb" sec="$out/securec" lz4="$out/lz4"
    mkdir -p "$uv" "$libusb" "$sec" "$lz4"
    generate_hash

    echo "== libuv"
    cmake -S "$SRC/third_party/libuv" -B "$out/libuv-cmake" -G "MinGW Makefiles" \
        -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER="$cc" -DCMAKE_MAKE_PROGRAM="$make" \
        -DLIBUV_BUILD_TESTS=OFF -DLIBUV_BUILD_BENCH=OFF >/dev/null
    cmake --build "$out/libuv-cmake" --target uv_a -j "$JOBS" >/dev/null
    cp "$out/libuv-cmake/libuv.a" "$uv/libuv.a"
    # 上游 CMake 漏掉了 log/trace 适配层（OHOS fork 新增）。
    for f in log_win trace_win; do
        "$cc" -O2 -w -c "$SRC/third_party/libuv/src/win/$f.c" \
            -I"$SRC/third_party/libuv/include" -I"$SRC/third_party/libuv/src" \
            -DWIN32_LEAN_AND_MEAN -D_WIN32_WINNT=0x0A00 -o "$uv/$f.o"
    done
    "$ar" rcs "$uv/libuv.a" "$uv/log_win.o" "$uv/trace_win.o"

    echo "== libusb"
    cat > "$libusb/config.h" <<'CFG'
#ifndef LIBUSB_CONFIG_H
#define LIBUSB_CONFIG_H
#define PLATFORM_WINDOWS 1
#define ENABLE_LOGGING 1
#define DEFAULT_VISIBILITY
#define PRINTF_FORMAT(a, b)
#endif
CFG
    local src="$SRC/third_party/libusb/libusb"
    for f in core descriptor hotplug io strerror sync os/events_windows os/threads_windows \
             os/windows_common os/windows_usbdk os/windows_winusb; do
        "$cc" -O2 -w -c "$src/$f.c" -I"$src" -I"$src/.." -I"$libusb" \
            -o "$libusb/$(echo "$f" | tr '/' '_').o"
    done
    "$cc" -shared -o "$libusb/libusb-1.0.dll" "$libusb"/*.o "$src/libusb-1.0.def" \
        -Wl,--out-implib,"$libusb/libusb-1.0.dll.a" -lsetupapi -lole32 -static-libgcc

    echo "== securec / lz4"
    build_securec "$cc" "$ar" "$sec"
    build_lz4 "$cc" "$ar" "$lz4"

    echo "== hdc"
    compile_hdc "$cxx" "$out/obj" \
        -DHDC_HOST -DHARMONY_PROJECT -DHOST_MINGW -DUSE_CONFIG_UV_THREADS -DSIZE_THREAD_POOL=128 \
        -DOPENSSL_SUPPRESS_DEPRECATED -DHDC_SUPPORT_ENCRYPT_TCP -DMEMORY_POOL_ENABLE \
        -DHDC_SUPPORT_UART -D_WIN32_WINNT=0x0601 \
        -- -I"$SRC/src/common" -I"$SRC/src/host" -I"$BUILD/gen" \
        -I"$SRC/third_party/bounds_checking_function/include" \
        -I"$SRC/third_party/libusb" -I"$SRC/third_party/libusb/libusb" \
        -I"$SRC/third_party/libuv/include" -I"$SRC/third_party/lz4/lib" \
        -I"$openssl/include"
    "$cxx" -o "$out/hdc.exe" "${HDC_OBJS[@]}" "$sec/libsecurec.a" "$uv/libuv.a" \
        "$libusb/libusb-1.0.dll.a" "$lz4/liblz4.a" -L"$openssl/lib" -lssl -lcrypto \
        -Wl,-Bstatic -lwinpthread -Wl,-Bdynamic \
        -lws2_32 -lsetupapi -luserenv -liphlpapi -lpsapi -lole32 -lshell32 -ladvapi32 -ldbghelp \
        -static-libgcc -static-libstdc++

    local dest="$ROOT/vendor/hdc/windows"
    mkdir -p "$dest"
    cp "$out/hdc.exe" "$dest/hdc.exe"
    cp "$libusb/libusb-1.0.dll" "$dest/"
    # Strawberry 的 OpenSSL 是 DLL 导入库，运行时需要同名 DLL。
    local dll
    for dll in "$openssl"/bin/libcrypto-3*.dll "$openssl"/bin/libssl-3*.dll; do
        [[ -f "$dll" ]] && cp "$dll" "$dest/"
    done
    if command -v strip >/dev/null 2>&1; then strip "$dest/hdc.exe" || true; fi
    echo "  -> $dest"
}

# ---------- Linux / macOS ----------

build_unix() { # <platform>
    local platform="$1" cxx="${HDC_CXX:-c++}" cc="${HDC_CC:-cc}" ar="${HDC_AR:-ar}"
    local host_macro="HOST_LINUX"
    [[ "$platform" == macos ]] && host_macro="HOST_MAC"
    local pkgs="openssl libuv liblz4"
    local cflags libs
    cflags="$(pkg-config --cflags $pkgs libusb-1.0)"
    # 使用发行版的动态库，避免 OpenSSL 3 的静态依赖（jitterentropy/zstd/xxhash）
    # 把链接命令拖成一长串；发行包只需系统里有对应运行库。
    libs="$(pkg-config --libs openssl libuv libusb-1.0) -llz4"
    local out="$BUILD/$platform" sec="$BUILD/$platform/securec"
    # 交叉/自带 sysroot 时，multiarch 头文件目录（opensslconf.h）不在默认搜索路径。
    local triple multiarch=""
    triple="$("$cc" -dumpmachine 2>/dev/null || true)"
    if [[ -n "$triple" && -d "${PKG_CONFIG_SYSROOT_DIR:-}/usr/include/$triple" ]]; then
        multiarch="-I${PKG_CONFIG_SYSROOT_DIR:-}/usr/include/$triple"
    fi
    mkdir -p "$out" "$sec"
    generate_hash
    echo "== securec"
    build_securec "$cc" "$ar" "$sec"
    echo "== hdc"
    compile_hdc "$cxx" "$out/obj" \
        -DHDC_HOST -DHARMONY_PROJECT -D"$host_macro" -DUSE_CONFIG_UV_THREADS -DSIZE_THREAD_POOL=128 \
        -DOPENSSL_SUPPRESS_DEPRECATED -DHDC_SUPPORT_ENCRYPT_TCP -DMEMORY_POOL_ENABLE -DHDC_SUPPORT_UART \
        -Wno-c++11-narrowing -Wno-narrowing \
        -- -I"$SRC/src/common" -I"$SRC/src/host" -I"$BUILD/gen" \
        -I"$SRC/third_party/bounds_checking_function/include" \
        -I"$SRC/third_party/libusb" -I"$SRC/third_party/libusb/libusb" \
        $cflags $multiarch
    local extra="-lpthread -ldl"
    [[ "$platform" == macos ]] && extra="-lpthread"
    # shellcheck disable=SC2086
    "$cxx" -o "$out/hdc" "${HDC_OBJS[@]}" "$sec/libsecurec.a" $libs $extra
    local dest="$ROOT/vendor/hdc/$platform"
    mkdir -p "$dest"
    cp "$out/hdc" "$dest/hdc"
    strip "$dest/hdc" 2>/dev/null || true
    echo "  -> $dest"
}

# ---------- Android ----------

find_ndk() {
    if [[ -n "${ANDROID_NDK_HOME:-}" && -d "$ANDROID_NDK_HOME" ]]; then
        echo "$ANDROID_NDK_HOME"; return
    fi
    local sdk="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-}}"
    if [[ -z "$sdk" ]]; then
        if [[ -n "${LOCALAPPDATA:-}" ]]; then sdk="$LOCALAPPDATA/Android/Sdk"; else sdk="$HOME/Android/Sdk"; fi
    fi
    local candidate
    for candidate in $(ls -d "$sdk"/ndk/* 2>/dev/null | sort -rV); do
        [[ -d "$candidate/toolchains" ]] && { echo "$candidate"; return; }
    done
    echo "找不到 Android NDK，请设置 ANDROID_NDK_HOME。" >&2
    return 1
}

build_android() {
    local ndk host_tag
    ndk="$(find_ndk)" || exit 1
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) host_tag=windows-x86_64 ;;
        Darwin) host_tag=darwin-x86_64 ;;
        *) host_tag=linux-x86_64 ;;
    esac
    local bin="$ndk/toolchains/llvm/prebuilt/$host_tag/bin"
    local abis=("$@")
    [[ ${#abis[@]} -eq 0 ]] && abis=(arm64-v8a x86_64)
    generate_hash
    bash "$ROOT/script/fetch_hdc_android_deps.sh" "${abis[@]}"

    local abi arch triple termux_arch
    for abi in "${abis[@]}"; do
        case "$abi" in
            arm64-v8a) triple=aarch64-linux-android; termux_arch=aarch64 ;;
            armeabi-v7a) triple=armv7a-linux-androideabi; termux_arch=arm ;;
            x86_64) triple=x86_64-linux-android; termux_arch=x86_64 ;;
            x86) triple=i686-linux-android; termux_arch=i686 ;;
            *) echo "未知 ABI: $abi" >&2; exit 1 ;;
        esac
        echo "== Android $abi"
        local cc="$bin/${triple}${API}-clang" cxx="$bin/${triple}${API}-clang++" ar="$bin/llvm-ar"
        [[ -x "$cc" ]] || cc="$cc.cmd"
        [[ -x "$cxx" ]] || cxx="$cxx.cmd"
        local out="$BUILD/android-$abi" stage="$ROOT/target/hdc-deps/termux/$termux_arch/stage"
        local uv="$out/libuv" lz4o="$out/lz4" sec="$out/securec" obj="$out/obj" pkg="$out/pkg"
        mkdir -p "$out" "$uv" "$lz4o" "$sec" "$pkg"

        # libuv（NDK 现编，避免 Termux 的 libandroid-spawn 间接依赖）。
        local gen="Unix Makefiles" make_args=()
        case "$(uname -s)" in
            MINGW*|MSYS*|CYGWIN*) gen="MinGW Makefiles"
                [[ -x C:/Strawberry/c/bin/mingw32-make.exe ]] && make_args=(-DCMAKE_MAKE_PROGRAM=C:/Strawberry/c/bin/mingw32-make.exe) ;;
        esac
        cmake -S "$SRC/third_party/libuv" -B "$out/libuv-cmake" -G "$gen" "${make_args[@]}" \
            -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" \
            -DANDROID_ABI="$abi" -DANDROID_PLATFORM="$API" -DCMAKE_BUILD_TYPE=Release \
            -DLIBUV_BUILD_TESTS=OFF -DLIBUV_BUILD_BENCH=OFF >/dev/null
        cmake --build "$out/libuv-cmake" --target uv_a -j "$JOBS" >/dev/null
        cp "$out/libuv-cmake/libuv.a" "$uv/libuv.a"
        for f in log_unix trace_unix; do
            "$cc" -O2 -fPIC -w -c "$SRC/third_party/libuv/src/unix/$f.c" \
                -I"$SRC/third_party/libuv/include" -I"$SRC/third_party/libuv/src" \
                -D_GNU_SOURCE -o "$uv/$f.o"
        done
        "$ar" rcs "$uv/libuv.a" "$uv/log_unix.o" "$uv/trace_unix.o"

        build_securec "$cc" "$ar" "$sec"
        build_lz4 "$cc" "$ar" "$lz4o"

        compile_hdc "$cxx" "$obj" \
            -DHDC_HOST -DHARMONY_PROJECT -DHOST_LINUX -DUSE_CONFIG_UV_THREADS -DSIZE_THREAD_POOL=128 \
            -DOPENSSL_SUPPRESS_DEPRECATED -DHDC_SUPPORT_ENCRYPT_TCP -DMEMORY_POOL_ENABLE -DHDC_SUPPORT_UART \
            -Wno-c++11-narrowing -Wno-narrowing \
            -- -I"$SRC/src/common" -I"$SRC/src/host" -I"$BUILD/gen" \
            -I"$SRC/third_party/bounds_checking_function/include" \
            -I"$stage/include" -I"$stage/include/libusb-1.0" -I"$SRC/third_party/libusb" \
            -I"$SRC/third_party/libuv/include" -I"$SRC/third_party/lz4/lib"
        "$cxx" -o "$pkg/libhdc.so" "${HDC_OBJS[@]}" "$sec/libsecurec.a" "$uv/libuv.a" "$lz4o/liblz4.a" \
            "$stage/lib/libssl.so.3" "$stage/lib/libcrypto.so.3" "$stage/lib/libusb-1.0.so"
        cp "$stage/lib/libssl.so.3" "$stage/lib/libcrypto.so.3" "$stage/lib/libusb-1.0.so" "$pkg/"
        "$PYTHON" "$ROOT/script/patch_android_hdc_libs.py" "$pkg"
        "$bin/llvm-strip" --strip-unneeded "$pkg"/*.so 2>/dev/null || true

        local dest="$ROOT/android/app/src/main/jniLibs/$abi"
        mkdir -p "$dest"
        cp "$pkg"/*.so "$dest/"
        echo "  -> $dest"
    done
}

case "$PLATFORM" in
    windows) build_windows ;;
    linux|macos) build_unix "$PLATFORM" ;;
    android) build_android "$@" ;;
    *) echo "未知平台：$PLATFORM" >&2; exit 1 ;;
esac

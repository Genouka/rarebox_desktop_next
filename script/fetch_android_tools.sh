#!/usr/bin/env bash
# 获取 Android 端内置的 adb（可选 hdc）可执行文件，放进 jniLibs 以便随 APK 分发。
#
# 背景：Android 10+ 禁止应用在自己的数据目录里 exec 文件，但安装时解压到
# `nativeLibraryDir` 的原生库仍然可执行。因此这里把 adb 以 `libadb.so` 的名字
# 放进 `android/app/src/main/jniLibs/<abi>/`（Gradle 只打包 `*.so`，见
# `MergeNativeLibsTask` 的 `**/*.so` 过滤器），应用启动后直接执行该路径。
#
# 二进制来源：Termux 官方仓库的 `android-tools` 包（AOSP platform-tools 交叉编译，
#  bionic libc，可直接在普通 Android 应用里运行）。adb 动态依赖若干 Termux 包
#  （abseil-cpp / protobuf / brotli / fmt / lz4 / zstd / zlib / libc++），脚本会把
#  soname 闭包里真正用到的库一并复制进来，并把形如 `libz.so.1` 的版本化 soname
#  重命名成 `libz.1.so`（长度不变，便于原地改写 DT_NEEDED 字符串）。
#  Termux 的 libc++ 比 NDK 新（adb 需要 `__hash_memory` 等符号），所以它被改名成
#  等长的 `libcxx_termux.so` 只供这些工具使用，应用自身仍用 NDK 的 `libc++_shared.so`。
#
#  APK 侧要求 `packaging.jniLibs.useLegacyPackaging = true`（即
#  `android:extractNativeLibs="true"`），否则 .so 只留在 APK 里、`nativeLibraryDir`
#  为空，无法执行。见 android/app/build.gradle.kts。
#
#  每次运行都会把用到的包版本 / SHA256 / 仓库路径写进
#  `<cache>/<arch>/provenance.txt`，便于复现和排查。
#
#  hdc：OpenHarmony 没有面向 Android 的预编译版本，但本仓库已把 developtools_hdc
#  源码裁剪进 vendor/hdc，由 script/build_hdc.sh 用 NDK 交叉编译成 libhdc.so，
#  与 OpenSSL/libusb 一起放进 jniLibs；本脚本只负责 adb。
#
# 用法：./script/fetch_android_tools.sh
#
# 依赖：bash、curl、python3（或 python）、Android NDK（llvm-readelf/llvm-strip）。
#
# 可用环境变量覆盖：
#   RAREBOX_ANDROID_ABIS        默认 arm64-v8a,x86_64
#   RAREBOX_ANDROID_TOOLS_CACHE 默认 target/android-tools
#   RAREBOX_TERMUX_MIRROR       逗号分隔的镜像根地址（默认清华 + 官方）
#   RAREBOX_ANDROID_TOOLS_SKIP_STRIP=1 保留调试符号
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

ABIS="${RAREBOX_ANDROID_ABIS:-arm64-v8a,x86_64}"
CACHE="${RAREBOX_ANDROID_TOOLS_CACHE:-$ROOT/target/android-tools}"
OUT_BASE="$ROOT/android/app/src/main/jniLibs"
DIST_PREFIX="lib"

IFS=',' read -r -a MIRRORS <<< "${RAREBOX_TERMUX_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/termux/apt/termux-main,https://packages.termux.dev/apt/termux-main}"
TERMUX_SUITE="stable"
# 用闭包筛选，这里多下几个也不会影响产物（体积都很小）。
TERMUX_PACKAGES=(android-tools abseil-cpp libprotobuf brotli fmt liblz4 zstd liblzma zlib libc++)

# ---------- 工具链 ----------

# Windows 下 LOCALAPPDATA 是反斜杠路径，bash 的通配符解析不了，统一换成正斜杠。
slashes() { printf '%s' "${1//\\//}"; }

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
    for sdk in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}" \
        "$(slashes "${LOCALAPPDATA:-}")/Android/Sdk" \
        "$(slashes "${USERPROFILE:-}")/AppData/Local/Android/Sdk" \
        "${HOME:-}/AppData/Local/Android/Sdk"; do
        [[ -n "$sdk" ]] || continue
        sdk="$(slashes "$sdk")"
        [[ -d "$sdk/ndk" ]] || continue
        # 取最新的一个可用的 NDK（没下完的目录会缺少 toolchains）。
        for candidate in $(ls -d "$sdk"/ndk/* 2>/dev/null | sort -rV); do
            if [[ -d "$candidate/toolchains" ]]; then
                ANDROID_NDK_HOME="$candidate"
                break
            fi
        done
        [[ -n "$ANDROID_NDK_HOME" ]] && break
    done
fi
if [[ -z "${ANDROID_NDK_HOME:-}" || ! -d "$ANDROID_NDK_HOME" ]]; then
    echo "找不到 Android NDK，请设置 ANDROID_NDK_HOME（或 ANDROID_SDK_ROOT）。" >&2
    exit 1
fi
export ANDROID_NDK_HOME

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) HOST_TAG="windows-x86_64" ;;
    Darwin) HOST_TAG="darwin-x86_64" ;;
    *) HOST_TAG="linux-x86_64" ;;
esac
READELF="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/bin/llvm-readelf"
STRIP="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/$HOST_TAG/bin/llvm-strip"
[[ -x "$READELF" ]] || READELF="$READELF.exe"
[[ -x "$STRIP" ]] || STRIP="$STRIP.exe"
if [[ ! -x "$READELF" ]]; then
    echo "找不到 llvm-readelf：$READELF" >&2
    exit 1
fi

PYTHON="${PYTHON:-}"
if [[ -z "$PYTHON" ]]; then
    for candidate in python3 python; do
        if command -v "$candidate" >/dev/null 2>&1; then
            PYTHON="$candidate"
            break
        fi
    done
fi
if [[ -z "$PYTHON" ]]; then
    echo "需要 python3（用于解包 .deb 与改写 ELF 依赖）。" >&2
    exit 1
fi

mkdir -p "$CACHE"

# ---------- 下载 ----------

# download <仓库内相对路径> <目标文件>
download() {
    local relative="$1" dest="$2"
    if [[ -s "$dest" ]]; then
        return 0
    fi
    mkdir -p "$(dirname "$dest")"
    local base url
    for base in "${MIRRORS[@]}"; do
        url="${base%/}/$relative"
        echo "  下载 ${url}"
        if curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors -m 900 -o "$dest.part" "$url"; then
            mv "$dest.part" "$dest"
            return 0
        fi
        rm -f "$dest.part"
    done
    echo "下载失败：$relative（已尝试 ${MIRRORS[*]}）" >&2
    return 1
}

# 从 Packages 索引里取某个包的字段值。
package_field() { # <索引文件> <包名> <字段>
    awk -v want_pkg="$2" -v want_field="$3" '
        BEGIN { RS = ""; FS = "\n" }
        {
            name = ""
            for (i = 1; i <= NF; i++) {
                if (substr($i, 1, 9) == "Package: ") {
                    name = substr($i, 10)
                }
            }
            if (name != want_pkg) {
                next
            }
            for (i = 1; i <= NF; i++) {
                if (index($i, want_field ": ") == 1) {
                    print substr($i, length(want_field) + 3)
                    exit
                }
            }
        }
    ' "$1"
}

# 解包 .deb（ar + tar.xz），符号链接按内容展开成普通文件。
extract_deb() { # <deb> <目标目录>
    "$PYTHON" - "$1" "$2" <<'PY'
import io
import os
import struct
import sys
import tarfile
import time

deb, dest = sys.argv[1], sys.argv[2]
blob = open(deb, 'rb').read()
if blob[:8] != b'!<arch>\n':
    sys.exit(f'{deb}: 不是 ar 归档')

members = {}
offset = 8
while offset + 60 <= len(blob):
    header = blob[offset:offset + 60]
    name = header[:16].decode('ascii', 'replace').strip().rstrip('/')
    size = int(header[48:58].decode('ascii').strip())
    members[name] = blob[offset + 60:offset + 60 + size]
    offset += 60 + size + (size & 1)

data = members.get('data.tar.xz') or members.get('data.tar.gz') or members.get('data.tar.zst')
if data is None:
    sys.exit(f'{deb}: 找不到 data.tar.*')

archive = tarfile.open(fileobj=io.BytesIO(data))
for member in archive.getmembers():
    relative = member.name
    while relative.startswith('./'):
        relative = relative[2:]
    for prefix in ('data/data/com.termux/files/usr/', 'data/data/com.termux/files/'):
        if relative.startswith(prefix):
            relative = relative[len(prefix):]
            break
    if not relative or relative.startswith('..'):
        continue
    target = os.path.join(dest, relative)
    if member.isdir():
        os.makedirs(target, exist_ok=True)
        continue
    os.makedirs(os.path.dirname(target), exist_ok=True)
    if member.issym():
        link = member.linkname
        source = os.path.normpath(os.path.join(os.path.dirname(target), link))
        if not os.path.isabs(link) and os.path.exists(source):
            with open(source, 'rb') as handle:
                payload = handle.read()
        else:
            continue
    elif member.islnk():
        continue
    else:
        payload = archive.extractfile(member).read()
    with open(target, 'wb') as handle:
        handle.write(payload)
    os.chmod(target, 0o644)
PY
}

# ---------- 闭包 / 重命名 / 改写 DT_NEEDED ----------

# stage: 解包后的 Termux 目录（usr/bin/adb、usr/lib/*.so*）
# out:   jniLibs/<abi>
# 只保留 adb 真正需要的库；把 `libfoo.so.N` 改名为 `libfoo.N.so` 并同步改写所有
# 引用者的 DT_NEEDED 字符串（长度不变，安全地原地替换）。
install_native_tools() { # <stage> <out> <source-files...>
    "$PYTHON" - "$1" "$2" "${@:3}" <<'PY'
import os
import re
import shutil
import struct
import subprocess
import sys

stage, out = sys.argv[1], sys.argv[2]
sources = sys.argv[3:]

# 这些库由 Android 系统提供，不需要随包分发。
SYSTEM_LIBS = {
    'libc.so', 'libm.so', 'libdl.so', 'liblog.so', 'libandroid.so',
    'libstdc++.so', 'libz.so', 'libnativehelper.so',
}
# Termux 里的 adb 用比 NDK 更新的 libc++（需要 __hash_memory 等新符号），而
# 应用自己的 .so 依赖 NDK 版 libc++_shared.so（同名不能共存）。这里把 Termux 的
# libc++ 改名成等长的 `libcxx_termux.so` 并同步改写引用，两个版本各自独立。
EXTRA_RENAMES = {'libc++_shared.so': 'libcxx_termux.so'}

DT_NEEDED, DT_SONAME, DT_STRTAB = 1, 14, 5


class Elf:
    """最小的 ELF64 LE 解析：读 DT_NEEDED / DT_SONAME，并能原地改写 dynstr。"""

    def __init__(self, path):
        with open(path, 'rb') as handle:
            self.data = bytearray(handle.read())
        data = self.data
        if len(data) < 64 or data[:4] != b'\x7fELF' or data[4] != 2 or data[5] != 1:
            raise ValueError(f'{path}: 不是 ELF64 LE')
        self.path = path
        shoff, = struct.unpack_from('<Q', data, 0x28)
        shentsize, shnum, shstrndx = struct.unpack_from('<HHH', data, 0x3a)
        self.sections = []
        for index in range(shnum):
            base = shoff + index * shentsize
            fields = struct.unpack_from('<IIQQQQIIQQ', data, base)
            self.sections.append({
                'name_offset': fields[0], 'type': fields[1], 'addr': fields[3],
                'offset': fields[4], 'size': fields[5], 'link': fields[6],
            })
        shstr = self.sections[shstrndx]
        self.shstr_off = shstr['offset']
        self._dyn = next((s for s in self.sections if s['type'] == 6), None)
        if self._dyn is None:
            raise ValueError(f'{path}: 没有 .dynamic')
        self._dynstr = self.sections[self._dyn['link']]
        self.dynstr_off = self._dynstr['offset']
        self._read_dynamic()

    def _string(self, offset):
        start = self.dynstr_off + offset
        end = self.data.index(b'\0', start)
        return self.data[start:end].decode('utf-8', 'replace')

    def _read_dynamic(self):
        self.needed = []      # (dynstr 内偏移, 字符串)
        self.soname = None
        entry_size = 16
        count = self._dyn['size'] // entry_size
        for index in range(count):
            base = self._dyn['offset'] + index * entry_size
            tag, value = struct.unpack_from('<qQ', self.data, base)
            if tag == 0:
                break
            if tag == DT_NEEDED:
                self.needed.append((value, self._string(value)))
            elif tag == DT_SONAME:
                self.soname = self._string(value)

    def rename_needed(self, mapping):
        """mapping: 旧 soname -> 新文件名（长度必须相同）。"""
        for offset, name in self.needed:
            replacement = mapping.get(name)
            if replacement is None:
                continue
            if len(replacement) != len(name):
                raise ValueError(f'{self.path}: 无法等长重命名 {name} -> {replacement}')
            start = self.dynstr_off + offset
            self.data[start:start + len(name)] = replacement.encode()
        self._read_dynamic()

    def save(self, path):
        with open(path, 'wb') as handle:
            handle.write(self.data)
        os.chmod(path, 0o644)


def parse_soname(path):
    try:
        elf = Elf(path)
    except ValueError:
        return None, []
    return elf.soname, [name for _, name in elf.needed]


def versioned_name(soname):
    """libfoo.so.1 -> libfoo.1.so（长度不变，且仍以 .so 结尾供 Gradle 打包）。"""
    match = re.match(r'^(lib.+?)\.so(\.[0-9][0-9.]*)$', soname)
    if not match:
        return soname
    return f'{match.group(1)}{match.group(2)}.so'


# 建立 soname / 文件名 -> 路径 的索引。
providers = {}
for directory, _, files in os.walk(stage):
    for name in files:
        path = os.path.join(directory, name)
        soname, _ = parse_soname(path)
        if soname:
            providers.setdefault(soname, path)
        providers.setdefault(name, path)

queue = list(sources)
selected = {}          # soname -> 源路径（仅第三方库）
missing = set()
visited = set()
while queue:
    current = queue.pop()
    if current in visited:
        continue
    visited.add(current)
    _, needed = parse_soname(current) if os.path.isfile(current) else (None, [])
    for name in needed:
        if name in SYSTEM_LIBS:
            continue
        path = providers.get(name)
        if path is None:
            missing.add(name)
            continue
        if name not in selected:
            selected[name] = path
            queue.append(path)

if missing:
    sys.exit('缺少依赖：' + ', '.join(sorted(missing)))

renames = {name: versioned_name(name) for name in selected if versioned_name(name) != name}
renames.update({name: new for name, new in EXTRA_RENAMES.items() if name in selected})
renames = {name: new for name, new in renames.items() if len(name) == len(new)}

os.makedirs(out, exist_ok=True)

# 先收集所有需要处理的目标文件：adb 本体 + 依赖库。
targets = {}
for source, output_name in zip(sources, [os.path.basename(s) for s in sources]):
    targets[output_name] = source
for soname, path in selected.items():
    targets[renames.get(soname, soname)] = path

written = []
for output_name, source in targets.items():
    elf = Elf(source)
    elf.rename_needed(renames)
    destination = os.path.join(out, output_name)
    elf.save(destination)
    written.append(destination)

print('\n'.join(written))
PY
}

# ---------- 主流程 ----------

for abi in ${ABIS//,/ }; do
    case "$abi" in
        arm64-v8a) arch="aarch64" ;;
        armeabi-v7a) arch="arm" ;;
        x86_64) arch="x86_64" ;;
        x86) arch="i686" ;;
        *) echo "未知 ABI: $abi" >&2; exit 1 ;;
    esac

    echo "== $abi ($arch)"
    cache="$CACHE/$arch"
    archive="$cache/Packages.gz"
    index="$cache/Packages"
    download "dists/$TERMUX_SUITE/main/binary-$arch/Packages.gz" "$archive"
    if [[ ! -s "$index" || "$archive" -nt "$index" ]]; then
        "$PYTHON" - "$archive" "$index" <<'PY'
import gzip
import sys
with gzip.open(sys.argv[1], 'rb') as handle:
    payload = handle.read()
with open(sys.argv[2], 'wb') as handle:
    handle.write(payload)
PY
    fi

    stage="$cache/stage"
    rm -rf "$stage"
    mkdir -p "$stage"

    manifest=""
    for package in "${TERMUX_PACKAGES[@]}"; do
        filename="$(package_field "$index" "$package" Filename)"
        sha256="$(package_field "$index" "$package" SHA256)"
        version="$(package_field "$index" "$package" Version)"
        if [[ -z "$filename" ]]; then
            echo "  跳过 $package（索引里没有该包）"
            continue
        fi
        # Termux 的版本号里可能带冒号（如 libprotobuf_2:36.2-1），Windows 文件名不允许。
        deb="$cache/$(basename "$filename" | tr ':<>:"/\\|?*' '_')"
        download "$filename" "$deb"
        if [[ -n "$sha256" ]]; then
            actual="$("$PYTHON" - "$deb" <<'PY'
import hashlib
import sys
print(hashlib.sha256(open(sys.argv[1], 'rb').read()).hexdigest())
PY
)"
            if [[ "$actual" != "$sha256" ]]; then
                echo "  $package 校验和不匹配：$actual != $sha256" >&2
                exit 1
            fi
        fi
        extract_deb "$deb" "$stage"
        manifest+="$package $version $sha256 $filename"$'\n'
        echo "  + $package $version"
    done
    printf '%s' "$manifest" > "$cache/provenance.txt"

    adb_bin="$stage/bin/adb"
    if [[ ! -f "$adb_bin" ]]; then
        echo "  解包后找不到 adb（$adb_bin）" >&2
        exit 1
    fi

    out="$OUT_BASE/$abi"
    staging_out="$cache/out-$abi"
    rm -rf "$staging_out"
    mkdir -p "$staging_out"
    install_native_tools "$stage" "$staging_out" "$adb_bin" >/dev/null

    # adb 本体命名为 libadb.so：Gradle 只打包 *.so，且 nativeLibraryDir 里的文件可执行。
    mv "$staging_out/adb" "$staging_out/${DIST_PREFIX}adb.so"

    if [[ -z "${RAREBOX_ANDROID_TOOLS_SKIP_STRIP:-}" && -x "$STRIP" ]]; then
        for file in "$staging_out"/*.so; do
            "$STRIP" --strip-unneeded "$file" 2>/dev/null || true
        done
    fi

    # 只同步本脚本产出的文件，不动 jniLibs 里 Rust/NDK 的产物。
    mkdir -p "$out"
    for file in "$staging_out"/*.so; do
        cp -f "$file" "$out/"
    done

    echo "  -> $out"
    total=0
    for file in "$staging_out"/*.so; do
        size="$(stat -c %s "$file")"
        total=$((total + size))
        printf '     %9s  %s\n' "$size" "$(basename "$file")"
    done
    echo "     合计 $((total / 1024)) KiB（来源见 $cache/provenance.txt）"
done

echo "完成。别忘了 package_android.sh 会把 .so 一起打进 APK。"

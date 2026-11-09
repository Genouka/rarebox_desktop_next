#!/usr/bin/env bash
# 获取 Android 构建 hdc 所需的 Termux 预编译库（OpenSSL、libusb）。
#
# OpenSSL 没有随 Android NDK 提供，自行交叉编译成本高且容易和 Termux 生态不一致；
# Termux 的 openssl/libusb 都是 bionic 目标，可直接给 hdc 链接。libuv、lz4、
# securec 由 build_hdc.sh 用 NDK 现编，不在这里下载。
#
# 产物：target/hdc-deps/termux/<termux-arch>/stage/{include,lib}
#
# 用法：./script/fetch_hdc_android_deps.sh [arm64-v8a|x86_64|armeabi-v7a|x86 ...]
# 环境变量：RAREBOX_TERMUX_MIRROR（逗号分隔镜像根地址）
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$(pwd)"

ABIS=("$@")
[[ ${#ABIS[@]} -eq 0 ]] && ABIS=(arm64-v8a x86_64)
CACHE="$ROOT/target/hdc-deps/termux"
IFS=',' read -r -a MIRRORS <<< "${RAREBOX_TERMUX_MIRROR:-https://mirrors.tuna.tsinghua.edu.cn/termux/apt/termux-main,https://packages.termux.dev/apt/termux-main}"

PYTHON="${PYTHON:-}"
if [[ -z "$PYTHON" ]]; then
    for candidate in python3 python; do
        command -v "$candidate" >/dev/null 2>&1 && { PYTHON="$candidate"; break; }
    done
fi
[[ -n "$PYTHON" ]] || { echo "需要 python3。" >&2; exit 1; }

termux_arch() {
    case "$1" in
        arm64-v8a) echo aarch64 ;;
        armeabi-v7a) echo arm ;;
        x86_64) echo x86_64 ;;
        x86) echo i686 ;;
        *) echo "未知 ABI: $1" >&2; return 1 ;;
    esac
}

download() { # <仓库内相对路径> <目标文件>
    local relative="$1" dest="$2" base url
    [[ -s "$dest" ]] && return 0
    mkdir -p "$(dirname "$dest")"
    for base in "${MIRRORS[@]}"; do
        url="${base%/}/$relative"
        if curl -fsSL --retry 3 --retry-delay 2 --retry-all-errors -m 900 -o "$dest.part" "$url"; then
            mv "$dest.part" "$dest"
            return 0
        fi
        rm -f "$dest.part"
    done
    echo "下载失败：$relative" >&2
    return 1
}

for abi in "${ABIS[@]}"; do
    arch="$(termux_arch "$abi")"
    dir="$CACHE/$arch"
    mkdir -p "$dir"
    echo "== $abi ($arch)"
    download "dists/stable/main/binary-$arch/Packages.gz" "$dir/Packages.gz"
    gzip -dc "$dir/Packages.gz" > "$dir/Packages"

    "$PYTHON" - "$dir/Packages" > "$dir/packages.txt" <<'PY'
import re, sys
want = {'openssl', 'libusb'}
for block in open(sys.argv[1], encoding='utf-8', errors='replace').read().split('\n\n'):
    m = re.search(r'^Package: (.+)$', block, re.M)
    if m and m.group(1) in want:
        d = dict(line.split(': ', 1) for line in block.splitlines() if ': ' in line)
        print(d['Filename'], d.get('SHA256', ''))
PY
    mapfile -t packages < "$dir/packages.txt"

    rm -rf "$dir/stage"
    for line in "${packages[@]}"; do
        line="${line%$'\r'}"
        filename="${line%% *}"
        sha256="${line##* }"
        deb="$dir/$(basename "$filename" | tr ':<>:"/\\|?*' '_')"
        download "$filename" "$deb"
        if [[ -n "$sha256" && "$sha256" != "$line" ]]; then
            actual="$("$PYTHON" -c 'import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],"rb").read()).hexdigest())' "$deb")"
            [[ "$actual" == "$sha256" ]] || { echo "校验和不匹配：$deb" >&2; exit 1; }
        fi
        "$PYTHON" - "$deb" "$dir/stage" <<'PY'
import io, os, sys, tarfile
deb, dest = sys.argv[1], sys.argv[2]
blob = open(deb, 'rb').read()
assert blob[:8] == b'!<arch>\n', f'{deb}: 不是 ar 归档'
offset, members = 8, {}
while offset + 60 <= len(blob):
    header = blob[offset:offset + 60]
    name = header[:16].decode('ascii', 'replace').strip().rstrip('/')
    size = int(header[48:58].decode('ascii').strip())
    members[name] = blob[offset + 60:offset + 60 + size]
    offset += 60 + size + (size & 1)
data = next(v for k, v in members.items() if k.startswith('data.tar'))
archive = tarfile.open(fileobj=io.BytesIO(data))
for member in archive.getmembers():
    relative = member.name
    for prefix in ('./data/data/com.termux/files/usr/', 'data/data/com.termux/files/usr/'):
        if relative.startswith(prefix):
            relative = relative[len(prefix):]
    if not relative or relative.startswith('..'):
        continue
    target = os.path.join(dest, relative)
    if member.isdir():
        os.makedirs(target, exist_ok=True)
        continue
    os.makedirs(os.path.dirname(target), exist_ok=True)
    if member.issym():
        source = os.path.normpath(os.path.join(os.path.dirname(target), member.linkname))
        if os.path.exists(source):
            open(target, 'wb').write(open(source, 'rb').read())
        continue
    if member.islnk():
        continue
    open(target, 'wb').write(archive.extractfile(member).read())
PY
    done
    echo "  -> $dir/stage"
done

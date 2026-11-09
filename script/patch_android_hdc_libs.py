#!/usr/bin/env python3
"""把 Android hdc 依赖的版本化 soname 改成 Gradle 能打包的名字。

Gradle 的 jniLibs 只收集 `**/*.so`，而 Termux 的 OpenSSL 叫 `libssl.so.3`、
`libcrypto.so.3`，不会被打进 APK。这里在**不改变字符串长度**的前提下改名：

    libssl.so.3    -> libssl.3.so
    libcrypto.so.3 -> libcrypto.3.so

同时改写 ELF 的 .dynstr：DT_NEEDED、DT_SONAME 和 .gnu.version_r 的 vn_file
都指向同一份字符串表，全部替换后，加载器按新文件名查找、版本校验也能对上。

用法：python patch_android_hdc_libs.py <目录>
目录内所有 ELF 都会被处理；文件按映射表重命名。
"""

import os
import struct
import sys

MAPPING = {
    b"libssl.so.3": b"libssl.3.so",
    b"libcrypto.so.3": b"libcrypto.3.so",
}

SHT_DYNAMIC = 6


def dynamic_strtab(path: bytes):
    data = bytearray(open(path, "rb").read())
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        raise SystemExit(f"{path}: 只支持 ELF64 小端")
    shoff = struct.unpack_from("<Q", data, 0x28)[0]
    shentsize, shnum, _ = struct.unpack_from("<HHH", data, 0x3A)
    sections = []
    for index in range(shnum):
        fields = struct.unpack_from("<IIQQQQIIQQ", data, shoff + index * shentsize)
        sections.append({"type": fields[1], "offset": fields[4], "size": fields[5], "link": fields[6]})
    dynamic = next((s for s in sections if s["type"] == SHT_DYNAMIC), None)
    if dynamic is None:
        return data, None
    return data, sections[dynamic["link"]]


def patch(path: bytes) -> None:
    data, strtab = dynamic_strtab(path)
    if strtab is None:
        return
    start, size = strtab["offset"], strtab["size"]
    blob = bytes(data[start:start + size])
    changed = False
    for old, new in MAPPING.items():
        assert len(old) == len(new)
        needle = old + b"\0"
        if needle in blob:
            blob = blob.replace(needle, new + b"\0")
            changed = True
    if changed:
        data[start:start + size] = blob
        open(path, "wb").write(data)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    root = sys.argv[1]
    for name in sorted(os.listdir(root)):
        path = os.path.join(root, name)
        if not os.path.isfile(path):
            continue
        patch(path.encode())
        new_name = MAPPING.get(name.encode())
        if new_name:
            os.replace(path, os.path.join(root, new_name.decode()))
            print(f"  {name} -> {new_name.decode()}")


if __name__ == "__main__":
    main()

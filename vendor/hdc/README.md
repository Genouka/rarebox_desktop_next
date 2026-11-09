# 内置 hdc 源码

这里是从 OpenHarmony `developtools_hdc` 裁剪出的 host（PC 端）源码，用于构建
RareBox Desktop 各平台内置的 hdc。与 SDK 里预编译的 hdc 相比，这份源码只做
**平台适配**，不改协议与命令行为。

## 来源

| 组件 | 仓库 | 提交 |
| --- | --- | --- |
| hdc | https://gitcode.com/openharmony/developtools_hdc | `20cdb7860ecb441f6c0b055eab77b27fc5ac2e5d`（2026-10-31，3.2.0g） |
| securec | https://gitcode.com/openharmony/third_party_bounds_checking_function | `2ae82839ecaaa7d031e66ccbd2076d671acfd615` |
| libuv | https://gitcode.com/openharmony/third_party_libuv | `9a772139aaf711a9156764a41bcd2fdd476aef5f` |
| lz4 | https://gitcode.com/openharmony/third_party_lz4 | `28269928b534a399b64f41f4f4d6c3878a27993e` |
| libusb | https://gitcode.com/openharmony/third_party_libusb（1.0.28） | `0725dbccfbead43e130679ad774a8db4e1c780ff` |

## 目录

```
src/                 hdc 源码（common + host 参与构建；daemon/register 仅作保留）
scripts/             hdc_hash_gen.py：生成协议结构哈希 hdc_hash_gen.h
third_party/
  bounds_checking_function/  securec（memcpy_s / sprintf_s 等）
  libuv/                     libuv（事件循环，含 OHOS fork 的 uv__log/uv_start_trace）
  lz4/                       LZ4（hdc file 传输压缩）
  libusb/                    libusb 源码（Windows/Android 需自行编译；Linux/macOS 用系统库）
```

## 平台差异

- **Windows**：MinGW 构建。libusb 以 `libusb-1.0.dll` 动态链接，避免 LGPL 静态链接问题。
- **Linux / macOS**：`HOST_LINUX` / `HOST_MAC`，链接系统或 Homebrew 提供的 libuv、libusb、lz4、OpenSSL。产物不入库（受构建机 glibc / SDK 影响），由 `script/build_hdc.sh` 或 CI 生成；macOS 打包时把 hdc 放进 `.app/Contents/MacOS/`，应用会优先使用它。
- **Android**：用 NDK 交叉编译，输出 `libhdc.so` 放进 `jniLibs/<abi>/`。OpenSSL 与 libusb 取自 Termux（bionic 预编译），libuv/lz4/securec 由 NDK 现编。通过 USB OTG 可直连 HarmonyOS 设备（在系统弹窗中允许 USB 权限）；不支持 USB 主机模式时使用无线 hdc（`hdc tconn`）。
- 所有平台共用同一份 `HDC_MSG_HASH`（由 `scripts/hdc_hash_gen.py` 对协议结构计算），保证与设备端 daemon 的版本校验一致。

## 构建

```bash
./script/build_hdc.sh            # 当前平台
./script/build_hdc.sh android    # arm64-v8a + x86_64，输出到 jniLibs
./script/build_hdc.sh windows    # vendor/hdc/windows/
./script/build_hdc.sh linux      # vendor/hdc/linux/
./script/build_hdc.sh macos      # vendor/hdc/macos/
```

依赖见脚本顶部注释。构建脚本会生成 `target/hdc-build/` 下的中间文件，不写入源码目录。

## 许可

hdc 与 securec 为 Apache-2.0；libuv 为 MIT；`lz4/lib` 为 BSD-2-Clause；libusb 为
LGPL-2.1。各目录保留原始 LICENSE 文件。

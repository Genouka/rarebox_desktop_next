# RareBox Desktop

RareBox Desktop 是一个面向 Android / Wear OS 与 HarmonyOS 设备的桌面调试工具箱，使用 Rust、[GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 和 [material3-gpui](https://github.com/TNTyep520/material3-gpui) 构建。界面使用中文，所有设备数据、文件操作和投屏画面都来自真实 ADB / hdc，不使用模拟数据。

## 功能

- **概览**：设备型号、系统版本、电量、存储、内存、运行时间、电源与重启入口。
- **连接**：设备列表与切换、USB 首次连接引导、无线调试与配对、TCP/IP 开关。没有任何设备时自动进入该页引导连接。
- **HarmonyOS（hdc）**：不单独占页，与 Android 共用连接、概览、应用、文件、终端、日志与屏幕页面。在「连接」页或顶栏设备菜单切换到「HarmonyOS（hdc）」后，通过 hdc（HarmonyOS Device Connector，不依赖 adb）操作 HarmonyOS / OpenHarmony 设备——设备列表与在线状态、`hdc tconn` / `tmode` 无线连接、端口转发（`fport` / `rport`）、设备信息（型号、系统版本、API、屏幕、电池温度、Wi-Fi、存储与内存）、HAP/HSP/APP 逐包安装、应用列表 / 详情 / 启停 / 清缓存 / 清数据 / 卸载、文件传输、HDC shell 终端、HiLog 级别与标签过滤及导出、截图与 UI 控制（`uitest`、深链）、重启 / 关机，以及 hdc 服务重启。仅单方支持的控件（如 Android 的 APK 导出、录屏、远程控制）会在另一套工作区隐藏。
- **HarmonyOS 签名与安装**：在 HarmonyOS 工作区新增「签名与安装」页。自动模式登录华为开发者账号后，在本地生成 P-256 密钥/CSR、向 AGC 申请调试证书、注册设备、按 bundle + 设备 + ACL 申请调试 Profile，并用纯 Rust 签名 HAP/HSP 后通过 hdc 安装；发布或自定义材料使用「导入材料」模式。签名块默认 V3，可切换 V2；不需要 Java 或 DevEco Studio。
- **应用管理**：浏览用户/系统应用，列表显示应用名称、包名、版本与系统/用户标记，支持搜索、启动、停止、卸载、导出 APK。
- **APK 安装**：一次选择多个 APK，按包名自动分组（分包与主 APK 归为一组并发起 `install-multiple`），可切换「覆盖安装 `-r`」「允许降级 `-d`」「授予全部权限 `-g`」「允许测试包 `-t`」，安装前预览 adb 命令，安装后逐项显示成功/失败原因。
- **屏幕与显示**：截图、录屏、修改分辨率/DPI、发送文本；HarmonyOS 工作区提供 hdc 截图与 uitest 控制。
- **文件传输**：浏览设备文件系统，上传、下载、删除文件或目录。
- **终端**：在设备 shell 中执行 ADB / hdc 命令并保留历史。
- **日志查看器**：抓取 logcat / HiLog，按级别和关键字过滤，导出日志。
- **远程控制**：在应用内实时镜像手表画面，用鼠标点击/拖动操作设备，并发送返回、主页、最近任务、电源按键和文字。
- **重启**：普通重启，以及重启到 Recovery、Bootloader、Fastbootd、EDL（对应 `adb reboot` 的各个参数）。
- **设置**：ADB / HDC 路径、动态取色/深色模式、截图与录屏保存目录；可手动结束 ADB / HDC 进程，并可设置退出程序时自动结束这些后台进程（默认开启）。
- **检查更新与公告**：启动时自动检查新版本（也可在「设置 → 关于」中手动检查），有更新时弹窗展示版本、更新说明与下载入口；关闭弹窗后左下角保留「发现新版本」胶囊提醒。顶栏铃铛打开公告弹窗，可翻阅最新与以往公告。

## HarmonyOS 签名与安装

「签名与安装」页只在 HarmonyOS（hdc）工作区显示；Android 工作区仍然只显示 ADB 安装，不会同时出现两套安装入口。

1. **登录**：点击「登录华为开发者账号」，应用打开系统浏览器并监听 `127.0.0.1` 回调端口。登录成功后 token 保存在应用数据目录，启动时自动校验并在失效时用 jwt 刷新；退出登录会删除本地 token。
2. **证书（自动模式，仅调试）**：本地生成 NIST P-256 密钥库和 CSR（`hapsigner`，`vendor/hapsigner`），再调用 AGC 申请调试证书（`certType=1`）。同名证书会先校验公钥指纹；密钥不匹配时必须在界面显式确认「重置证书」才会删除旧证书。发布证书/Profile 没有在参考实现的可用流程中出现，本工具不猜测发布接口。
3. **设备**：通过 `bm get --udid` 读取当前设备 UDID，按需注册到 AGC。调试 Profile 只绑定当前设备及用户额外选择的设备。
4. **Profile / ACL（自动模式，仅调试）**：从 HAP 的 `module.json` 提取 `requestPermissions`，与内置 AGC ACL 白名单求交集；可手动增减。调试 Profile 下载后会离线校验 bundle、证书指纹、UDID、ACL 和有效期，不匹配会自动重新申请。发布 Profile 请在 AGC 网站申请后，用下面的「导入材料」模式使用。
5. **签名与安装**：对 `.hap` / `.hsp` 逐文件签名并立即验签，再执行 `hdc install`。安装失败会显示 hdc 原始输出和中文错误提示；遇到 `9568322`（Profile/证书不匹配）会自动重新申请 Profile 并重试一次。
6. **导入材料（发布/自定义）**：如果已有 DevEco 或 AGC 网站生成的密钥库、应用证书和 `.p7b` Profile，切换到「导入材料」模式，选择 `.p12`/`.jks`、`.cer`、`.p7b` 并填写密钥别名与口令。导入模式不要求登录，不申请或修改 AGC 资源；签名前会校验 bundle、证书指纹、UDID、ACL 和有效期，不匹配会直接报错。
7. **签名块**：默认 V3（与 DevEco 6.x 的 `compatibleVersion=9` 一致），可在「签名并安装」卡片切换 V2 以兼容旧设备。`.app` 包暂不支持重新签名，仍可使用原「安装 HAP」入口按原样安装。

签名内核是 MIT 许可的 [hapsigner](https://github.com/harmony-contrib/hapsigner-rs)，仓库内副本与本地改动见 `vendor/hapsigner/VENDORED.md`。

签名材料保存在应用数据目录的 `signing/` 下（Windows 为 `%APPDATA%\RareBox Desktop\signing`）：登录 token、密钥库口令与本地证书都在这里。Unix 上文件权限收紧为 0600；Windows 依赖用户目录 ACL。不要把该目录提交到版本库或分享给他人。

### 已知限制

- AGC/Eco 登录与证书接口是 DevEco Studio 使用的未公开接口，可能随官方更新失效；接口集中在 `src/signing/api.rs`，便于修复。
- 自动模式只覆盖调试证书/调试 Profile，这与 auto-installer 当前可用的安装流程一致；它的发布路径依赖 AGC 网页 Cookie，且相关方法在参考实现中已注释停用。发布签名请先在 AGC 网站准备好材料，再使用「导入材料」。
- 需要先在 AGC 创建对应包名的应用；本工具不会自动创建 AGC 项目/应用。
- 受限 ACL 可能需要在 AGC 先申请并通过审核；Profile 列表会显示「ACL 审核中」。证书/Profile 有数量与月度配额限制，错误信息会提示到 AGC 清理。
- 网络需要能访问 `cn.devecostudio.huawei.com` 与 `connect-api.cloud.huawei.com`；国内网络/关闭代理通常更稳定。

## 远程控制

远程控制没有重写 scrcpy 的采集端，也没有依赖本机安装 scrcpy：

1. 应用内置官方 `scrcpy-server` 3.3.4（由 `fadb-scrcpy` 提供），启动时推送到 `/data/local/tmp/scrcpy-server.jar`。
2. 通过 `adb forward tcp:0 localabstract:scrcpy_<id>` 建立视频和控制两条通道，复用 scrcpy 的启动参数、H.264 视频协议和控制协议。
3. 视频流在应用内由 OpenH264 解码，转换为 GPUI 可显示的图像；控制消息通过 `scrcpy-protocol` 发送。
4. 只保留“最新一帧”，UI 以约 8 ms 间隔取帧；旧的待显示帧会被直接替换，避免解码/渲染较慢时累积延迟。
5. 离开远程控制页、切换设备或退出应用时，会终止 server、删除端口转发，避免残留进程。

这样采集和编码完全复用 scrcpy，Windows/macOS/Linux 上只需要 ADB 和 Rust 即可运行，不需要平台相关的 scrcpy 可执行文件或窗口嵌入。

### 延迟说明

远程画面会经过“设备编码 → ADB 转发 → 应用解码 → GPU 绘制”几个环节，因此输入回显不会像设备本地触控一样即时。使用 `cargo run --release` 可以获得明显优于 debug 构建的帧率和输入响应；应用内已经避免帧队列积压，并关闭了控制 socket 的 Nagle 算法。若设备编码性能较弱，可在后续版本中增加分辨率/码率/帧率选项。

## 环境要求
- Windows 10/11（当前主要验证平台）、macOS 或 Linux。
- Rust 1.90 或更新版本。
- Wear OS 设备或模拟器，已开启开发者选项和 USB 调试，并在设备上授权本机。

应用内置了 Windows 版 ADB（35.0.2），首次运行自动解包到数据目录下的 `adb/` 子目录，因此 Windows 上通常不需要单独安装 platform-tools。macOS / Linux 上仍需要本机 `adb`，或可在设置中指定自定义路径。应用按以下顺序查找 ADB：配置中的 `adb_path` 路径、`RAREBOX_ADB` 环境变量、内置 ADB、应用目录旁的 `platform-tools/adb`、`ANDROID_HOME` / `ANDROID_SDK_ROOT` / `LOCALAPPDATA\Android\Sdk` 下的 platform-tools，最后是 `PATH` 中的 `adb`。

各平台都内置了从 [developtools_hdc](https://gitcode.com/openharmony/developtools_hdc) 源码构建的 hdc（3.2.0g）：Windows 为 `hdc.exe` + `libusb-1.0.dll` + OpenSSL DLL，首次运行自动解包到数据目录下的 `hdc/` 子目录；Android 为 `jniLibs/<abi>/libhdc.so` 及 OpenSSL/libusb，从 `nativeLibraryDir` 直接执行；macOS 打包脚本会把 `hdc` 放进 `.app/Contents/MacOS/hdc`；Linux 发行包把 `vendor/hdc/linux/hdc` 放在可执行文件同目录即可。应用按以下顺序查找 hdc：配置中的 `hdc_path` 路径、`RAREBOX_HDC` 环境变量、Android 内置 `libhdc.so`、内置 hdc（Windows）、应用目录旁的 `hdc` / `toolchains/hdc`、`DEVECO_SDK_HOME` / `HOS_SDK_HOME` / `OHOS_SDK_HOME` 环境变量、`%LOCALAPPDATA%\Huawei\Sdk`、`%LOCALAPPDATA%\OpenHarmony\Sdk`、`C:\Program Files\Huawei\DevEco Studio`（macOS / Linux 为 `~/Library/Huawei/Sdk`、`~/Huawei/Sdk` 等对应位置）下的 `toolchains/hdc`（递归最多 5 层，覆盖 SDK 的多个 API 版本目录），最后是 `PATH` 中的 `hdc`。找不到时在「连接」页的 HarmonyOS 工作区与「设置 → HDC」中给出提示，可手动填写完整路径。

设备只充电、不出现在 ADB 列表时，多半是系统没有把它识别为「Android ADB Interface」。应用在「连接」页内置了 [UniversalAdbDriver](https://github.com/koush/UniversalAdbDriver)（来自 koush，绑定到系统自带 `WinUSB.sys`），可一键安装/卸载；安装/卸载会写入系统驱动存储，因此需要管理员权限——应用本身无需以管理员运行，点击按钮后会单独弹出 Windows 用户账户控制（UAC）请求，仅提权 `pnputil` 这一步。


## 构建与运行

```powershell
# 开发运行
cargo run

# 优化的发布构建
cargo run --release
# 或直接运行 target\release\rarebox-desktop.exe
```

如果依赖 Git 访问受限，仓库已经将 `material3-gpui` 放到 `vendor/material3-gpui`，并通过 `Cargo.toml` 的 `[patch]` 指向本地副本；`.cargo/config.toml` 启用了 `git-fetch-with-cli`。

### macOS

macOS 版需要 Apple 平台工具链，无法用普通 Rust 交叉编译从 Windows 产出：gpui 要链接 Metal/AppKit，`ring`（rustls 的加密实现）与 `openh264-sys2`（解码器）要编译 C/C++ 与汇编，均依赖 Xcode 的系统 SDK 与头文件。在 Mac 上执行：

```bash
./script/package_macos.sh
```

脚本会分别构建 Apple Silicon（`aarch64-apple-darwin`）与 Intel（`x86_64-apple-darwin`）版本，用 `lipo` 合并为通用二进制，产出 `dist/RareBox Desktop.app` 与 `dist/RareBox-Desktop-<版本>-macos-universal.zip`，并做 ad-hoc 签名（自行分发需换成开发者证书签名与公证）。`.github/workflows/macos.yml` 在 macOS runner 上执行同一脚本并上传产物。

### Android

Android 版由 `gpui-mobile`（android-activity + wgpu/Vulkan）提供 GPUI 平台层，复用同一套界面，产物为 APK。需要 Android SDK（platform 35）、NDK 27.2.12479018、JDK 17+、`cargo-ndk`，以及 Rust 目标 `aarch64-linux-android`（真机）与 `x86_64-linux-android`（模拟器）：

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
./script/package_android.sh release
```

脚本用 `cargo ndk` 交叉编译 `librarebox_desktop.so`（API 26，默认 `arm64-v8a,x86_64`），再调用 Gradle 打包，产出 `dist/RareBox-Desktop-<版本>-android.apk`。默认使用 debug 签名以便直接侧载；正式分发时通过 `-Prarebox.keystore=...`、`-Prarebox.keystore.password=...`、`-Prarebox.key.alias=...`、`-Prarebox.key.password=...` 传入自己的密钥。`.github/workflows/android.yml` 在 Ubuntu runner 上执行同一脚本并上传 APK。

Android 端说明：

- 应用内置 Android 版 `adb`（来自 Termux 的 `android-tools` 包，随 APK 以 `libadb.so` 及依赖库分发），启动后从 `nativeLibraryDir` 直接执行，无需设备已安装 adb。APK 需要 `android:extractNativeLibs="true"`（Gradle 的 `useLegacyPackaging = true`），否则原生库不会解压到磁盘，无法执行。
- 查找顺序：配置中的 `adb_path` → `RAREBOX_ADB` → `nativeLibraryDir/libadb.so` → Termux `/data/data/com.termux/files/usr/bin/adb` → 系统路径 → 应用目录与 `PATH`。
- 支持 **USB OTG 直连**：用 OTG 线把手表/手机接到 Android 设备，在系统弹窗中允许 USB 权限后即可像电脑一样操作；设备不支持 USB 主机模式时，用「连接 → 无线 ADB」连接同一网络中的手表/手机。
- `hdc` 已随 APK 分发：`nativeLibraryDir/{libhdc.so,libssl.3.so,libcrypto.3.so,libusb-1.0.so}`，由 `./script/build_hdc.sh android` 从 `vendor/hdc` 源码用 NDK 交叉编译（OpenSSL/libusb 取自 Termux）。内置 hdc 支持 USB OTG 直连 HarmonyOS 设备（在系统弹窗中允许 USB 权限），无线连接用 `hdc tconn <IP:端口>`。也可以把自定义 hdc 放入应用目录或设置 `hdc_path` 覆盖。
- 重新抓取/更新内置 adb 及其依赖库：`./script/fetch_android_tools.sh`（需要 curl、python3 与 NDK 的 `llvm-readelf`；可用 `RAREBOX_ANDROID_ABIS`、`RAREBOX_TERMUX_MIRROR` 覆盖）。
- 手机/平板上侧栏改为抽屉（左上角菜单按钮），标题栏压缩为一行，并自动避让状态栏与导航栏。
- 应用内部存储路径为 `files/config.json`，截图、录屏与导出文件位于 `Android/data/com.rarebox.desktop/files/`。

### Windows / Linux

```powershell
# Windows
pwsh ./script/package_windows.ps1
```

```bash
# Linux（需 X11/Wayland、fontconfig、xkbcommon 开发包，以及 OpenSSL/libuv/libusb/lz4）
./script/package_linux.sh
```

产物分别是 `dist/RareBox-Desktop-<版本>-windows-x64.zip` 与
`dist/RareBox-Desktop-<版本>-linux-x64.tar.gz`。`.github/workflows/windows.yml`
和 `linux.yml` 在对应 runner 上构建并上传产物。

### iOS（未签名）

iOS 版由 `gpui-mobile`（UIKit + wgpu/Metal）提供平台层，需要 macOS、Xcode 15+
与 [XcodeGen](https://github.com/yonaskolb/XcodeGen)：

```bash
brew install xcodegen
rustup target add aarch64-apple-ios
./script/package_ios.sh release
```

产物为 `dist/RareBox-Desktop-<版本>-ios-unsigned.ipa`。该包未签名，只能用于
构建验证或自行签名后侧载；`.github/workflows/ios.yml` 在 macOS runner 上构建
并上传该未签名包。


## 代码结构

工程分为三个 crate，业务逻辑与界面彻底分离：

- `crates/rarebox-core`：无界面引擎，不依赖 GPUI。包含设备命令层（ADB / hdc / scrcpy、
  APK/HAP 签名与安装、更新、配置）以及可克隆的 `CoreState`、串行执行的 `Engine`
  （命令队列 + 事件通道 + 状态快照）。所有业务逻辑与单元测试都在这一层。
- `crates/rarebox-ffi`：跨语言绑定（C ABI + Android JNI），命令/事件走 JSON，
  远程画面走独立 BGRA 二进制回调。
- 根 crate `rarebox-desktop`：只保留 GPUI 渲染与桌面/Android 平台入口。
  `src/app.rs` 的 `RareBox` 是引擎状态的只读镜像 + 输入框实体；
  `src/app/actions.rs` 把界面动作转成 `rarebox_core::Command`；
  `src/app/bridge.rs` 消费 `Event` 并同步镜像；`src/app/<page>_ui.rs` 只读镜像、绑定交互。

新增页面时：在 core 中加状态与命令，在 `src/app/actions.rs` 加适配方法，
在 `<page>_ui.rs` 加渲染。不要在 UI 层直接执行 adb / hdc / 文件操作。

### 作为库嵌入

`rarebox-core` 可以直接被其他 Rust 程序使用：

```rust
use rarebox_core::{config::Config, Command, Engine};

let engine = Engine::new(Config::load());
let events = engine.events();
engine.command(Command::RefreshDevices)?;
// 处理 events，或随时 engine.state() 读取快照
```

其他语言通过 `rarebox-ffi`：

```text
rarebox_engine_new() -> handle
rarebox_engine_send(handle, "{\"cmd\":\"refresh_devices\"}")
rarebox_engine_set_callbacks(handle, on_json, on_frame)
rarebox_engine_state(handle) -> JSON   // 用 rarebox_string_free 释放
```

构建引擎库：

```bash
cargo build -p rarebox-ffi --release

# Android / Wear OS：产物为 dist/jniLibs/<abi>/librarebox_ffi.so，
# 脚本同时复制 OpenH264 需要的 libc++_shared.so。
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
./script/build_watch_lib.sh
```

C ABI 头文件在 `bindings/c/rarebox.h`，可直接被 C/C++ 或其他 FFI 框调用。

Kotlin 封装在 `bindings/kotlin/com/rarebox/core/CoreEngine.kt`：JNI 类名为
`com.rarebox.core.CoreEngine`。Android 上推荐 `CoreEngine.create(context)`，
它会先把 `applicationInfo.nativeLibraryDir` 注入核心（内置 adb/hdc 从该目录执行），
再创建引擎；随后即可 `send(commandJson)`、`stateJson()`、`startEvents(listener)`。
事件与命令的 JSON 结构与 `rarebox_core::command` / `rarebox_core::event` 一致；
`CoreListener.onFrame(generation, width, height, bgra)` 提供 BGRA8888 原始画面。


## 配置

配置文件保存在：

- Windows：`%APPDATA%\RareBox Desktop\config.json`
- macOS：`~/Library/Application Support/RareBox Desktop/config.json`
- Linux：`$XDG_DATA_HOME/RareBox Desktop/config.json`（未设置时使用 `~/.local/share`）
- Android：应用内部存储 `files/config.json`（截图与导出在 `Android/data/com.rarebox.desktop/files/`）

配置中的 `adb_path` 留空时启用自动检测。内置 scrcpy server 的 JAR 会缓存在同一数据目录下。

## 已知限制

- 当前一次只连接并控制一台设备。
- 远程控制只传输视频，不传输音频。
- 控制功能目前包括触控、返回/主页/最近任务/电源键和文字注入，暂不支持剪贴板、文件拖放、多指手势和游戏手柄。
- 需要在设备上授权 USB 调试；部分厂商系统可能限制 `app_process` 或 shell 权限。
- 远程画面默认使用 1024 的最大边长和 8 Mbps 码率，适合手表分辨率；更高分辨率设备可在代码中调整。
- Android 端内置 adb 与 hdc，均可通过 USB OTG 直连设备（在系统弹窗中允许 USB 权限）；不支持 USB 主机模式时回退无线 ADB / `hdc tconn`。界面以手表场景为主，未针对大屏重新设计。
- Android 上个别按钮在刚进入页面时点击可能暂时无响应，轻滚一下页面即可恢复（gpui-mobile 触摸命中测试的已知问题，后续随上游修复）。
- 各平台内置 hdc 3.2.0g：Windows 随程序解包，Android 随 APK 分发，macOS 打包时复制进 `.app/Contents/MacOS/`，Linux 需把构建产物放在可执行文件同目录；若发行包未附带，则回退到本机 DevEco Studio / OpenHarmony SDK。截图优先 `snapshot_display`、失败回退 `uitest screenCap`，应用列表与详情依赖 `bm dump`，UI 控制依赖设备启用 `uitest`。不同 HarmonyOS / OpenHarmony 版本的 `hidumper`、`hilog`、`uitest` 参数可能略有差异，失败时会在界面显示原始错误。

## 官网与更新服务

- **官网**：`rare.genouka.top`，源码在独立仓库 `rareplan_website`（Jekyll + GitHub Pages）。RareBox Desktop 的产品页是 `pages/rareboxdesktop.md`，下载区会读取下面的更新服务，展示当前可用的构建。
- **更新服务**：`website/rareboxnextapi.qiumingsanyu.top`（PHP 8 + SQLite，部署在 `rareboxnextapi.qiumingsanyu.top`）。提供 `/api/version`、`/api/update/check`、`/api/announcements`、`/api/releases`、`/api/stats`、`/api/download` 等接口（带 ETag 缓存），后台可发布版本与公告、查看下载统计、导出 CSV 与下载数据库备份。

桌面应用的「检查更新」与公告弹窗读取的正是这套接口，接口约定见 `website/rareboxnextapi.qiumingsanyu.top/README.md`。本地调试：

```bash
cd website/rareboxnextapi.qiumingsanyu.top
php -S 127.0.0.1:8080 router.php        # 需要 PHP 8.0+
```

部署脚本与服务器地址不随仓库公开；nginx 配置见 `website/nginx/sites-available/rarebox`。

## 许可证

本项目以 **AGPL-3.0-only** 开源。若你需要在不符合 AGPL 的场景（例如闭源分发、闭源 SaaS）使用本项目，需要另行取得项目所有者的**书面商业授权**；详见 [LICENSING.md](LICENSING.md)。贡献代码需完成 DCO Sign-off 并同意 [CLA.md](CLA.md) 中的不可撤销商用再许可授权。内置 scrcpy-server、OpenH264、GPUI、material3-gpui 等第三方组件的许可证与归属见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

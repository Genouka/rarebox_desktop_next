//! GPUI 的 HarmonyOS (OpenHarmony) 平台后端。
//!
//! # 结构
//!
//! | 模块 | 职责 | 平台相关 |
//! |---|---|---|
//! | [`keyboard`] | `oh_key_code.h` 键码表 → GPUI `Keystroke`，修饰键状态跟踪 | 否 |
//! | [`dispatcher`] | 主线程队列（渲染线程）+ 后台线程池 + 延迟队列 | 否 |
//! | [`frame_source`] | 帧节拍 `FramePacer`（宿主可测）+ `OH_NativeVSync` 驱动 | 混合 |
//! | [`fonts`] | `/system/fonts/` 装载表、`UI_FONT` / `MONO_FONT` | 否 |
//! | [`text_input`] | IME 事件队列 / 会话号 / 删除范围计算 | 否 |
//! | [`surface`] | vp↔px、surface 尺寸推导、`RawWindowHandle::OhosNdk` 包装 | 混合 |
//! | `platform` / `window` / `display` / `host` / `document_picker` / `napi` | 真正的平台实现 | 是（`target_env = "ohos"`） |
//!
//! # 线程模型
//!
//! ```text
//! ArkTS/UI 线程                     渲染线程 ("gpui-ohos-main")
//!   napi start() ────────────────►  创建 OhosDispatcher（主线程 = 本线程）
//!   attach_surface() ────────────►  open_window + Application::run_embedded
//!   touch/key/ime ───────────────►  PlatformInput → GPUI
//!                                  循环: 命令队列 → dispatcher 任务 → 一帧
//!   on_event(cb) ◄───────────────  ThreadsafeFunction（NonBlocking）
//! ```
//!
//! 与 Android 的 `host.rs` 一样，渲染线程是进程级、跨 surface 存活的：
//! `Platform::run` 立即返回（`Application::run_embedded` 的形态），渲染循环由
//! `host` 自己驱动。GPUI 的 `App::new_app` 断言 `is_main_thread()`，而
//! `OhosDispatcher` 在渲染线程构造，所以断言自然成立（风险 R3）。
//!
//! # 渲染
//!
//! 只有 GLES/EGL：wgpu 的 Vulkan 后端在 29.0.4 里没有 `Rwh::OhosNdk` 分支
//! （`wgpu-hal/src/vulkan/instance.rs`），因此本 crate 在创建 `WgpuContext` 前
//! 显式用 `wgpu::Backends::GL` 建实例，再交给 `WgpuRenderer`（风险 R4）。
//! surface 通过 `RawWindowHandle::OhosNdk` 交给 wgpu。
//!
//! # 实施地图风险 R1–R15 的落点
//!
//! | # | 风险 | 处理位置 |
//! |---|---|---|
//! | R1 | vsync 回调可能收不到 | [`frame_source::FramePacer::recover_stalled`]：超过 2 个周期把请求当丢失，退回时钟节拍；另有 `install()` 在 `OH_NativeVSync_Create` 失败时直接用时钟 |
//! | R2 | `Platform::run` 与 ArkTS 线程 | [`platform`] 的 `run` 在渲染线程上立即调用 launch 并返回（`run_embedded` 形态）；**阻塞的是 host 的渲染循环**，见 [`host`] |
//! | R3 | `App::new_app` 的主线程断言 | [`dispatcher::OhosDispatcher`] 在渲染线程构造，"主线程"就是它；`App` 也在这里构建 |
//! | R4 | Vulkan 不支持 OHOS surface | [`window`] 的 `ensure_gl_context` 固定 `wgpu::Backends::GL`；`OhosBackend::Vulkan` 仅作诊断枚举 |
//! | R5 | 必须先有 XComponent surface | [`host`] 只在首个 `attach_surface` 之后 `run_embedded`；`open_window` 找不到窗口时报错并提示等 `"ready"` |
//! | R6 | 几何类型双份 | 全 crate 只用 `gpui::Pixels/Size/Point/Bounds`，[`surface`] 里只有 `InsetsPx`（px 整数避让） |
//! | R7 | IME `TextConfig.inputAttribute` | 属宿主 ArkTS 的职责；[`napi::api::ime`] 的协议已覆盖 begin/preview/insert/delete/done |
//! | R8 | CJK 缺字体 | [`fonts`] 按 fontconfig 顺序装载 `HarmonyOS_Sans[_SC/_TC]` 等，SC/TC 标记为 essential |
//! | R9 | NAPI 跨线程 | ArkTS → 渲染线程走 [`host`] 命令队列；渲染线程 → ArkTS 走 `ThreadsafeFunction`（NonBlocking） |
//! | R10 | external 指针生命周期 | [`host::detach_surface`] 阻塞到渲染线程 `term_surface`；窗口只在 attached 区间保存指针 |
//! | R11 | vp/px 混用 | 所有 NAPI 入口统一 `vp * density`（[`vp_to_px`]）；内部只流转物理像素 |
//! | R12 | `rarebox-ohos` 的 `mod api` | 本次范围外（该 crate 已有 `src/api.rs`）；本 crate 不依赖它 |
//! | R13 | Vulkan adapter 枚举噪声 | R4 的 GL-only 实例使 Vulkan 完全不被枚举 |
//! | R14 | 键码表返工 | [`keyboard`] 按 `oh_key_code.h` 重写（2000+ 段），字符输入交给 IME |
//! | R15 | `GetPeriod` 首次无效 | [`frame_source`] 只在收到首个 vsync 回调后读周期，并校验 `> 0`；之前用 16.667ms 兜底 |
//!
//! 实现不了、只能降级的点（代码里均有注释）：
//!
//! * `Platform::prevent_idle_sleep` → `Err`：阻止息屏只能由 ArkTS `setWindowKeepScreenOn` 完成；
//! * `Platform::on_thermal_state_change` → 只保存回调：OHOS NDK 没有热状态订阅；
//! * `Platform::prompt`（系统对话框）→ `None`：`promptAction.showDialog` 只能在 ArkTS 调用；
//! * `Platform::path_for_auxiliary_executable` → `Err`：鸿蒙禁止应用执行随身二进制；
//! * 触控板的轴事件（滚动/捏合）需 ArkTS `onAxisEvent` 转成 `mouse(eventJson)` 的 `scroll` 动作，
//!   XComponent 的 native 轴回调只覆盖 axis 事件且为可选 API。

#![allow(unsafe_code)]

pub use gpui::{
    App, Application, ApplicationHandle, Bounds, DevicePixels, Edges, Pixels, Point, Size, Window,
    point, px, size,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// 绑定层版本，供宿主启动日志与「关于」页核对 `.so` 是否与 HAP 匹配。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// `"gpui-ohos <版本>"`。
pub fn version_string() -> String {
    format!("gpui-ohos {VERSION}")
}

// ── 纯逻辑模块（宿主可编译、可单测） ─────────────────────────────────────────
pub mod dispatcher;
pub mod fonts;
pub mod frame_source;
pub mod keyboard;
pub mod surface;
pub mod text_input;

/// UI 字体族名（根 crate 的 `theme.rs` 直接引用）。
pub use fonts::UI_FONT;
/// 等宽字体族名（根 crate 的 `ui.rs` 直接引用）。
pub use fonts::MONO_FONT;

// ── OHOS 平台实现 ────────────────────────────────────────────────────────────
#[cfg(target_env = "ohos")]
pub mod display;
#[cfg(target_env = "ohos")]
pub mod document_picker;
#[cfg(target_env = "ohos")]
pub mod host;
#[cfg(target_env = "ohos")]
pub mod napi;
#[cfg(target_env = "ohos")]
pub mod platform;
#[cfg(target_env = "ohos")]
pub mod window;

#[cfg(target_env = "ohos")]
pub use platform::{OhosPlatform, SharedPlatform};
#[cfg(target_env = "ohos")]
pub use surface::InsetsPx;
#[cfg(target_env = "ohos")]
pub use window::{OhosPlatformWindow, OhosWindow};

// ── 后端选择 ─────────────────────────────────────────────────────────────────

/// 渲染后端。
///
/// 目前只实现 [`OhosBackend::Gles`]：`wgpu-hal 29.0.4` 的 Vulkan 后端不认识
/// `RawWindowHandle::OhosNdk`，需要在 `vkCreateSurfaceOHOS` 上打补丁才能用
/// （地图 R4）。枚举保留 `Vulkan` 变体只是为了让宿主日志/诊断能表达意图，
/// `OhosPlatform::new` 会拒绝非 `Gles` 的请求并记录原因。
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum OhosBackend {
    /// EGL + GLES3（唯一可用路径）。
    #[default]
    Gles,
    /// `VK_OHOS_surface`：wgpu 上游尚未支持，选中它只会得到一条错误日志。
    Vulkan,
}

impl std::fmt::Display for OhosBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OhosBackend::Gles => f.write_str("GLES"),
            OhosBackend::Vulkan => f.write_str("Vulkan"),
        }
    }
}

// ── IME 脏标记 ───────────────────────────────────────────────────────────────
//
// 软键盘文本绕过 GPUI 的 invalidator：输入到达时置位，渲染循环把它转成一次
// `force_render`（`OhosPlatformWindow::on_request_frame` 读取后清零）。

/// 自上一帧以来有 IME 文本到达。
pub static TEXT_INPUT_DIRTY: AtomicBool = AtomicBool::new(false);

/// 标记有 IME 文本待应用；下一次帧请求会强制重绘。
pub fn mark_text_input_dirty() {
    TEXT_INPUT_DIRTY.store(true, Ordering::Release);
}

/// 读取并清除 [`TEXT_INPUT_DIRTY`]。
pub fn take_text_input_dirty() -> bool {
    TEXT_INPUT_DIRTY.swap(false, Ordering::AcqRel)
}

// ── 单位换算（风险 R11） ─────────────────────────────────────────────────────
//
// XComponent 的触摸/鼠标坐标是 vp（鼠标事件头文件注释明确写了 `Unit: vp`），
// 而 surface 尺寸、EGL 缓冲、GPUI 的 `DevicePixels` 都是物理像素。所有 NAPI
// 入口都在一处做 `px = vp * density`，内部只流转物理像素。

/// vp（鸿蒙的逻辑像素）→ 物理像素。
///
/// `density` 是 `display.getDefaultDisplaySync().densityPixels`，由 `start`
/// 传入。非有限/非正的密度按 1.0 处理，避免把整条输入链变成 NaN。
pub fn vp_to_px(vp: f32, density: f32) -> f32 {
    let density = sane_density(density);
    if vp.is_finite() { vp * density } else { 0.0 }
}

/// 物理像素 → vp。
pub fn px_to_vp(px: f32, density: f32) -> f32 {
    let density = sane_density(density);
    if px.is_finite() { px / density } else { 0.0 }
}

/// vp 尺寸 → 物理像素尺寸（四舍五入，最小 1）。
pub fn device_size_from_vp(width_vp: f32, height_vp: f32, density: f32) -> (u32, u32) {
    let width = vp_to_px(width_vp, density).round().max(1.0) as u32;
    let height = vp_to_px(height_vp, density).round().max(1.0) as u32;
    (width, height)
}

/// 物理像素尺寸 → GPUI 的逻辑尺寸。
pub fn logical_size_from_device(width_px: u32, height_px: u32, density: f32) -> Size<Pixels> {
    size(
        px(px_to_vp(width_px as f32, density)),
        px(px_to_vp(height_px as f32, density)),
    )
}

fn sane_density(density: f32) -> f32 {
    if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    }
}

// ── 日志 ─────────────────────────────────────────────────────────────────────

/// 安装一个把 `log::*` 写到 stderr 的最小 logger。
///
/// OHOS 的 `hilog` 会收集进程 stderr（调试版本），所以不额外依赖 `hilog-sys`：
/// 多一个系统库链接对「编译通过 + 宿主单测」这条验收线没有收益，而 stderr
/// 在 DevEco 的 HiLog 面板里同样可见。重复调用是 no-op。
pub fn init_logger() {
    static INSTALLED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    INSTALLED.get_or_init(|| {
        struct StderrLogger;
        impl log::Log for StderrLogger {
            fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
                metadata.level() <= log::max_level()
            }

            fn log(&self, record: &log::Record<'_>) {
                if self.enabled(record.metadata()) {
                    eprintln!(
                        "[gpui-ohos {} {}] {}",
                        record.level(),
                        record.target(),
                        record.args()
                    );
                }
            }

            fn flush(&self) {}
        }

        let level = match std::env::var("GPUI_OHOS_LOG").as_deref() {
            Ok("trace") => log::LevelFilter::Trace,
            Ok("debug") => log::LevelFilter::Debug,
            Ok("info") => log::LevelFilter::Info,
            Ok("warn") => log::LevelFilter::Warn,
            Ok("error") => log::LevelFilter::Error,
            Ok("off") => log::LevelFilter::Off,
            _ => log::LevelFilter::Info,
        };
        let _ = log::set_boxed_logger(Box::new(StderrLogger));
        log::set_max_level(level);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_carries_crate_version() {
        let text = version_string();
        assert!(text.starts_with("gpui-ohos "));
        assert!(text.ends_with(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn vp_px_round_trip() {
        for density in [1.0f32, 1.5, 2.0, 2.75, 3.0, 3.5] {
            for vp in [0.0f32, 1.0, 12.5, 360.0, 1234.5] {
                let px = vp_to_px(vp, density);
                assert!((px - vp * density).abs() < 1e-3, "{vp}vp @ {density}");
                let back = px_to_vp(px, density);
                assert!((back - vp).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn device_size_rounds_and_clamps() {
        assert_eq!(device_size_from_vp(360.0, 640.0, 3.0), (1080, 1920));
        // 0vp 的组件在拿到第一帧之前很常见；保持 1px 让 wgpu 不会配置 0×0 surface。
        assert_eq!(device_size_from_vp(0.0, 0.0, 2.0), (1, 1));
        assert_eq!(device_size_from_vp(-5.0, 10.4, 2.0), (1, 21));
    }

    #[test]
    fn invalid_density_is_treated_as_one() {
        assert_eq!(vp_to_px(10.0, 0.0), 10.0);
        assert_eq!(vp_to_px(10.0, f32::NAN), 10.0);
        assert_eq!(px_to_vp(10.0, -2.0), 10.0);
        assert_eq!(vp_to_px(f32::INFINITY, 2.0), 0.0);
    }

    #[test]
    fn logical_size_divides_by_density() {
        let size = logical_size_from_device(1080, 2400, 3.0);
        assert!((f32::from(size.width) - 360.0).abs() < 1e-3);
        assert!((f32::from(size.height) - 800.0).abs() < 1e-3);
    }

    #[test]
    fn text_input_dirty_is_consumed_once() {
        mark_text_input_dirty();
        assert!(take_text_input_dirty());
        assert!(!take_text_input_dirty());
    }
}

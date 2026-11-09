//! RareBox Desktop 的界面库。
//!
//! 设备逻辑、状态与命令都在 `rarebox-core`；这里只保留 GPUI 渲染与
//! 桌面端平台入口。桌面二进制入口在 `main.rs`，Android 入口在 `android.rs`。

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;
mod app;
mod assets;
mod theme;
mod titlebar;
mod ui;

// 界面模块沿用 `crate::adb` / `crate::config` 等路径；统一从核心 crate 重导出。
pub(crate) use rarebox_core::{adb, adb_driver, config, hdc, install, signing, update};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn run_desktop() {
    use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size};

    use crate::app::RareBox;
    use rarebox_core::config::Config;

    gpui_platform::application()
        .with_assets(crate::assets::md3_assets())
        .run(|cx: &mut App| {
            material3_gpui::init(cx);

            let config = Config::load();
            theme::apply(cx, config.seed, config.dark);

            let bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("RareBox Desktop".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(24.))),
                }),
                window_min_size: Some(size(px(1080.), px(700.))),
                ..Default::default()
            };

            let window_config = config.clone();
            if let Err(err) = cx.open_window(options, move |_, cx| {
                RareBox::new(cx, window_config.clone())
            }) {
                eprintln!("RareBox Desktop: 无法创建窗口: {err}");
            }
            cx.activate(true);
        });
}

//! iOS 入口：由 Objective-C `AppDelegate` 通过 C ABI 调用。
//!
//! `gpui-mobile` 提供 UIKit + wgpu/Metal 平台层。Xcode 工程在
//! `application:didFinishLaunchingWithOptions:` 中先调用
//! `gpui_ios_register_app()` 注册根视图，再调用 `gpui_ios_run_demo()`
//! 进入 GPUI 运行循环。

use gpui::{App, WindowOptions};

use crate::app::RareBox;
use crate::config::Config;

/// 注册 RareBox 的根视图回调。必须在 `gpui_ios_run_demo()` 之前调用。
#[unsafe(no_mangle)]
pub extern "C" fn gpui_ios_register_app() {
    gpui_mobile::ios::ffi::set_app_callback(Box::new(|cx: &mut App| {
        material3_gpui::init(cx);

        let config = Config::load();
        crate::theme::apply(cx, config.seed, config.dark);

        let window_config = config.clone();
        if let Err(err) = cx.open_window(WindowOptions::default(), move |_, cx| {
            RareBox::new(cx, window_config.clone())
        }) {
            eprintln!("RareBox iOS: 无法创建窗口: {err}");
        }
        cx.activate(true);
    }));
}

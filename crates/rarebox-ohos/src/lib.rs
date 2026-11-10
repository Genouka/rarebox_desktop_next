//! RareBox Desktop 的 HarmonyOS / OpenHarmony 宿主绑定。
//!
//! 鸿蒙侧不允许执行随身二进制（官方 FAQ：普通应用不能 fork/exec），因此宿主通过
//! Node-API 直接加载本 crate 的 `.so`，由它持有 `rarebox_core::Engine`。桌面、Android、
//! iOS 构建该 crate 时只编译出空库，真正的绑定在 `cfg(target_env = "ohos")` 下。
//!
//! Node-API 导出面在 `api` 模块，宿主侧的 ArkTS 声明见 `ohos/entry/src/main/cpp/types`。

pub use rarebox_core;

/// 绑定层版本，供宿主启动日志与「关于」页核对 `.so` 是否与 HAP 匹配。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 返回 `"rarebox-ohos <版本>"`，宿主在启动时打印一次。
pub fn version_string() -> String {
    format!("rarebox-ohos {VERSION}")
}

// 句柄表与事件转发不依赖 Node-API，Windows 上也编译，让
// `cargo check -p rarebox-ohos` 能覆盖这部分逻辑；只有 OHOS 导出面引用它。
#[cfg_attr(not(target_env = "ohos"), allow(dead_code))]
mod registry;

#[cfg(target_env = "ohos")]
mod api;

#[cfg(test)]
mod tests {
    #[test]
    fn version_string_carries_crate_version() {
        let text = super::version_string();
        assert!(text.starts_with("rarebox-ohos "));
        assert!(text.ends_with(env!("CARGO_PKG_VERSION")));
    }
}

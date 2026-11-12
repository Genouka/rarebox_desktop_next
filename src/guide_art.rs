//! 教程配图：编译期嵌入 SVG，避免运行时依赖资源目录。
//!
//! 配图是单色线稿，GPUI 把它渲染成 alpha mask 后统一着色，
//! 因此会自动跟随浅色/深色主题。多个步骤可以复用同一张示意图。

pub fn bytes(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "nav" | "apps_nav" | "screen_nav" | "remote_nav" | "files_nav" => {
            include_bytes!("../assets/guide/nav.svg")
        }
        "dev_options" => include_bytes!("../assets/guide/dev_options.svg"),
        "usb_debug" => include_bytes!("../assets/guide/usb_debug.svg"),
        "usb_cable" => include_bytes!("../assets/guide/usb_cable.svg"),
        "device_select" => include_bytes!("../assets/guide/device_select.svg"),
        "ready" => include_bytes!("../assets/guide/ready.svg"),
        "wireless_debug" | "wireless_pair" | "wireless_connect" => {
            include_bytes!("../assets/guide/wireless.svg")
        }
        "connect_choose" => include_bytes!("../assets/guide/connect_choose.svg"),
        "wlan_debug" | "wlan_tcpip" => include_bytes!("../assets/guide/wlan.svg"),
        "wlan_connect" => include_bytes!("../assets/guide/wlan_connect.svg"),
        "app_pair" => include_bytes!("../assets/guide/app_pair.svg"),
        "driver_problem" | "driver_uac" | "driver_done" | "authorize" => {
            include_bytes!("../assets/guide/driver.svg")
        }
        "apps_install" | "install_button" => include_bytes!("../assets/guide/apps_install.svg"),
        "apk_picker" | "hap_picker" => include_bytes!("../assets/guide/picker.svg"),
        "install_options" => include_bytes!("../assets/guide/options.svg"),
        "install_result" => include_bytes!("../assets/guide/result.svg"),
        "harmony_switch" | "harmony_install" | "hdc_status" | "harmony_connect" => {
            include_bytes!("../assets/guide/harmony.svg")
        }
        "dpi_panel" | "dpi_input" | "dpi_apply" => include_bytes!("../assets/guide/dpi.svg"),
        "dpi_reset" => include_bytes!("../assets/guide/dpi_reset.svg"),
        "screenshot" => include_bytes!("../assets/guide/screenshot.svg"),
        "files_path" | "files_transfer" | "files_done" => {
            include_bytes!("../assets/guide/files.svg")
        }
        "remote_start" | "remote_control" | "remote_done" => {
            include_bytes!("../assets/guide/remote.svg")
        }
        "troubleshoot_states" | "offline" => {
            include_bytes!("../assets/guide/troubleshoot.svg")
        }
        "save_dir" | "adb_settings" | "adb_restart" => {
            include_bytes!("../assets/guide/settings.svg")
        }
        "settings" => include_bytes!("../assets/guide/settings_alt.svg"),
        "input_method" | "appops" | "forward" | "signing" | "logs" | "terminal" | "reboot"
        | "overview" | "update" => include_bytes!("../assets/guide/advanced.svg"),
        _ => return None,
    })
}

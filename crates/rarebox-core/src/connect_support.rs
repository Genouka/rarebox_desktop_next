//! 手表调试能力推理：把一次快照抓到的原始读数，变成
//! 「无线调试（TLS 配对）/ WLAN 调试（旧版 5555）/ USB 调试」三种通道的结论。
//!
//! 规则全部是纯函数，便于单测；证据不足时显示「未检测」而不是猜一个结论。
//! 依据（AOSP）：
//! - `WirelessDebuggingPreferenceController.isAvailable()` ≙ `isAdbWifiSupported()`
//!   ≙ 设备带 Wi-Fi 或以太网，且系统含该开关（Android 11 / API 30 起）；
//! - `adb_wifi_enabled` 全局设置 = 无线调试开关状态；
//! - `adb_enabled` 全局设置 = USB 调试开关状态；
//! - `service.adb.tcp.port` = 旧版 WLAN 调试（`adb tcpip 5555`）监听端口；
//! - `/sys/class/udc` 有控制器才可能走 USB gadget。

use serde::{Deserialize, Serialize};

/// 三种调试通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// Android 11+ 的「无线调试」（TLS 配对码 / 二维码）。
    Wireless,
    /// 旧版「WLAN 调试 / TCP/IP 5555」，需要先用 USB 授权一次。
    Wlan,
    /// USB 数据线（部分手表需要触点底座）。
    Usb,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Wireless => "无线调试",
            Transport::Wlan => "WLAN 调试",
            Transport::Usb => "USB 调试",
        }
    }

    /// 对应的图文教程 id。
    pub fn guide_id(self) -> &'static str {
        match self {
            Transport::Wireless => "connect_wireless",
            Transport::Wlan => "connect_wlan",
            Transport::Usb => "connect_usb",
        }
    }
}

/// 一次快照抓到的调试相关原始读数。字段缺省表示「读不到」，不参与判定。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugProbe {
    pub sdk: u32,
    pub has_wifi: bool,
    pub has_ethernet: bool,
    /// `settings get global adb_enabled`；`None` = 读不到。
    pub adb_enabled: Option<bool>,
    /// `settings get global adb_wifi_enabled`；`None` = 键不存在或读不到。
    pub adb_wifi_enabled: Option<bool>,
    /// `getprop service.adb.tcp.port`。
    pub tcp_port: Option<u16>,
    /// `persist.sys.usb.config` 里是否带 adb。
    pub usb_config_adb: bool,
    /// `/sys/class/udc`：Some(true) 有控制器、Some(false) 无、None 读不到。
    pub usb_gadget: Option<bool>,
}

/// 单个通道的结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capability {
    /// 是否支持：`None` 表示证据不足，界面显示「未检测」而不是猜。
    pub supported: Option<bool>,
    /// 开关当前是否已打开。
    pub enabled: bool,
    /// 置灰 / 未检测时的短说明（≤12 个字）。
    pub hint: &'static str,
}


/// 三通道结论 + 推荐顺序。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionSupport {
    pub wireless: Capability,
    pub wlan: Capability,
    pub usb: Capability,
    /// 设备的局域网 IP（来自 `ip route`）。
    pub ip: Option<String>,
}

impl ConnectionSupport {
    pub fn infer(probe: &DebugProbe) -> Self {
        let networked = probe.has_wifi || probe.has_ethernet;

        // 无线调试（TLS）：API ≥ 30 且有 Wi-Fi / 以太网。
        let wireless_supported = networked && probe.sdk >= 30;
        let wireless = Capability {
            supported: Some(wireless_supported),
            enabled: probe.adb_wifi_enabled == Some(true),
            hint: if !networked {
                "无 Wi-Fi"
            } else if probe.sdk < 30 {
                "系统版本太低"
            } else if probe.adb_wifi_enabled.is_none() {
                "未检测"
            } else {
                ""
            },
        };

        // 旧版 WLAN 调试：有 Wi-Fi 就可能可用（先 USB 一次 `adb tcpip 5555`）。
        let wlan = Capability {
            supported: Some(networked),
            enabled: probe.tcp_port.is_some(),
            hint: if !networked { "无 Wi-Fi" } else { "" },
        };

        // USB：只有既无 UDC 控制器、USB 配置里也没有 adb 时才判「不支持」。
        let usb_supported = match (probe.usb_gadget, probe.usb_config_adb) {
            (Some(false), false) => Some(false),
            (None, false) => None,
            _ => Some(true),
        };
        let usb = Capability {
            supported: usb_supported,
            enabled: probe.adb_enabled == Some(true),
            hint: match usb_supported {
                Some(false) => "无 USB 口",
                None => "未检测",
                _ => "",
            },
        };

        Self {
            wireless,
            wlan,
            usb,
            ip: None,
        }
    }

    /// 推荐通道；`None` 表示没有任何可行路径。
    pub fn recommended(&self) -> Option<Transport> {
        if self.wireless.enabled {
            Some(Transport::Wireless)
        } else if self.wlan.enabled {
            Some(Transport::Wlan)
        } else if self.wireless.supported == Some(true) {
            Some(Transport::Wireless)
        } else if self.usb.supported != Some(false) {
            Some(Transport::Usb)
        } else if self.wlan.supported == Some(true) {
            Some(Transport::Wlan)
        } else {
            None
        }
    }

    /// 旧版 WLAN 调试可用时的完整连接地址。
    pub fn wlan_address(&self) -> Option<String> {
        let ip = self.ip.as_deref()?;
        Some(format!("{ip}:5555"))
    }

    /// 推荐教程：有结论走推荐，否则进入三选一。
    pub fn guide_id(&self) -> &'static str {
        self.recommended()
            .map(Transport::guide_id)
            .unwrap_or("connect_choose")
    }
}

/// 解析 `settings get` 输出：`1`=开、`0`=关、`null`/错误=读不到。
pub fn parse_setting(raw: &str) -> Option<bool> {
    match raw.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// 解析 `getprop service.adb.tcp.port`。
pub fn parse_tcp_port(raw: &str) -> Option<u16> {
    raw.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

/// 从 `pm list features` 判断是否带 Wi-Fi / 以太网。
///
/// 行形如 `feature:req:android.hardware.wifi`；用后缀匹配，避免
/// `android.hardware.wifi.direct` 误判成基础 Wi-Fi。
pub fn parse_network_features(raw: &str) -> (bool, bool) {
    let mut has_wifi = false;
    let mut has_ethernet = false;
    for line in raw.lines() {
        let line = line.trim();
        if line.ends_with("android.hardware.wifi") {
            has_wifi = true;
        } else if line.ends_with("android.hardware.ethernet") {
            has_ethernet = true;
        }
    }
    (has_wifi, has_ethernet)
}

/// 判断本机 adb 是否支持 `adb pair`（平台工具 ≥ 30，版本串 1.0.41+）。
pub fn adb_supports_pairing(version: &str) -> bool {
    // 形如 "Android Debug Bridge version 1.0.41" / "Version 34.0.5"。
    let number = version
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|part| part.contains('.'))
        .unwrap_or("");
    let mut parts = number.split('.');
    let major = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    (major, minor, patch) >= (1, 0, 41)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(sdk: u32, wifi: bool) -> DebugProbe {
        DebugProbe {
            sdk,
            has_wifi: wifi,
            ..DebugProbe::default()
        }
    }

    #[test]
    fn modern_watch_with_wireless_on() {
        let mut p = probe(33, true);
        p.adb_wifi_enabled = Some(true);
        p.adb_enabled = Some(false);
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.wireless.supported, Some(true));
        assert!(s.wireless.enabled);
        assert_eq!(s.recommended(), Some(Transport::Wireless));
        assert_eq!(s.guide_id(), "connect_wireless");
    }

    #[test]
    fn modern_watch_wireless_off_but_supported() {
        let mut p = probe(30, true);
        p.adb_wifi_enabled = Some(false);
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.wireless.supported, Some(true));
        assert!(!s.wireless.enabled);
        // 没有已开的通道时，仍优先引导走无线（先开启开关）。
        assert_eq!(s.recommended(), Some(Transport::Wireless));
    }

    #[test]
    fn legacy_watch_uses_wlan() {
        let mut p = probe(28, true);
        p.tcp_port = Some(5555);
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.wireless.supported, Some(false));
        assert_eq!(s.wlan.supported, Some(true));
        assert!(s.wlan.enabled);
        assert_eq!(s.recommended(), Some(Transport::Wlan));
        assert_eq!(s.guide_id(), "connect_wlan");
        assert_eq!(s.wlan_address().as_deref(), None); // 没有ip不拼地址
    }

    #[test]
    fn wlan_address_needs_ip() {
        let mut p = probe(28, true);
        p.tcp_port = Some(5555);
        let mut s = ConnectionSupport::infer(&p);
        s.ip = Some("192.168.1.8".to_string());
        assert_eq!(s.wlan_address().as_deref(), Some("192.168.1.8:5555"));
    }

    #[test]
    fn no_wifi_falls_back_to_usb() {
        let mut p = probe(30, false);
        p.usb_gadget = Some(true);
        p.adb_enabled = Some(true);
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.wireless.supported, Some(false));
        assert_eq!(s.wlan.supported, Some(false));
        assert_eq!(s.usb.enabled, true);
        assert_eq!(s.recommended(), Some(Transport::Usb));
        assert_eq!(s.guide_id(), "connect_usb");
    }

    #[test]
    fn usb_without_gadget_is_unsupported() {
        let mut p = probe(30, false);
        p.usb_gadget = Some(false);
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.usb.supported, Some(false));
        assert_eq!(s.recommended(), None);
    }

    #[test]
    fn missing_settings_reads_show_unknown_not_off() {
        let mut p = probe(30, true);
        p.adb_wifi_enabled = None;
        let s = ConnectionSupport::infer(&p);
        assert_eq!(s.wireless.supported, Some(true));
        assert!(!s.wireless.enabled);
        assert_eq!(s.wireless.hint, "未检测");
    }

    #[test]
    fn parse_setting_values() {
        assert_eq!(parse_setting("1"), Some(true));
        assert_eq!(parse_setting("0"), Some(false));
        assert_eq!(parse_setting("null"), None);
        assert_eq!(parse_setting("Error: not found"), None);
    }

    #[test]
    fn parse_tcp_port_values() {
        assert_eq!(parse_tcp_port("5555"), Some(5555));
        assert_eq!(parse_tcp_port(""), None);
        assert_eq!(parse_tcp_port("0"), None);
    }

    #[test]
    fn network_features_use_exact_suffix() {
        let raw = "feature:req:android.hardware.wifi\n\
                   feature:req:android.hardware.wifi.direct\n\
                   feature:avail:android.hardware.ethernet\n";
        let (wifi, ethernet) = parse_network_features(raw);
        assert!(wifi);
        assert!(ethernet);
        let (wifi2, _) = parse_network_features("feature:req:android.hardware.wifi.direct\n");
        assert!(!wifi2);
    }

    #[test]
    fn pairing_needs_adb_41() {
        assert!(adb_supports_pairing("Android Debug Bridge version 1.0.41"));
        assert!(adb_supports_pairing("Android Debug Bridge version 1.0.41, version 34.0.5"));
        assert!(!adb_supports_pairing("Android Debug Bridge version 1.0.40"));
        assert!(!adb_supports_pairing("no version here"));
    }
}

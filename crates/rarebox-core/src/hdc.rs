//! HarmonyOS / OpenHarmony 调试（hdc）命令层与数据解析。
//!
//! 与 `adb` 模块平级：这一层不依赖 GPUI，所有函数都可以在后台线程执行，
//! 也便于单元测试。`hdc`（HarmonyOS Device Connector）是 HarmonyOS 调试工具，
//! 各平台随应用内置（Windows 由 `hdc_bundle` 解包，Android 用 jniLibs 里的
//! `libhdc.so`，macOS / Linux 放在可执行文件同目录），常见命令包括 `hdc list targets`、
//! `hdc tconn`、`hdc install`、`hdc shell`、`hdc file send/recv` 等。
//!
//! 这里不依赖设备端任何自定义脚本：概览通过 `param get`、`bm get -u`、
//! `hidumper`、`df`、`/proc` 等系统自带命令拼出；应用列表来自 `bm dump`。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use serde_json::Value;

use crate::adb::{LogLevel, format_duration, format_kilobytes};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 一条 hdc 命令的文本结果。
#[derive(Clone, Debug, Default)]
pub struct HdcOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl HdcOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// hdc 不少失败仍返回退出码 0，只在正文里打印 `[Fail]`。
    pub fn is_failure(&self) -> bool {
        if !self.success() {
            return true;
        }
        let text = self.combined().to_ascii_lowercase();
        text.contains("[fail]")
            || text.contains("error:")
            || text.contains("failed")
            || text.contains("unknown command")
    }

    /// 优先返回错误输出，便于直接把原因展示给用户。
    pub fn error_message(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return stdout.to_string();
        }
        match self.code {
            Some(code) => format!("HDC 命令执行失败（退出码 {code}）"),
            None => "HDC 命令执行失败".to_string(),
        }
    }

    pub fn combined(&self) -> String {
        match (self.stdout.trim(), self.stderr.trim()) {
            ("", "") => String::new(),
            ("", stderr) => stderr.to_string(),
            (stdout, "") => stdout.to_string(),
            (stdout, stderr) => format!("{stdout}\n{stderr}"),
        }
    }
}

/// 设备的连接方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdcConnection {
    Usb,
    Tcp,
}

impl HdcConnection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Tcp => "无线",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Tcp => "wifi",
        }
    }
}

/// `hdc list targets -v` 中的连接状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HdcTargetState {
    #[default]
    Unknown,
    Ready,
    Connected,
    Offline,
    Unauthorized,
}

impl HdcTargetState {
    fn parse(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "ready" => Self::Ready,
            "connected" => Self::Connected,
            "offline" => Self::Offline,
            "unauthorized" => Self::Unauthorized,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ready | Self::Connected => "在线",
            Self::Offline => "离线",
            Self::Unauthorized => "未授权",
            Self::Unknown => "未知",
        }
    }

    /// 旧版 hdc 的普通列表只有 connect-key，此时按在线处理，让操作自行报错。
    pub fn is_online(self) -> bool {
        !matches!(self, Self::Offline | Self::Unauthorized)
    }
}

/// `hdc list targets` 中的一台设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HdcDevice {
    /// connect-key：USB 序列号或 IP:端口。
    pub key: String,
    pub connection: HdcConnection,
    pub state: HdcTargetState,
    /// `-v` 输出的第四列（设备名/主机名），可能为空。
    pub host: Option<String>,
}

impl HdcDevice {
    pub fn display_name(&self) -> String {
        self.key.clone()
    }

    pub fn is_online(&self) -> bool {
        self.state.is_online()
    }
}

/// 解析 `hdc list targets[-v]`。无设备时 hdc 输出 `[Empty]`。
///
/// `-v` 输出：connect-key、连接方式、状态、设备名、标签。
/// 只给出 connect-key 的旧格式同样可以解析。
pub fn parse_targets(raw: &str) -> Vec<HdcDevice> {
    let mut devices = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('[') || line.starts_with("connect-key") {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(key) = tokens.first().copied() else {
            continue;
        };
        // hdc 3.x 在 `[Empty]` 后会多打印一行服务器标记 `hdc`。
        if key.eq_ignore_ascii_case("empty") || key.eq_ignore_ascii_case("hdc") {
            continue;
        }
        if !seen.insert(key.to_string()) {
            continue;
        }
        let connection = match tokens.get(1).map(|value| value.to_ascii_lowercase()) {
            Some(value) if value == "tcp" => HdcConnection::Tcp,
            Some(value) if value == "usb" => HdcConnection::Usb,
            _ if key.contains(':') => HdcConnection::Tcp,
            _ => HdcConnection::Usb,
        };
        let state = tokens
            .get(2)
            .map(|value| HdcTargetState::parse(value))
            .unwrap_or_default();
        let host = tokens
            .get(3..)
            .filter(|rest| !rest.is_empty())
            .map(|rest| rest.join(" "))
            .filter(|rest| {
                !rest.is_empty()
                    && !rest.starts_with('<')
                    && !rest.eq_ignore_ascii_case("unknown")
                    && !rest.eq_ignore_ascii_case("hdc")
            });
        devices.push(HdcDevice {
            key: key.to_string(),
            connection,
            state,
            host,
        });
    }
    devices
}

/// 设备概览。字段缺失时保持为空，界面按「—」展示。
#[derive(Clone, Debug, Default)]
pub struct HdcSnapshot {
    pub key: String,
    pub model: String,
    pub brand: String,
    pub os_version: String,
    pub api_version: String,
    pub abi: String,
    pub udid: String,
    pub ip: Option<String>,
    pub battery: Option<u8>,
    /// 充电状态描述；无法判断时为空。
    pub battery_status: String,
    pub memory_total_kb: Option<u64>,
    pub memory_available_kb: Option<u64>,
    pub storage_total_kb: Option<u64>,
    pub storage_used_kb: Option<u64>,
    pub uptime_seconds: Option<u64>,
    /// 设备名称（`const.product.name`）。
    pub device_name: String,
    /// 亮屏状态：亮屏 / 熄屏 / 已关机；未知时为空。
    pub screen_state: String,
    pub screen_resolution: Option<(u32, u32)>,
    pub screen_rotation: Option<u16>,
    pub battery_temperature_c: Option<f32>,
    pub wifi_ssid: Option<String>,
}

impl HdcSnapshot {
    pub fn title(&self) -> String {
        if self.model.trim().is_empty() {
            self.key.clone()
        } else {
            self.model.clone()
        }
    }

    pub fn identity(&self) -> String {
        let brand = self.brand.trim();
        let os = self.os_version.trim();
        match (brand.is_empty(), os.is_empty()) {
            (false, false) => format!("{brand} · {os}"),
            (false, true) => brand.to_string(),
            (true, false) => os.to_string(),
            (true, true) => "HarmonyOS 设备".to_string(),
        }
    }

    pub fn battery_percent(&self) -> Option<f32> {
        self.battery.map(f32::from)
    }

    pub fn battery_status_label(&self) -> &str {
        if self.battery_status.is_empty() {
            "—"
        } else {
            self.battery_status.as_str()
        }
    }

    pub fn memory_percent(&self) -> Option<f32> {
        let total = self.memory_total_kb?;
        if total == 0 {
            return None;
        }
        let used = total.saturating_sub(self.memory_available_kb.unwrap_or(0));
        Some(used as f32 / total as f32 * 100.0)
    }

    pub fn memory_used_label(&self) -> String {
        match (self.memory_total_kb, self.memory_available_kb) {
            (Some(total), Some(available)) => format!(
                "{} / {}",
                format_kilobytes(total.saturating_sub(available.min(total))),
                format_kilobytes(total)
            ),
            _ => "—".to_string(),
        }
    }

    pub fn storage_percent(&self) -> Option<f32> {
        let total = self.storage_total_kb?;
        if total == 0 {
            return None;
        }
        Some(self.storage_used_kb.unwrap_or(0) as f32 / total as f32 * 100.0)
    }

    pub fn storage_used_label(&self) -> String {
        match (self.storage_total_kb, self.storage_used_kb) {
            (Some(total), Some(used)) => format!(
                "{} / {}",
                format_kilobytes(used.min(total)),
                format_kilobytes(total)
            ),
            _ => "—".to_string(),
        }
    }

    pub fn uptime_label(&self) -> String {
        self.uptime_seconds
            .map(format_duration)
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn screen_label(&self) -> String {
        let mut parts = Vec::new();
        if !self.screen_state.is_empty() {
            parts.push(self.screen_state.clone());
        }
        if let Some((width, height)) = self.screen_resolution {
            parts.push(format!("{width}×{height}"));
        }
        if let Some(rotation) = self.screen_rotation {
            parts.push(format!("旋转 {rotation}°"));
        }
        if parts.is_empty() {
            "—".to_string()
        } else {
            parts.join(" · ")
        }
    }

    pub fn temperature_label(&self) -> String {
        self.battery_temperature_c
            .map(|value| format!("{value:.1} °C"))
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn wifi_label(&self) -> &str {
        self.wifi_ssid.as_deref().unwrap_or("—")
    }

    pub fn device_name_label(&self) -> &str {
        if self.device_name.is_empty() {
            "—"
        } else {
            self.device_name.as_str()
        }
    }
}

/// 一次 shell 往返抓取 HarmonyOS 概览所需的信息。
const SNAPSHOT_SCRIPT: &str = r#"
echo @@MODEL
param get const.product.model
echo @@BRAND
param get const.product.brand
echo @@OS
param get const.product.software.version
echo @@FULLNAME
param get const.ohos.fullname
echo @@API
param get const.ohos.apiversion
echo @@ABI
param get const.product.cpu.abilist
echo @@NAME
param get const.product.name
echo @@UDID
bm get -u
echo @@BATTERY
hidumper -s 3302 -a -i
echo @@POWER
hidumper -s PowerManagerService -a -s
echo @@SCREEN
hidumper -s RenderService -a screen
echo @@DISPLAY
hidumper -s DisplayManagerService -a -a
echo @@WIFI
hidumper -s WifiDevice
echo @@MEM
cat /proc/meminfo
echo @@DF
df -k /data
echo @@UPTIME
cat /proc/uptime
echo @@IP
ifconfig
"#;

pub fn parse_snapshot(key: &str, raw: &str) -> HdcSnapshot {
    let sections = split_markers(raw);
    let section = |name: &str| sections.get(name).map(String::as_str).unwrap_or("");

    let model = clean_param(section("MODEL"));
    let brand = clean_param(section("BRAND"));
    let software = clean_param(section("OS"));
    let fullname = clean_param(section("FULLNAME"));
    let os_version = if software.is_empty() {
        fullname
    } else {
        software
    };
    let api_version = clean_param(section("API"));
    let abi = clean_param(section("ABI"));
    let udid = parse_udid(section("UDID"));
    let (battery, battery_status) = parse_battery(section("BATTERY"));
    let (memory_total_kb, memory_available_kb) = parse_memory(section("MEM"));
    let (storage_total_kb, storage_used_kb) = parse_storage(section("DF"));
    let uptime_seconds = parse_uptime(section("UPTIME"));
    let ip = parse_ip(section("IP"));
    let device_name = clean_param(section("NAME"));
    let screen_state = parse_screen_state(section("POWER"));
    let screen_resolution = parse_resolution(section("SCREEN"), section("DISPLAY"));
    let screen_rotation = parse_rotation(section("DISPLAY"));
    let battery_temperature_c = parse_battery_temperature(section("BATTERY"))
        .or_else(|| parse_battery_temperature(section("POWER")));
    let wifi_ssid = parse_wifi_ssid(section("WIFI"));

    HdcSnapshot {
        key: key.to_string(),
        model,
        brand,
        os_version,
        api_version,
        abi,
        udid,
        ip,
        battery,
        battery_status,
        memory_total_kb,
        memory_available_kb,
        storage_total_kb,
        storage_used_kb,
        uptime_seconds,
        device_name,
        screen_state,
        screen_resolution,
        screen_rotation,
        battery_temperature_c,
        wifi_ssid,
    }
}

fn split_markers(raw: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer: Vec<&str> = Vec::new();
    for line in raw.lines() {
        if let Some(name) = line.trim().strip_prefix("@@") {
            if let Some(previous) = current.take() {
                sections.insert(previous, buffer.join("\n"));
            }
            buffer.clear();
            current = Some(name.trim().to_string());
        } else if current.is_some() {
            buffer.push(line);
        }
    }
    if let Some(previous) = current {
        sections.insert(previous, buffer.join("\n"));
    }
    sections
}

/// `param get` 在参数不存在时会打印提示文本，不能当作型号展示。
fn clean_param(raw: &str) -> String {
    let value = raw.lines().next().unwrap_or("").trim();
    if value.is_empty()
        || value.starts_with('[')
        || value.to_ascii_lowercase().contains("not set")
        || value.to_ascii_lowercase().contains("fail")
        || value.eq_ignore_ascii_case("unknown")
    {
        String::new()
    } else {
        value.to_string()
    }
}

/// `bm get -u` 输出：
/// `udid of current device is:` 换行 `23CADE0C...`。
fn parse_udid(raw: &str) -> String {
    let mut udid = String::new();
    for line in raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('['))
    {
        if !line.to_ascii_lowercase().contains("udid of") && looks_like_udid(line) {
            udid = line.to_string();
        }
    }
    udid
}

/// UDID 通常是十六进制串（HarmonyOS 常见为 64 位），但也可能是带 `-` 的设备串号。
/// 只接受由字母、数字、`-`、`_` 组成的行，避免把 `Not match target founded`
/// 之类的错误文本当成 UDID。
fn looks_like_udid(value: &str) -> bool {
    (8..=128).contains(&value.len())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn first_number(text: &str) -> Option<u64> {
    let digits: String = text
        .chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn parse_battery(raw: &str) -> (Option<u8>, String) {
    let mut capacity = None;
    let mut status = String::new();
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if capacity.is_none()
            && lower.contains("capacity")
            && let Some(value) = first_number(&lower)
            && value <= 100
        {
            capacity = Some(value as u8);
        }
        if status.is_empty() {
            if lower.contains("discharging") || lower.contains("not charging") {
                status = "未充电".to_string();
            } else if lower.contains("full") {
                status = "已充满".to_string();
            } else if lower.contains("charging") {
                status = "充电中".to_string();
            }
        }
    }
    // hidumper 偶尔只给出百分比行，例如 `battery percent: 85%`。
    if capacity.is_none() {
        for line in raw.lines() {
            let lower = line.to_ascii_lowercase();
            if lower.contains('%')
                && let Some(value) = first_number(&lower)
                && value <= 100
            {
                capacity = Some(value as u8);
                break;
            }
        }
    }
    (capacity, status)
}

fn first_float(text: &str) -> Option<f32> {
    let start = text.find(|ch: char| ch.is_ascii_digit() || ch == '-')?;
    let rest = &text[start..];
    let end = rest
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn parse_screen_state(raw: &str) -> String {
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        let value = if let Some((_, rest)) = lower.split_once("current state:") {
            Some(rest)
        } else if let Some((_, rest)) = lower.split_once("wakefulness=") {
            Some(rest)
        } else {
            None
        };
        let Some(value) = value else {
            continue;
        };
        let value = value.trim().trim_matches(|ch| ch == '"' || ch == '\'');
        if value.contains("awake") {
            return "亮屏".to_string();
        }
        if value.contains("shutdown") {
            return "已关机".to_string();
        }
        if ["sleep", "inactive", "doze", "freeze", "hibernate"]
            .iter()
            .any(|state| value.contains(state))
        {
            return "熄屏".to_string();
        }
    }
    String::new()
}

fn take_digits(text: &str) -> (&str, &str) {
    let end = text
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(text.len());
    (&text[..end], &text[end..])
}

fn dimensions_in(text: &str) -> Option<(u32, u32)> {
    let mut index = 0;
    while index < text.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        if !ch.is_ascii_digit() {
            index += ch.len_utf8();
            continue;
        }
        let (width, rest) = take_digits(&text[index..]);
        let rest = rest.trim_start();
        let rest = rest.strip_prefix('x').or_else(|| rest.strip_prefix('×'));
        if let Some(rest) = rest {
            let (height, _) = take_digits(rest.trim_start());
            if let (Ok(width), Ok(height)) = (width.parse::<u32>(), height.parse::<u32>())
                && (100..=20000).contains(&width)
                && (100..=20000).contains(&height)
            {
                return Some((width, height));
            }
        }
        index += width.len().max(1);
    }
    None
}

fn parse_resolution(screen: &str, display: &str) -> Option<(u32, u32)> {
    const LABELS: [&str; 3] = [
        "render size",
        "physical screen resolution",
        "screen resolution",
    ];
    for line in screen.lines() {
        let lower = line.to_ascii_lowercase();
        if LABELS.iter().any(|label| lower.contains(label))
            && let Some(size) = dimensions_in(line)
        {
            return Some(size);
        }
    }
    for line in display.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("bounds") && !lower.contains("resolution") {
            continue;
        }
        if let Some(size) = dimensions_in(line) {
            return Some(size);
        }
        let numbers: Vec<u32> = line
            .split(|ch: char| !ch.is_ascii_digit())
            .filter(|token| !token.is_empty())
            .filter_map(|token| token.parse().ok())
            .collect();
        if numbers.len() >= 4 {
            let (width, height) = (numbers[2], numbers[3]);
            if (100..=20000).contains(&width) && (100..=20000).contains(&height) {
                return Some((width, height));
            }
        }
    }
    None
}

fn parse_rotation(display: &str) -> Option<u16> {
    for line in display.lines() {
        let compact = line.to_ascii_lowercase().replace(' ', "");
        if !compact.contains("screenrotation") && !compact.starts_with("rotation") {
            continue;
        }
        let value = compact
            .split_once('=')
            .map(|(_, value)| value)
            .or_else(|| compact.split_once(':').map(|(_, value)| value));
        if let Some(value) = value
            && let Some(rotation) = first_number(value)
            && rotation <= 360
        {
            return Some(rotation as u16);
        }
    }
    None
}

fn parse_battery_temperature(raw: &str) -> Option<f32> {
    for line in raw.lines() {
        if !line.to_ascii_lowercase().contains("temperature") {
            continue;
        }
        let Some(mut value) = first_float(line) else {
            continue;
        };
        // BatteryService 的温度单位是 0.1°C，例如 280 表示 28°C。
        if value > 100.0 {
            value /= 10.0;
        }
        if (-20.0..=100.0).contains(&value) {
            return Some(value);
        }
    }
    None
}

fn parse_wifi_ssid(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("ssid") {
            continue;
        }
        let value = line
            .split_once(':')
            .map(|(_, value)| value)
            .or_else(|| line.split_once('=').map(|(_, value)| value))
            .unwrap_or("");
        let value = value.trim().trim_matches('"').trim_matches('\'');
        let value_lower = value.to_ascii_lowercase();
        if value.is_empty()
            || value.starts_with('<')
            || value_lower.contains("unknown")
            || value_lower == "null"
            || value_lower == "none"
        {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

fn parse_memory(raw: &str) -> (Option<u64>, Option<u64>) {
    let mut total = None;
    let mut available = None;
    for line in raw.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let value = first_number(rest);
        match name.trim() {
            "MemTotal" => total = value,
            "MemAvailable" => available = value,
            _ => {}
        }
    }
    (total, available)
}

/// `df -k /data` 的列：Filesystem 1K-blocks Used Available Use% Mounted on。
fn parse_storage(raw: &str) -> (Option<u64>, Option<u64>) {
    for line in raw.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 5 {
            continue;
        }
        let mounted = tokens
            .last()
            .is_some_and(|value| *value == "/data" || *value == "/");
        if !mounted {
            continue;
        }
        let total = tokens[1].parse::<u64>().ok();
        let used = tokens[2].parse::<u64>().ok();
        if total.is_some() {
            return (total, used);
        }
    }
    (None, None)
}

fn parse_uptime(raw: &str) -> Option<u64> {
    raw.split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|seconds| seconds as u64)
}

/// `ifconfig` 输出中优先取 wlan/eth 接口的非回环 IPv4。
pub fn parse_ip(raw: &str) -> Option<String> {
    let mut current = String::new();
    let mut candidates: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace)
            && let Some(token) = trimmed.split_whitespace().next()
        {
            let name = token.trim_end_matches(':');
            if !name.is_empty() && !name.contains("inet") && !name.contains("addr") {
                current = name.to_string();
            }
        }
        if let Some(ip) = ipv4_in_line(trimmed)
            && !ip.starts_with("127.")
        {
            candidates.push((current.clone(), ip));
        }
    }
    let preferred = candidates.iter().find(|(name, _)| {
        name.starts_with("wlan") || name.starts_with("eth") || name.starts_with("en")
    });
    let fallback = candidates.iter().find(|(name, _)| {
        !name.starts_with("p2p") && !name.starts_with("docker") && !name.starts_with("br-")
    });
    preferred
        .or(fallback)
        .or_else(|| candidates.first())
        .map(|(_, ip)| ip.clone())
}

fn ipv4_in_line(line: &str) -> Option<String> {
    let rest = line
        .split("inet addr:")
        .nth(1)
        .or_else(|| line.split("inet ").nth(1))?;
    let candidate = rest.split_whitespace().next()?;
    let candidate = candidate.split('/').next()?;
    if candidate.matches('.').count() == 3
        && candidate.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// 一个已安装的 HarmonyOS 应用（Bundle）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HdcPackage {
    pub name: String,
    pub label: Option<String>,
}

impl HdcPackage {
    pub fn display_name(&self) -> String {
        match self
            .label
            .as_deref()
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            Some(label) => format!("{label}（{}）", self.name),
            None => self.name.clone(),
        }
    }
}

fn looks_like_bundle(name: &str) -> bool {
    !name.is_empty()
        && name.len() > 2
        && name.contains('.')
        && !name.chars().any(char::is_whitespace)
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '$' | '-'))
}

/// 解析 `bm dump -a` 与 `bm dump -a -l`。
///
/// `-a` 形如：
/// ```text
/// ID: 100:
///   com.example.app
/// ```
/// `-a -l` 在 HarmonyOS / OpenHarmony 上通常输出 JSON 数组：
/// ```json
/// [ { "bundleName": "com.example.app", "label": "示例应用" } ]
/// ```
/// 旧版本可能只把标签附在包名后（`com.example.app: 示例应用`），
/// 因此两种格式都要支持，其余行一律跳过。
pub fn parse_packages(raw: &str) -> Vec<HdcPackage> {
    let mut packages = Vec::new();
    let mut seen = HashSet::new();

    for package in parse_package_json(raw) {
        if seen.insert(package.name.clone()) {
            packages.push(package);
        }
    }

    for line in raw.lines() {
        let trimmed = line.trim().trim_matches('"');
        if trimmed.is_empty() || trimmed.starts_with("ID:") {
            continue;
        }
        let (name, label) = match trimmed.split_once(':') {
            Some((name, label))
                if looks_like_bundle(name.trim())
                    && !label.trim().is_empty()
                    && !label.trim().chars().all(|ch| ch.is_ascii_digit()) =>
            {
                (name.trim(), Some(label.trim().to_string()))
            }
            _ => (trimmed, None),
        };
        if !looks_like_bundle(name) || !seen.insert(name.to_string()) {
            continue;
        }
        packages.push(HdcPackage {
            name: name.to_string(),
            label,
        });
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages
}

/// 解析 `bm dump -a -l` 的 JSON 数组输出；文本格式返回空。
fn parse_package_json(raw: &str) -> Vec<HdcPackage> {
    // 输出前可能混有 `[Info]` 一类日志行，只把后面紧跟对象或空数组的 `[` 当作起点。
    let start = raw.match_indices('[').find_map(|(index, _)| {
        let rest = raw[index + 1..].trim_start();
        (rest.starts_with('{') || rest.starts_with(']')).then_some(index)
    });
    let Some(start) = start else {
        return Vec::new();
    };
    let mut stream = serde_json::Deserializer::from_str(&raw[start..]).into_iter::<Value>();
    let Ok(Some(value)) = stream.next().transpose() else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut packages = Vec::new();
    for item in items {
        let Some(name) = item.get("bundleName").and_then(Value::as_str) else {
            continue;
        };
        let name = name.trim();
        if !looks_like_bundle(name) {
            continue;
        }
        let label = item
            .get("label")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
            .map(str::to_string);
        packages.push(HdcPackage {
            name: name.to_string(),
            label,
        });
    }
    packages
}

/// `hdc fport ls` / `hdc rport ls` 中的一条端口任务。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HdcForward {
    /// 规则所属设备；单设备时可能没有前缀。
    pub device: Option<String>,
    /// 命令中靠前的端口（正向是 PC 端口，反向是设备端口）。
    pub first: String,
    /// 命令中靠后的端口。
    pub second: String,
    pub reverse: bool,
}

impl HdcForward {
    pub fn local(&self) -> &str {
        if self.reverse {
            &self.second
        } else {
            &self.first
        }
    }

    pub fn remote(&self) -> &str {
        if self.reverse {
            &self.first
        } else {
            &self.second
        }
    }

    /// 删除规则时要原样传回的端口对。
    pub fn rule(&self) -> String {
        format!("{} {}", self.first, self.second)
    }

    pub fn direction_label(&self) -> &'static str {
        if self.reverse { "反向" } else { "正向" }
    }
}

/// 解析端口列表；行格式：
/// `FMR... tcp:2080 tcp:2345 [Reverse]` 或 `tcp:1234 tcp:1080 [Forward]`。
pub fn parse_forwards(raw: &str) -> Vec<HdcForward> {
    let mut forwards = Vec::new();
    let mut seen = HashSet::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with('[') || line.starts_with("connect-key") {
            continue;
        }
        let reverse = line.to_ascii_lowercase().contains("[reverse]");
        let tokens: Vec<&str> = line
            .split_whitespace()
            .filter(|token| !token.starts_with('['))
            .collect();
        if tokens.len() < 2 {
            continue;
        }
        let (first, second) = (tokens[tokens.len() - 2], tokens[tokens.len() - 1]);
        if !first.starts_with("tcp:") || !second.starts_with("tcp:") {
            continue;
        }
        let device = if tokens.len() > 2 {
            let device = tokens[..tokens.len() - 2].join(" ");
            (!device.is_empty()).then_some(device)
        } else {
            None
        };
        let key = format!("{first} {second} {reverse}");
        if !seen.insert(key) {
            continue;
        }
        forwards.push(HdcForward {
            device,
            first: first.to_string(),
            second: second.to_string(),
            reverse,
        });
    }
    forwards
}

/// `hilog -x` 的过滤参数。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HilogOptions {
    /// `-L`：最低级别 D/I/W/E/F。
    pub level: Option<String>,
    /// `-T`：标签（逗号分隔，最多 10 个）。
    pub tag: Option<String>,
}

impl HilogOptions {
    pub fn command(&self) -> String {
        let mut command = String::from("hilog -x");
        if let Some(level) = self.level.as_deref().filter(|value| !value.is_empty()) {
            command.push_str(" -L ");
            command.push_str(level);
        }
        if let Some(tag) = self
            .tag
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            command.push_str(" -T ");
            command.push_str(&shell_quote(tag));
        }
        command
    }
}

/// 把任意文本安全地放进设备端 shell 的单引号参数中。
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn first_nonempty_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("hdc")
        .to_string()
}

/// 点击类 UI 操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdcTap {
    Click,
    DoubleClick,
    LongClick,
}

impl HdcTap {
    pub fn label(self) -> &'static str {
        match self {
            Self::Click => "点击",
            Self::DoubleClick => "双击",
            Self::LongClick => "长按",
        }
    }

    fn command(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::DoubleClick => "doubleClick",
            Self::LongClick => "longClick",
        }
    }
}

/// `uitest uiInput dircFling` 的方向编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdcSwipeDirection {
    Left,
    Right,
    Up,
    Down,
}

impl HdcSwipeDirection {
    pub const ALL: [HdcSwipeDirection; 4] = [
        HdcSwipeDirection::Left,
        HdcSwipeDirection::Right,
        HdcSwipeDirection::Up,
        HdcSwipeDirection::Down,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "左滑",
            Self::Right => "右滑",
            Self::Up => "上滑",
            Self::Down => "下滑",
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Up => 2,
            Self::Down => 3,
        }
    }
}

/// 常用硬件按键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdcKey {
    Back,
    Home,
    Power,
}

impl HdcKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::Back => "返回",
            Self::Home => "主页",
            Self::Power => "电源",
        }
    }

    fn event(self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Home => "Home",
            Self::Power => "Power",
        }
    }
}

/// `bm dump -n <bundle>` 的常用字段。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HdcBundleInfo {
    pub bundle: String,
    pub version_name: String,
    pub version_code: Option<u64>,
    pub vendor: String,
    pub debug: bool,
    pub code_path: String,
    pub main_ability: Option<String>,
    pub abilities: Vec<String>,
    pub permissions: Vec<String>,
}

/// 解析 `bm dump -n` 的 JSON 输出；开头可能有一行 `bundleName:`。
pub fn parse_bundle_info(bundle: &str, raw: &str) -> Option<HdcBundleInfo> {
    let start = raw.find('{')?;
    let mut stream = serde_json::Deserializer::from_str(&raw[start..]).into_iter::<Value>();
    let value = stream.next()?.ok()?;

    let mut abilities = Vec::new();
    collect_abilities(&value, &mut abilities);
    let mut seen = HashSet::new();
    abilities.retain(|(name, _)| seen.insert(name.clone()));

    let mut permissions = Vec::new();
    collect_permissions(&value, &mut permissions);
    let mut seen = HashSet::new();
    permissions.retain(|name| seen.insert(name.clone()));

    let main_ability = abilities
        .iter()
        .find(|(_, launcher)| *launcher)
        .map(|(name, _)| strip_ability_name(name))
        .or_else(|| {
            find_json_string(&value, "mainElementName")
                .or_else(|| find_json_string(&value, "mainAbility"))
                .map(|name| strip_ability_name(&name))
        })
        .or_else(|| abilities.first().map(|(name, _)| name.clone()));

    Some(HdcBundleInfo {
        bundle: find_json_string(&value, "bundleName").unwrap_or_else(|| bundle.to_string()),
        version_name: find_json_string(&value, "versionName").unwrap_or_default(),
        version_code: find_json_u64(&value, "versionCode"),
        vendor: find_json_string(&value, "vendor").unwrap_or_default(),
        debug: find_json_bool(&value, "debug").unwrap_or(false),
        code_path: find_json_string(&value, "codePath").unwrap_or_default(),
        main_ability,
        abilities: abilities.into_iter().map(|(name, _)| name).collect(),
        permissions,
    })
}

fn strip_ability_name(name: &str) -> String {
    name.rsplit(['/', ':']).next().unwrap_or(name).to_string()
}

fn find_json_string(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                return Some(found.to_string());
            }
            map.values().find_map(|child| find_json_string(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find_json_string(child, key)),
        _ => None,
    }
}

fn find_json_u64(value: &Value, key: &str) -> Option<u64> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            }) {
                return Some(found);
            }
            map.values().find_map(|child| find_json_u64(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find_json_u64(child, key)),
        _ => None,
    }
}

fn find_json_bool(value: &Value, key: &str) -> Option<bool> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_bool) {
                return Some(found);
            }
            map.values().find_map(|child| find_json_bool(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find_json_bool(child, key)),
        _ => None,
    }
}

fn collect_abilities(value: &Value, out: &mut Vec<(String, bool)>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(
                    key.as_str(),
                    "abilityInfos" | "abilities" | "extensionAbilityInfos"
                ) && let Some(items) = child.as_array()
                {
                    for item in items {
                        if let Some(name) = item.get("name").and_then(Value::as_str) {
                            out.push((name.to_string(), skills_mark_launcher(item)));
                        }
                    }
                }
                collect_abilities(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_abilities(item, out);
            }
        }
        _ => {}
    }
}

fn skills_mark_launcher(ability: &Value) -> bool {
    ability
        .get("skills")
        .and_then(Value::as_array)
        .is_some_and(|skills| {
            skills.iter().any(|skill| {
                ["actions", "entities"].iter().any(|key| {
                    skill
                        .get(*key)
                        .and_then(Value::as_array)
                        .is_some_and(|values| {
                            values.iter().any(|value| {
                                value.as_str().is_some_and(|value| {
                                    value.eq_ignore_ascii_case("ohos.want.action.home")
                                        || value.eq_ignore_ascii_case("entity.system.home")
                                })
                            })
                        })
                })
            })
        })
}

fn collect_permissions(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if matches!(key.as_str(), "reqPermissions" | "permissions")
                    && let Some(items) = child.as_array()
                {
                    for item in items {
                        let name = item.as_str().map(str::to_string).or_else(|| {
                            item.get("name").and_then(Value::as_str).map(str::to_string)
                        });
                        if let Some(name) = name {
                            out.push(name);
                        }
                    }
                }
                collect_permissions(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_permissions(item, out);
            }
        }
        _ => {}
    }
}

/// `hdc install` 的参数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HdcInstallOptions {
    /// `-r`：覆盖安装已有的 HAP。
    pub replace: bool,
    /// `-s`：安装共享包（HSP）。
    pub shared: bool,
}

impl HdcInstallOptions {
    pub fn flags(&self) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if self.replace {
            flags.push("-r");
        }
        if self.shared {
            flags.push("-s");
        }
        flags
    }
}

pub fn install_succeeded(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("fail") || lower.contains("error:") {
        return false;
    }
    lower.contains("success") || lower.contains("appmod finish")
}

/// 把 `bm` / `hdc install` 的错误码翻译成可操作的中文说明。
/// 错误码含义与 `hap_installer` 项目中的处理保持一致。
pub fn install_error_hint(raw: &str) -> Option<&'static str> {
    let lower = raw.to_ascii_lowercase();
    let hints = [
        (
            "9568322",
            "Profile 与证书不匹配：请检查 Profile 是否包含该调试设备的 UDID，且签名证书与创建 Profile 时一致。",
        ),
        (
            "9568289",
            "权限申请失败：应用声明了 system_basic / system_core 等级权限，调试签名无法授予。",
        ),
        (
            "9568297",
            "设备 SDK 版本低于安装包要求：请用与设备镜像匹配的 SDK 重新编译。",
        ),
        (
            "9568332",
            "签名不一致：设备上已安装的应用与本次安装包的签名不同，请先卸载旧版本。",
        ),
        (
            "9568329",
            "签名信息中的包名与应用 bundleName 不一致：请检查 HSP 模块或签名配置。",
        ),
        ("9568320", "HAP 未签名：请先使用调试证书签名再安装。"),
        ("9568263", "不支持降级安装：设备上已安装更高版本。"),
        ("9568304", "安装包不支持当前设备类型。"),
        ("9568407", "HNP 包安装失败：HNP 签名校验未通过。"),
        ("e001005", "当前设备未连接：请重新连接设备后再安装。"),
        ("9568305", "设备存储空间不足或安装包损坏。"),
    ];
    for (code, hint) in hints {
        if lower.contains(code) {
            return Some(hint);
        }
    }
    None
}

pub fn uninstall_succeeded(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("fail") || lower.contains("error:") {
        return false;
    }
    lower.contains("success") || lower.contains("appmod finish") || lower.is_empty()
}

/// 一行 HiLog。
#[derive(Clone, Debug)]
pub struct HdcLogLine {
    pub raw: String,
    pub time: String,
    pub level: LogLevel,
    pub tag: String,
    pub message: String,
}

/// 解析 HiLog 文本，格式：
/// `04-19 17:02:14.735  5394  5394 I A03200/testTag: this is a log`。
pub fn parse_hilog(raw: &str) -> Vec<HdcLogLine> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let level_at = tokens
            .iter()
            .enumerate()
            .skip(4)
            .find(|(_, token)| {
                token.len() == 1
                    && matches!(
                        token.chars().next(),
                        Some('V' | 'D' | 'I' | 'W' | 'E' | 'F' | 'A')
                    )
            })
            .map(|(index, _)| index);
        if let Some(index) = level_at {
            let level = LogLevel::from_char(tokens[index].chars().next().unwrap_or('?'));
            let time = if index >= 4 {
                format!("{} {}", tokens[index - 4], tokens[index - 3])
            } else if index >= 1 {
                tokens[index - 1].to_string()
            } else {
                String::new()
            };
            let tag = tokens
                .get(index + 1)
                .map(|value| value.trim_end_matches(':').to_string())
                .unwrap_or_default();
            let message = tokens
                .get(index + 2..)
                .map(|rest| rest.join(" "))
                .unwrap_or_default();
            lines.push(HdcLogLine {
                raw: line.to_string(),
                time,
                level,
                tag,
                message,
            });
            continue;
        }
        lines.push(HdcLogLine {
            raw: line.to_string(),
            time: String::new(),
            level: LogLevel::Unknown,
            tag: String::new(),
            message: line.to_string(),
        });
    }
    lines
}

/// 重启目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HdcRebootTarget {
    System,
    Shutdown,
    Recovery,
    Bootloader,
}

impl HdcRebootTarget {
    pub const ALL: [HdcRebootTarget; 4] = [
        HdcRebootTarget::System,
        HdcRebootTarget::Shutdown,
        HdcRebootTarget::Recovery,
        HdcRebootTarget::Bootloader,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "重启",
            Self::Shutdown => "关机",
            Self::Recovery => "重启到恢复模式",
            Self::Bootloader => "重启到 Bootloader",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::System => "restart_alt",
            Self::Shutdown => "power_settings_new",
            Self::Recovery => "settings",
            Self::Bootloader => "build",
        }
    }

    /// `hdc target boot` 的参数；重启不带参数。
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::System => &[],
            Self::Shutdown => &["shutdown"],
            Self::Recovery => &["-recovery"],
            Self::Bootloader => &["-bootloader"],
        }
    }

    pub fn outcome(self) -> &'static str {
        match self {
            Self::System => "重启指令已发送",
            Self::Shutdown => "关机指令已发送",
            Self::Recovery => "恢复模式指令已发送",
            Self::Bootloader => "Bootloader 指令已发送",
        }
    }
}

/// hdc 可执行文件封装。
#[derive(Clone, Debug)]
pub struct HdcClient {
    program: PathBuf,
}

impl HdcClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        crate::adb::prepare_command(&mut command);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Android 的 libhdc.so 依赖同目录的 libssl.3.so / libcrypto.3.so / libusb-1.0.so。
        // 显式把 nativeLibraryDir 放进加载器搜索路径，避免个别系统版本不放行应用
        // 命名空间之外的 DT_NEEDED。
        #[cfg(target_os = "android")]
        {
            if let Some(dir) = crate::platform::native_lib_dir() {
                command.env("LD_LIBRARY_PATH", dir);
            }
            // 应用进程通常没有 HOME/TMPDIR。hdc 用 HOME 存客户端密钥（~/.harmony），
            // 用 TMPDIR 存 server 信息与日志；指向应用私有目录才能生成密钥并启动服务。
            let data = crate::config::data_dir();
            command.env("HOME", &data);
            let tmp = data.join("tmp");
            if std::fs::create_dir_all(&tmp).is_ok() {
                command.env("TMPDIR", &tmp);
            }
        }
        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    pub fn version(&self) -> Result<String> {
        let output = self.run(&["-v".to_string()])?;
        let text = output.combined();
        if !output.is_failure() && !text.trim().is_empty() {
            return Ok(first_nonempty_line(&text));
        }
        let fallback = self.run(&["version".to_string()])?;
        let text = fallback.combined();
        if fallback.code.is_none() && text.trim().is_empty() {
            bail!("{}", fallback.error_message());
        }
        Ok(first_nonempty_line(&text))
    }

    pub fn targets(&self) -> Result<Vec<HdcDevice>> {
        let output = self.run(&["list".to_string(), "targets".to_string(), "-v".to_string()])?;
        if output.code.is_none() && output.stdout.trim().is_empty() {
            bail!("{}", output.error_message());
        }
        let devices = parse_targets(&output.stdout);
        if !devices.is_empty() || !output.is_failure() {
            return Ok(devices);
        }
        let plain = self.run(&["list".to_string(), "targets".to_string()])?;
        if !plain.success() && plain.stdout.trim().is_empty() {
            bail!("{}", plain.error_message());
        }
        Ok(parse_targets(&plain.stdout))
    }

    /// 重启 hdc 服务端（`kill -r`），失败时退回 `start -r`。
    pub fn restart_server(&self) -> Result<HdcOutput> {
        let output = self.run(&["kill".to_string(), "-r".to_string()])?;
        if output.is_failure() {
            let start = self.run(&["start".to_string(), "-r".to_string()])?;
            if start.is_failure() {
                bail!("{}", start.error_message());
            }
            std::thread::sleep(Duration::from_millis(300));
            return Ok(start);
        }
        std::thread::sleep(Duration::from_millis(300));
        Ok(output)
    }

    /// 结束 hdc 服务端进程（`hdc kill`）。
    ///
    /// 返回后服务端进程会退出；下一次执行 hdc 命令时会自动重新启动。
    pub fn kill_server(&self) -> Result<HdcOutput> {
        self.run(&["kill".to_string()])
    }

    /// 通过 TCP 连接设备（`hdc tconn IP:port`）。
    pub fn tconn(&self, address: &str) -> Result<HdcOutput> {
        self.run(&["tconn".to_string(), address.to_string()])
    }

    pub fn tconn_remove(&self, address: &str) -> Result<HdcOutput> {
        self.run(&[
            "tconn".to_string(),
            address.to_string(),
            "-remove".to_string(),
        ])
    }

    /// 在 USB 设备上开启无线调试端口：`tmode port <port>`。
    pub fn tmode_port(&self, key: &str, port: u16) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "tmode".to_string(),
            "port".to_string(),
            port.to_string(),
        ])
    }

    /// 关闭设备端无线调试端口：`tmode port close`。
    pub fn tmode_close(&self, key: &str) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "tmode".to_string(),
            "port".to_string(),
            "close".to_string(),
        ])
    }

    pub fn ip_address(&self, key: &str) -> Result<Option<String>> {
        let output = self.shell(key, "ifconfig")?;
        Ok(parse_ip(&output.stdout))
    }

    /// 读取当前设备 UDID（`bm get --udid`）。部分设备只支持 `-u`，回退一次。
    pub fn udid(&self, key: &str) -> Result<Option<String>> {
        for command in ["bm get --udid", "bm get -u"] {
            let output = self.shell(key, command)?;
            let text = output.combined();
            let udid = parse_udid(&text);
            if !udid.is_empty() {
                return Ok(Some(udid));
            }
        }
        Ok(None)
    }

    pub fn shell(&self, key: &str, command: &str) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "shell".to_string(),
            command.to_string(),
        ])
    }

    pub fn install(&self, key: &str, path: &Path, options: HdcInstallOptions) -> Result<HdcOutput> {
        let mut args = vec!["-t".to_string(), key.to_string(), "install".to_string()];
        args.extend(options.flags().into_iter().map(str::to_string));
        args.push(path.to_string_lossy().to_string());
        self.run(&args)
    }

    pub fn uninstall(&self, key: &str, bundle: &str) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "uninstall".to_string(),
            bundle.to_string(),
        ])
    }

    pub fn start_ability(&self, key: &str, bundle: &str, ability: &str) -> Result<HdcOutput> {
        self.shell(
            key,
            &format!(
                "aa start -a {} -b {}",
                shell_quote(ability),
                shell_quote(bundle)
            ),
        )
    }

    /// 无法确定主 Ability 时只按 Bundle 启动，由系统选择默认入口。
    pub fn start_bundle(&self, key: &str, bundle: &str) -> Result<HdcOutput> {
        self.shell(key, &format!("aa start -b {}", shell_quote(bundle)))
    }

    /// 通过 URI / Scheme 拉起应用：`aa start -U <uri>`。
    pub fn start_uri(&self, key: &str, uri: &str) -> Result<HdcOutput> {
        self.shell(key, &format!("aa start -U {}", shell_quote(uri)))
    }

    pub fn bundle_info(&self, key: &str, bundle: &str) -> Result<HdcBundleInfo> {
        let output = self.shell(key, &format!("bm dump -n {}", shell_quote(bundle)))?;
        if output.is_failure() && output.stdout.trim().is_empty() {
            bail!("{}", output.error_message());
        }
        let Some(info) = parse_bundle_info(bundle, &output.stdout) else {
            bail!(
                "无法解析应用详情：{}",
                output.combined().chars().take(240).collect::<String>()
            );
        };
        Ok(info)
    }

    /// `bm clean -c` 清缓存，`bm clean -d` 清数据。
    pub fn clean_app(&self, key: &str, bundle: &str, cache_only: bool) -> Result<HdcOutput> {
        let flag = if cache_only { "-c" } else { "-d" };
        self.shell(key, &format!("bm clean {flag} -n {}", shell_quote(bundle)))
    }

    /// 列出正向与反向端口任务。
    pub fn forwards(&self, key: &str) -> Result<Vec<HdcForward>> {
        let forward = self.run(&[
            "-t".to_string(),
            key.to_string(),
            "fport".to_string(),
            "ls".to_string(),
        ])?;
        if !forward.success() && forward.stdout.trim().is_empty() {
            bail!("{}", forward.error_message());
        }
        let mut forwards = parse_forwards(&forward.stdout);
        let reverse = self.run(&[
            "-t".to_string(),
            key.to_string(),
            "rport".to_string(),
            "ls".to_string(),
        ])?;
        for item in parse_forwards(&reverse.stdout) {
            if !forwards
                .iter()
                .any(|existing| existing.rule() == item.rule() && existing.device == item.device)
            {
                forwards.push(item);
            }
        }
        Ok(forwards)
    }

    /// 正向：PC 端口 → 设备端口；反向：设备端口 → PC 端口。
    pub fn add_forward(
        &self,
        key: &str,
        local: u16,
        remote: u16,
        reverse: bool,
    ) -> Result<HdcOutput> {
        let (command, first, second) = if reverse {
            ("rport", remote, local)
        } else {
            ("fport", local, remote)
        };
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            command.to_string(),
            format!("tcp:{first}"),
            format!("tcp:{second}"),
        ])
    }

    pub fn remove_forward(&self, key: &str, forward: &HdcForward) -> Result<HdcOutput> {
        let mut args = vec![
            "-t".to_string(),
            key.to_string(),
            "fport".to_string(),
            "rm".to_string(),
        ];
        args.extend(forward.rule().split_whitespace().map(str::to_string));
        self.run(&args)
    }

    pub fn force_stop(&self, key: &str, bundle: &str) -> Result<HdcOutput> {
        self.shell(key, &format!("aa force-stop {}", shell_quote(bundle)))
    }

    pub fn reboot(&self, key: &str, target: HdcRebootTarget) -> Result<HdcOutput> {
        let mut args = vec![
            "-t".to_string(),
            key.to_string(),
            "target".to_string(),
            "boot".to_string(),
        ];
        args.extend(target.args().iter().map(|arg| (*arg).to_string()));
        self.run(&args)
    }

    pub fn push(&self, key: &str, local: &Path, remote: &str) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "file".to_string(),
            "send".to_string(),
            local.to_string_lossy().to_string(),
            remote.to_string(),
        ])
    }

    pub fn pull(&self, key: &str, remote: &str, local: &Path) -> Result<HdcOutput> {
        self.run(&[
            "-t".to_string(),
            key.to_string(),
            "file".to_string(),
            "recv".to_string(),
            remote.to_string(),
            local.to_string_lossy().to_string(),
        ])
    }

    /// 非阻塞读取 HiLog 缓冲区后退出（`hilog -x`），可附带级别与标签过滤。
    pub fn hilog_capture(&self, key: &str, options: &HilogOptions) -> Result<HdcOutput> {
        self.shell(key, &options.command())
    }

    pub fn hilog_clear(&self, key: &str) -> Result<HdcOutput> {
        self.shell(key, "hilog -r")
    }

    pub fn uitest_key(&self, key: &str, key_event: HdcKey) -> Result<HdcOutput> {
        self.shell(
            key,
            &format!("uitest uiInput keyEvent {}", key_event.event()),
        )
    }

    pub fn uitest_tap(&self, key: &str, x: u32, y: u32, tap: HdcTap) -> Result<HdcOutput> {
        self.shell(key, &format!("uitest uiInput {} {x} {y}", tap.command()))
    }

    pub fn uitest_fling(
        &self,
        key: &str,
        direction: HdcSwipeDirection,
        speed: u32,
    ) -> Result<HdcOutput> {
        self.shell(
            key,
            &format!("uitest uiInput dircFling {} {speed}", direction.code()),
        )
    }

    pub fn uitest_input_text(&self, key: &str, x: u32, y: u32, text: &str) -> Result<HdcOutput> {
        self.shell(
            key,
            &format!("uitest uiInput inputText {x} {y} {}", shell_quote(text)),
        )
    }

    pub fn wake_screen(&self, key: &str) -> Result<HdcOutput> {
        self.shell(key, "power-shell wakeup")
    }

    pub fn screen_cap(&self, key: &str, remote: &str) -> Result<HdcOutput> {
        self.shell(key, &format!("uitest screenCap -p {}", shell_quote(remote)))
    }

    pub fn snapshot(&self, key: &str) -> Result<HdcSnapshot> {
        let output = self.shell(key, SNAPSHOT_SCRIPT)?;
        if !output.success() && output.stdout.trim().is_empty() {
            bail!("{}", output.error_message());
        }
        Ok(parse_snapshot(key, &output.stdout))
    }

    /// 读取已安装应用。优先 `bm dump -a -l`（带标签），旧版本不支持时退回 `bm dump -a`。
    pub fn packages(&self, key: &str) -> Result<Vec<HdcPackage>> {
        let with_labels = self.shell(key, "bm dump -a -l")?;
        let mut packages = parse_packages(&with_labels.stdout);
        if packages.is_empty() {
            let plain = self.shell(key, "bm dump -a")?;
            if !plain.success() && plain.stdout.trim().is_empty() {
                bail!("{}", plain.error_message());
            }
            packages = parse_packages(&plain.stdout);
            if packages.is_empty() {
                bail!(
                    "设备没有返回应用列表：{}",
                    plain.combined().chars().take(200).collect::<String>()
                );
            }
        }
        Ok(packages)
    }

    /// 执行 hdc 子命令，不绑定设备。
    pub fn run(&self, args: &[String]) -> Result<HdcOutput> {
        let output = self
            .command()
            .args(args)
            .output()
            .with_context(|| format!("无法执行 HDC：{}", self.program.display()))?;
        Ok(HdcOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

/// 在常见位置查找 hdc，返回候选列表（包含来自 PATH 的 `hdc`）。
pub fn hdc_candidates(configured: &str) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |path: PathBuf| {
        if seen.insert(path.clone()) {
            candidates.push(path);
        }
    };

    let configured = configured.trim();
    if !configured.is_empty() {
        push(PathBuf::from(configured));
    }
    if let Some(from_env) = std::env::var_os("RAREBOX_HDC") {
        push(PathBuf::from(from_env));
    }

    // Android 端优先用随 APK 分发的 hdc（`nativeLibraryDir/libhdc.so`），
    // 没有时再退回 Termux / 系统路径。
    #[cfg(target_os = "android")]
    {
        if let Some(dir) = crate::platform::native_lib_dir() {
            push(dir.join("libhdc.so"));
        }
        for path in [
            "/data/data/com.termux/files/usr/bin/hdc",
            "/system/bin/hdc",
            "/product/bin/hdc",
            "/vendor/bin/hdc",
        ] {
            push(PathBuf::from(path));
        }
    }

    // 内置 hdc 优先于外部查找：若已随应用分发，就不需要用户单独安装 DevEco Studio。
    if let Some(embedded) = crate::hdc_bundle::ensure_embedded_hdc() {
        push(embedded);
    }

    let executable = if cfg!(target_os = "windows") {
        "hdc.exe"
    } else {
        "hdc"
    };

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        push(parent.join(executable));
        push(parent.join("toolchains").join(executable));
    }
    if let Ok(current_dir) = std::env::current_dir() {
        push(current_dir.join(executable));
        push(current_dir.join("toolchains").join(executable));
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["DEVECO_SDK_HOME", "HOS_SDK_HOME", "OHOS_SDK_HOME"] {
        if let Some(root) = std::env::var_os(variable) {
            roots.push(PathBuf::from(root));
        }
    }
    if cfg!(target_os = "windows") {
        for variable in ["LOCALAPPDATA", "USERPROFILE"] {
            if let Some(root) = std::env::var_os(variable) {
                let base = PathBuf::from(root);
                roots.push(base.join("Huawei/Sdk"));
                roots.push(base.join("AppData/Local/Huawei/Sdk"));
                roots.push(base.join("AppData/Local/OpenHarmony/Sdk"));
            }
        }
        roots.push(PathBuf::from("C:/Program Files/Huawei/DevEco Studio"));
        roots.push(PathBuf::from("C:/Program Files/Huawei/DevEco Studio/sdk"));
    } else if cfg!(target_os = "macos") {
        if let Some(home) = std::env::var_os("HOME") {
            let base = PathBuf::from(home);
            roots.push(base.join("Library/Huawei/Sdk"));
            roots.push(base.join("Library/OpenHarmony/Sdk"));
        }
        roots.push(PathBuf::from(
            "/Applications/DevEco-Studio.app/Contents/sdk",
        ));
    } else if let Some(home) = std::env::var_os("HOME") {
        let base = PathBuf::from(home);
        roots.push(base.join("Huawei/Sdk"));
        roots.push(base.join("OpenHarmony/Sdk"));
        roots.push(base.join("DevEcoStudio/sdk"));
    }

    let mut discovered = Vec::new();
    for root in &roots {
        if root.is_dir() {
            collect_toolchains(root, 5, &mut discovered);
        }
    }
    // 同一 SDK 下可能装了多个 API 版本；路径倒序通常能拿到较新的版本。
    discovered.sort_by(|left, right| right.cmp(left));
    for path in discovered {
        push(path);
    }

    push(PathBuf::from("hdc"));
    candidates
}

fn collect_toolchains(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let executable = if cfg!(target_os = "windows") {
        "hdc.exe"
    } else {
        "hdc"
    };
    let direct = root.join("toolchains").join(executable);
    if direct.is_file() {
        out.push(direct);
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_toolchains(&path, depth - 1, out);
        }
    }
}

/// 验证候选 hdc 是否可用，返回第一个可执行的路径与版本。
pub fn detect_hdc(configured: &str) -> Option<(PathBuf, String)> {
    for candidate in hdc_candidates(configured) {
        if candidate.components().count() > 1 && !candidate.is_file() {
            continue;
        }
        let client = HdcClient::new(candidate.clone());
        if let Ok(version) = client.version() {
            return Some((candidate, version));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_list() {
        let devices = parse_targets("127.0.0.1:5555\nFMR0223C13000649\n");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].connection, HdcConnection::Tcp);
        assert_eq!(devices[1].connection, HdcConnection::Usb);
        assert_eq!(devices[1].display_name(), "FMR0223C13000649");
    }

    #[test]
    fn parses_empty_target_list() {
        assert!(parse_targets("[Empty]\n").is_empty());
        assert!(parse_targets("[Empty]\n\thdc\n").is_empty());
        assert!(parse_targets("Empty\n").is_empty());
        assert!(parse_targets("").is_empty());
    }

    #[test]
    fn parses_verbose_target_list() {
        let raw = "FMR0223C13000649\tUSB\tConnected\tlocalhost\tHDC\n\
                   192.168.1.20:5555\tTCP\tOffline\tMate 60\tHDC\n";
        let devices = parse_targets(raw);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].key, "FMR0223C13000649");
        assert_eq!(devices[0].state, HdcTargetState::Connected);
        assert!(devices[0].is_online());
        assert_eq!(devices[1].connection, HdcConnection::Tcp);
        assert_eq!(devices[1].state, HdcTargetState::Offline);
        assert!(!devices[1].is_online());
    }

    #[test]
    fn parses_snapshot_sections() {
        let raw = "@@MODEL\nHUAWEI Mate 60\n@@BRAND\nHUAWEI\n\
                   @@OS\n5.0.0.102\n@@FULLNAME\nOpenHarmony-5.0.0.71\n\
                   @@API\n12\n@@ABI\narm64-v8a\n@@NAME\nMate 60 Pro\n\
                   @@UDID\nudid of current device is:\n23CADE0C1234\n\
                   @@BATTERY\nBatteryService:\n  capacity: 87\n  state: charging\n  temperature: 281\n\
                   @@POWER\nCurrent State: AWAKE\n\
                   @@SCREEN\nrender size: 1260x2720\n\
                   @@DISPLAY\nScreenRotation: 90\nBounds<L,T,W,H>: 0, 0, 1260, 2720\n\
                   @@WIFI\nConnection.ssid: K-Lab\n\
                   @@MEM\nMemTotal: 8000000 kB\nMemAvailable: 3000000 kB\n\
                   @@DF\nFilesystem 1K-blocks Used Available Use% Mounted on\n\
                   /dev/block/dm-3 10000000 2500000 7500000 25% /data\n\
                   @@UPTIME\n12345.67 100.0\n\
                   @@IP\nlo: inet addr:127.0.0.1\nwlan0: inet addr:192.168.1.20\n";
        let snapshot = parse_snapshot("FMR123", raw);
        assert_eq!(snapshot.model, "HUAWEI Mate 60");
        assert_eq!(snapshot.brand, "HUAWEI");
        assert_eq!(snapshot.os_version, "5.0.0.102");
        assert_eq!(snapshot.api_version, "12");
        assert_eq!(snapshot.abi, "arm64-v8a");
        assert_eq!(snapshot.udid, "23CADE0C1234");
        assert_eq!(snapshot.battery, Some(87));
        assert_eq!(snapshot.battery_status, "充电中");
        assert_eq!(snapshot.memory_total_kb, Some(8_000_000));
        assert_eq!(snapshot.memory_available_kb, Some(3_000_000));
        assert_eq!(snapshot.storage_total_kb, Some(10_000_000));
        assert_eq!(snapshot.storage_used_kb, Some(2_500_000));
        assert_eq!(snapshot.uptime_seconds, Some(12_345));
        assert_eq!(snapshot.ip.as_deref(), Some("192.168.1.20"));
        assert_eq!(snapshot.device_name, "Mate 60 Pro");
        assert_eq!(snapshot.screen_state, "亮屏");
        assert_eq!(snapshot.screen_resolution, Some((1260, 2720)));
        assert_eq!(snapshot.screen_rotation, Some(90));
        assert!((snapshot.battery_temperature_c.unwrap() - 28.1).abs() < 0.01);
        assert_eq!(snapshot.wifi_ssid.as_deref(), Some("K-Lab"));
        assert_eq!(snapshot.screen_label(), "亮屏 · 1260×2720 · 旋转 90°");
        assert_eq!(snapshot.battery_status_label(), "充电中");
        assert!(snapshot.memory_percent().is_some());
    }

    #[test]
    fn missing_params_are_not_shown_as_model() {
        let raw = "@@MODEL\nSystem parameter not set\n@@BRAND\n[Fail]Not found\n@@OS\n\n";
        let snapshot = parse_snapshot("key", raw);
        assert!(snapshot.model.is_empty());
        assert!(snapshot.brand.is_empty());
        assert!(snapshot.os_version.is_empty());
    }

    #[test]
    fn parses_bundle_list() {
        let raw = "ID: 100:\n  com.example.app\n  com.example.other\n";
        let packages = parse_packages(raw);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "com.example.app");
        assert!(packages[0].label.is_none());
    }

    #[test]
    fn parses_bundle_list_with_labels() {
        let raw = "ID: 100:\n  com.example.app: 示例应用\n  com.example.other: 其他\n";
        let packages = parse_packages(raw);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].label.as_deref(), Some("示例应用"));
        assert_eq!(packages[0].display_name(), "示例应用（com.example.app）");
    }
    #[test]
    fn parses_json_bundle_list_with_labels() {
        let raw = "[\n\
                   {\n\
                       \"bundleName\": \"com.genouka.poet\",\n\
                       \"label\": \"离线诗词\"\n\
                   },\n\
                   {\n\
                       \"bundleName\": \"com.huawei.hmos.settings\",\n\
                       \"label\": \"设置\"\n\
                   },\n\
                   {\n\
                       \"bundleName\": \"com.ohos.no.label\"\n\
                   }\n\
                   ]\n";
        let packages = parse_packages(raw);
        assert_eq!(packages.len(), 3);
        assert_eq!(packages[0].name, "com.genouka.poet");
        assert_eq!(packages[0].display_name(), "离线诗词（com.genouka.poet）");
        assert_eq!(
            packages[1].display_name(),
            "设置（com.huawei.hmos.settings）"
        );
        assert_eq!(packages[2].name, "com.ohos.no.label");
        assert!(packages[2].label.is_none());
    }

    #[test]
    fn skips_non_bundle_lines() {
        let raw = "ID: 100:\nBundleName: com.example.app\n  started\n";
        assert!(parse_packages(raw).is_empty());
    }

    #[test]
    fn parses_hilog_lines() {
        let raw = "04-19 17:02:14.735  5394  5394 I A03200/testTag: this is a log\n\
                   not a log line\n";
        let lines = parse_hilog(raw);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].level, LogLevel::Info);
        assert_eq!(lines[0].tag, "A03200/testTag");
        assert_eq!(lines[0].message, "this is a log");
        assert_eq!(lines[0].time, "04-19 17:02:14.735");
        assert_eq!(lines[1].level, LogLevel::Unknown);
    }

    #[test]
    fn rejects_error_text_as_udid() {
        assert_eq!(parse_udid("[Info]Not match target founded"), "");
        assert_eq!(parse_udid("error: device not found"), "");
        assert_eq!(
            parse_udid("udid of current device is:\n23CADE0C1234"),
            "23CADE0C1234"
        );
    }

    #[test]
    fn detects_install_result() {
        assert!(install_succeeded(
            "[Info]App install path:/data/local/tmp/a.hap, msg:install bundle successfully.\nAppMod finish"
        ));
        assert!(!install_succeeded(
            "error: failed to install bundle. code:9568322"
        ));
    }

    #[test]
    fn explains_install_errors() {
        assert!(
            install_error_hint("error: failed to install bundle. code:9568322")
                .unwrap()
                .contains("Profile")
        );
        assert!(
            install_error_hint("code:9568320")
                .unwrap()
                .contains("未签名")
        );
        assert!(install_error_hint("something else").is_none());
    }

    #[test]
    fn reboot_targets_map_to_hdc_arguments() {
        assert_eq!(HdcRebootTarget::System.args(), &[] as &[&str]);
        assert_eq!(HdcRebootTarget::Shutdown.args(), &["shutdown"]);
        assert_eq!(HdcRebootTarget::Recovery.args(), &["-recovery"]);
        assert_eq!(HdcRebootTarget::Bootloader.args(), &["-bootloader"]);
    }

    #[test]
    fn parses_ip_from_ifconfig() {
        assert_eq!(
            parse_ip("lo: inet addr:127.0.0.1\nwlan0: inet addr:10.0.0.8\n"),
            Some("10.0.0.8".to_string())
        );
        assert_eq!(
            parse_ip(
                "p2p0: inet addr:192.168.49.1\n\
                 wlan0     Link encap:Ethernet\n\
                   inet addr:192.168.1.23  Bcast:192.168.1.255\n"
            ),
            Some("192.168.1.23".to_string())
        );
        assert_eq!(parse_ip("no addresses here"), None);
    }

    #[test]
    fn parses_forward_and_reverse_rules() {
        let raw = "FMR123 tcp:2080 tcp:2345 [Reverse]\n\
                   FMR123 tcp:1234 tcp:1080 [Forward]\n\
                   [Empty]\n";
        let forwards = parse_forwards(raw);
        assert_eq!(forwards.len(), 2);
        assert!(forwards[0].reverse);
        assert_eq!(forwards[0].remote(), "tcp:2080");
        assert_eq!(forwards[0].local(), "tcp:2345");
        assert_eq!(forwards[0].rule(), "tcp:2080 tcp:2345");
        assert!(!forwards[1].reverse);
        assert_eq!(forwards[1].local(), "tcp:1234");
        assert_eq!(forwards[1].remote(), "tcp:1080");
    }

    #[test]
    fn parses_bundle_details() {
        let raw = "com.example.app:\n{\n  \"bundleName\": \"com.example.app\",\n\
                   \"versionCode\": 1001,\n  \"versionName\": \"1.0.1\",\n\
                   \"vendor\": \"Example\",\n  \"mainElementName\": \"entry/EntryAbility\",\n\
                   \"applicationInfo\": {\n    \"debug\": true,\n\
                     \"codePath\": \"/data/app/el1/bundle/public/com.example.app\",\n\
                     \"reqPermissions\": [\"ohos.permission.INTERNET\"]\n  },\n\
                   \"hapModuleInfos\": [{\"abilityInfos\": [\n\
                     {\"name\": \"EntryAbility\", \"skills\": [{\"actions\": [\"ohos.want.action.home\"], \"entities\": [\"entity.system.home\"]}]},\n\
                     {\"name\": \"OtherAbility\"}\n  ]}]}\n";
        let info = parse_bundle_info("com.example.app", raw).unwrap();
        assert_eq!(info.bundle, "com.example.app");
        assert_eq!(info.version_name, "1.0.1");
        assert_eq!(info.version_code, Some(1001));
        assert_eq!(info.vendor, "Example");
        assert!(info.debug);
        assert_eq!(info.main_ability.as_deref(), Some("EntryAbility"));
        assert_eq!(info.abilities, ["EntryAbility", "OtherAbility"]);
        assert_eq!(info.permissions, ["ohos.permission.INTERNET"]);
        assert!(info.code_path.ends_with("com.example.app"));
    }

    #[test]
    fn hilog_options_build_filter_command() {
        let options = HilogOptions {
            level: Some("W".to_string()),
            tag: Some("My Tag".to_string()),
        };
        assert_eq!(options.command(), "hilog -x -L W -T 'My Tag'");
        assert_eq!(HilogOptions::default().command(), "hilog -x");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn install_options_map_to_flags() {
        assert_eq!(
            HdcInstallOptions {
                replace: true,
                shared: true,
            }
            .flags(),
            ["-r", "-s"]
        );
        assert!(HdcInstallOptions::default().flags().is_empty());
    }

    #[test]
    fn bundle_info_falls_back_to_main_element() {
        let raw = r#"{
            "bundleName": "com.example.fallback",
            "versionName": "2.0.0",
            "mainElementName": "entry/MainAbility",
            "hapModuleInfos": [{"name": "entry"}]
        }"#;
        let info = parse_bundle_info("com.example.fallback", raw).unwrap();
        assert_eq!(info.main_ability.as_deref(), Some("MainAbility"));
        assert!(info.abilities.is_empty());
    }

    #[test]
    fn configured_path_is_first_candidate() {
        let candidates = hdc_candidates("D:/sdk/toolchains/hdc.exe");
        assert_eq!(
            candidates.first().unwrap(),
            Path::new("D:/sdk/toolchains/hdc.exe")
        );
    }
}

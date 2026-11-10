//! 设备概览、文件与应用的解析。与桌面端 `rarebox-core` 的展示字段保持一致。

use serde::Serialize;

use crate::adb::RemoteEntry;

/// 一次抓取设备概览所需的命令。`exec:` 输出里用 `@@` 标记分段。
pub const SNAPSHOT_SCRIPT: &str = r#"
echo @@MODEL
getprop ro.product.model
echo @@MANUFACTURER
getprop ro.product.manufacturer
echo @@ANDROID
getprop ro.build.version.release
echo @@SDK
getprop ro.build.version.sdk
echo @@BUILD
getprop ro.build.display.id
echo @@CHARACTERISTICS
getprop ro.build.characteristics
echo @@PATCH
getprop ro.build.version.security_patch
echo @@ABI
getprop ro.product.cpu.abi
echo @@SIZE
wm size
echo @@DENSITY
wm density
echo @@BATTERY
dumpsys battery
echo @@MEM
cat /proc/meminfo
echo @@DF
df -h /data
echo @@UPTIME
cat /proc/uptime
echo @@LOAD
cat /proc/loadavg
echo @@ROTATION
settings get system user_rotation
echo @@IP
ip route
echo @@END
"#;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub physical_size: String,
    pub override_size: Option<String>,
    pub physical_density: String,
    pub override_density: Option<String>,
    pub rotation: u8,
    pub rotation_label: &'static str,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub level: Option<u8>,
    pub status: Option<u8>,
    pub status_label: String,
    pub temperature: Option<f32>,
    pub voltage: Option<u32>,
    pub powered: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_kb: Option<u64>,
    pub available_kb: Option<u64>,
    pub used_label: String,
    pub total_label: String,
    pub percent: u8,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub size: String,
    pub used: String,
    pub available: String,
    pub percent: u8,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub serial: String,
    pub title: String,
    pub model: String,
    pub manufacturer: String,
    pub android: String,
    pub sdk: String,
    pub build_id: String,
    pub characteristics: String,
    pub patch: String,
    pub abi: String,
    pub wearable: bool,
    pub display: DisplayInfo,
    pub battery: BatteryInfo,
    pub memory: MemoryInfo,
    pub storage: StorageInfo,
    pub uptime_seconds: Option<u64>,
    pub uptime_label: String,
    pub load: Option<String>,
    pub ip: Option<String>,
}

pub fn parse_snapshot(serial: &str, raw: &str) -> Snapshot {
    let sections = split_markers(raw);
    let get = |name: &str| -> String {
        sections
            .get(name)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };

    let display = parse_display(&get("SIZE"), &get("DENSITY"), &get("ROTATION"));
    let battery = parse_battery(&get("BATTERY"));
    let memory = parse_memory(&get("MEM"));
    let storage = parse_storage(&get("DF"));
    let uptime_seconds = parse_uptime(&get("UPTIME"));
    let load = {
        let value = get("LOAD")
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        (!value.trim().is_empty()).then(|| value.trim().to_string())
    };
    let ip = parse_ip(&get("IP"));

    let model = get("MODEL");
    let manufacturer = get("MANUFACTURER");
    let characteristics = get("CHARACTERISTICS");

    Snapshot {
        serial: serial.to_string(),
        title: device_title(&manufacturer, &model, serial),
        wearable: characteristics.to_ascii_lowercase().contains("watch")
            || model.to_ascii_lowercase().contains("watch"),
        model,
        manufacturer,
        android: get("ANDROID"),
        sdk: get("SDK"),
        build_id: get("BUILD"),
        characteristics,
        patch: get("PATCH"),
        abi: get("ABI"),
        display,
        battery,
        memory,
        storage,
        uptime_seconds,
        uptime_label: uptime_seconds
            .map(format_duration)
            .unwrap_or_else(|| "—".to_string()),
        load,
        ip,
    }
}

fn device_title(manufacturer: &str, model: &str, serial: &str) -> String {
    let manufacturer = manufacturer.trim();
    let model = model.trim();
    if manufacturer.is_empty() || manufacturer.eq_ignore_ascii_case("unknown") {
        if model.is_empty() {
            serial.to_string()
        } else {
            model.to_string()
        }
    } else if model.is_empty() {
        manufacturer.to_string()
    } else if model
        .to_ascii_lowercase()
        .starts_with(&manufacturer.to_ascii_lowercase())
    {
        model.to_string()
    } else {
        format!("{manufacturer} {model}")
    }
}

fn split_markers(raw: &str) -> std::collections::HashMap<String, String> {
    let mut sections = std::collections::HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(marker) = line.trim().strip_prefix("@@") {
            if let Some(name) = current.take() {
                sections.insert(name, buffer.trim().to_string());
            }
            buffer.clear();
            current = Some(marker.trim().to_string());
        } else if current.is_some() {
            if !buffer.is_empty() {
                buffer.push('\n');
            }
            buffer.push_str(line);
        }
    }
    if let Some(name) = current.take() {
        sections.insert(name, buffer.trim().to_string());
    }
    sections
}

fn parse_display(size_raw: &str, density_raw: &str, rotation_raw: &str) -> DisplayInfo {
    let mut physical_size = String::new();
    let mut override_size = None;
    for line in size_raw.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Physical size:") {
            physical_size = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Override size:") {
            override_size = Some(value.trim().to_string());
        }
    }

    let mut physical_density = String::new();
    let mut override_density = None;
    for line in density_raw.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("Physical density:") {
            physical_density = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Override density:") {
            override_density = Some(value.trim().to_string());
        }
    }

    let rotation = rotation_raw
        .trim()
        .parse::<i32>()
        .ok()
        .map(|value| value.rem_euclid(4) as u8)
        .unwrap_or(0);

    DisplayInfo {
        physical_size,
        override_size,
        physical_density,
        override_density,
        rotation,
        rotation_label: rotation_label(rotation),
    }
}

fn rotation_label(rotation: u8) -> &'static str {
    match rotation {
        1 => "90°",
        2 => "180°",
        3 => "270°",
        _ => "0°",
    }
}

fn parse_battery(raw: &str) -> BatteryInfo {
    let mut info = BatteryInfo::default();
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "level" => info.level = value.parse::<u8>().ok().map(|level| level.min(100)),
            "status" => info.status = value.parse::<u8>().ok(),
            "temperature" => {
                info.temperature = value.parse::<i32>().ok().map(|value| value as f32 / 10.0);
            }
            "voltage" => info.voltage = value.parse::<u32>().ok(),
            "AC powered" | "USB powered" | "Wireless powered" | "Dock powered" => {
                info.powered |= value.eq_ignore_ascii_case("true");
            }
            _ => {}
        }
    }
    info.status_label = match info.status {
        Some(2) => "充电中",
        Some(3) => "放电中",
        Some(4) => "未充电",
        Some(5) => "已充满",
        _ if info.powered => "外接电源",
        _ => "电池供电",
    }
    .to_string();
    info
}

fn parse_memory(raw: &str) -> MemoryInfo {
    let mut total = None;
    let mut available = None;
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = value
            .split_whitespace()
            .next()
            .and_then(|number| number.parse::<u64>().ok());
        match key.trim() {
            "MemTotal" => total = number,
            "MemAvailable" => available = number,
            _ => {}
        }
    }
    let percent = match (total, available) {
        (Some(total), Some(available)) if total > 0 => {
            ((total.saturating_sub(available)) as f64 / total as f64 * 100.0).round() as u8
        }
        _ => 0,
    };
    MemoryInfo {
        total_kb: total,
        available_kb: available,
        used_label: match (total, available) {
            (Some(total), Some(available)) => format_kilobytes(total.saturating_sub(available)),
            _ => "—".to_string(),
        },
        total_label: total
            .map(format_kilobytes)
            .unwrap_or_else(|| "—".to_string()),
        percent: percent.min(100),
    }
}

fn parse_storage(raw: &str) -> StorageInfo {
    let line = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("Filesystem"))
        .find(|line| line.contains("/data"))
        .or_else(|| {
            raw.lines()
                .map(str::trim)
                .rfind(|line| !line.is_empty() && !line.starts_with("Filesystem"))
        });
    let Some(line) = line else {
        return StorageInfo::default();
    };
    let fields: Vec<&str> = line.split_whitespace().collect();
    let Some(percent_index) = fields.iter().position(|field| field.ends_with('%')) else {
        return StorageInfo::default();
    };
    let percent = fields[percent_index]
        .trim_end_matches('%')
        .parse::<u8>()
        .unwrap_or(0);
    if percent_index < 4 {
        return StorageInfo::default();
    }
    StorageInfo {
        size: fields[percent_index - 3].to_string(),
        used: fields[percent_index - 2].to_string(),
        available: fields[percent_index - 1].to_string(),
        percent: percent.min(100),
    }
}

fn parse_uptime(raw: &str) -> Option<u64> {
    raw.split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
}

fn parse_ip(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if let Some(index) = fields.iter().position(|field| *field == "src")
            && let Some(ip) = fields.get(index + 1)
        {
            return Some((*ip).to_string());
        }
    }
    None
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub size_label: String,
    pub mtime: u64,
    pub permissions: String,
}

impl From<RemoteEntry> for FileEntry {
    fn from(entry: RemoteEntry) -> Self {
        Self {
            name: entry.name,
            is_dir: entry.is_dir,
            is_link: entry.is_link,
            size: entry.size,
            size_label: if entry.is_dir {
                "—".to_string()
            } else {
                format_bytes(entry.size)
            },
            mtime: entry.mtime,
            permissions: format_mode(entry.mode),
        }
    }
}

fn format_mode(mode: u32) -> String {
    let kind = match mode & 0o170_000 {
        0o040_000 => 'd',
        0o120_000 => 'l',
        0o060_000 => 'b',
        0o020_000 => 'c',
        0o010_000 => 'p',
        0o140_000 => 's',
        _ => '-',
    };
    let mut text = String::with_capacity(10);
    text.push(kind);
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        text.push(if bits & 0o4 != 0 { 'r' } else { '-' });
        text.push(if bits & 0o2 != 0 { 'w' } else { '-' });
        text.push(if bits & 0o1 != 0 { 'x' } else { '-' });
    }
    text
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageEntry {
    pub name: String,
    pub version_code: Option<u64>,
    pub system: bool,
}

/// `pm list packages --show-versioncode` 的一行。
fn parse_package_line(line: &str) -> Option<(&str, Option<u64>)> {
    let rest = line.trim().strip_prefix("package:")?;
    let (name, code) = match rest.rsplit_once(" versionCode:") {
        Some((name, code)) => (name, code.trim().parse().ok()),
        None => (rest, None),
    };
    let name = name.trim();
    (!name.is_empty()).then_some((name, code))
}

pub fn merge_packages(all: &str, third_party: &str) -> Vec<PackageEntry> {
    let third_party_names: std::collections::HashSet<String> =
        parse_package_list(third_party, true)
            .into_iter()
            .map(|package| package.name)
            .collect();
    let mut packages = parse_package_list(all, false);
    for package in &mut packages {
        package.system = !third_party_names.contains(&package.name);
    }
    packages
}

fn parse_package_list(raw: &str, system: bool) -> Vec<PackageEntry> {
    let mut packages: Vec<PackageEntry> = raw
        .lines()
        .filter_map(parse_package_line)
        .map(|(name, version_code)| PackageEntry {
            name: name.to_string(),
            version_code,
            system,
        })
        .collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages.dedup_by(|left, right| left.name == right.name);
    packages
}

/// `pm path` 返回的第一条 APK 路径。
pub fn parse_package_path(raw: &str) -> Option<String> {
    raw.lines()
        .find_map(|line| line.trim().strip_prefix("package:"))
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_kilobytes(kilobytes: u64) -> String {
    format_bytes(kilobytes.saturating_mul(1024))
}

pub fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_parses_markers_and_values() {
        let raw = r#"
@@MODEL
Pixel Watch
@@MANUFACTURER
Google
@@ANDROID
13
@@SDK
33
@@BUILD
TWD9.240505.001
@@CHARACTERISTICS
watch
@@PATCH
2024-05-05
@@ABI
arm64-v8a
@@SIZE
Physical size: 450x450
Override size: 400x400
@@DENSITY
Physical density: 320
@@BATTERY
level: 87
status: 2
temperature: 305
voltage: 4021
AC powered: true
@@MEM
MemTotal:        1024000 kB
MemAvailable:     512000 kB
@@DF
Filesystem      Size  Used Avail Use% Mounted on
/dev/block/dm-4 4.0G  1.5G  2.5G  38% /data
@@UPTIME
12345.67 999.0
@@LOAD
0.10 0.20 0.30 1/100 1234
@@ROTATION
1
@@IP
default via 192.168.1.1 dev wlan0 proto dhcp src 192.168.1.20 metric 300
@@END
"#;
        let snapshot = parse_snapshot("ABC123", raw);
        assert_eq!(snapshot.title, "Google Pixel Watch");
        assert_eq!(snapshot.android, "13");
        assert_eq!(snapshot.display.override_size.as_deref(), Some("400x400"));
        assert_eq!(snapshot.display.rotation_label, "90°");
        assert_eq!(snapshot.battery.level, Some(87));
        assert_eq!(snapshot.battery.temperature, Some(30.5));
        assert!(snapshot.battery.powered);
        assert_eq!(snapshot.memory.total_kb, Some(1_024_000));
        assert_eq!(snapshot.memory.percent, 50);
        assert_eq!(snapshot.storage.size, "4.0G");
        assert_eq!(snapshot.storage.percent, 38);
        assert_eq!(snapshot.uptime_seconds, Some(12_345));
        assert_eq!(snapshot.ip.as_deref(), Some("192.168.1.20"));
        assert!(snapshot.wearable);
    }

    #[test]
    fn packages_split_user_and_system() {
        let all = "package:com.example.app versionCode:12\npackage:android versionCode:33\n";
        let third = "package:com.example.app\n";
        let packages = merge_packages(all, third);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "android");
        assert!(packages[0].system);
        assert_eq!(packages[1].name, "com.example.app");
        assert!(!packages[1].system);
        assert_eq!(packages[1].version_code, Some(12));
    }

    #[test]
    fn mode_formats_like_ls() {
        assert_eq!(format_mode(0o040_755), "drwxr-xr-x");
        assert_eq!(format_mode(0o100_644), "-rw-r--r--");
        assert_eq!(format_mode(0o120_777), "lrwxrwxrwx");
    }

    #[test]
    fn package_path_uses_first_line() {
        assert_eq!(
            parse_package_path("package:/data/app/~~x/base.apk\npackage:/data/app/~~x/split.apk"),
            Some("/data/app/~~x/base.apk".to_string())
        );
    }
}

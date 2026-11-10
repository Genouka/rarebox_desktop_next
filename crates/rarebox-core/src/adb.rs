//! ADB 命令层与设备数据解析。
//!
//! 这一层不依赖 GPUI，所有函数都可以在后台线程中安全执行，也便于单元测试。

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;

use crate::apk::{self, ApkMetadata};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 一条 ADB 命令的文本结果。
#[derive(Clone, Debug, Default)]
pub struct AdbOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl AdbOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
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
            Some(code) => format!("ADB 命令执行失败（退出码 {code}）"),
            None => "ADB 命令执行失败".to_string(),
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

/// ADC 与设备的连接方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceConnection {
    Usb,
    Wireless,
    Emulator,
    Unknown,
}

impl DeviceConnection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Usb => "USB",
            Self::Wireless => "无线",
            Self::Emulator => "模拟器",
            Self::Unknown => "未知",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Usb => "usb",
            Self::Wireless => "wifi",
            Self::Emulator => "smartphone",
            Self::Unknown => "cable",
        }
    }
}

/// `adb devices -l` 中的一台设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: String,
    pub model: Option<String>,
    pub product: Option<String>,
    pub device: Option<String>,
    pub transport_id: Option<String>,
    pub connection: DeviceConnection,
}

impl Device {
    pub fn is_online(&self) -> bool {
        self.state == "device"
    }

    #[cfg(test)]
    pub fn is_wearable(&self) -> bool {
        let haystack = format!(
            "{} {} {}",
            self.model.as_deref().unwrap_or_default(),
            self.product.as_deref().unwrap_or_default(),
            self.device.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        haystack.contains("watch") || haystack.contains("wear") || haystack.contains("eureka")
    }

    pub fn display_name(&self) -> String {
        self.model
            .as_deref()
            .map(|model| model.replace('_', " "))
            .filter(|model| !model.trim().is_empty())
            .unwrap_or_else(|| self.serial.clone())
    }

    pub fn state_label(&self) -> &'static str {
        match self.state.as_str() {
            "device" => "已连接",
            "offline" => "离线",
            "unauthorized" => "未授权",
            "no permissions" => "无权限",
            "connecting" => "连接中",
            "bootloader" => "Bootloader",
            "recovery" => "Recovery",
            "sideload" => "Sideload",
            "rescue" => "Rescue",
            _ => "未知状态",
        }
    }
}

/// 屏幕尺寸与密度信息。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayInfo {
    pub physical_size: String,
    pub override_size: Option<String>,
    pub physical_density: String,
    pub override_density: Option<String>,
    pub rotation: u8,
}

impl DisplayInfo {
    pub fn size(&self) -> &str {
        self.override_size
            .as_deref()
            .unwrap_or(self.physical_size.as_str())
    }

    pub fn density(&self) -> &str {
        self.override_density
            .as_deref()
            .unwrap_or(self.physical_density.as_str())
    }
}

/// 存储使用情况。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageInfo {
    pub size: String,
    pub used: String,
    pub available: String,
    pub percent: u8,
}

/// 一次设备概览刷新得到的完整快照。
#[derive(Clone, Debug, Default)]
pub struct DeviceSnapshot {
    pub serial: String,
    pub model: String,
    pub manufacturer: String,
    pub android: String,
    pub sdk: String,
    pub build_id: String,
    pub characteristics: String,
    pub patch: String,
    pub abi: String,
    pub display: DisplayInfo,
    pub battery_level: Option<u8>,
    pub battery_status: Option<u8>,
    pub battery_temperature: Option<f32>,
    pub battery_voltage: Option<u32>,
    pub battery_powered: bool,
    pub memory_total_kb: Option<u64>,
    pub memory_available_kb: Option<u64>,
    pub storage: StorageInfo,
    pub uptime_seconds: Option<u64>,
    pub load: Option<String>,
    pub ip: Option<String>,
}

impl DeviceSnapshot {
    pub fn is_wearable(&self) -> bool {
        let characteristics = self.characteristics.to_ascii_lowercase();
        characteristics.contains("watch") || self.model.to_ascii_lowercase().contains("watch")
    }

    pub fn title(&self) -> String {
        let manufacturer = self.manufacturer.trim();
        let model = self.model.trim();
        if manufacturer.is_empty() || manufacturer.eq_ignore_ascii_case("unknown") {
            if model.is_empty() {
                self.serial.clone()
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

    pub fn battery_percent(&self) -> Option<f32> {
        self.battery_level.map(|level| f32::from(level) / 100.0)
    }

    pub fn memory_percent(&self) -> Option<f32> {
        match (self.memory_total_kb, self.memory_available_kb) {
            (Some(total), Some(available)) if total > 0 => {
                let used = total.saturating_sub(available);
                Some(used as f32 / total as f32)
            }
            _ => None,
        }
    }

    pub fn memory_used_label(&self) -> String {
        match (self.memory_total_kb, self.memory_available_kb) {
            (Some(total), Some(available)) => {
                let used = total.saturating_sub(available);
                format!("{} / {}", format_kilobytes(used), format_kilobytes(total))
            }
            _ => "—".to_string(),
        }
    }

    pub fn battery_status_label(&self) -> &'static str {
        match self.battery_status {
            Some(2) => "充电中",
            Some(3) => "放电中",
            Some(4) => "未充电",
            Some(5) => "已充满",
            _ => {
                if self.battery_powered {
                    "外接电源"
                } else {
                    "未知"
                }
            }
        }
    }
}

/// 设备的重启目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RebootTarget {
    System,
    Recovery,
    Bootloader,
    Fastbootd,
    Edl,
}

impl RebootTarget {
    pub const ALL: [RebootTarget; 5] = [
        RebootTarget::System,
        RebootTarget::Recovery,
        RebootTarget::Bootloader,
        RebootTarget::Fastbootd,
        RebootTarget::Edl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "重启",
            Self::Recovery => "Recovery",
            Self::Bootloader => "Bootloader",
            Self::Fastbootd => "Fastbootd",
            Self::Edl => "EDL",
        }
    }

    /// 菜单与提示里使用的完整名称。
    pub fn menu_label(self) -> &'static str {
        match self {
            Self::System => "重启",
            Self::Recovery => "重启到 Recovery",
            Self::Bootloader => "重启到 Bootloader",
            Self::Fastbootd => "重启到 Fastbootd",
            Self::Edl => "重启到 EDL",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::System => "restart_alt",
            Self::Recovery => "build",
            Self::Bootloader => "memory",
            Self::Fastbootd => "speed",
            Self::Edl => "bolt",
        }
    }

    /// `adb reboot` 之后的参数；空表示普通重启。
    pub fn reboot_args(self) -> &'static [&'static str] {
        match self {
            Self::System => &[],
            Self::Recovery => &["recovery"],
            Self::Bootloader => &["bootloader"],
            Self::Fastbootd => &["fastboot"],
            Self::Edl => &["edl"],
        }
    }

    /// 命令发出后的可预期结果，用于提示用户。
    pub fn outcome(self) -> &'static str {
        match self {
            Self::System => "设备会在几秒后断开并自动重新连接。",
            Self::Recovery => "ADB 会在恢复模式下重新可用，退出恢复模式即可正常开机。",
            Self::Bootloader => "此时 ADB 不可用，需要用 fastboot 或手动退出 Bootloader。",
            Self::Fastbootd => "此时 ADB 不可用，可以用 fastboot 命令刷写动态分区。",
            Self::Edl => "EDL 由厂商实现：不支持该模式的设备会直接普通重启，EDL 下 ADB 不可用。",
        }
    }
}
/// 设备上的一个输入法。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputMethod {
    /// `ime` 命令使用的完整 ID（组件扁平名，如 `com.foo/.Bar`）。
    pub id: String,
    /// `ime list -a` 输出的显示名，可能缺失。
    pub label: Option<String>,
    pub enabled: bool,
    /// 是否为当前默认输入法。
    pub active: bool,
}

/// AppOps 的执行模式；取值与 Android `appops` 命令一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppOpMode {
    Default,
    Allowed,
    Ignored,
    Denied,
    Foreground,
}

impl AppOpMode {
    pub const ALL: [AppOpMode; 5] = [
        AppOpMode::Default,
        AppOpMode::Allowed,
        AppOpMode::Ignored,
        AppOpMode::Denied,
        AppOpMode::Foreground,
    ];

    /// `appops set` 接受的模式名。
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Allowed => "allow",
            Self::Ignored => "ignore",
            Self::Denied => "deny",
            Self::Foreground => "foreground",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::Allowed => "允许",
            Self::Ignored => "忽略",
            Self::Denied => "拒绝",
            Self::Foreground => "仅前台",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        AppOpMode::ALL.into_iter().find(|mode| mode.as_arg() == raw)
    }
}

/// 一个 AppOps 操作（op）的当前状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppOp {
    pub name: String,
    pub mode: AppOpMode,
    /// 输出里 `mode` 之后的原始字段（访问时间、时长等）。
    pub detail: Option<String>,
}

/// 解析 `ime list [-a]`：每行 `<id>: <label>` 或仅 `<id>`。
pub fn parse_ime_list(output: &str) -> Vec<(String, Option<String>)> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            // ID 里不会出现冒号（组件扁平名只有字母数字与 . $ /）。
            let (id, label) = match line.split_once(':') {
                Some((id, label)) => (id.trim(), Some(label.trim().to_string())),
                None => (line, None),
            };
            let id = id.to_string();
            if id.contains(' ') || !id.contains('.') {
                // 形如「1 input method(s)」之类的噪声行直接跳过。
                return None;
            }
            let label = label.filter(|label| !label.is_empty());
            Some((id, label))
        })
        .collect()
}

/// 解析 `settings get secure default_input_method` 的输出。
pub fn parse_default_ime(output: &str) -> Option<String> {
    let value = output.trim();
    if value.is_empty() || value == "null" {
        None
    } else {
        Some(value.to_string())
    }
}

/// 解析 `appops get <包名>`：每行 `<OP>: <mode>; <字段>=<值>...`。
pub fn parse_app_ops(output: &str) -> Vec<AppOp> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let name = name.trim();
            // AppOps 名称全大写下划线；过滤掉提示文本等噪声行。
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
            {
                return None;
            }
            let rest = rest.trim();
            let (mode_raw, detail) = match rest.split_once(';') {
                Some((mode, detail)) => (mode, Some(detail.trim().to_string())),
                None => (rest, None),
            };
            let mode = AppOpMode::parse(mode_raw)?;
            Some(AppOp {
                name: name.to_string(),
                mode,
                detail: detail.filter(|detail| !detail.is_empty()),
            })
        })
        .collect()
}

/// 包名 / op 名的合法性校验：只允许构造安全 shell 命令所需的字符。
fn validate_package(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '$'))
    {
        bail!("包名不合法：{value}");
    }
    Ok(())
}

fn validate_op(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    {
        bail!("操作名不合法：{value}");
    }
    Ok(())
}

/// 把任意文本安全地放进设备端 shell 的单引号参数中。
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// 执行前给子进程准备 Android 端环境；桌面端什么都不做。
///
/// Android 上随包的 `libadb.so` 依赖同目录的 Termux 库（`LD_LIBRARY_PATH` 需要指向
/// `nativeLibraryDir`），并且 adb 会在 `$HOME/.android` 下创建目录与密钥，
/// 应用进程默认没有 `HOME`，不设置会直接 abort（`Cannot mkdir '//.android'`）。
#[cfg(target_os = "android")]
pub(crate) fn prepare_command(command: &mut Command) {
    use std::sync::LazyLock;

    if let Some(dir) = crate::platform::native_lib_dir() {
        let mut paths = vec![dir];
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            command.env("LD_LIBRARY_PATH", joined);
        }
    }

    static HOME: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
        let home = crate::config::data_dir().join("android-home");
        std::fs::create_dir_all(&home).ok()?;
        Some(home)
    });
    if let Some(home) = HOME.as_ref() {
        command.env("HOME", home);
        command.env("TMPDIR", home);
    }
}

#[cfg(not(target_os = "android"))]
pub(crate) fn prepare_command(_command: &mut Command) {}

/// ADB 可执行文件封装。
#[derive(Clone, Debug)]
pub struct AdbClient {
    program: PathBuf,
}

impl AdbClient {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    /// 结束 ADB 服务端进程（`adb kill-server`）。
    ///
    /// 返回后服务端进程会退出；下一次执行 ADB 命令时会自动重新启动。
    pub fn kill_server(&self) -> Result<AdbOutput> {
        self.run(&["kill-server".to_string()])
    }

    pub fn version(&self) -> Result<String> {
        let output = self.run(&["version".to_string()])?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        let first_line = output
            .stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("ADB");
        Ok(first_line.trim().to_string())
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        let output = self.run(&["devices".to_string(), "-l".to_string()])?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(parse_devices(&output.stdout))
    }

    pub fn shell(&self, serial: &str, command: &str) -> Result<AdbOutput> {
        self.run(&[
            "-s".to_string(),
            serial.to_string(),
            "shell".to_string(),
            command.to_string(),
        ])
    }

    pub fn serial_command(&self, serial: &str, args: &[&str]) -> Result<AdbOutput> {
        let mut full_args = vec!["-s".to_string(), serial.to_string()];
        full_args.extend(args.iter().map(|arg| (*arg).to_string()));
        self.run(&full_args)
    }

    /// 读取二进制 stdout，用于截屏等场景。
    pub fn exec_out_bytes(&self, serial: &str, args: &[&str]) -> Result<Vec<u8>> {
        let mut command = Command::new(&self.program);
        prepare_command(&mut command);
        command
            .arg("-s")
            .arg(serial)
            .arg("exec-out")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);

        let output = command
            .output()
            .with_context(|| format!("无法执行 ADB：{}", self.program.display()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("ADB 截屏失败：{}", stderr.trim());
        }
        if output.stdout.is_empty() {
            bail!("ADB 没有返回图像数据");
        }
        Ok(output.stdout)
    }

    /// 读取全部已安装应用的 APK 元数据（应用名称、版本名、版本号）。
    ///
    /// 设备端用 `unzip -p` 取出每个 APK 的 `AndroidManifest.xml` 与 `resources.arsc`，
    /// 按 `<包名> <manifest 长度> <arsc 长度>` 加裸数据的方式连续输出并整体 gzip，
    /// 本地边解压边解析，避免把上百 MB 的原始二进制留在内存里。
    pub fn package_metadata(&self, serial: &str) -> Result<HashMap<String, ApkMetadata>> {
        let mut command = Command::new(&self.program);
        prepare_command(&mut command);
        command
            .arg("-s")
            .arg(serial)
            .arg("exec-out")
            .arg("sh")
            .arg("-c")
            .arg(PACKAGE_METADATA_SCRIPT)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command
            .spawn()
            .with_context(|| format!("无法执行 ADB：{}", self.program.display()))?;
        let stdout = child.stdout.take().context("无法读取 ADB 输出")?;
        let stderr = child.stderr.take();
        let stderr_reader = std::thread::spawn(move || {
            let mut buffer = String::new();
            if let Some(mut stderr) = stderr {
                let _ = stderr.read_to_string(&mut buffer);
            }
            buffer
        });

        let mut decoder = GzDecoder::new(stdout);
        let parsed = read_metadata_frames(&mut decoder);
        child.wait().context("等待 ADB 结束失败")?;
        let stderr = stderr_reader.join().unwrap_or_default();
        let stderr = stderr.trim();

        match parsed {
            // 部分 APK 读不到是正常的（例如 split APK 的 base 缺失），只要有结果就算成功。
            Ok(metadata) if !metadata.is_empty() => Ok(metadata),
            Ok(_) if stderr.is_empty() => bail!("设备没有返回应用信息"),
            Ok(_) => bail!("{stderr}"),
            Err(_) if !stderr.is_empty() => bail!("{stderr}"),
            Err(err) => Err(err),
        }
    }

    /// 让设备重启到指定目标。
    ///
    /// `adb reboot` 成功时通常直接返回，随后设备断开；部分设备会把拒绝原因写在 stderr 上，
    /// 所以这里把非空输出一并返回，交给调用方决定是否提示。
    pub fn reboot(&self, serial: &str, target: RebootTarget) -> Result<AdbOutput> {
        let mut args = vec!["reboot"];
        args.extend(target.reboot_args().iter().copied());
        self.serial_command(serial, &args)
    }

    /// 直接执行 adb 子命令，不绑定设备。
    pub fn run(&self, args: &[String]) -> Result<AdbOutput> {
        let mut command = Command::new(&self.program);
        prepare_command(&mut command);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW);

        let output = command
            .output()
            .with_context(|| format!("无法执行 ADB：{}", self.program.display()))?;
        Ok(AdbOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    pub fn snapshot(&self, serial: &str) -> Result<DeviceSnapshot> {
        let output = self.shell(serial, SNAPSHOT_SCRIPT)?;
        if !output.success() && output.stdout.trim().is_empty() {
            bail!("{}", output.error_message());
        }
        Ok(parse_snapshot(serial, &output.stdout))
    }

    /// 读取设备上的输入法（含未启用的），并标记当前默认输入法。
    pub fn list_input_methods(&self, serial: &str) -> Result<Vec<InputMethod>> {
        let all = self.shell(serial, "ime list -a")?;
        if !all.success() {
            bail!("{}", all.error_message());
        }
        let enabled = self.shell(serial, "ime list -s")?;
        if !enabled.success() {
            bail!("{}", enabled.error_message());
        }
        let current = self.shell(serial, "settings get secure default_input_method")?;
        if !current.success() {
            bail!("{}", current.error_message());
        }

        let enabled_ids: HashSet<String> = parse_ime_list(&enabled.stdout)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let active = parse_default_ime(&current.stdout);
        Ok(parse_ime_list(&all.stdout)
            .into_iter()
            .map(|(id, label)| InputMethod {
                enabled: enabled_ids.contains(&id),
                active: active.as_deref() == Some(id.as_str()),
                id,
                label,
            })
            .collect())
    }

    /// 启用或停用一个输入法。停用当前默认输入法时设备可能拒绝，错误原样返回。
    pub fn set_input_method_enabled(&self, serial: &str, id: &str, enabled: bool) -> Result<()> {
        let verb = if enabled { "enable" } else { "disable" };
        let output = self.shell(serial, &format!("ime {verb} {}", shell_quote(id)))?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(())
    }

    /// 把一个输入法设为当前默认（必要时先启用它）。
    pub fn activate_input_method(&self, serial: &str, id: &str) -> Result<()> {
        self.set_input_method_enabled(serial, id, true)?;
        let output = self.shell(serial, &format!("ime set {}", shell_quote(id)))?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(())
    }

    /// 读取一个应用当前被显式调整过的 AppOps（默认态的 op 不会出现在输出里）。
    pub fn app_ops(&self, serial: &str, package: &str) -> Result<Vec<AppOp>> {
        validate_package(package)?;
        let output = self.shell(serial, &format!("appops get {package}"))?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(parse_app_ops(&output.stdout))
    }

    /// 设置一个 AppOps 的模式。
    pub fn set_app_op(&self, serial: &str, package: &str, op: &str, mode: AppOpMode) -> Result<()> {
        validate_package(package)?;
        validate_op(op)?;
        let output = self.shell(
            serial,
            &format!("appops set {package} {op} {}", mode.as_arg()),
        )?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(())
    }

    /// 重置 AppOps：`op` 为空时重置该包的全部操作。
    pub fn reset_app_ops(&self, serial: &str, package: &str, op: Option<&str>) -> Result<()> {
        validate_package(package)?;
        if let Some(op) = op {
            validate_op(op)?;
        }
        let command = match op {
            Some(op) => format!("appops reset {package} {op}"),
            None => format!("appops reset {package}"),
        };
        let output = self.shell(serial, &command)?;
        if !output.success() {
            bail!("{}", output.error_message());
        }
        Ok(())
    }
}

/// 一次 shell 往返抓取设备概览所需的信息。
const SNAPSHOT_SCRIPT: &str = r#"
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

/// 在设备上逐个读取 APK 的 manifest 与资源表。
///
/// 每行 `<包名> <manifest 字节数> <arsc 字节数>` 之后紧跟两段裸数据；
/// 长度前缀让本地可以按字节精确切分，不依赖二进制里不会出现的分隔符。
/// 整段输出在设备上 gzip，再把占多数体积的 `resources.arsc` 压到约四分之一。
const PACKAGE_METADATA_SCRIPT: &str = r#"
tmp=/data/local/tmp/rarebox_meta.$$
{
  pm list packages -f --show-versioncode | sed "s/^package://" | while IFS= read -r line; do
    apk=${line%=*}
    rest=${line##*=}
    name=${rest%% *}
    [ -n "$name" ] || continue
    unzip -p "$apk" AndroidManifest.xml > "$tmp.m" 2>/dev/null || : > "$tmp.m"
    unzip -p "$apk" resources.arsc > "$tmp.a" 2>/dev/null || : > "$tmp.a"
    printf "@@%s %s %s\n" "$name" "$(wc -c < "$tmp.m")" "$(wc -c < "$tmp.a")"
    cat "$tmp.m" "$tmp.a"
  done
  rm -f "$tmp.m" "$tmp.a"
} | gzip -6
"#;

/// 解析 `PACKAGE_METADATA_SCRIPT` 的输出帧。
fn read_metadata_frames(reader: &mut impl Read) -> Result<HashMap<String, ApkMetadata>> {
    let mut metadata = HashMap::new();
    let mut header = Vec::with_capacity(256);
    loop {
        header.clear();
        if !read_line(reader, &mut header)? {
            break;
        }
        let line = String::from_utf8_lossy(&header);
        let Some(rest) = line.trim_end().strip_prefix("@@") else {
            continue;
        };
        let mut fields = rest.split(' ');
        let (Some(name), Some(manifest_len), Some(resources_len)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(manifest_len), Ok(resources_len)) = (
            manifest_len.parse::<usize>(),
            resources_len.parse::<usize>(),
        ) else {
            continue;
        };
        // 长度异常时继续往下列，避免一个坏帧把整张表带偏。
        let mut manifest = vec![0u8; manifest_len];
        let mut resources = vec![0u8; resources_len];
        if !read_exact(reader, &mut manifest)? || !read_exact(reader, &mut resources)? {
            break;
        }
        let manifest = (manifest_len > 0).then_some(manifest.as_slice());
        let resources = (resources_len > 0).then_some(resources.as_slice());
        metadata.insert(name.to_string(), apk::metadata(manifest, resources));
    }
    Ok(metadata)
}

/// 读一行（含换行符），返回是否读到了内容。
fn read_line(reader: &mut impl Read, buffer: &mut Vec<u8>) -> Result<bool> {
    let mut byte = [0u8; 1];
    let mut read_any = false;
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(read_any),
            Ok(_) => {
                read_any = true;
                buffer.push(byte[0]);
                if byte[0] == b'\n' {
                    return Ok(true);
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        }
    }
}

/// 读满缓冲区；流提前结束时返回 `false`。
fn read_exact(reader: &mut impl Read, buffer: &mut [u8]) -> Result<bool> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => return Ok(false),
            Ok(read) => filled += read,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(true)
}

pub fn parse_devices(raw: &str) -> Vec<Device> {
    let mut devices = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("List of devices")
            || line.starts_with('*')
            || line.starts_with("adb server")
        {
            continue;
        }

        let mut fields = line.split_whitespace();
        let Some(serial) = fields.next() else {
            continue;
        };
        let Some(first_state) = fields.next() else {
            continue;
        };
        let mut state = first_state.to_string();
        if state == "no"
            && let Some(next) = fields.next()
        {
            state = format!("no {next}");
        }

        let mut properties: HashMap<&str, &str> = HashMap::new();
        for field in fields {
            if let Some((key, value)) = field.split_once(':') {
                properties.insert(key, value);
            }
        }

        let connection = if serial.starts_with("emulator-") {
            DeviceConnection::Emulator
        } else if serial.contains("_adb-tls-") || serial.contains(':') {
            DeviceConnection::Wireless
        } else if properties.contains_key("usb") {
            DeviceConnection::Usb
        } else {
            DeviceConnection::Unknown
        };

        devices.push(Device {
            serial: serial.to_string(),
            state,
            model: properties.get("model").map(|value| (*value).to_string()),
            product: properties.get("product").map(|value| (*value).to_string()),
            device: properties.get("device").map(|value| (*value).to_string()),
            transport_id: properties
                .get("transport_id")
                .map(|value| (*value).to_string()),
            connection,
        });
    }
    devices
}

pub fn parse_snapshot(serial: &str, raw: &str) -> DeviceSnapshot {
    let sections = split_markers(raw);
    let get = |name: &str| -> String {
        sections
            .get(name)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };

    let display = parse_display(&get("SIZE"), &get("DENSITY"), &get("ROTATION"));
    let battery = parse_battery(&get("BATTERY"));
    let (memory_total_kb, memory_available_kb) = parse_memory(&get("MEM"));
    let storage = parse_storage(&get("DF"));
    let uptime_seconds = parse_uptime(&get("UPTIME"));
    let load = get("LOAD")
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
        .into_option();
    let ip = parse_ip(&get("IP"));

    DeviceSnapshot {
        serial: serial.to_string(),
        model: get("MODEL"),
        manufacturer: get("MANUFACTURER"),
        android: get("ANDROID"),
        sdk: get("SDK"),
        build_id: get("BUILD"),
        characteristics: get("CHARACTERISTICS"),
        patch: get("PATCH"),
        abi: get("ABI"),
        display,
        battery_level: battery.level,
        battery_status: battery.status,
        battery_temperature: battery.temperature,
        battery_voltage: battery.voltage,
        battery_powered: battery.powered,
        memory_total_kb,
        memory_available_kb,
        storage,
        uptime_seconds,
        load,
        ip,
    }
}

fn split_markers(raw: &str) -> HashMap<String, String> {
    let mut result: HashMap<String, String> = HashMap::new();
    let mut current: Option<String> = None;
    let mut buffer = String::new();

    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(marker) = line.trim().strip_prefix("@@") {
            if let Some(name) = current.take() {
                result.insert(name, buffer.trim().to_string());
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
        result.insert(name, buffer.trim().to_string());
    }
    result
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
    }
}

#[derive(Default)]
struct BatteryInfo {
    level: Option<u8>,
    status: Option<u8>,
    temperature: Option<f32>,
    voltage: Option<u32>,
    powered: bool,
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
    info
}

fn parse_memory(raw: &str) -> (Option<u64>, Option<u64>) {
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
    (total, available)
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
    let percent_index = fields.iter().position(|field| field.ends_with('%'));
    let Some(percent_index) = percent_index else {
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

/// 日志级别。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogLevel {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
    #[default]
    Unknown,
}

impl LogLevel {
    pub fn from_char(value: char) -> Self {
        match value {
            'V' => Self::Verbose,
            'D' => Self::Debug,
            'I' => Self::Info,
            'W' => Self::Warn,
            'E' => Self::Error,
            'F' | 'A' => Self::Fatal,
            _ => Self::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Verbose => "V",
            Self::Debug => "D",
            Self::Info => "I",
            Self::Warn => "W",
            Self::Error => "E",
            Self::Fatal => "F",
            Self::Unknown => "·",
        }
    }
}

/// 一行 Logcat。
#[derive(Clone, Debug)]
pub struct LogLine {
    pub raw: String,
    pub time: String,
    pub level: LogLevel,
    pub tag: String,
    pub message: String,
}

pub fn parse_logcat(raw: &str) -> Vec<LogLine> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() >= 6 && tokens[4].len() == 1 {
            let level = LogLevel::from_char(tokens[4].chars().next().unwrap_or('?'));
            if level != LogLevel::Unknown {
                lines.push(LogLine {
                    raw: line.to_string(),
                    time: tokens[1].to_string(),
                    level,
                    tag: tokens[5].trim_end_matches(':').to_string(),
                    message: tokens
                        .get(6..)
                        .map(|rest| rest.join(" "))
                        .unwrap_or_default(),
                });
                continue;
            }
        }
        lines.push(LogLine {
            raw: line.to_string(),
            time: String::new(),
            level: LogLevel::Unknown,
            tag: String::new(),
            message: line.to_string(),
        });
    }
    lines
}

/// 一个已安装应用。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppPackage {
    /// 包名，例如 `com.example.watch`。
    pub name: String,
    /// 应用名称，来自 APK 的 `application@label`；解析失败时与包名相同。
    pub label: String,
    /// `versionName`；设备未提供时为空。
    pub version_name: String,
    /// `versionCode`。
    pub version_code: Option<u64>,
    pub system: bool,
}

impl AppPackage {
    /// 列表项标题：优先显示应用名称。
    pub fn label_or_name(&self) -> &str {
        if self.label.is_empty() {
            &self.name
        } else {
            &self.label
        }
    }

    /// 是否成功解析到了应用名称。
    pub fn has_label(&self) -> bool {
        !self.label.is_empty() && self.label != self.name
    }

    /// 版本展示文本，例如 `3.5 (25)`；两者都没有时返回空串。
    pub fn version_label(&self) -> String {
        let code = self.version_code;
        match (self.version_name.is_empty(), code) {
            (false, Some(code)) => format!("{} ({code})", self.version_name),
            (false, None) => self.version_name.clone(),
            (true, Some(code)) => code.to_string(),
            (true, None) => String::new(),
        }
    }

    /// 用解析出的 APK 元数据覆盖名称与版本字段。
    fn apply_metadata(&mut self, metadata: &ApkMetadata) {
        if let Some(label) = metadata.label.as_deref() {
            self.label = label.to_string();
        }
        if let Some(version) = metadata.version_name.as_deref() {
            self.version_name = version.to_string();
        }
        if let Some(code) = metadata.version_code {
            self.version_code = Some(code);
        }
    }
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

pub fn parse_package_list(raw: &str, system: bool) -> Vec<AppPackage> {
    let mut packages: Vec<AppPackage> = raw
        .lines()
        .filter_map(parse_package_line)
        .map(|(name, version_code)| AppPackage {
            name: name.to_string(),
            version_code,
            system,
            ..AppPackage::default()
        })
        .collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    packages.dedup_by(|left, right| left.name == right.name);
    packages
}

pub fn merge_packages(all: &str, third_party: &str) -> Vec<AppPackage> {
    let third_party_names: HashSet<String> = parse_package_list(third_party, true)
        .into_iter()
        .map(|package| package.name)
        .collect();
    let mut packages = parse_package_list(all, false);
    for package in &mut packages {
        package.system = !third_party_names.contains(&package.name);
    }
    packages
}

/// 补全应用名称与版本信息。
///
/// 元数据可能只覆盖部分包，缺失字段保持原值。
pub fn apply_package_metadata(
    packages: &mut [AppPackage],
    metadata: &HashMap<String, ApkMetadata>,
) {
    for package in packages.iter_mut() {
        if let Some(meta) = metadata.get(&package.name) {
            package.apply_metadata(meta);
        }
    }
}

/// 文件浏览器中的条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub permissions: String,
    pub size: Option<u64>,
    pub size_label: String,
    pub modified: String,
}

pub fn parse_ls(raw: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() < 8 {
            continue;
        }
        let permissions = tokens[0];
        let kind = permissions.chars().next().unwrap_or('-');
        if !matches!(kind, 'd' | '-' | 'l' | 'c' | 'b' | 'p' | 's') {
            continue;
        }
        let is_dir = kind == 'd';
        let is_link = kind == 'l';
        let size = tokens[4].parse::<u64>().ok();
        let name = tokens[7..].join(" ");
        if name == "." || name == ".." {
            continue;
        }
        entries.push(FileEntry {
            name,
            is_dir,
            is_link,
            permissions: permissions.to_string(),
            size,
            size_label: size.map(format_bytes).unwrap_or_else(|| "—".to_string()),
            modified: format!("{} {}", tokens[5], tokens[6]),
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    entries
}

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

pub fn format_kilobytes(kilobytes: u64) -> String {
    format_bytes(kilobytes.saturating_mul(1024))
}

pub fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分钟")
    }
}

trait IntoOption {
    fn into_option(self) -> Option<String>;
}

impl IntoOption for String {
    fn into_option(self) -> Option<String> {
        if self.trim().is_empty() {
            None
        } else {
            Some(self)
        }
    }
}

/// 尝试在常见位置查找 ADB，返回候选列表（包含来自 PATH 的 `adb`）。
pub fn adb_candidates(configured: &str) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let configured = configured.trim();
    if !configured.is_empty() {
        candidates.push(PathBuf::from(configured));
    }
    if let Some(from_env) = std::env::var_os("RAREBOX_ADB") {
        candidates.push(PathBuf::from(from_env));
    }

    // Android 端优先用随 APK 分发的 adb：安装时解压到 `nativeLibraryDir`，
    // 文件名是 `libadb.so`（Android 10+ 只有这个目录里的文件可以执行）。
    // 没有时再退回 Termux / 系统自带 adb。
    #[cfg(target_os = "android")]
    {
        if let Some(dir) = crate::platform::native_lib_dir() {
            candidates.push(dir.join("libadb.so"));
        }
        for path in [
            "/data/data/com.termux/files/usr/bin/adb",
            "/system/bin/adb",
            "/product/bin/adb",
            "/vendor/bin/adb",
        ] {
            candidates.push(PathBuf::from(path));
        }
    }

    // 内置 ADB 优先于外部查找：若已随应用分发，就不需要用户单独安装 platform-tools。
    if let Some(embedded) = crate::adb_bundle::ensure_embedded_adb() {
        candidates.push(embedded);
    }

    let executable = if cfg!(target_os = "windows") {
        "adb.exe"
    } else {
        "adb"
    };

    if let Ok(current_exe) = std::env::current_exe()
        && let Some(parent) = current_exe.parent()
    {
        candidates.push(parent.join("platform-tools").join(executable));
        candidates.push(parent.join(executable));
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir.join("platform-tools").join(executable));
        candidates.push(current_dir.join(executable));
    }

    for variable in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(root).join("platform-tools").join(executable));
        }
    }
    if cfg!(target_os = "windows") {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(local)
                    .join("Android/Sdk/platform-tools")
                    .join(executable),
            );
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            candidates.push(
                PathBuf::from(profile)
                    .join("AppData/Local/Android/Sdk/platform-tools")
                    .join(executable),
            );
        }
        candidates.push(PathBuf::from("C:/platform-tools").join(executable));
    }

    candidates.push(PathBuf::from("adb"));
    candidates
}

/// 验证候选 ADB 是否可用，返回第一个可执行的路径与版本。
pub fn detect_adb(configured: &str) -> Option<(PathBuf, String)> {
    for candidate in adb_candidates(configured) {
        if candidate.components().count() > 1 && !candidate.exists() {
            continue;
        }
        let client = AdbClient::new(candidate.clone());
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
    fn reboot_targets_map_to_adb_arguments() {
        assert_eq!(RebootTarget::System.reboot_args(), &[] as &[&str]);
        assert_eq!(RebootTarget::Recovery.reboot_args(), &["recovery"]);
        assert_eq!(RebootTarget::Bootloader.reboot_args(), &["bootloader"]);
        assert_eq!(RebootTarget::Fastbootd.reboot_args(), &["fastboot"]);
        assert_eq!(RebootTarget::Edl.reboot_args(), &["edl"]);
        // 菜单必须覆盖全部目标，且每一项都有独立的图标。
        let icons: HashSet<&str> = RebootTarget::ALL.iter().map(|t| t.icon()).collect();
        assert_eq!(icons.len(), RebootTarget::ALL.len());
    }

    #[test]
    fn parses_ime_list_with_labels_and_noise() {
        let raw = "2 input method(s)\n\
                   com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME: Gboard\n\
                   com.android.btservices/.BluetoothKeyboard: 蓝牙键盘\n\
                   \n";
        let imes = parse_ime_list(raw);
        assert_eq!(
            imes,
            vec![
                (
                    "com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME"
                        .to_string(),
                    Some("Gboard".to_string())
                ),
                (
                    "com.android.btservices/.BluetoothKeyboard".to_string(),
                    Some("蓝牙键盘".to_string())
                ),
            ]
        );
        // 短格式只有 ID。
        let short = parse_ime_list("com.android.btservices/.BluetoothKeyboard\n");
        assert_eq!(short[0].1, None);
    }

    #[test]
    fn parses_default_ime_setting() {
        assert_eq!(
            parse_default_ime("com.foo/.Bar\n"),
            Some("com.foo/.Bar".to_string())
        );
        assert_eq!(parse_default_ime("null"), None);
        assert_eq!(parse_default_ime(""), None);
    }

    #[test]
    fn parses_app_ops_output() {
        let raw = "COARSE_LOCATION: foreground; time=+2m3s ago; duration=+123ms\n\
                   READ_CLIPBOARD: allow\n\
                   \n\
                   No operations.\n\
                   Unexpected text without colon\n";
        let ops = parse_app_ops(raw);
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].name, "COARSE_LOCATION");
        assert_eq!(ops[0].mode, AppOpMode::Foreground);
        assert_eq!(
            ops[0].detail.as_deref(),
            Some("time=+2m3s ago; duration=+123ms")
        );
        assert_eq!(ops[1].name, "READ_CLIPBOARD");
        assert_eq!(ops[1].mode, AppOpMode::Allowed);
        assert_eq!(ops[1].detail, None);
    }

    #[test]
    fn appop_mode_names_match_android_args() {
        let args: HashSet<&str> = AppOpMode::ALL.iter().map(|mode| mode.as_arg()).collect();
        assert_eq!(args.len(), 5);
        assert_eq!(AppOpMode::Allowed.as_arg(), "allow");
        assert_eq!(AppOpMode::Denied.as_arg(), "deny");
        assert_eq!(AppOpMode::Ignored.as_arg(), "ignore");
        assert_eq!(AppOpMode::Foreground.as_arg(), "foreground");
        assert_eq!(AppOpMode::Default.as_arg(), "default");
        assert_eq!(AppOpMode::parse("allow"), Some(AppOpMode::Allowed));
    }

    #[test]
    fn validates_package_and_op_names() {
        assert!(validate_package("com.example.watch").is_ok());
        assert!(validate_package("").is_err());
        assert!(validate_package("com.foo; rm -rf /").is_err());
        assert!(validate_op("READ_CLIPBOARD").is_ok());
        assert!(validate_op("coarse_location").is_err());
        assert!(validate_op("").is_err());
    }

    #[test]
    fn parses_device_list() {
        let raw = "List of devices attached\n\
                   R5CT30ABCDE            device product:eureka model:Galaxy_Watch5_44mm device:eureka transport_id:1 usb:1-3\n\
                   192.168.1.22:5555      device product:eureka model:Galaxy_Watch5 device:eureka transport_id:2\n\
                   emulator-5554          offline\n\
                   ABC123                 unauthorized usb:2-1\n";
        let devices = parse_devices(raw);
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].display_name(), "Galaxy Watch5 44mm");
        assert_eq!(devices[0].connection, DeviceConnection::Usb);
        assert!(devices[0].is_wearable());
        assert_eq!(devices[1].connection, DeviceConnection::Wireless);
        assert!(!devices[2].is_online());
        assert_eq!(devices[3].state, "unauthorized");
    }

    #[test]
    fn parses_snapshot_sections() {
        let raw = "@@MODEL\nGalaxy Watch5\n@@MANUFACTURER\nsamsung\n@@ANDROID\n13\n@@SDK\n33\n\
                   @@BUILD\nTP1A.220624.014\n@@CHARACTERISTICS\nwatch\n@@PATCH\n2023-08-01\n@@ABI\narm64-v8a\n\
                   @@SIZE\nPhysical size: 450x450\nOverride size: 384x384\n\
                   @@DENSITY\nPhysical density: 320\nOverride density: 300\n\
                   @@BATTERY\n  AC powered: false\n  USB powered: true\n  status: 2\n  level: 87\n  voltage: 4012\n  temperature: 305\n\
                   @@MEM\nMemTotal:        1900000 kB\nMemAvailable:     700000 kB\n\
                   @@DF\nFilesystem      Size  Used Avail Use% Mounted on\n/dev/block/dm-5  1.9G  1.2G  700M  63% /data\n\
                   @@UPTIME\n12345.67 890.12\n@@LOAD\n0.45 0.32 0.20 1/345 12345\n@@ROTATION\n1\n@@IP\ndefault via 10.0.0.1 dev wlan0 src 10.0.0.23\n@@END\n";
        let snapshot = parse_snapshot("R5CT30ABCDE", raw);
        assert_eq!(snapshot.title(), "samsung Galaxy Watch5");
        assert!(snapshot.is_wearable());
        assert_eq!(snapshot.display.size(), "384x384");
        assert_eq!(snapshot.display.density(), "300");
        assert_eq!(snapshot.display.rotation, 1);
        assert_eq!(snapshot.battery_level, Some(87));
        assert_eq!(snapshot.battery_temperature, Some(30.5));
        assert!(snapshot.battery_powered);
        assert_eq!(snapshot.memory_total_kb, Some(1_900_000));
        assert_eq!(snapshot.storage.percent, 63);
        assert_eq!(snapshot.ip.as_deref(), Some("10.0.0.23"));
        assert_eq!(snapshot.load.as_deref(), Some("0.45 0.32 0.20"));
    }

    #[test]
    fn parses_logcat_levels() {
        let raw = "10-21 12:00:00.123  1234  5678 I ActivityManager: Start proc\n\
                   10-21 12:00:01.456  1234  5678 E AndroidRuntime: FATAL EXCEPTION\n\
                   --------- beginning of main\n";
        let lines = parse_logcat(raw);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].level, LogLevel::Info);
        assert_eq!(lines[0].tag, "ActivityManager");
        assert_eq!(lines[1].level, LogLevel::Error);
        assert_eq!(lines[2].level, LogLevel::Unknown);
    }

    #[test]
    fn parses_files_and_packages() {
        let raw = "total 8\n\
                   drwxrwx--x 4 root sdcard_rw 4096 2024-01-01 12:00 Android\n\
                   -rw-rw---- 1 root sdcard_rw 1234 2024-01-01 12:00 my file.txt\n";
        let files = parse_ls(raw);
        assert_eq!(files.len(), 2);
        assert!(files[0].is_dir);
        assert_eq!(files[1].name, "my file.txt");
        assert_eq!(files[1].size, Some(1234));

        let packages = merge_packages(
            "package:com.example.watch\npackage:com.android.settings\n",
            "package:com.example.watch\n",
        );
        assert_eq!(packages.len(), 2);
        let watch = packages
            .iter()
            .find(|package| package.name == "com.example.watch")
            .expect("user package should be present");
        let settings = packages
            .iter()
            .find(|package| package.name == "com.android.settings")
            .expect("system package should be present");
        assert!(!watch.system);
        assert!(settings.system);
    }

    #[test]
    fn merges_package_metadata_without_losing_list_data() {
        let mut packages = merge_packages(
            "package:com.example.watch versionCode:42\npackage:com.android.settings versionCode:7\n",
            "package:com.example.watch\n",
        );
        let watch = packages
            .iter()
            .find(|package| package.name == "com.example.watch")
            .expect("user package should be present");
        assert_eq!(watch.version_code, Some(42));
        // 没有解析到名称时，标题退回包名。
        assert_eq!(watch.label_or_name(), "com.example.watch");
        assert!(!watch.has_label());

        let mut metadata = HashMap::new();
        metadata.insert(
            "com.example.watch".to_string(),
            ApkMetadata {
                label: Some("Sample Watch".to_string()),
                version_name: Some("1.2.3".to_string()),
                version_code: Some(43),
                ..ApkMetadata::default()
            },
        );
        apply_package_metadata(&mut packages, &metadata);

        let watch = packages
            .iter()
            .find(|package| package.name == "com.example.watch")
            .expect("user package should be present");
        assert_eq!(watch.label_or_name(), "Sample Watch");
        assert!(watch.has_label());
        assert_eq!(watch.version_label(), "1.2.3 (43)");
        assert!(!watch.system);

        // 未提供元数据的包保持原样。
        let settings = packages
            .iter()
            .find(|package| package.name == "com.android.settings")
            .expect("system package should be present");
        assert_eq!(settings.label_or_name(), "com.android.settings");
        assert_eq!(settings.version_label(), "7");
    }

    #[test]
    fn parses_metadata_frames() {
        let manifest = b"manifest-bytes";
        let resources = b"arsc";
        let mut stream = Vec::new();
        stream.extend_from_slice(
            format!(
                "@@com.example.watch {} {}\n",
                manifest.len(),
                resources.len()
            )
            .as_bytes(),
        );
        stream.extend_from_slice(manifest);
        stream.extend_from_slice(resources);

        let metadata = read_metadata_frames(&mut stream.as_slice()).expect("frames should parse");
        // 帧本身可读，内容不是有效 APK，因此各项为空但包名被记录。
        assert_eq!(metadata.len(), 1);
        assert!(metadata.contains_key("com.example.watch"));
    }

    #[test]
    fn parses_package_path() {
        assert_eq!(
            parse_package_path("package:/data/app/~~abc==/com.foo-xyz==/base.apk\n"),
            Some("/data/app/~~abc==/com.foo-xyz==/base.apk".to_string())
        );
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(1234), "1.2 KB");
        assert_eq!(format_kilobytes(1024 * 1024), "1.0 GB");
        assert_eq!(format_duration(90_061), "1 天 1 小时");
    }
}

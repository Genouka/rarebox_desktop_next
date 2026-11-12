//! 与界面无关的引擎状态。
//!
//! 这里是 RareBox 的全部业务数据：设备、应用、文件、日志、签名流水线等。
//! 桌面 UI 与 Kotlin/其他前端只持有它的只读快照，不直接修改。

use std::path::PathBuf;

use super::adb::{
    AppOp, AppOpMode, AppPackage, Device, DeviceSnapshot, FileEntry, InputMethod, LogLevel, LogLine,
};
use super::config::Config;
use super::hdc::{HdcBundleInfo, HdcDevice, HdcForward, HdcLogLine, HdcPackage, HdcRebootTarget};
use super::install::{InstallBatch, InstallPlan};
use super::signing::ui::SigningUi;
use super::update::{Announcement, UpdateStatus};

/// 当前调试设备的传输协议。
///
/// ADB 与 hdc 共用同一套页面，具体操作按当前设备类型分发；只有一方支持的功能
/// 会在对应页面中隐藏或禁用。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    #[default]
    Android,
    Harmony,
}

impl DeviceKind {
    pub fn label(self) -> &'static str {
        match self {
            DeviceKind::Android => "Android（ADB）",
            DeviceKind::Harmony => "HarmonyOS（hdc）",
        }
    }
}

/// 顶栏协议选择：自动识别（推荐）或手动锁定。
///
/// 用户通常分不清 Android 手表和 HarmonyOS 手表，默认 `Auto`：
/// 哪边探测到在线设备就用哪边，只有用户主动点芯片才锁定。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KindMode {
    #[default]
    Auto,
    Android,
    Harmony,
}

impl KindMode {
    pub fn is_auto(self) -> bool {
        matches!(self, KindMode::Auto)
    }

    pub fn locked(self) -> Option<DeviceKind> {
        match self {
            KindMode::Auto => None,
            KindMode::Android => Some(DeviceKind::Android),
            KindMode::Harmony => Some(DeviceKind::Harmony),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            KindMode::Auto => "自动",
            KindMode::Android => "Android",
            KindMode::Harmony => "HarmonyOS",
        }
    }
}

/// 盲扫结果的一条候选。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanHit {
    /// `IP:端口`。
    pub address: String,
    /// 协议校验成功后识别出的设备类型；`None` 表示端口开放但不像调试端口。
    pub kind: Option<DeviceKind>,
    pub state: String,
}

/// 盲扫的实时状态。
#[derive(Clone, Debug, Default)]
pub struct ScanState {
    pub running: bool,
    pub done: u32,
    pub total: u32,
    pub hits: Vec<ScanHit>,
    /// 扫描范围描述，例如「192.168.1.0/24」。
    pub scope: String,
    /// 深度扫描（单机全端口）比快扫慢，界面要提示可取消。
    pub deep: bool,
}

impl ScanState {
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }
}

/// 连接失败的原因分类，决定界面给哪个修复入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectErrorKind {
    /// 端口拒绝：可能端口填错。
    PortMismatch,
    /// 超时 / 不可达：可能 IP 错或不同网段。
    Unreachable,
    /// 需要先配对。
    NeedsPairing,
    /// 其他（协议错误、工具缺失等）。
    Other,
}

impl ConnectErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            ConnectErrorKind::PortMismatch => "端口可能填错",
            ConnectErrorKind::Unreachable => "IP 或网络可能不对",
            ConnectErrorKind::NeedsPairing => "需要先配对",
            ConnectErrorKind::Other => "连接失败",
        }
    }
}

/// 最近一次连接失败的上下文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectErrorInfo {
    pub address: String,
    pub kind: ConnectErrorKind,
    pub detail: String,
}

/// 远程投屏会话的状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemotePhase {
    Idle,
    Starting,
    Running,
    Failed,
}

/// 应用列表的筛选范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageScope {
    User,
    System,
    All,
}

impl PackageScope {
    pub const ALL: [PackageScope; 3] =
        [PackageScope::User, PackageScope::System, PackageScope::All];

    pub fn label(self) -> &'static str {
        match self {
            PackageScope::User => "用户应用",
            PackageScope::System => "系统应用",
            PackageScope::All => "全部应用",
        }
    }
}

/// Logcat / HiLog 的级别筛选。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogFilter {
    All,
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogFilter {
    pub const ALL: [LogFilter; 6] = [
        LogFilter::All,
        LogFilter::Verbose,
        LogFilter::Debug,
        LogFilter::Info,
        LogFilter::Warn,
        LogFilter::Error,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LogFilter::All => "全部",
            LogFilter::Verbose => "详细",
            LogFilter::Debug => "调试",
            LogFilter::Info => "信息",
            LogFilter::Warn => "警告",
            LogFilter::Error => "错误",
        }
    }

    pub fn hilog_level(self) -> Option<&'static str> {
        match self {
            LogFilter::All | LogFilter::Verbose => None,
            LogFilter::Debug => Some("D"),
            LogFilter::Info => Some("I"),
            LogFilter::Warn => Some("W"),
            LogFilter::Error => Some("E"),
        }
    }

    pub fn accepts(self, level: LogLevel) -> bool {
        match self {
            LogFilter::All => true,
            LogFilter::Verbose => true,
            LogFilter::Debug => matches!(
                level,
                LogLevel::Debug
                    | LogLevel::Info
                    | LogLevel::Warn
                    | LogLevel::Error
                    | LogLevel::Fatal
            ),
            LogFilter::Info => matches!(
                level,
                LogLevel::Info | LogLevel::Warn | LogLevel::Error | LogLevel::Fatal
            ),
            LogFilter::Warn => matches!(level, LogLevel::Warn | LogLevel::Error | LogLevel::Fatal),
            LogFilter::Error => matches!(level, LogLevel::Error | LogLevel::Fatal),
        }
    }
}

/// 需要二次确认的危险操作。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmKind {
    Uninstall(String),
    ClearData(String),
    Reboot(super::adb::RebootTarget),
    HdcUninstall(String),
    HdcReboot(HdcRebootTarget),
    HdcRestartServer,
    HdcClearLogs,
    HdcClearAppData { bundle: String, cache: bool },
    AppOpsReset(String),
}

/// 需要宿主展示的二次确认框。
#[derive(Clone, Debug)]
pub struct ConfirmRequest {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub kind: ConfirmKind,
}

/// 页面右下角的轻提示级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

/// 终端中的一次命令记录。
#[derive(Clone, Debug)]
pub struct TerminalEntry {
    pub command: String,
    pub output: String,
    pub success: bool,
    pub duration_ms: u128,
    pub time: String,
}

/// 一次 HAP / HSP / APP 安装的结果。
#[derive(Clone, Debug)]
pub struct HdcInstallReport {
    pub title: String,
    pub detail: String,
    pub success: bool,
}

/// 鸿蒙调试状态。
#[derive(Clone, Debug)]
pub struct HarmonyState {
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub error: Option<String>,

    pub devices: Vec<HdcDevice>,
    pub selected: Option<String>,
    pub snapshot: Option<super::hdc::HdcSnapshot>,
    pub snapshot_at: Option<String>,

    pub packages: Vec<HdcPackage>,
    pub packages_for: Option<String>,
    pub install_reports: Vec<HdcInstallReport>,
    pub install_replace: bool,
    pub install_shared: bool,
    pub bundle_details: Option<HdcBundleInfo>,
    /// 正在加载/展示详情的包名。
    pub details_for: Option<String>,

    pub forwards: Vec<HdcForward>,
    pub forward_reverse: bool,

    pub address: String,
    pub query: String,

    pub files_path: String,
    pub files: Vec<FileEntry>,
    pub last_file: Option<PathBuf>,

    pub terminal_input: String,
    pub terminal: Vec<TerminalEntry>,

    pub logs: Vec<HdcLogLine>,
    pub logs_at: Option<String>,
    pub logs_query: String,
    pub logs_filter: LogFilter,
    pub last_log_export: Option<PathBuf>,

    pub screenshot: Option<PathBuf>,
}

impl Default for HarmonyState {
    fn default() -> Self {
        Self {
            path: None,
            version: None,
            error: None,
            devices: Vec::new(),
            selected: None,
            snapshot: None,
            snapshot_at: None,
            packages: Vec::new(),
            packages_for: None,
            install_reports: Vec::new(),
            install_replace: true,
            install_shared: false,
            bundle_details: None,
            details_for: None,
            forwards: Vec::new(),
            forward_reverse: false,
            address: String::new(),
            query: String::new(),
            files_path: "/data/local/tmp".to_string(),
            files: Vec::new(),
            last_file: None,
            terminal_input: String::new(),
            terminal: Vec::new(),
            logs: Vec::new(),
            logs_at: None,
            logs_query: String::new(),
            logs_filter: LogFilter::All,
            last_log_export: None,
            screenshot: None,
        }
    }
}

/// 更新检查数据（弹窗是否打开由宿主决定）。
#[derive(Clone, Debug, Default)]
pub struct UpdateData {
    pub status: Option<UpdateStatus>,
    pub checking: bool,
}

/// 引擎状态：可克隆、可跨线程传递，不包含任何界面句柄。
#[derive(Clone, Debug)]
pub struct CoreState {
    pub kind_mode: KindMode,
    pub device_kind: DeviceKind,
    pub config: Config,

    pub adb_path: Option<PathBuf>,
    pub adb_version: Option<String>,
    pub adb_error: Option<String>,

    /// `adb mdns services` 发现的附近设备（配对 / 连接 / 旧版无线）。
    pub mdns: Vec<crate::adb::MdnsService>,
    /// 盲扫的实时状态。
    pub scan: ScanState,
    /// 最近一次连接失败的原因，界面据此给出修复入口。
    pub connect_error: Option<ConnectErrorInfo>,

    pub hdc_input: String,
    pub harmony: HarmonyState,
    pub signing: SigningUi,

    pub devices: Vec<Device>,
    pub selected_serial: Option<String>,
    pub snapshot: Option<DeviceSnapshot>,
    pub snapshot_at: Option<String>,

    /// 后台任务进度文案；`None` 表示空闲。
    pub activity: Option<String>,

    pub packages: Vec<AppPackage>,
    pub package_scope: PackageScope,
    pub package_query: String,
    pub package_loaded_for: Option<String>,
    /// 正在解析应用名称的设备序列号；`None` 表示没有进行中的解析。
    pub package_metadata_for: Option<String>,
    /// 最近一批安装的结果，显示在应用列表上方。
    pub install_batch: Option<InstallBatch>,
    /// 已经解析出包名、等待用户确认参数的安装方案。
    pub install_plan: Option<InstallPlan>,

    pub screen_size: String,
    pub screen_density: String,
    pub screen_text: String,
    pub last_screenshot: Option<PathBuf>,
    pub last_recording: Option<PathBuf>,

    pub files_path: String,
    pub files_entries: Vec<FileEntry>,
    pub files_selected: Option<String>,
    pub last_export: Option<PathBuf>,

    pub terminal_input: String,
    pub terminal_history: Vec<TerminalEntry>,

    pub logs: Vec<LogLine>,
    pub logs_raw: String,
    pub logs_filter: LogFilter,
    pub logs_query: String,
    pub logs_at: Option<String>,

    // 调试工具页：输入法与 AppOps。
    pub imes: Vec<InputMethod>,
    pub ime_loaded_for: Option<String>,
    pub appops_package: String,
    pub appops_ops: Vec<AppOp>,
    pub appops_loaded_for: Option<String>,
    /// 自定义 op 输入框的文本与所选模式。
    pub appops_op_input: String,
    pub appops_mode: AppOpMode,

    pub remote_phase: RemotePhase,
    pub remote_error: Option<String>,
    pub remote_size: (u16, u16),
    pub remote_generation: u64,
    pub remote_frame_count: u64,

    pub adb_input: String,
    pub screenshot_input: String,
    pub wireless_address: String,
    pub pair_address: String,
    pub pair_code: String,

    pub update: UpdateData,
    pub announcements: Vec<Announcement>,
    pub announcements_error: Option<String>,
}

impl Default for CoreState {
    fn default() -> Self {
        Self {
            kind_mode: KindMode::Auto,
            device_kind: DeviceKind::default(),
            config: Config::load(),
            adb_path: None,
            adb_version: None,
            adb_error: None,
            mdns: Vec::new(),
            scan: ScanState::default(),
            connect_error: None,
            hdc_input: String::new(),
            harmony: HarmonyState::default(),
            signing: SigningUi::new(),
            devices: Vec::new(),
            selected_serial: None,
            snapshot: None,
            snapshot_at: None,
            activity: None,
            packages: Vec::new(),
            package_scope: PackageScope::User,
            package_query: String::new(),
            package_loaded_for: None,
            package_metadata_for: None,
            install_batch: None,
            install_plan: None,
            screen_size: String::new(),
            screen_density: String::new(),
            screen_text: String::new(),
            last_screenshot: None,
            last_recording: None,
            files_path: "/sdcard".to_string(),
            files_entries: Vec::new(),
            files_selected: None,
            last_export: None,
            terminal_input: String::new(),
            terminal_history: Vec::new(),
            logs: Vec::new(),
            logs_raw: String::new(),
            logs_filter: LogFilter::All,
            logs_query: String::new(),
            logs_at: None,
            imes: Vec::new(),
            ime_loaded_for: None,
            appops_package: String::new(),
            appops_ops: Vec::new(),
            appops_loaded_for: None,
            appops_op_input: String::new(),
            appops_mode: AppOpMode::Allowed,
            remote_phase: RemotePhase::Idle,
            remote_error: None,
            remote_size: (0, 0),
            remote_generation: 0,
            remote_frame_count: 0,
            adb_input: String::new(),
            screenshot_input: String::new(),
            wireless_address: String::new(),
            pair_address: String::new(),
            pair_code: String::new(),
            update: UpdateData::default(),
            announcements: Vec::new(),
            announcements_error: None,
        }
    }
}

impl CoreState {
    pub fn has_device(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn is_harmony(&self) -> bool {
        self.device_kind == DeviceKind::Harmony
    }

    pub fn current_harmony_device(&self) -> Option<&HdcDevice> {
        let key = self.harmony.selected.as_deref()?;
        self.harmony.devices.iter().find(|device| device.key == key)
    }

    pub fn current_device(&self) -> Option<&Device> {
        let serial = self.selected_serial.as_deref()?;
        self.devices.iter().find(|device| device.serial == serial)
    }
}

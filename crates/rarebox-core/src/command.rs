//! 宿主 → 引擎的命令。
//!
//! 命令是纯数据，可直接序列化为 JSON（JNI / C ABI）。所有耗时操作都由引擎
//! 自己在后台线程执行，命令投递立即返回。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::adb::{AppOpMode, RebootTarget};
use crate::hdc::{HdcForward, HdcKey, HdcRebootTarget, HdcSwipeDirection, HdcTap};
use crate::install::InstallToggle;
use crate::signing::{ImportFile, SigningBlock};
use crate::state::{ConfirmKind, DeviceKind, KindMode, LogFilter};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    // 设备与连接
    RefreshDevices,
    RefreshHdc,
    LoadSnapshot,
    LoadHdcSnapshot,
    SelectDevice {
        serial: String,
    },
    SelectHdcDevice {
        key: String,
    },
    SetDeviceKind {
        kind: DeviceKind,
    },
    /// 切换自动识别 / 手动锁定。
    SetKindMode {
        mode: KindMode,
    },
    /// 同时刷新 ADB、hdc 与 mDNS，用来自动识别设备类型。
    RefreshAll,
    /// 一个输入框连接任意设备：自动区分 ADB / hdc。
    ConnectSmart {
        address: String,
        code: Option<String>,
    },
    /// 盲扫局域网里可能的调试端口。
    ScanDevices {
        address: String,
        deep: bool,
    },
    CancelScan,
    WirelessConnect {
        address: String,
    },
    WirelessPair {
        address: String,
        code: String,
    },
    WirelessDisconnect,
    EnableTcpip,
    DisconnectDevice {
        serial: String,
    },
    InstallDriver,
    UninstallDriver,

    // 法律条款、更新与公告
    FetchLegal,
    AcceptLegal,
    CheckUpdates {
        manual: bool,
    },
    RefreshAnnouncements,
    DownloadUpdate,

    // Android 应用
    LoadPackages,
    PickInstallApks,
    InstallApks {
        paths: Vec<PathBuf>,
    },
    CancelInstall,
    ToggleInstallOption {
        toggle: InstallToggle,
    },
    RunInstallPlan,
    LaunchPackage {
        package: String,
    },
    StopPackage {
        package: String,
    },
    ClearPackage {
        package: String,
    },
    UninstallPackage {
        package: String,
    },
    ExportPackage {
        package: String,
    },

    // Android 文件
    ListFiles,
    NavigateFiles {
        path: String,
    },
    GoParentFiles,
    PullFile {
        remote: String,
    },
    PickPushFiles,
    PushFiles {
        paths: Vec<PathBuf>,
    },

    // 调试工具
    LoadImes,
    ActivateIme {
        id: String,
    },
    SetImeEnabled {
        id: String,
        enabled: bool,
    },
    OpenImeSettings,
    OpenAppOps {
        package: String,
    },
    LoadAppOps {
        package: String,
    },
    ApplyCustomAppOp {
        package: String,
        op: String,
        mode: AppOpMode,
    },
    SetAppOp {
        package: String,
        op: String,
        mode: AppOpMode,
    },
    ResetAppOps {
        package: String,
    },

    // 日志
    LoadLogs,
    ClearLogcat,
    ExportLogs {
        query: String,
        filter: LogFilter,
    },

    // 屏幕与显示
    TakeScreenshot,
    RecordScreen,
    ApplyDisplaySize {
        size: String,
    },
    ResetDisplaySize,
    ApplyDensity {
        density: String,
    },
    ResetDensity,
    SetRotation {
        rotation: u8,
    },
    SendKey {
        code: u32,
        label: String,
    },
    SendText {
        text: String,
    },

    // 宿主文件选择
    PickScreenshotDir,
    PickSigningHap,
    PickSigningImportFile {
        which: ImportFile,
    },

    // 终端
    SubmitTerminal {
        command: String,
    },
    RunQuickCommand {
        command: String,
    },

    // 远程控制
    StartRemote,
    StopRemote,
    RestartRemote,
    RemotePointerDown {
        x: u32,
        y: u32,
    },
    RemotePointerMove {
        x: u32,
        y: u32,
    },
    RemotePointerUp {
        x: u32,
        y: u32,
    },
    SendRemoteKey {
        keycode: u32,
    },
    SendRemoteText {
        text: String,
    },

    // 设置
    ApplyAdbPath {
        path: String,
    },
    ResetAdbToAuto,
    ApplyHdcPath {
        path: String,
    },
    ResetHdcToAuto,
    OpenConfigDir,
    SetScreenshotDir {
        path: String,
    },
    ResetScreenshotDir,
    OpenMediaDir,
    SetTheme {
        dark: bool,
        seed: u32,
    },
    /// 结束 adb 服务端进程（adb kill-server）。
    KillAdbProcess,
    /// 结束 hdc 服务端进程（hdc kill）。
    KillHdcProcess,
    /// 关闭程序时是否自动结束 adb / hdc 进程。
    SetKillProcessesOnExit {
        value: bool,
    },

    // HarmonyOS 签名
    EnsureSigningLoaded,
    RefreshSigning,
    SigningLogin,
    SigningLogout,
    SigningSelectTeam {
        team_id: String,
    },
    SigningSwitchBlock {
        block: SigningBlock,
    },
    SigningApplyCert {
        reset: bool,
    },
    SigningSetInputs {
        paths: Vec<PathBuf>,
    },
    SigningToggleImported,
    SigningImportFile {
        which: ImportFile,
        path: PathBuf,
    },
    LoadSigningUdid,
    SigningRegisterDevice,
    SigningStart,
    SigningToggleAcl {
        permission: String,
    },
    SigningToggleReplace {
        value: bool,
    },
    SigningToggleResetCert,
    SigningToggleRefreshProfile,
    SigningToggleLogs,
    SigningSetImportAlias {
        value: String,
    },
    SigningSetImportPassword {
        value: String,
    },
    SigningSetAclQuery {
        query: String,
    },

    // HarmonyOS 设备操作
    HdcConnect {
        address: String,
    },
    HdcDisconnect,
    HdcRestartServer,
    HdcEnableWireless {
        port: String,
    },
    HdcDisableWireless,
    HdcLoadForwards,
    HdcToggleForwardDirection,
    HdcAddForward {
        local: String,
        remote: String,
        reverse: bool,
    },
    HdcRemoveForward {
        forward: HdcForward,
    },
    HdcLoadPackages,
    PickInstallHaps,
    HdcInstallHaps {
        paths: Vec<PathBuf>,
    },
    HdcToggleInstallOption {
        shared: bool,
    },
    HdcLaunchPackage {
        bundle: String,
    },
    HdcStopPackage {
        bundle: String,
    },
    HdcUninstallPackage {
        bundle: String,
    },
    HdcCopyBundle {
        bundle: String,
    },
    HdcLoadBundleDetails {
        bundle: String,
    },
    HdcCloseBundleDetails,
    HdcClearAppData {
        bundle: String,
        cache: bool,
    },
    HdcOpenScheme {
        uri: String,
    },
    HdcLoadFiles,
    HdcNavigateFiles {
        path: String,
    },
    HdcGoParentFiles,
    HdcPullFile {
        remote: String,
    },
    PickPushHdcFiles,
    HdcUploadFiles {
        paths: Vec<PathBuf>,
    },
    HdcSendKey {
        key: HdcKey,
    },
    HdcTap {
        x: String,
        y: String,
        tap: HdcTap,
    },
    HdcFling {
        direction: HdcSwipeDirection,
    },
    HdcInputText {
        x: String,
        y: String,
        text: String,
    },
    HdcWakeScreen,
    HdcSubmitTerminal {
        command: String,
    },
    HdcLoadLogs {
        tag: String,
        filter: LogFilter,
    },
    HdcExportLogs,
    HdcClearLogs,
    HdcExportFaultlog,
    HdcExportHilogFiles,
    HdcScreenshot,
    HdcReboot {
        target: HdcRebootTarget,
    },
    HdcOpenReboot {
        target: HdcRebootTarget,
    },
    HdcOpenRestartServer,
    HdcOpenClearLogs,
    HdcOpenClearAppData {
        bundle: String,
        cache: bool,
    },

    // 二次确认框的最终动作
    ExecuteConfirm {
        kind: ConfirmKind,
    },
    RequestReboot {
        target: RebootTarget,
    },
}

impl Command {
    /// 解析跨语言（JNI / C ABI）传入的 JSON 命令。
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        serde_json::from_str(json).map_err(|err| anyhow::anyhow!("invalid command: {err}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

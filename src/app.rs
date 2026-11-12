//! RareBox 桌面端的状态镜像与页面调度。
//!
//! 业务状态与命令都由 `rarebox_core::Engine` 持有；这里的字段是渲染用的
//! 只读镜像，通过引擎事件同步。页面动作在 `app/actions.rs` 里转成命令，
//! `app/bridge.rs` 负责事件与状态镜像的同步。

mod actions;
mod apps_ui;
mod bridge;
mod connect_ui;
mod debug_ui;
mod files_ui;
mod guide_ui;
mod harmony_tools_ui;
mod harmony_ui;
mod logs_ui;
mod overview_ui;
mod remote_ui;
mod screen_ui;
mod settings_ui;
mod shell_ui;
mod signing_ui;
mod terminal_ui;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use gpui::{App, Context, Entity, RenderImage, ScrollHandle, SharedString, Window, prelude::*};
use material3_gpui::icon::IconName;
use material3_gpui::interaction::BoundsHandle;
use material3_gpui::prelude::*;

use rarebox_core::adb::{
    AppOp, AppOpMode, AppPackage, Device, DeviceSnapshot, FileEntry, InputMethod, LogLine,
};
use rarebox_core::config::Config;
pub(crate) use rarebox_core::signing::{ImportFile, LoginPhase, SigningUi};
pub(crate) use rarebox_core::state::{
    ConfirmKind, CoreState, DeviceKind, HarmonyState, HdcInstallReport, KindMode, LogFilter,
    PackageScope, RemotePhase, TerminalEntry, ToastKind,
};
use rarebox_core::update;
use rarebox_core::{Command, Engine};
pub(crate) use rarebox_core::{
    DISPLAY_LIMIT, KEYCODE_APP_SWITCH, KEYCODE_BACK, KEYCODE_HOME, KEYCODE_POWER, ime_display,
    rotation_label,
};

use crate::theme;

/// 侧边导航中的应用页面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Guide,
    Overview,
    Connect,
    Apps,
    Screen,
    Remote,
    Files,
    Terminal,
    Logs,
    Tools,
    Signing,
    Settings,
}

impl Page {
    pub(crate) const ALL: [Page; 12] = [
        Page::Guide,
        Page::Overview,
        Page::Connect,
        Page::Apps,
        Page::Screen,
        Page::Remote,
        Page::Files,
        Page::Terminal,
        Page::Logs,
        Page::Tools,
        Page::Signing,
        Page::Settings,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Page::Guide => "快速指引",
            Page::Overview => "概览",
            Page::Connect => "连接",
            Page::Apps => "应用管理",
            Page::Screen => "屏幕与显示",
            Page::Remote => "远程控制",
            Page::Files => "文件传输",
            Page::Terminal => "终端",
            Page::Logs => "日志查看器",
            Page::Tools => "调试工具",
            Page::Signing => "签名与安装",
            Page::Settings => "设置",
        }
    }

    pub(crate) fn icon(self) -> &'static str {
        match self {
            Page::Guide => "help",
            Page::Overview => "dashboard",
            Page::Connect => "cable",
            Page::Apps => "apps",
            Page::Screen => "screenshot_monitor",
            Page::Remote => "settings_remote",
            Page::Files => "folder_open",
            Page::Terminal => "terminal",
            Page::Logs => "subject",
            Page::Tools => "build",
            Page::Signing => "security",
            Page::Settings => "settings",
        }
    }

    pub(crate) fn subtitle(self) -> &'static str {
        match self {
            Page::Guide => "本地操作百科：说出你想做的事，跟着图文一步步完成",
            Page::Overview => "设备状态、电量与常用快捷操作",
            Page::Connect => "连接 Android 与 HarmonyOS 设备、无线调试与配对",
            Page::Apps => "安装、启动、清理与导出应用（APK / HAP）",
            Page::Screen => "截图、录屏、分辨率与按键控制",
            Page::Remote => "在应用内实时镜像并直接操作 Android 设备屏幕",
            Page::Files => "浏览设备文件，并在本机与设备之间传输",
            Page::Terminal => "在设备上直接执行 ADB / hdc shell 命令",
            Page::Logs => "抓取 Logcat / HiLog，并按级别与关键字筛选",
            Page::Tools => "输入法切换与 AppOps 权限管理",
            Page::Signing => "华为账号登录、调试/发布证书、Profile、ACL 与签名安装",
            Page::Settings => "ADB / HDC 路径、外观主题与无线调试",
        }
    }
}

/// 首次启动（或条款版本变更）时的 EULA / 隐私政策展示弹窗。
#[derive(Clone, Debug)]
pub(crate) struct LegalModal {
    pub(crate) eula: String,
    pub(crate) privacy: String,
    pub(crate) tab: LegalTab,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LegalTab {
    Eula,
    Privacy,
}

/// 更新检查与更新弹窗的状态。
#[derive(Clone, Debug, Default)]
pub(crate) struct UpdateState {
    pub(crate) status: Option<update::UpdateStatus>,
    pub(crate) checking: bool,
    pub(crate) dialog: bool,
    pub(crate) dismissed: bool,
}

/// 公告弹窗：左侧是公告列表，右侧是选中公告的正文。
#[derive(Clone, Debug, Default)]
pub(crate) struct AnnouncementModal {
    pub(crate) items: Vec<update::Announcement>,
    pub(crate) selected: Option<i64>,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Confirm {
    pub(crate) title: SharedString,
    pub(crate) message: SharedString,
    pub(crate) confirm_label: SharedString,
    pub(crate) kind: rarebox_core::ConfirmKind,
}

#[derive(Clone, Debug)]
pub(crate) struct Activity {
    pub(crate) label: SharedString,
}

#[derive(Clone, Debug)]
struct Toast {
    id: u64,
    message: SharedString,
    kind: ToastKind,
}

/// 应用根节点：界面实体 + 引擎状态镜像。
pub struct RareBox {
    pub(crate) core: Engine,
    /// 最近一次同步的引擎状态，用于判断哪些输入框需要跟随引擎更新。
    core_state: Arc<CoreState>,

    page: Page,
    device_kind: DeviceKind,
    config: Config,

    adb_path: Option<PathBuf>,
    adb_version: Option<String>,
    adb_error: Option<String>,

    harmony: HarmonyState,
    hdc_input: String,
    hdc_field: Entity<TextFieldState>,
    harmony_address_field: Entity<TextFieldState>,
    harmony_query_field: Entity<TextFieldState>,
    harmony_terminal_field: Entity<TextFieldState>,
    harmony_files_field: Entity<TextFieldState>,
    harmony_logs_field: Entity<TextFieldState>,
    harmony_logs_tag_field: Entity<TextFieldState>,
    harmony_tmode_port_field: Entity<TextFieldState>,
    harmony_forward_local_field: Entity<TextFieldState>,
    harmony_forward_remote_field: Entity<TextFieldState>,
    harmony_control_x_field: Entity<TextFieldState>,
    harmony_control_y_field: Entity<TextFieldState>,
    harmony_control_text_field: Entity<TextFieldState>,
    harmony_deep_link_field: Entity<TextFieldState>,
    signing: SigningUi,
    signing_acl_field: Entity<TextFieldState>,
    signing_import_alias_field: Entity<TextFieldState>,
    signing_import_password_field: Entity<TextFieldState>,

    devices: Vec<Device>,
    selected_serial: Option<String>,
    snapshot: Option<DeviceSnapshot>,
    snapshot_at: Option<String>,

    activity: Option<Activity>,

    packages: Vec<AppPackage>,
    package_scope: PackageScope,
    package_query: String,
    package_query_field: Entity<TextFieldState>,
    package_loaded_for: Option<String>,
    package_metadata_for: Option<String>,
    install_batch: Option<rarebox_core::install::InstallBatch>,
    install_plan: Option<rarebox_core::install::InstallPlan>,

    screen_size_field: Entity<TextFieldState>,
    screen_density_field: Entity<TextFieldState>,
    screen_text_field: Entity<TextFieldState>,
    last_screenshot: Option<PathBuf>,
    last_recording: Option<PathBuf>,

    files_path: String,
    files_entries: Vec<FileEntry>,
    files_selected: Option<String>,
    files_path_field: Entity<TextFieldState>,
    last_export: Option<PathBuf>,

    terminal_input: String,
    terminal_field: Entity<TextFieldState>,
    terminal_history: Vec<TerminalEntry>,

    logs: Vec<LogLine>,
    logs_raw: String,
    logs_filter: LogFilter,
    logs_query: String,
    logs_query_field: Entity<TextFieldState>,
    logs_at: Option<String>,

    imes: Vec<InputMethod>,
    ime_loaded_for: Option<String>,
    appops_package: String,
    appops_package_field: Entity<TextFieldState>,
    appops_ops: Vec<AppOp>,
    appops_loaded_for: Option<String>,
    appops_op_input: String,
    appops_op_field: Entity<TextFieldState>,
    appops_mode: AppOpMode,

    remote_phase: RemotePhase,
    remote_error: Option<String>,
    remote_frame: Option<Arc<RenderImage>>,
    remote_retired_frames: Vec<Arc<RenderImage>>,
    remote_size: (u16, u16),
    remote_bounds: BoundsHandle,
    remote_touch: Option<(u32, u32)>,
    remote_generation: u64,
    remote_text_input: String,
    remote_text_field: Entity<TextFieldState>,
    remote_input_at: Option<Instant>,
    remote_frame_count: u64,

    adb_input: String,
    adb_field: Entity<TextFieldState>,
    screenshot_input: String,
    screenshot_field: Entity<TextFieldState>,
    dark_switch: Entity<SwitchState>,
    kill_on_exit_switch: Entity<SwitchState>,
    wireless_address: String,
    wireless_field: Entity<TextFieldState>,
    pair_address: String,
    pair_field: Entity<TextFieldState>,
    pair_code: String,
    pair_code_field: Entity<TextFieldState>,

    /// 自动识别 / 手动锁定设备类型。
    kind_mode: KindMode,
    /// `adb mdns services` 发现的附近设备。
    mdns: Vec<rarebox_core::adb::MdnsService>,
    /// 盲扫的实时状态。
    scan: rarebox_core::state::ScanState,
    /// 最近一次连接失败的原因。
    connect_error: Option<rarebox_core::state::ConnectErrorInfo>,
    /// 扫描输入（留空时扫本机网段）。
    scan_address: String,
    scan_field: Entity<TextFieldState>,
    /// USB 驱动卡是否展开（默认收起，减少文字）。
    driver_open: bool,
    /// 手动输入地址区是否展开。
    manual_open: bool,

    scroll_handles: [ScrollHandle; Page::ALL.len()],
    sidebar_open: bool,
    confirm: Option<Confirm>,
    legal: Option<LegalModal>,
    update: UpdateState,
    announcements: Option<AnnouncementModal>,
    toasts: Vec<Toast>,
    next_toast: u64,
    guide: guide_ui::GuideUi,
}

impl RareBox {
    pub fn new(cx: &mut App, config: Config) -> Entity<Self> {
        let core = Engine::new(config.clone());
        let core_state = core.state();
        let entity = cx.new(move |cx| {
            let weak = cx.entity().downgrade();

            let package_query_field = {
                let weak = weak.clone();
                TextField::new("apps-query", "搜索应用名称或包名")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.package_query = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新应用搜索失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let screen_size_field =
                TextField::new("screen-size", "分辨率（例如 450x450）").build(cx);
            let screen_density_field =
                TextField::new("screen-density", "DPI（例如 320）").build(cx);
            let screen_text_field = TextField::new("screen-text", "输入要发送到设备的文本")
                .leading_icon(IconName::new("keyboard"))
                .build(cx);

            let files_path_field = {
                let weak = weak.clone();
                TextField::new("files-path", "设备路径")
                    .value("/sdcard")
                    .leading_icon(IconName::new("folder"))
                    .on_submit(move |_, _, cx| {
                        let weak = weak.clone();
                        cx.defer(move |cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                let path = app.files_path_field.read(cx).value().to_string();
                                app.navigate_files(path, cx);
                            }) {
                                eprintln!("RareBox Desktop: 文件路径跳转失败: {err}");
                            }
                        });
                    })
                    .build(cx)
            };

            let terminal_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("terminal-command", "输入 shell 命令，按回车执行")
                    .leading_icon(IconName::new("terminal"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.terminal_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新终端输入失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                    app.submit_terminal(cx);
                                }) {
                                    eprintln!("RareBox Desktop: 执行终端命令失败: {err}");
                                }
                            });
                        }
                    })
                    .build(cx)
            };

            let logs_query_field = {
                let weak = weak.clone();
                TextField::new("logs-query", "按标签或内容筛选")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.logs_query = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新日志筛选失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let remote_text_field = {
                let weak = weak.clone();
                let submit_weak = weak.clone();
                TextField::new("remote-text", "向手表发送文字")
                    .leading_icon(IconName::new("keyboard"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.remote_text_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新远程文字失败: {err}");
                        }
                    })
                    .on_submit(move |_, _, cx| {
                        let submit_weak = submit_weak.clone();
                        cx.defer(move |cx| {
                            if let Err(err) = submit_weak.update(cx, |app: &mut Self, cx| {
                                app.send_remote_text(cx);
                            }) {
                                eprintln!("RareBox Desktop: 发送远程文字失败: {err}");
                            }
                        });
                    })
                    .build(cx)
            };

            let adb_input = config.adb_path.clone();
            let adb_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("settings-adb-path", "ADB 可执行文件路径")
                    .value(adb_input.clone())
                    .leading_icon(IconName::new("adb"))
                    .helper(if cfg!(any(target_os = "android", target_os = "ios")) {
                        "留空可自动在 Termux、系统路径与 APK 自带目录中查找"
                    } else {
                        "留空可自动在 PATH、Android SDK 与项目目录中查找"
                    })
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.adb_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新 ADB 路径失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                app.apply_adb_settings(cx);
                            }) {
                                eprintln!("RareBox Desktop: 应用 ADB 路径失败: {err}");
                            }
                        }
                    })
                    .build(cx)
            };

            let screenshot_input = config.screenshot_dir.clone().unwrap_or_default();
            let screenshot_field = {
                let weak = weak.clone();
                TextField::new("settings-shot-dir", "截图与录屏保存目录")
                    .value(screenshot_input.clone())
                    .leading_icon(IconName::new("image"))
                    .helper("留空时使用图片目录下的 RareBox Desktop 文件夹")
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.screenshot_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新截图目录失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let dark_switch = {
                let weak = weak.clone();
                let dark = config.dark;
                Switch::new("settings-dark")
                    .checked(dark)
                    .on_change(move |checked, window, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.set_dark(checked, window, cx);
                        }) {
                            eprintln!("RareBox Desktop: 切换主题失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let kill_on_exit_switch = {
                let weak = weak.clone();
                let value = config.kill_processes_on_exit;
                Switch::new("settings-kill-on-exit")
                    .checked(value)
                    .on_change(move |checked, _window, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.set_kill_processes_on_exit(checked, cx);
                        }) {
                            eprintln!("RareBox Desktop: 切换退出清理失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let wireless_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("wireless-address", "手表地址（例如 192.168.1.20:5555）")
                    .leading_icon(IconName::new("wifi"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.wireless_address = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新无线地址失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                app.connect_smart(cx);
                            }) {
                                eprintln!("RareBox Desktop: 无线连接失败: {err}");
                            }
                        }
                    })
                    .build(cx)
            };

            let pair_field = {
                let weak = weak.clone();
                TextField::new("wireless-pair-address", "配对地址（IP:端口）")
                    .leading_icon(IconName::new("link"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.pair_address = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新配对地址失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let pair_code_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("wireless-pair-code", "配对码")
                    .leading_icon(IconName::new("security"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.pair_code = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新配对码失败: {err}");
                        }
                    })
                    .on_submit(move |_, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.wireless_pair(cx);
                        }) {
                            eprintln!("RareBox Desktop: 无线配对失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let scan_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("connect-scan-address", "留空扫本机网段，或填 IP")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.scan_address = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新扫描地址失败: {err}");
                        }
                    })
                    .on_submit(move |_, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.start_scan(false, cx);
                        }) {
                            eprintln!("RareBox Desktop: 启动扫描失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let guide_query_field = {
                let change_weak = weak.clone();
                let submit_weak = weak.clone();
                TextField::new(
                    "guide-query",
                    "描述你想做的事，例如：字太小、装应用、连不上",
                )
                .leading_icon(IconName::new("search"))
                .on_change(move |value, _, cx| {
                    if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                        app.guide.query = value.to_string();
                        cx.notify();
                    }) {
                        eprintln!("RareBox Desktop: 更新指引搜索失败: {err}");
                    }
                })
                .on_submit(move |value, _, cx| {
                    let value = value.to_string();
                    if let Err(err) =
                        submit_weak.update(cx, |app: &mut Self, cx| app.guide_ask(value, cx))
                    {
                        eprintln!("RareBox Desktop: 发送指引问题失败: {err}");
                    }
                })
                .build(cx)
            };

            let appops_package_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("appops-package", "包名（例如 com.example.app）")
                    .leading_icon(IconName::new("android"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.appops_package = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新 AppOps 包名失败: {err}");
                        }
                    })
                    .on_submit({
                        move |_, _, cx| {
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                    app.load_app_ops(cx);
                                }) {
                                    eprintln!("RareBox Desktop: 读取 AppOps 失败: {err}");
                                }
                            });
                        }
                    })
                    .build(cx)
            };

            let appops_op_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("appops-op", "操作名（例如 READ_CLIPBOARD）")
                    .leading_icon(IconName::new("data_usage"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.appops_op_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新 AppOps 操作名失败: {err}");
                        }
                    })
                    .on_submit({
                        move |_, _, cx| {
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                    app.apply_custom_app_op(cx);
                                }) {
                                    eprintln!("RareBox Desktop: 设置 AppOps 失败: {err}");
                                }
                            });
                        }
                    })
                    .build(cx)
            };

            let hdc_input = config.hdc_path.clone();
            let hdc_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("settings-hdc-path", "hdc 可执行文件路径")
                    .value(hdc_input.clone())
                    .leading_icon(IconName::new("devices"))
                    .helper(if cfg!(any(target_os = "android", target_os = "ios")) {
                        "已内置 hdc（libhdc.so）；留空时优先使用内置版本"
                    } else {
                        "留空时优先使用内置 hdc，再查找 DevEco Studio 与 OpenHarmony SDK"
                    })
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.hdc_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新 HDC 路径失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                app.apply_hdc_settings(cx);
                            }) {
                                eprintln!("RareBox Desktop: 应用 HDC 路径失败: {err}");
                            }
                        }
                    })
                    .build(cx)
            };

            let harmony_address_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("harmony-address", "设备地址（例如 192.168.1.20:5555）")
                    .leading_icon(IconName::new("wifi"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.harmony.address = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新鸿蒙设备地址失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                app.hdc_connect(cx);
                            }) {
                                eprintln!("RareBox Desktop: 连接鸿蒙设备失败: {err}");
                            }
                        }
                    })
                    .build(cx)
            };

            let harmony_query_field = {
                let weak = weak.clone();
                TextField::new("harmony-query", "搜索 Bundle 名")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.harmony.query = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新鸿蒙应用搜索失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let harmony_terminal_field = {
                let weak = weak.clone();
                let change_weak = weak.clone();
                TextField::new("harmony-terminal", "输入 hdc shell 命令，按回车执行")
                    .leading_icon(IconName::new("terminal"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = change_weak.update(cx, |app: &mut Self, cx| {
                            app.harmony.terminal_input = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新鸿蒙终端输入失败: {err}");
                        }
                    })
                    .on_submit({
                        let weak = weak.clone();
                        move |_, _, cx| {
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                    app.submit_hdc_terminal(cx);
                                }) {
                                    eprintln!("RareBox Desktop: 执行鸿蒙终端命令失败: {err}");
                                }
                            });
                        }
                    })
                    .build(cx)
            };

            let harmony_files_field = {
                let weak = weak.clone();
                TextField::new("harmony-files-path", "设备路径")
                    .value("/data/local/tmp")
                    .leading_icon(IconName::new("folder"))
                    .on_submit(move |_, _, cx| {
                        let weak = weak.clone();
                        cx.defer(move |cx| {
                            if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                                let path = app.harmony_files_field.read(cx).value().to_string();
                                app.navigate_hdc_files(path, cx);
                            }) {
                                eprintln!("RareBox Desktop: 鸿蒙文件路径跳转失败: {err}");
                            }
                        });
                    })
                    .build(cx)
            };

            let harmony_logs_field = {
                let weak = weak.clone();
                TextField::new("harmony-logs-query", "按标签或内容筛选")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.harmony.logs_query = value.to_string();
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新鸿蒙日志筛选失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let harmony_logs_tag_field =
                TextField::new("harmony-logs-tag", "HiLog 标签 -T（可选）").build(cx);
            let harmony_tmode_port_field = TextField::new("harmony-tmode-port", "无线端口")
                .value("5555")
                .build(cx);
            let harmony_forward_local_field =
                TextField::new("harmony-forward-local", "PC 端口").build(cx);
            let harmony_forward_remote_field =
                TextField::new("harmony-forward-remote", "设备端口").build(cx);
            let harmony_control_x_field = TextField::new("harmony-control-x", "X").build(cx);
            let harmony_control_y_field = TextField::new("harmony-control-y", "Y").build(cx);
            let harmony_control_text_field =
                TextField::new("harmony-control-text", "要输入到设备的文字").build(cx);
            let harmony_deep_link_field =
                TextField::new("harmony-deep-link", "myscheme://path 或 https://…").build(cx);

            let signing_import_alias_field = {
                let weak = weak.clone();
                TextField::new("signing-import-alias", "密钥别名")
                    .value("rarebox")
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, _| {
                            app.set_signing_alias(value.to_string());
                        }) {
                            eprintln!("RareBox Desktop: 更新导入别名失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let signing_import_password_field = {
                let weak = weak.clone();
                TextField::new("signing-import-password", "密钥库口令")
                    .password(true)
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, _| {
                            app.set_signing_password(value.to_string());
                        }) {
                            eprintln!("RareBox Desktop: 更新导入口令失败: {err}");
                        }
                    })
                    .build(cx)
            };

            let signing_acl_field = {
                let weak = weak.clone();
                TextField::new("signing-acl-query", "搜索 ACL 权限")
                    .leading_icon(IconName::new("search"))
                    .on_change(move |value, _, cx| {
                        if let Err(err) = weak.update(cx, |app: &mut Self, cx| {
                            app.set_signing_acl_query(value.to_string());
                            cx.notify();
                        }) {
                            eprintln!("RareBox Desktop: 更新 ACL 搜索失败: {err}");
                        }
                    })
                    .build(cx)
            };

            // 关闭程序时按设置结束后台调试进程。读取界面镜像，
            // 与开关的即时状态一致，避免引擎命令尚未处理时读到旧值。
            cx.on_app_quit(|app: &mut Self, _cx: &mut Context<Self>| {
                app.kill_debug_processes();
                async {}
            })
            .detach();

            Self {
                core,
                core_state,
                page: Page::Guide,
                device_kind: DeviceKind::Android,
                config,
                adb_path: None,
                adb_version: None,
                adb_error: None,
                harmony: HarmonyState::default(),
                hdc_input,
                hdc_field,
                harmony_address_field,
                harmony_query_field,
                harmony_terminal_field,
                harmony_files_field,
                harmony_logs_field,
                harmony_logs_tag_field,
                harmony_tmode_port_field,
                harmony_forward_local_field,
                harmony_forward_remote_field,
                harmony_control_x_field,
                harmony_control_y_field,
                harmony_control_text_field,
                harmony_deep_link_field,
                signing: SigningUi::new(),
                signing_acl_field,
                signing_import_alias_field,
                signing_import_password_field,
                devices: Vec::new(),
                selected_serial: None,
                snapshot: None,
                snapshot_at: None,
                activity: None,
                packages: Vec::new(),
                package_scope: PackageScope::User,
                package_query: String::new(),
                package_query_field,
                package_loaded_for: None,
                package_metadata_for: None,
                install_batch: None,
                install_plan: None,
                screen_size_field,
                screen_density_field,
                screen_text_field,
                last_screenshot: None,
                last_recording: None,
                files_path: "/sdcard".to_string(),
                files_entries: Vec::new(),
                files_selected: None,
                files_path_field,
                last_export: None,
                terminal_input: String::new(),
                terminal_field,
                terminal_history: Vec::new(),
                logs: Vec::new(),
                logs_raw: String::new(),
                logs_filter: LogFilter::All,
                logs_query: String::new(),
                logs_query_field,
                logs_at: None,
                imes: Vec::new(),
                ime_loaded_for: None,
                appops_package: String::new(),
                appops_package_field,
                appops_ops: Vec::new(),
                appops_loaded_for: None,
                appops_op_input: String::new(),
                appops_op_field,
                appops_mode: AppOpMode::Allowed,
                remote_phase: RemotePhase::Idle,
                remote_error: None,
                remote_frame: None,
                remote_retired_frames: Vec::new(),
                remote_size: (0, 0),
                remote_bounds: BoundsHandle::new(),
                remote_touch: None,
                remote_generation: 0,
                remote_text_input: String::new(),
                remote_text_field,
                remote_input_at: None,
                remote_frame_count: 0,
                adb_input,
                adb_field,
                screenshot_input,
                screenshot_field,
                dark_switch,
                kill_on_exit_switch,
                wireless_address: String::new(),
                wireless_field,
                pair_address: String::new(),
                pair_field,
                pair_code: String::new(),
                pair_code_field,
                kind_mode: KindMode::Auto,
                mdns: Vec::new(),
                scan: rarebox_core::state::ScanState::default(),
                connect_error: None,
                scan_address: String::new(),
                scan_field,
                driver_open: false,
                manual_open: false,
                scroll_handles: std::array::from_fn(|_| ScrollHandle::new()),
                guide: guide_ui::GuideUi::new(guide_query_field),
                sidebar_open: false,
                confirm: None,
                legal: None,
                update: UpdateState::default(),
                announcements: None,
                toasts: Vec::new(),
                next_toast: 0,
            }
        });

        entity.update(cx, |app, cx| app.start_event_loop(cx));
        entity.update(cx, |app, cx| app.bootstrap_ui(cx));

        entity
    }

    pub(crate) fn has_device(&self) -> bool {
        self.snapshot.is_some()
    }

    pub(crate) fn is_harmony(&self) -> bool {
        self.device_kind == DeviceKind::Harmony
    }

    pub(crate) fn harmony_device(&self) -> Option<&rarebox_core::hdc::HdcDevice> {
        let key = self.harmony.selected.as_deref()?;
        self.harmony.devices.iter().find(|device| device.key == key)
    }

    pub(crate) fn current_device(&self) -> Option<&Device> {
        let serial = self.selected_serial.as_deref()?;
        self.devices.iter().find(|device| device.serial == serial)
    }

    fn bootstrap_ui(&mut self, cx: &mut Context<Self>) {
        self.apply_core_state(cx);
        self.maybe_show_legal(cx);
        if self.legal.is_none() {
            self.check_updates(cx, false);
        }
    }

    /// 首次启动或条款版本变更时，展示 EULA / 隐私政策弹窗。
    /// 即便网络不可达，也会用内置文案展示，保证「必须确认」这一要求。
    fn maybe_show_legal(&mut self, cx: &mut Context<Self>) {
        if self.config.accepted_legal_version.as_deref() == Some(rarebox_core::legal::LEGAL_VERSION)
        {
            return;
        }
        self.legal = Some(LegalModal {
            eula: rarebox_core::legal::FALLBACK_EULA.to_string(),
            privacy: rarebox_core::legal::FALLBACK_PRIVACY.to_string(),
            tab: LegalTab::Eula,
            loading: true,
            error: None,
        });
        self.fetch_legal(cx);
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // 更新与公告
    // ---------------------------------------------------------------------

    /// 检查更新。`manual` 为真时，无论结果如何都用轻提示反馈。
    pub(crate) fn check_updates(&mut self, cx: &mut Context<Self>, manual: bool) {
        if self.update.checking {
            return;
        }
        self.update.checking = true;
        self.core_command(Command::CheckUpdates { manual });
        cx.notify();
    }

    /// 打开更新弹窗；还没有检查结果时先手动检查一次。
    pub(crate) fn open_update_dialog(&mut self, cx: &mut Context<Self>) {
        if self.update.status.is_none() {
            self.check_updates(cx, true);
            return;
        }
        self.update.dialog = true;
        self.update.dismissed = false;
        cx.notify();
    }

    /// 关闭更新弹窗：有可用更新时在左下角保留胶囊提醒。
    pub(crate) fn close_update_dialog(&mut self, cx: &mut Context<Self>) {
        self.update.dialog = false;
        if self
            .update
            .status
            .as_ref()
            .is_some_and(|status| status.update_available)
        {
            self.update.dismissed = true;
        }
        cx.notify();
    }

    /// 打开下载页；实际打开浏览器由引擎的 `open_url` 事件完成。
    pub(crate) fn download_update(&mut self, cx: &mut Context<Self>) {
        self.close_update_dialog(cx);
        self.core_command(Command::DownloadUpdate);
    }

    /// 打开公告弹窗，并在后台刷新公告列表。
    pub(crate) fn open_announcements(&mut self, cx: &mut Context<Self>) {
        if self.announcements.is_none() {
            self.announcements = Some(AnnouncementModal::default());
        }
        self.refresh_announcements(cx);
    }

    /// 重新拉取公告（含历史公告）。
    pub(crate) fn refresh_announcements(&mut self, cx: &mut Context<Self>) {
        if let Some(modal) = self.announcements.as_mut() {
            modal.loading = true;
        } else {
            return;
        }
        self.core_command(Command::RefreshAnnouncements);
        cx.notify();
    }

    pub(crate) fn select_announcement(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(modal) = self.announcements.as_mut() {
            modal.selected = Some(id);
            cx.notify();
        }
    }

    pub(crate) fn close_announcements(&mut self, cx: &mut Context<Self>) {
        self.announcements = None;
        cx.notify();
    }

    /// 用户同意条款：记录已接受版本并持久化，关闭弹窗。
    pub(crate) fn accept_legal(&mut self, cx: &mut Context<Self>) {
        self.config.accepted_legal_version = Some(rarebox_core::legal::LEGAL_VERSION.to_string());
        self.core_command(Command::AcceptLegal);
        self.legal = None;
        cx.notify();
        self.check_updates(cx, false);
    }

    /// 用户拒绝条款：退出应用。
    pub(crate) fn decline_legal(&mut self, cx: &mut Context<Self>) {
        cx.quit();
    }

    /// 切换条款标签页。
    pub(crate) fn set_legal_tab(&mut self, tab: LegalTab, cx: &mut Context<Self>) {
        if let Some(modal) = self.legal.as_mut() {
            modal.tab = tab;
            cx.notify();
        }
    }

    // ---------------------------------------------------------------------
    // 页面调度
    // ---------------------------------------------------------------------

    /// 切换 Android / HarmonyOS 工作区，并按需加载当前页面的数据。
    pub(crate) fn set_device_kind(&mut self, kind: DeviceKind, cx: &mut Context<Self>) {
        if self.device_kind == kind {
            return;
        }
        self.device_kind = kind;
        self.core_command(Command::SetDeviceKind { kind });
        if kind == DeviceKind::Harmony {
            if self.page == Page::Remote {
                self.stop_remote(cx);
                self.page = Page::Screen;
            }
            if self.page == Page::Tools {
                self.page = Page::Overview;
            }
        }
        self.load_page_data(cx);
    }

    /// 从设置页或空状态跳转到连接页，并切到 HarmonyOS（hdc）工作区。
    pub(crate) fn open_harmony_connect(&mut self, cx: &mut Context<Self>) {
        self.set_device_kind(DeviceKind::Harmony, cx);
        self.set_page(Page::Connect, cx);
    }

    pub(crate) fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.sidebar_open = false;
        if self.page == page {
            return;
        }
        // 用户在“高亮导航、等待自动切页”的停顿里自己点了别的页面：
        // 以用户为准，取消这次自动切换，不要稍后又把界面抢回去。
        if let Some(session) = self.guide.session.as_mut()
            && let Some(pending) = session.pending_page
            && pending != page
        {
            session.pending_page = None;
            session.switch_token = session.switch_token.wrapping_add(1);
        }
        if self.page == Page::Remote {
            self.stop_remote(cx);
        }
        self.page = page;
        self.load_page_data(cx);
        cx.notify();
    }

    /// 进入页面时按需加载该页数据；加载命令是幂等的，引擎会忽略重复请求。
    fn load_page_data(&mut self, cx: &mut Context<Self>) {
        if self.is_harmony() {
            match self.page {
                Page::Apps
                    if self.harmony.packages.is_empty() && self.harmony.selected.is_some() =>
                {
                    self.load_hdc_packages(cx)
                }
                Page::Logs if self.harmony.logs.is_empty() && self.harmony.selected.is_some() => {
                    self.load_hdc_logs(cx)
                }
                Page::Files if self.harmony.files.is_empty() && self.harmony.selected.is_some() => {
                    self.load_hdc_files(cx)
                }
                Page::Signing => self.ensure_signing_loaded(cx),
                Page::Overview if self.harmony.path.is_none() => self.refresh_hdc(cx),
                _ => {}
            }
            return;
        }
        match self.page {
            Page::Apps if self.packages.is_empty() && self.has_device() => self.load_packages(cx),
            Page::Logs if self.logs.is_empty() && self.has_device() => self.load_logs(cx),
            Page::Files if self.files_entries.is_empty() && self.has_device() => {
                self.list_files(cx)
            }
            Page::Tools if self.ime_loaded_for.is_none() && self.has_device() => self.load_imes(cx),
            _ => {}
        }
    }

    // ---------------------------------------------------------------------
    // 外观
    // ---------------------------------------------------------------------

    pub(crate) fn set_dark(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.config.dark == dark {
            return;
        }
        self.config.dark = dark;
        self.save_theme();
        theme::apply(cx, self.config.seed, dark);
        let _ = window;
        cx.notify();
    }

    pub(crate) fn toggle_dark(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = !self.config.dark;
        self.config.dark = dark;
        self.save_theme();
        theme::apply(cx, self.config.seed, dark);
        self.dark_switch
            .update(cx, |switch, cx| switch.set_checked(dark, window, cx));
        cx.notify();
    }

    /// 切换「关闭程序时自动结束调试进程」。
    pub(crate) fn set_kill_processes_on_exit(&mut self, value: bool, cx: &mut Context<Self>) {
        if self.config.kill_processes_on_exit == value {
            return;
        }
        self.config.kill_processes_on_exit = value;
        self.core_command(Command::SetKillProcessesOnExit { value });
        cx.notify();
    }

    /// 结束后台 adb / hdc 服务端进程（退出清理用）。
    fn kill_debug_processes(&self) {
        if !self.config.kill_processes_on_exit {
            return;
        }
        if let Some(path) = self.core_state.adb_path.clone() {
            let _ = rarebox_core::adb::AdbClient::new(path).kill_server();
        }
        if let Some(path) = self.core_state.harmony.path.clone() {
            let _ = rarebox_core::hdc::HdcClient::new(path).kill_server();
        }
    }

    pub(crate) fn set_seed(&mut self, seed: u32, cx: &mut Context<Self>) {
        if self.config.seed == seed {
            return;
        }
        self.config.seed = seed;
        self.save_theme();
        theme::apply(cx, seed, self.config.dark);
        self.toast(
            cx,
            ToastKind::Info,
            format!("已切换为「{}」配色", theme::accent_name(seed)),
        );
        cx.notify();
    }

    fn save_theme(&self) {
        // 主题字段属于引擎配置；引擎会负责落盘。这里先写入镜像，避免界面滞后。
        let dark = self.config.dark;
        let seed = self.config.seed;
        self.core_command(Command::SetTheme { dark, seed });
    }

    // ---------------------------------------------------------------------
    // 提示与确认框
    // ---------------------------------------------------------------------

    pub(crate) fn toast(
        &mut self,
        cx: &mut Context<Self>,
        kind: ToastKind,
        message: impl Into<SharedString>,
    ) {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast {
            id,
            message: message.into(),
            kind,
        });
        if self.toasts.len() > 4 {
            self.toasts.remove(0);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(4600))
                .await;
            if let Err(err) = this.update(cx, |app, cx| {
                app.toasts.retain(|toast| toast.id != id);
                cx.notify();
            }) {
                eprintln!("RareBox Desktop: 关闭提示失败: {err}");
            }
        })
        .detach();
    }

    pub(crate) fn open_confirm(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.confirm = Some(confirm);
        cx.notify();
    }

    pub(crate) fn execute_confirm(
        &mut self,
        kind: rarebox_core::ConfirmKind,
        cx: &mut Context<Self>,
    ) {
        self.confirm = None;
        self.core_command(Command::ExecuteConfirm { kind });
        cx.notify();
    }

    // ---------------------------------------------------------------------
    // 远程画面
    // ---------------------------------------------------------------------

    /// 丢弃已经不在界面上的旧帧，释放 GPU 纹理。
    pub(crate) fn drop_remote_frames(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for image in self.remote_retired_frames.drain(..) {
            cx.drop_image(image, Some(window));
        }
    }

    fn remote_device_point(&self, position: gpui::Point<gpui::Pixels>) -> Option<(u32, u32)> {
        let (width, height) = self.remote_size;
        if width < 2 || height < 2 {
            return None;
        }
        let bounds = self.remote_bounds.get();
        let bounds_width = f32::from(bounds.size.width);
        let bounds_height = f32::from(bounds.size.height);
        if bounds_width <= 0.0 || bounds_height <= 0.0 {
            return None;
        }
        let scale = (bounds_width / f32::from(width)).min(bounds_height / f32::from(height));
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let draw_width = f32::from(width) * scale;
        let draw_height = f32::from(height) * scale;
        let left = f32::from(bounds.origin.x) + (bounds_width - draw_width) / 2.0;
        let top = f32::from(bounds.origin.y) + (bounds_height - draw_height) / 2.0;
        let x = (f32::from(position.x) - left) / draw_width;
        let y = (f32::from(position.y) - top) / draw_height;
        if !(0.0..1.0).contains(&x) || !(0.0..1.0).contains(&y) {
            return None;
        }
        Some((
            (x * f32::from(width)).floor() as u32,
            (y * f32::from(height)).floor() as u32,
        ))
    }

    pub(crate) fn remote_pointer_down(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        _cx: &mut Context<Self>,
    ) {
        let Some(point) = self.remote_device_point(position) else {
            return;
        };
        self.remote_touch = Some(point);
        self.remote_input_at = Some(Instant::now());
        self.core_command(Command::RemotePointerDown {
            x: point.0,
            y: point.1,
        });
    }

    pub(crate) fn remote_pointer_move(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        pressed: Option<gpui::MouseButton>,
        _cx: &mut Context<Self>,
    ) {
        if self.remote_touch.is_none() || pressed != Some(gpui::MouseButton::Left) {
            return;
        }
        let Some(point) = self.remote_device_point(position) else {
            return;
        };
        self.remote_touch = Some(point);
        self.core_command(Command::RemotePointerMove {
            x: point.0,
            y: point.1,
        });
    }

    pub(crate) fn remote_pointer_up(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        _cx: &mut Context<Self>,
    ) {
        let Some(last) = self.remote_touch.take() else {
            return;
        };
        let point = self.remote_device_point(position).unwrap_or(last);
        self.remote_input_at = Some(Instant::now());
        self.core_command(Command::RemotePointerUp {
            x: point.0,
            y: point.1,
        });
    }
}

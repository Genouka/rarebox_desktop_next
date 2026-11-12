//! 内置操作手册与本地检索。
//!
//! 「快速指引」不联网、不调用模型：所有内容都是随应用编译的静态步骤，
//! 这里提供确定性的中文检索、平台过滤与手把手步骤数据。界面层只负责
//! 把 [`GuideTarget`] 映射到真实控件并渲染遮罩，不参与检索。
//!
//! 内容维护约定：
//! - 每个教程的步骤顺序就是用户实际操作的顺序；
//! - `page` 表示这一步应该在第几页完成，界面会在高亮导航项后自动切页；
//! - `gate` 只用于必须满足才允许继续的条件（设备已选中、ADB 就绪）；
//! - `stuck` 是用户在某个步骤卡住时展开的排查项，必须给出可执行动作。

/// 当前工作区对应的设备协议。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideScope {
    Android,
    Harmony,
}

/// 教程支持范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platforms {
    pub android: bool,
    pub harmony: bool,
    /// 仅在 Windows 主机上显示（例如 USB 驱动）。
    pub windows_only: bool,
}

impl Platforms {
    pub const ANDROID: Self = Self {
        android: true,
        harmony: false,
        windows_only: false,
    };
    pub const HARMONY: Self = Self {
        android: false,
        harmony: true,
        windows_only: false,
    };
    pub const BOTH: Self = Self {
        android: true,
        harmony: true,
        windows_only: false,
    };
    pub const WINDOWS: Self = Self {
        android: true,
        harmony: true,
        windows_only: true,
    };

    pub fn supports(self, scope: GuideScope, windows: bool) -> bool {
        if self.windows_only && !windows {
            return false;
        }
        match scope {
            GuideScope::Android => self.android,
            GuideScope::Harmony => self.harmony,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideCategory {
    Connect,
    Install,
    Display,
    Files,
    Advanced,
    Troubleshoot,
}

impl GuideCategory {
    pub const ALL: [Self; 6] = [
        Self::Connect,
        Self::Install,
        Self::Display,
        Self::Files,
        Self::Advanced,
        Self::Troubleshoot,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Connect => "连接设备",
            Self::Install => "安装应用",
            Self::Display => "屏幕与显示",
            Self::Files => "文件与截图",
            Self::Advanced => "进阶功能",
            Self::Troubleshoot => "排查问题",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Connect => "cable",
            Self::Install => "apps",
            Self::Display => "screenshot_monitor",
            Self::Files => "folder_open",
            Self::Advanced => "build",
            Self::Troubleshoot => "bug_report",
        }
    }
}

/// 教程步骤对应的页面。与界面层的 `Page` 解耦，避免 core 依赖 GPUI。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuidePage {
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

impl GuidePage {
    pub const ALL: [Self; 11] = [
        Self::Overview,
        Self::Connect,
        Self::Apps,
        Self::Screen,
        Self::Remote,
        Self::Files,
        Self::Terminal,
        Self::Logs,
        Self::Tools,
        Self::Signing,
        Self::Settings,
    ];
}

/// 需要高亮的真实控件。界面层负责采集每个控件的位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GuideTarget {
    None,
    Nav(GuidePage),
    TopbarDevice,
    ConnectTransportSwitch,
    ConnectDeviceList,
    ConnectPairCard,
    ConnectDriverCard,
    ConnectHdcCard,
    /// 连接页顶部的能力矩阵（无线 / WLAN / USB 三张图块）。
    ConnectSupportCard,
    /// 「开启 TCP/IP 5555」动作。
    ConnectTcpipAction,
    /// 盲扫面板。
    ConnectScanPanel,
    AppsInstallButton,
    AppsInstallPanel,
    HarmonyAppsInstall,
    ScreenDensityRow,
    ScreenApplyDensity,
    ScreenResetDensity,
    ScreenScreenshot,
    FilesList,
    FilesUpload,
    RemoteStart,
    SettingsAdbPath,
    SettingsSaveDir,
    SettingsKillServer,
}

/// 进入下一步前必须满足的条件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideGate {
    None,
    /// ADB 已找到并可用。
    AdbReady,
    /// 已经选中一台在线设备（Android 看 snapshot，HarmonyOS 看在线 target）。
    DeviceSelected,
}

/// 卡住时可直接执行的界面动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAction {
    OpenPage(GuidePage),
    RefreshDevices,
}

#[derive(Clone, Copy, Debug)]
pub struct StuckHelp {
    pub title: &'static str,
    pub detail: &'static str,
    pub action: Option<GuideAction>,
}

impl StuckHelp {
    const fn new(title: &'static str, detail: &'static str, action: Option<GuideAction>) -> Self {
        Self {
            title,
            detail,
            action,
        }
    }
}

/// 分支步骤里的一个选项：选择后直接启动另一个教程。
#[derive(Clone, Copy, Debug)]
pub struct GuideChoice {
    pub label: &'static str,
    pub hint: &'static str,
    pub icon: &'static str,
    pub guide: &'static str,
}

impl GuideChoice {
    const fn new(
        label: &'static str,
        hint: &'static str,
        icon: &'static str,
        guide: &'static str,
    ) -> Self {
        Self {
            label,
            hint,
            icon,
            guide,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GuideStep {
    pub title: &'static str,
    pub body: &'static str,
    /// `assets/guide/<art>.svg` 的文件名（不含扩展名）；`None` 表示无配图。
    pub art: Option<&'static str>,
    pub checklist: &'static [&'static str],
    pub page: Option<GuidePage>,
    pub target: GuideTarget,
    pub gate: GuideGate,
    pub stuck: &'static [StuckHelp],
    pub choices: &'static [GuideChoice],
}

impl GuideStep {
    const fn new(
        title: &'static str,
        body: &'static str,
        art: Option<&'static str>,
        checklist: &'static [&'static str],
        page: Option<GuidePage>,
        target: GuideTarget,
        gate: GuideGate,
        stuck: &'static [StuckHelp],
    ) -> Self {
        Self {
            title,
            body,
            art,
            checklist,
            page,
            target,
            gate,
            stuck,
            choices: &[],
        }
    }

    #[allow(clippy::too_many_arguments)]
    const fn choose(
        title: &'static str,
        body: &'static str,
        art: Option<&'static str>,
        checklist: &'static [&'static str],
        page: Option<GuidePage>,
        target: GuideTarget,
        gate: GuideGate,
        stuck: &'static [StuckHelp],
        choices: &'static [GuideChoice],
    ) -> Self {
        Self {
            title,
            body,
            art,
            checklist,
            page,
            target,
            gate,
            stuck,
            choices,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub id: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub category: GuideCategory,
    pub platforms: Platforms,
    pub keywords: &'static [&'static str],
    pub duration: &'static str,
    /// `true` 表示完整手把手教程；`false` 表示一步到位的快捷操作。
    pub tutorial: bool,
    pub steps: &'static [GuideStep],
}

impl Guide {
    /// 这个教程是否必须先选中一台在线设备才能做。
    ///
    /// 连接类教程（USB / 配对 / 驱动 / 排查）本身就是在解决“没有设备”，
    /// 因此不算；安装、屏幕、文件、投屏和依赖设备的快捷操作都算。
    pub fn requires_device(&self) -> bool {
        matches!(
            self.id,
            "install_apk"
                | "install_hap"
                | "dpi"
                | "screenshot"
                | "files"
                | "remote"
                | "op_export_apk"
                | "op_app_list"
                | "op_logs"
                | "op_shell"
                | "op_reboot"
                | "op_input_method"
                | "op_appops"
                | "op_port_forward"
                | "op_device_info"
        )
    }
}

const NO_STUCK: &[StuckHelp] = &[];

// ---------------------------------------------------------------------------
// 教程内容
// ---------------------------------------------------------------------------

pub static GUIDES: &[Guide] = &[
    Guide {
        id: "connect_choose",
        title: "连接手表",
        summary: "不用分辨系统：选一种连接方式，程序自动识别。",
        category: GuideCategory::Connect,
        platforms: Platforms::ANDROID,
        keywords: &[
            "连接",
            "怎么连",
            "usb",
            "无线",
            "无线调试",
            "wlan",
            "配对",
            "选择",
            "识别",
        ],
        duration: "约 1 分钟",
        tutorial: true,
        steps: &[GuideStep::choose(
            "选一种连接方式",
            "不用判断是 Android 还是 HarmonyOS，连上后程序会自动识别。",
            Some("connect_choose"),
            &[],
            None,
            GuideTarget::ConnectSupportCard,
            GuideGate::None,
            &[
                StuckHelp::new(
                    "找不到开发者选项",
                    "「设置 → 系统 → 关于」连点版本号 7 次。",
                    None,
                ),
                StuckHelp::new(
                    "手表只显示二维码",
                    "在手表上选「使用配对码配对设备」。",
                    None,
                ),
            ],
            &[
                GuideChoice::new(
                    "无线调试",
                    "有「配对码」，最省线",
                    "tabler-wifi",
                    "connect_wireless",
                ),
                GuideChoice::new(
                    "WLAN 调试",
                    "旧机型，端口 5555",
                    "tabler-router",
                    "connect_wlan",
                ),
                GuideChoice::new(
                    "USB 数据线",
                    "第一次最稳",
                    "tabler-usb",
                    "connect_usb",
                ),
            ],
        )],
    },
    Guide {
        id: "connect_usb",
        title: "用 USB 连接 Android 手表",
        summary: "从开启开发者选项到在 RareBox 里选中手表，5 步完成。",
        category: GuideCategory::Connect,
        platforms: Platforms::ANDROID,
        keywords: &[
            "usb",
            "数据线",
            "接电脑",
            "连接手表",
            "开发者选项",
            "usb调试",
            "授权",
            "允许",
            "未授权",
            "连不上",
            "wear os",
            "手表",
            "adb",
            "驱动",
            "连接",
        ],
        duration: "约 3 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "打开「开发者选项」",
                "手表：设置 → 系统 → 关于 → 连点「版本号」7 次。返回设置后能看到「开发者选项」。",
                Some("dev_options"),
                &["已经看到「开发者选项」菜单"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "找不到「关于」或「版本号」",
                        "Wear OS：设置 → 系统 → 关于。部分手表在 设置 → 关于手表 → 版本号。",
                        None,
                    ),
                    StuckHelp::new(
                        "连点没有反应",
                        "确认点的是「版本号」而不是「版本」；连续点 7 次，中间不要退出设置。",
                        None,
                    ),
                ],
            ),
            GuideStep::new(
                "打开「USB 调试」",
                "进入开发者选项，把「USB 调试」开关打开。",
                Some("usb_debug"),
                &["「USB 调试」已打开"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "开关是灰色的",
                        "先打开开发者选项；部分手表需要先关闭演示模式，或插上充电器再操作。",
                        None,
                    ),
                    StuckHelp::new(
                        "重启后自动关闭",
                        "开发者选项在重启后可能恢复默认，重新打开 USB 调试即可。",
                        None,
                    ),
                ],
            ),
            GuideStep::new(
                "用数据线连接电脑",
                "用能传数据的线连接手表和电脑。手表弹出「允许 USB 调试吗？」时勾选「始终允许」并点「允许」。",
                Some("usb_cable"),
                &["手表上已点「允许」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::AdbReady,
                &[
                    StuckHelp::new(
                        "没有弹出授权窗口",
                        "换一根数据线或换一个 USB 口后重新插拔；在手表上关闭再打开 USB 调试。",
                        None,
                    ),
                    StuckHelp::new(
                        "只在充电，电脑没反应",
                        "线可能只能充电；换原装或明确支持数据传输的线。",
                        None,
                    ),
                    StuckHelp::new(
                        "提示 ADB 未就绪",
                        "打开设置指定 adb 路径，或回到连接页点「重新检测」。",
                        Some(GuideAction::OpenPage(GuidePage::Settings)),
                    ),
                ],
            ),
            GuideStep::new(
                "选中手表",
                "设备出现在「连接」页列表后，点一下手表，让它成为当前设备。",
                Some("device_select"),
                &["设备状态显示「已连接」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::DeviceSelected,
                &[
                    StuckHelp::new(
                        "列表是空的",
                        "Windows 先安装 USB 驱动；其他系统检查 adb 是否可用，再点「刷新列表」。",
                        Some(GuideAction::RefreshDevices),
                    ),
                    StuckHelp::new(
                        "显示「未授权」",
                        "在手表上重新确认授权；必要时撤销 USB 调试授权后重新插拔。",
                        None,
                    ),
                    StuckHelp::new(
                        "显示「离线」",
                        "拔插数据线，或在设置里结束 adb 服务后重试。",
                        Some(GuideAction::OpenPage(GuidePage::Settings)),
                    ),
                ],
            ),
            GuideStep::new(
                "完成",
                "现在可以打开概览、应用管理或屏幕与显示。设备断开时回到「连接」页重新选择。",
                Some("ready"),
                &[],
                Some(GuidePage::Overview),
                GuideTarget::None,
                GuideGate::None,
                NO_STUCK,
            ),
        ],
    },
    Guide {
        id: "connect_wireless",
        title: "无线调试与首次配对",
        summary: "Wear OS 无线调试：配对 → 连接 → 选中设备。",
        category: GuideCategory::Connect,
        platforms: Platforms::ANDROID,
        keywords: &[
            "无线",
            "无线调试",
            "配对",
            "配对码",
            "ip",
            "端口",
            "wifi",
            "同一网络",
            "tcpip",
            "5555",
            "连接地址",
            "连不上",
            "手表",
            "pair",
        ],
        duration: "约 4 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "打开无线调试",
                "设置 → 系统 → 开发者选项 → 无线调试。",
                Some("wireless_debug"),
                &["开关已打开"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[StuckHelp::new(
                    "找不到无线调试",
                    "先开启开发者选项；部分版本在列表最下方。",
                    None,
                )],
            ),
            GuideStep::new(
                "点「使用配对码配对设备」",
                "手表上会显示配对地址和六位码。",
                Some("wireless_pair"),
                &["已打开配对码页面"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "只有二维码",
                        "改点「使用配对码配对设备」。",
                        None,
                    ),
                    StuckHelp::new(
                        "想扫码",
                        "桌面端不读摄像头，请用配对码。",
                        None,
                    ),
                ],
            ),
            GuideStep::new(
                "记下配对地址与六位码",
                "配对地址只用于配对，和连接端口不同。",
                Some("wireless_pair"),
                &["已记录地址", "已记录六位码"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[StuckHelp::new(
                    "页面很快消失",
                    "重新点「使用配对码配对设备」。",
                    None,
                )],
            ),
            GuideStep::new(
                "在 RareBox 配对",
                "配对地址和六位码填进去，点「配对」。",
                Some("app_pair"),
                &["已点「配对」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectPairCard,
                GuideGate::None,
                &[StuckHelp::new(
                    "配对失败",
                    "确认同一 Wi-Fi，地址是配对端口。",
                    None,
                )],
            ),
            GuideStep::new(
                "点「连接」",
                "填连接地址（不是配对地址），或直接用「盲扫」。",
                Some("wireless_connect"),
                &["设备出现在列表"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectPairCard,
                GuideGate::None,
                &[StuckHelp::new(
                    "连接被拒绝",
                    "用连接地址；端口别抄错。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
            GuideStep::new(
                "选中设备",
                "点列表里的设备即可开始使用。",
                Some("device_select"),
                &["状态显示「已连接」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::DeviceSelected,
                NO_STUCK,
            ),
        ],
    },

    Guide {
        id: "connect_wlan",
        title: "WLAN 调试（旧版 5555）",
        summary: "没有「无线调试/配对码」的旧手表：先 USB 开一次 TCP 端口，再无线连。",
        category: GuideCategory::Connect,
        platforms: Platforms::ANDROID,
        keywords: &[
            "wlan",
            "5555",
            "tcpip",
            "局域网",
            "旧版",
            "没有无线调试",
            "adb tcpip",
        ],
        duration: "约 3 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "打开「WLAN 调试」",
                "设置 → 系统 → 开发者选项 → WLAN 调试。",
                Some("wlan_debug"),
                &["「WLAN 调试」已打开"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[StuckHelp::new(
                    "没有这个开关",
                    "用 USB 连一次，由程序开启 5555 端口。",
                    None,
                )],
            ),
            GuideStep::new(
                "先用 USB 连一次",
                "数据线连电脑，手表弹窗点「允许」。",
                Some("usb_cable"),
                &["设备已出现在列表"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::AdbReady,
                &[StuckHelp::new(
                    "没有弹窗",
                    "换数据线或 USB 口，重新插拔。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
            GuideStep::new(
                "开启 5555 端口",
                "在连接页点「开启 TCP/IP 5555」。",
                Some("wlan_tcpip"),
                &["已点「开启 5555」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectTcpipAction,
                GuideGate::None,
                &[StuckHelp::new(
                    "按钮是灰的",
                    "先选中一台在线设备。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
            GuideStep::new(
                "拔线，无线连接",
                "拔掉数据线，点「连接」；地址会自动填好。",
                Some("wlan_connect"),
                &["设备显示「已连接」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectPairCard,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "连不上",
                    "确认手表和电脑在同一 Wi-Fi；用「盲扫」找 5555 端口。",
                    None,
                )],
            ),
        ],
    },
    Guide {
        id: "driver",
        title: "电脑认不出手表：安装 USB 驱动",
        summary: "设备管理器有黄色感叹号、连接页列表为空时，用内置驱动修复。",
        category: GuideCategory::Troubleshoot,
        platforms: Platforms::WINDOWS,
        keywords: &[
            "驱动",
            "usb驱动",
            "windows",
            "设备管理器",
            "感叹号",
            "识别不了",
            "列表为空",
            "adb interface",
            "安装驱动",
            "uac",
            "未知设备",
        ],
        duration: "约 2 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "确认是驱动问题",
                "打开设备管理器：手表显示为未知设备/黄色感叹号，或「通用串行总线设备」里没有 Android ADB Interface，就是驱动问题。",
                Some("driver_problem"),
                &["设备管理器里看到异常设备"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDriverCard,
                GuideGate::None,
                &[StuckHelp::new(
                    "没有黄色感叹号但列表为空",
                    "先换线换口，再点「刷新列表」；也可能是手表没有授权。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
            GuideStep::new(
                "安装内置驱动",
                "连接页 →「USB 驱动」卡片 →「安装驱动」。Windows 弹出 UAC 时点「是」。",
                Some("driver_uac"),
                &["UAC 已允许"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDriverCard,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "没有弹出 UAC",
                        "确认点的是 RareBox 里的「安装驱动」；如果被安全软件拦截，允许后再试。",
                        None,
                    ),
                    StuckHelp::new(
                        "安装失败",
                        "展开驱动卡片查看错误；重启电脑后再安装一次。",
                        None,
                    ),
                ],
            ),
            GuideStep::new(
                "重新插拔并检测",
                "拔掉数据线，重新插上，点「刷新列表」。",
                Some("driver_done"),
                &["设备出现在列表", "状态不是「未授权」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "仍然没有设备",
                    "换 USB 口或数据线；确认手表已打开 USB 调试，并在弹窗点允许。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
        ],
    },
    Guide {
        id: "install_apk",
        title: "安装 APK",
        summary: "一次可以装多个 APK；分包会自动使用 install-multiple。",
        category: GuideCategory::Install,
        platforms: Platforms::ANDROID,
        keywords: &[
            "安装",
            "装app",
            "装应用",
            "apk",
            "安装包",
            "批量安装",
            "分包",
            "覆盖安装",
            "降级",
            "权限",
            "install",
            "应用",
        ],
        duration: "约 2 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "选择设备",
                "先在顶栏确认当前设备是你要安装的手表。",
                Some("device_select"),
                &[],
                None,
                GuideTarget::TopbarDevice,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "顶栏显示「未连接设备」",
                    "先按「用 USB 连接 Android 手表」完成连接。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "打开应用管理",
                "点侧栏「应用管理」。",
                Some("apps_nav"),
                &[],
                Some(GuidePage::Apps),
                GuideTarget::Nav(GuidePage::Apps),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "点「安装 APK」",
                "在应用管理页点「安装 APK」，会打开电脑的文件选择器。",
                Some("install_button"),
                &["文件选择器已打开"],
                Some(GuidePage::Apps),
                GuideTarget::AppsInstallButton,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "按钮是灰的",
                    "先选择一台在线设备；离线或未授权时不能安装。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "选择 APK",
                "可以多选。同一个应用的分包和主 APK 一起选，RareBox 会按包名自动分组并用 install-multiple 提交。",
                Some("apk_picker"),
                &["已选中要安装的 APK"],
                Some(GuidePage::Apps),
                GuideTarget::AppsInstallPanel,
                GuideGate::None,
                &[StuckHelp::new(
                    "看不到 APK",
                    "确认文件扩展名是 .apk；在文件选择器里切换到 APK 所在目录。",
                    None,
                )],
            ),
            GuideStep::new(
                "检查参数并开始",
                "按需勾选覆盖安装（-r）、允许降级（-d）、授予全部权限（-g）、允许测试包（-t）。核对命令预览后点「开始安装」。",
                Some("install_options"),
                &["参数已确认", "已点「开始安装」"],
                Some(GuidePage::Apps),
                GuideTarget::AppsInstallPanel,
                GuideGate::None,
                &[StuckHelp::new(
                    "不知道该勾什么",
                    "普通升级勾「覆盖安装」；签名不同或版本更低时再加「允许降级」。",
                    None,
                )],
            ),
            GuideStep::new(
                "查看结果",
                "安装结果会逐项显示成功或失败原因；失败时按提示处理后可以重试。",
                Some("install_result"),
                &["结果显示成功"],
                Some(GuidePage::Apps),
                GuideTarget::AppsInstallPanel,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "INSTALL_FAILED_UPDATE_INCOMPATIBLE",
                        "新旧包签名不同：先卸载旧版再装，或使用相同签名。",
                        None,
                    ),
                    StuckHelp::new(
                        "INSTALL_FAILED_VERSION_DOWNGRADE",
                        "勾选「允许降级」后再安装。",
                        None,
                    ),
                    StuckHelp::new(
                        "INSTALL_FAILED_INSUFFICIENT_STORAGE",
                        "清理设备空间后重试。",
                        None,
                    ),
                ],
            ),
        ],
    },
    Guide {
        id: "install_hap",
        title: "安装 HAP（HarmonyOS）",
        summary: "安装 HAP/HSP/APP；多 HAP 应用建议直接选 .app。",
        category: GuideCategory::Install,
        platforms: Platforms::HARMONY,
        keywords: &[
            "安装",
            "hap",
            "hsp",
            "app",
            "鸿蒙",
            "harmonyos",
            "hdc",
            "签名",
            "覆盖安装",
            "共享包",
            "9568322",
            "应用",
        ],
        duration: "约 3 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "切换到 HarmonyOS",
                "连接页顶部把工作区切到「HarmonyOS（hdc）」，并确认 hdc 已就绪。",
                Some("harmony_switch"),
                &["工作区显示 HarmonyOS", "hdc 已就绪"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectTransportSwitch,
                GuideGate::None,
                &[StuckHelp::new(
                    "未找到 hdc",
                    "在设置里指定 hdc 路径，或使用 DevEco Studio 自带的 toolchains/hdc。",
                    Some(GuideAction::OpenPage(GuidePage::Settings)),
                )],
            ),
            GuideStep::new(
                "打开应用管理",
                "点侧栏「应用管理」。",
                Some("apps_nav"),
                &[],
                Some(GuidePage::Apps),
                GuideTarget::Nav(GuidePage::Apps),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "点「安装应用」",
                "在 HarmonyOS 应用管理页点安装入口，选择 HAP/HSP；多 HAP 应用建议直接选 .app 应用包。",
                Some("harmony_install"),
                &["文件选择器已打开"],
                Some(GuidePage::Apps),
                GuideTarget::HarmonyAppsInstall,
                GuideGate::AdbReady,
                &[StuckHelp::new(
                    "安装按钮不可用",
                    "先选择一台在线 HarmonyOS 设备。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "选择安装包",
                "可多选并逐包安装。调试包必须已签名；未签名先到「签名与安装」处理。",
                Some("hap_picker"),
                &["已选中 HAP/HSP/APP"],
                Some(GuidePage::Apps),
                GuideTarget::HarmonyAppsInstall,
                GuideGate::None,
                &[StuckHelp::new(
                    "错误码 9568322",
                    "签名或 Profile 不匹配：到「签名与安装」重新签名，或改用已签名的包。",
                    Some(GuideAction::OpenPage(GuidePage::Signing)),
                )],
            ),
            GuideStep::new(
                "选择参数并安装",
                "按需切换「覆盖安装 -r」「共享包 -s」，然后开始安装。",
                Some("install_options"),
                &["参数已确认", "已开始安装"],
                Some(GuidePage::Apps),
                GuideTarget::HarmonyAppsInstall,
                GuideGate::None,
                &[StuckHelp::new(
                    "安装被拒绝",
                    "检查设备是否已授权调试；签名 Profile 是否包含当前设备 UDID。",
                    Some(GuideAction::OpenPage(GuidePage::Signing)),
                )],
            ),
            GuideStep::new(
                "查看结果",
                "安装失败会显示 hdc 原始输出和中文提示；签名类错误先处理签名再重试。",
                Some("install_result"),
                &["结果显示成功"],
                Some(GuidePage::Apps),
                GuideTarget::HarmonyAppsInstall,
                GuideGate::None,
                NO_STUCK,
            ),
        ],
    },
    Guide {
        id: "dpi",
        title: "调整 DPI 与分辨率",
        summary: "字太小或太大时调整显示密度；改错了可以一键恢复。",
        category: GuideCategory::Display,
        platforms: Platforms::ANDROID,
        keywords: &[
            "dpi",
            "密度",
            "分辨率",
            "字太小",
            "字太大",
            "显示太大",
            "显示太小",
            "屏幕",
            "字体",
            "wm density",
            "黑屏",
            "显示",
        ],
        duration: "约 2 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "选择设备",
                "先确认顶栏是当前手表；改显示参数只影响这台设备。",
                Some("device_select"),
                &[],
                None,
                GuideTarget::TopbarDevice,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "没有可选设备",
                    "先完成 USB 或无线连接。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "打开屏幕与显示",
                "点侧栏「屏幕与显示」，向下找到「分辨率与密度」。",
                Some("screen_nav"),
                &[],
                Some(GuidePage::Screen),
                GuideTarget::Nav(GuidePage::Screen),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "找到当前 DPI",
                "卡片第二行就是 DPI。输入框里显示当前密度，例如 320。",
                Some("dpi_panel"),
                &["已经看到当前 DPI"],
                Some(GuidePage::Screen),
                GuideTarget::ScreenDensityRow,
                GuideGate::None,
                &[StuckHelp::new(
                    "没有这一行",
                    "HarmonyOS 工作区不提供 DPI 调整；先切换到 Android 工作区。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "输入新的 DPI",
                "手表常用 320–400。数字越大字越大。一次只改 20–40，先小步试。",
                Some("dpi_input"),
                &["已输入新的 DPI", "没有同时改分辨率"],
                Some(GuidePage::Screen),
                GuideTarget::ScreenDensityRow,
                GuideGate::None,
                &[StuckHelp::new(
                    "不知道填多少",
                    "记下当前值，先加 20；不满意再继续加。",
                    None,
                )],
            ),
            GuideStep::new(
                "点「应用」",
                "点 DPI 行右侧的「应用」。画面会立即变化，设备信息会自动刷新。",
                Some("dpi_apply"),
                &["已点「应用」", "画面已经变化"],
                Some(GuidePage::Screen),
                GuideTarget::ScreenApplyDensity,
                GuideGate::None,
                &[
                    StuckHelp::new(
                        "画面太大，点不到按钮",
                        "在电脑端点同一行的「恢复物理值」；或到终端执行 wm density reset。",
                        Some(GuideAction::OpenPage(GuidePage::Terminal)),
                    ),
                    StuckHelp::new(
                        "黑屏或无法操作",
                        "在终端执行 wm density reset 恢复默认；仍不行就重启设备。",
                        Some(GuideAction::OpenPage(GuidePage::Terminal)),
                    ),
                ],
            ),
            GuideStep::new(
                "不满意就恢复",
                "点「恢复物理值」回到设备默认 DPI。改分辨率同理。",
                Some("dpi_reset"),
                &["已恢复默认显示"],
                Some(GuidePage::Screen),
                GuideTarget::ScreenResetDensity,
                GuideGate::None,
                &[StuckHelp::new(
                    "恢复后仍然不对",
                    "在终端执行 wm size reset 和 wm density reset，然后重启设备。",
                    Some(GuideAction::OpenPage(GuidePage::Terminal)),
                )],
            ),
        ],
    },
    Guide {
        id: "screenshot",
        title: "截图与录屏",
        summary: "把设备画面保存到电脑，文件位置可在设置里修改。",
        category: GuideCategory::Files,
        platforms: Platforms::BOTH,
        keywords: &[
            "截图", "录屏", "屏幕", "保存", "图片", "视频", "screen", "record",
        ],
        duration: "约 1 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "确认设备在线",
                "截图与录屏需要一台已授权的设备。先看顶栏是否显示设备名称。",
                Some("device_select"),
                &[],
                None,
                GuideTarget::TopbarDevice,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "顶栏显示「未连接设备」",
                    "先按「用 USB 连接 Android 手表」或「连接 HarmonyOS 设备」完成连接。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "打开屏幕与显示",
                "点侧栏「屏幕与显示」。",
                Some("screen_nav"),
                &[],
                Some(GuidePage::Screen),
                GuideTarget::Nav(GuidePage::Screen),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "截图或录屏",
                "点截图卡片里的「截图」保存当前画面；录屏会一直录制，直到再次点击停止。",
                Some("screenshot"),
                &["截图/录屏已经完成"],
                Some(GuidePage::Screen),
                GuideTarget::ScreenScreenshot,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "按钮不可用",
                    "先连接并选中设备；HarmonyOS 使用 snapshot_display/uitest 截图。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "查看保存位置",
                "在「设置」里可以查看和修改截图/录屏目录；完成后通常会提示文件路径。",
                Some("save_dir"),
                &["已经找到保存文件"],
                Some(GuidePage::Settings),
                GuideTarget::SettingsSaveDir,
                GuideGate::None,
                &[StuckHelp::new(
                    "找不到文件",
                    "在设置页复制保存目录路径，或在截图完成提示里点打开目录。",
                    Some(GuideAction::OpenPage(GuidePage::Settings)),
                )],
            ),
        ],
    },
    Guide {
        id: "files",
        title: "在电脑和设备之间传文件",
        summary: "浏览设备目录，上传或下载文件、文件夹。",
        category: GuideCategory::Files,
        platforms: Platforms::BOTH,
        keywords: &[
            "文件",
            "传输",
            "上传",
            "下载",
            "push",
            "pull",
            "文件夹",
            "目录",
            "文件传输",
        ],
        duration: "约 2 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "确认设备在线",
                "文件传输需要一台已授权的设备。先看顶栏是否显示设备名称。",
                Some("device_select"),
                &[],
                None,
                GuideTarget::TopbarDevice,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "顶栏显示「未连接设备」",
                    "先完成 USB 或无线连接。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "打开文件传输",
                "点侧栏「文件传输」。",
                Some("files_nav"),
                &[],
                Some(GuidePage::Files),
                GuideTarget::Nav(GuidePage::Files),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "进入目标目录",
                "在顶部路径栏输入目录（例如 /sdcard/Download）后回车，进入要操作的文件夹。",
                Some("files_path"),
                &["已经进入目标目录"],
                Some(GuidePage::Files),
                GuideTarget::FilesList,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "目录打不开",
                    "确认路径存在且有权限；系统目录可能需要 root。",
                    None,
                )],
            ),
            GuideStep::new(
                "上传或下载",
                "点上传选择电脑文件发到设备；在文件行点下载把设备文件保存到电脑。",
                Some("files_transfer"),
                &["文件已经开始传输"],
                Some(GuidePage::Files),
                GuideTarget::FilesUpload,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "传输失败",
                    "检查设备空间与路径权限；大文件保持设备连接稳定。",
                    None,
                )],
            ),
            GuideStep::new(
                "确认结果",
                "传输完成后刷新列表，确认文件大小和修改时间。",
                Some("files_done"),
                &["文件已出现在目标目录"],
                Some(GuidePage::Files),
                GuideTarget::FilesList,
                GuideGate::None,
                NO_STUCK,
            ),
        ],
    },
    Guide {
        id: "remote",
        title: "在电脑上操作设备屏幕",
        summary: "Android 专用：应用内实时镜像，鼠标点击和拖动。",
        category: GuideCategory::Advanced,
        platforms: Platforms::ANDROID,
        keywords: &[
            "远程",
            "远程控制",
            "投屏",
            "镜像",
            "scrcpy",
            "鼠标",
            "操作屏幕",
            "控制",
        ],
        duration: "约 2 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "确认 Android 设备在线",
                "远程控制仅支持 Android，并且需要一台已授权设备。",
                Some("device_select"),
                &[],
                None,
                GuideTarget::TopbarDevice,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "没有设备",
                    "先完成 USB 或无线连接。",
                    Some(GuideAction::OpenPage(GuidePage::Connect)),
                )],
            ),
            GuideStep::new(
                "打开远程控制",
                "点侧栏「远程控制」。应用会推送并启动内置 scrcpy-server。",
                Some("remote_nav"),
                &[],
                Some(GuidePage::Remote),
                GuideTarget::Nav(GuidePage::Remote),
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "等待第一帧",
                "首次启动需要几秒推送 server 并建立转发通道；画面出现前不要反复切页。",
                Some("remote_start"),
                &["已经看到设备画面"],
                Some(GuidePage::Remote),
                GuideTarget::RemoteStart,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "一直黑屏",
                    "确认设备已解锁且授权；使用 release 构建可获得更好的帧率。",
                    None,
                )],
            ),
            GuideStep::new(
                "用鼠标操作",
                "点击等于触摸，拖动等于滑动；底部按钮可发送返回、主页、最近任务和电源键。",
                Some("remote_control"),
                &["已经完成一次点击或滑动"],
                Some(GuidePage::Remote),
                GuideTarget::RemoteStart,
                GuideGate::DeviceSelected,
                &[StuckHelp::new(
                    "操作有延迟",
                    "画面经过编码、转发和解码；使用 release 构建或改用 USB 连接。",
                    None,
                )],
            ),
            GuideStep::new(
                "离开页面",
                "离开远程控制会自动停止 server 并清理端口转发，下次进入重新启动。",
                Some("remote_done"),
                &[],
                Some(GuidePage::Remote),
                GuideTarget::Nav(GuidePage::Remote),
                GuideGate::None,
                NO_STUCK,
            ),
        ],
    },
    Guide {
        id: "not_connecting",
        title: "什么都连不上：3 分钟排查",
        summary: "按顺序排除授权、USB、驱动、无线与 ADB。",
        category: GuideCategory::Troubleshoot,
        platforms: Platforms::BOTH,
        keywords: &[
            "连不上",
            "连接失败",
            "没反应",
            "列表为空",
            "未授权",
            "离线",
            "识别不了",
            "adb",
            "驱动",
            "无线",
            "排查",
            "卡住",
            "失败",
        ],
        duration: "约 3 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "先看设备状态",
                "打开连接页，看设备列表里的状态：已连接 / 未授权 / 离线 / 列表为空。",
                Some("troubleshoot_states"),
                &["已确认当前状态"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "如果显示「未授权」",
                "在手表上点「允许」。没有弹窗就重新插拔，或撤销 USB 调试授权后重试。",
                Some("authorize"),
                &["设备状态变为「已连接」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::None,
                &[StuckHelp::new(
                    "一直不弹授权窗",
                    "换线换口；在开发者选项里点「撤销 USB 调试授权」，再重新插拔。",
                    None,
                )],
            ),
            GuideStep::new(
                "如果列表为空",
                "换数据线和 USB 口；Windows 安装内置 USB 驱动；确认 USB 调试已打开。",
                Some("driver_problem"),
                &["设备出现在列表"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDriverCard,
                GuideGate::None,
                &[StuckHelp::new(
                    "换线也没有用",
                    "换一台电脑测试，排除手表接口或线材问题。",
                    None,
                )],
            ),
            GuideStep::new(
                "如果显示「离线」",
                "无线：确认电脑和手表同一网络、地址和端口正确。USB：拔插或重启 adb 服务。",
                Some("offline"),
                &["设备状态恢复「已连接」"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::None,
                &[StuckHelp::new(
                    "无线一直离线",
                    "重新配对获取新的连接端口；确认路由器没有开启 AP 隔离。",
                    None,
                )],
            ),
            GuideStep::new(
                "如果 ADB 未就绪",
                "在设置里指定 adb 路径，或点「重新检测」。内置 ADB 首次启动需要解包时间。",
                Some("adb_settings"),
                &["ADB 显示已就绪"],
                Some(GuidePage::Settings),
                GuideTarget::SettingsAdbPath,
                GuideGate::None,
                &[StuckHelp::new(
                    "指定路径后仍然失败",
                    "确认选中的是 adb 可执行文件，而不是目录；macOS/Linux 需要可执行权限。",
                    Some(GuideAction::OpenPage(GuidePage::Settings)),
                )],
            ),
            GuideStep::new(
                "最后再结束调试进程",
                "在设置里结束 adb / hdc 服务，再回连接页重新检测；这会清掉卡住的旧连接。",
                Some("adb_restart"),
                &["重新检测后设备出现"],
                Some(GuidePage::Settings),
                GuideTarget::SettingsKillServer,
                GuideGate::None,
                &[StuckHelp::new(
                    "还是不行",
                    "重启电脑和设备；保留错误提示，换线换口排除硬件问题。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
        ],
    },
    Guide {
        id: "harmony_connect",
        title: "连接 HarmonyOS 设备",
        summary: "开启调试 → 切换 hdc 工作区 → 选中设备。",
        category: GuideCategory::Connect,
        platforms: Platforms::HARMONY,
        keywords: &[
            "鸿蒙",
            "harmonyos",
            "openharmony",
            "hdc",
            "连接",
            "无线",
            "tconn",
            "tmode",
            "5555",
            "开发者模式",
            "usb调试",
            "设备",
        ],
        duration: "约 3 分钟",
        tutorial: true,
        steps: &[
            GuideStep::new(
                "打开开发者模式与 USB 调试",
                "设备：设置 → 关于本机 → 连点版本号；然后 系统和更新 → 开发者选项 → 打开 USB 调试。",
                Some("dev_options"),
                &["USB 调试已打开"],
                None,
                GuideTarget::None,
                GuideGate::None,
                &[StuckHelp::new(
                    "没有开发者选项",
                    "确认连点的是「版本号」；鸿蒙设备通常在「关于本机」里。",
                    None,
                )],
            ),
            GuideStep::new(
                "切换到 HarmonyOS 工作区",
                "连接页顶部把工作区切到「HarmonyOS（hdc）」。",
                Some("harmony_switch"),
                &["顶栏或连接页显示 HarmonyOS"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectTransportSwitch,
                GuideGate::None,
                NO_STUCK,
            ),
            GuideStep::new(
                "确认 hdc 可用",
                "连接页的 hdc 卡片应显示「hdc 已就绪」。未找到时在设置里指定路径。",
                Some("hdc_status"),
                &["hdc 已就绪"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectHdcCard,
                GuideGate::AdbReady,
                &[StuckHelp::new(
                    "未找到 hdc",
                    "使用 DevEco Studio 的 toolchains/hdc，或在设置里指定 hdc 可执行文件。",
                    Some(GuideAction::OpenPage(GuidePage::Settings)),
                )],
            ),
            GuideStep::new(
                "连接设备",
                "USB 直接插线并在设备上允许调试；无线可在连接页开启无线调试，或执行 hdc tconn IP:5555。",
                Some("harmony_connect"),
                &["设备出现在列表"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::AdbReady,
                &[StuckHelp::new(
                    "tconn 失败",
                    "确认电脑和设备在同一网段、端口正确；先在设备上打开无线调试。",
                    Some(GuideAction::RefreshDevices),
                )],
            ),
            GuideStep::new(
                "选中设备",
                "点列表中的设备，状态变为在线后即可使用其他页面。",
                Some("device_select"),
                &["设备状态在线"],
                Some(GuidePage::Connect),
                GuideTarget::ConnectDeviceList,
                GuideGate::DeviceSelected,
                NO_STUCK,
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // 一步到位的快捷操作
    // -----------------------------------------------------------------------
    Guide {
        id: "op_export_apk",
        title: "导出已安装应用的 APK",
        summary: "把设备上的应用安装包保存到电脑。",
        category: GuideCategory::Install,
        platforms: Platforms::ANDROID,
        keywords: &["导出", "apk", "备份", "应用", "安装包"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "在应用管理中导出",
            "打开应用管理，找到应用后在操作菜单里选择「导出 APK」。",
            Some("install_result"),
            &[],
            Some(GuidePage::Apps),
            GuideTarget::Nav(GuidePage::Apps),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_app_list",
        title: "查看、搜索和卸载应用",
        summary: "应用列表、系统/用户筛选、启动与卸载。",
        category: GuideCategory::Install,
        platforms: Platforms::BOTH,
        keywords: &["应用列表", "搜索应用", "卸载", "启动", "停止", "清缓存"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开应用管理",
            "在应用管理里筛选用户/系统应用，搜索名称或包名，再对单个应用执行启动、停止或卸载。",
            Some("apps_nav"),
            &[],
            Some(GuidePage::Apps),
            GuideTarget::Nav(GuidePage::Apps),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_logs",
        title: "查看与过滤日志",
        summary: "Logcat / HiLog，按级别和关键字筛选并导出。",
        category: GuideCategory::Advanced,
        platforms: Platforms::BOTH,
        keywords: &["日志", "logcat", "hilog", "过滤", "导出日志", "报错"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开日志查看器",
            "选择级别和关键字过滤；需要发给别人时点导出日志。",
            Some("logs"),
            &[],
            Some(GuidePage::Logs),
            GuideTarget::Nav(GuidePage::Logs),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_shell",
        title: "执行 ADB / hdc 命令",
        summary: "在设备 shell 里直接运行命令并保留历史。",
        category: GuideCategory::Advanced,
        platforms: Platforms::BOTH,
        keywords: &["终端", "shell", "命令", "adb shell", "hdc shell", "wm"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开终端",
            "输入命令后回车；命令历史会保留，方便重复执行。",
            Some("terminal"),
            &[],
            Some(GuidePage::Terminal),
            GuideTarget::Nav(GuidePage::Terminal),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_reboot",
        title: "重启设备或进入 Recovery",
        summary: "普通重启、Recovery、Bootloader、Fastbootd、EDL。",
        category: GuideCategory::Advanced,
        platforms: Platforms::BOTH,
        keywords: &["重启", "recovery", "bootloader", "fastboot", "edl", "关机"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开概览页的电源操作",
            "在概览页找到重启/电源入口，选择目标模式；危险模式会再次确认。",
            Some("reboot"),
            &[],
            Some(GuidePage::Overview),
            GuideTarget::Nav(GuidePage::Overview),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_input_method",
        title: "切换输入法",
        summary: "查看设备上的输入法并切换默认输入法。",
        category: GuideCategory::Advanced,
        platforms: Platforms::ANDROID,
        keywords: &["输入法", "ime", "键盘", "切换输入法"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开调试工具",
            "在「调试工具」的输入法区域选择目标输入法并应用。",
            Some("input_method"),
            &[],
            Some(GuidePage::Tools),
            GuideTarget::Nav(GuidePage::Tools),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_appops",
        title: "查看和管理 AppOps 权限",
        summary: "按应用查看权限项并切换允许/忽略/拒绝。",
        category: GuideCategory::Advanced,
        platforms: Platforms::ANDROID,
        keywords: &["权限", "appops", "授权", "拒绝", "忽略"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开调试工具",
            "在「调试工具」选择应用，搜索 op 后设置模式。",
            Some("appops"),
            &[],
            Some(GuidePage::Tools),
            GuideTarget::Nav(GuidePage::Tools),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_port_forward",
        title: "设置端口转发",
        summary: "HarmonyOS 的 hdc fport / rport 增删与刷新。",
        category: GuideCategory::Advanced,
        platforms: Platforms::HARMONY,
        keywords: &["端口转发", "fport", "rport", "forward", "端口"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开连接页的端口转发",
            "在 HarmonyOS 连接页的端口转发卡片里填写本地/远端端口，然后添加。",
            Some("forward"),
            &[],
            Some(GuidePage::Connect),
            GuideTarget::ConnectDeviceList,
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_signing",
        title: "HAP 签名与安装",
        summary: "华为账号登录、证书、Profile、ACL 与签名安装。",
        category: GuideCategory::Install,
        platforms: Platforms::HARMONY,
        keywords: &[
            "签名",
            "hap",
            "profile",
            "证书",
            "acl",
            "华为账号",
            "9568322",
        ],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开签名与安装",
            "登录华为开发者账号或导入证书/Profile，选择 HAP 后签名并安装。",
            Some("signing"),
            &[],
            Some(GuidePage::Signing),
            GuideTarget::Nav(GuidePage::Signing),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_save_dir",
        title: "修改截图/录屏保存目录",
        summary: "指定截图、录屏和导出文件的落盘位置。",
        category: GuideCategory::Files,
        platforms: Platforms::BOTH,
        keywords: &["保存目录", "截图目录", "录屏目录", "路径", "设置"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开设置",
            "在设置里找到保存目录，选择新文件夹。",
            Some("save_dir"),
            &[],
            Some(GuidePage::Settings),
            GuideTarget::SettingsSaveDir,
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_kill_server",
        title: "结束 adb / hdc 调试进程",
        summary: "清理卡住的旧连接，下次操作时按需重启。",
        category: GuideCategory::Troubleshoot,
        platforms: Platforms::BOTH,
        keywords: &["结束进程", "kill-server", "重启adb", "adb", "hdc", "卡住"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开设置的调试进程卡片",
            "点结束 adb / hdc 服务，然后回连接页重新检测设备。",
            Some("adb_restart"),
            &[],
            Some(GuidePage::Settings),
            GuideTarget::SettingsKillServer,
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_device_info",
        title: "查看设备信息与电量",
        summary: "型号、系统、电量、存储、内存与运行时间。",
        category: GuideCategory::Advanced,
        platforms: Platforms::BOTH,
        keywords: &[
            "设备信息",
            "电量",
            "存储",
            "内存",
            "型号",
            "系统版本",
            "概览",
        ],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开概览",
            "概览页展示设备状态；只读字段点击即可复制。",
            Some("overview"),
            &[],
            Some(GuidePage::Overview),
            GuideTarget::Nav(GuidePage::Overview),
            GuideGate::None,
            NO_STUCK,
        )],
    },
    Guide {
        id: "op_update",
        title: "检查 RareBox 更新",
        summary: "查看新版本、更新说明与强制更新提示。",
        category: GuideCategory::Advanced,
        platforms: Platforms::BOTH,
        keywords: &["更新", "新版本", "升级", "检查更新"],
        duration: "一步",
        tutorial: false,
        steps: &[GuideStep::new(
            "打开设置 → 关于",
            "在关于卡片里检查更新；有新版本会打开更新弹窗。",
            Some("update"),
            &[],
            Some(GuidePage::Settings),
            GuideTarget::Nav(GuidePage::Settings),
            GuideGate::None,
            NO_STUCK,
        )],
    },
];

// ---------------------------------------------------------------------------
// 检索
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub guide: &'static Guide,
    pub score: i32,
    /// 用于界面展示的命中词，例如「安装」「APK」。
    pub matched: Vec<String>,
}

const SYNONYMS: &[(&str, &[&str])] = &[
    ("dpi", &["密度", "显示大小", "字太小", "字太大"]),
    ("字太小", &["dpi", "密度", "显示大小"]),
    ("字太大", &["dpi", "密度", "显示大小"]),
    ("显示太大", &["dpi", "密度"]),
    ("显示太小", &["dpi", "密度"]),
    ("黑屏", &["dpi", "密度", "恢复", "wm density"]),
    ("投屏", &["远程控制", "镜像", "scrcpy"]),
    ("远程", &["远程控制", "投屏", "scrcpy"]),
    ("装app", &["安装", "apk"]),
    ("装应用", &["安装", "apk", "hap"]),
    ("安装", &["apk", "hap", "install"]),
    ("连不上", &["未授权", "离线", "驱动", "adb", "无线", "排查"]),
    ("连接失败", &["未授权", "离线", "驱动", "排查"]),
    ("没反应", &["未授权", "驱动", "adb", "排查"]),
    ("配对", &["无线调试", "pair", "无线"]),
    ("无线", &["配对", "wifi", "tcpip"]),
    ("截图", &["屏幕", "保存"]),
    ("录屏", &["录制", "屏幕"]),
    ("传文件", &["文件传输", "上传", "下载"]),
    ("驱动", &["windows", "usb", "设备管理器"]),
    ("鸿蒙", &["harmonyos", "hdc", "hap"]),
    ("手表", &["wear os", "android"]),
    ("日志", &["logcat", "hilog"]),
    ("权限", &["appops", "授权"]),
    ("签名", &["hap", "profile", "证书"]),
];

/// 用户口语里常见但没有区分度的词，检索时忽略。
const STOPWORDS: &[&str] = &[
    "手表", "设备", "电脑", "手机", "怎么", "如何", "帮我", "我要", "想要", "一下", "一个", "可以",
    "能用", "使用", "进行", "个", "的", "了", "吗", "呢",
];

/// 常见意图 → 首选教程。命中后给该教程一个明显的排序加分，
/// 避免「给手表装 app」被「手表」这种通用词带到连接教程。
///
/// 顺序有意义：越具体的意图越靠前，先命中先返回。
const INTENTS: &[(
    Option<&'static str>,
    Option<&'static str>,
    &'static [&'static str],
)] = &[
    (Some("op_appops"), None, &["appops", "应用权限", "权限管理"]),
    (
        Some("op_export_apk"),
        None,
        &["导出apk", "导出", "备份应用", "备份"],
    ),
    (
        Some("op_save_dir"),
        Some("op_save_dir"),
        &["保存目录", "截图目录", "录屏目录"],
    ),
    (
        Some("op_kill_server"),
        Some("op_kill_server"),
        &["结束进程", "kill-server", "重启adb", "结束adb", "结束hdc"],
    ),
    (
        Some("op_update"),
        Some("op_update"),
        &["检查更新", "新版本", "升级"],
    ),
    (
        Some("op_reboot"),
        Some("op_reboot"),
        &["重启", "recovery", "bootloader", "fastboot", "edl", "关机"],
    ),
    (
        Some("op_shell"),
        Some("op_shell"),
        &[
            "终端",
            "shell",
            "命令行",
            "adb shell",
            "hdc shell",
            "wm density",
        ],
    ),
    (
        Some("op_logs"),
        Some("op_logs"),
        &["日志", "logcat", "hilog", "报错"],
    ),
    (
        None,
        Some("op_signing"),
        &["签名", "profile", "证书", "acl"],
    ),
    (
        Some("screenshot"),
        Some("screenshot"),
        &["截图", "录屏", "录像"],
    ),
    (
        Some("files"),
        Some("files"),
        &["传文件", "文件传输", "上传", "下载"],
    ),
    (
        Some("remote"),
        None,
        &["投屏", "远程控制", "镜像", "scrcpy", "远程"],
    ),
    (
        Some("dpi"),
        None,
        &[
            "dpi",
            "字太小",
            "字太大",
            "显示太大",
            "显示太小",
            "密度",
            "字体大小",
        ],
    ),
    (
        Some("driver"),
        Some("driver"),
        &["驱动", "设备管理器", "感叹号", "认不出", "识别不了"],
    ),
    (
        Some("not_connecting"),
        Some("not_connecting"),
        &[
            "连不上",
            "连接失败",
            "没反应",
            "离线",
            "未授权",
            "列表为空",
            "排查",
        ],
    ),
    (
        Some("connect_wireless"),
        None,
        &["配对", "无线调试", "tcpip", "无线连"],
    ),
    (
        None,
        Some("harmony_connect"),
        &["鸿蒙", "harmonyos", "openharmony", "hdc", "连接鸿蒙"],
    ),
    (
        Some("op_device_info"),
        Some("op_device_info"),
        &["设备信息", "电量", "型号", "概览"],
    ),
    (
        Some("install_apk"),
        Some("install_hap"),
        &[
            "装app",
            "安装app",
            "装应用",
            "安装应用",
            "装软件",
            "安装软件",
            "装一个",
            "安装包",
            "安装",
            "apk",
            "hap",
            "app",
        ],
    ),
];

fn is_stopword(term: &str) -> bool {
    STOPWORDS.contains(&term)
}

/// 返回当前查询最明确的意图目标；目标在当前平台不可用时继续找下一个。
fn intent_id(query: &str, scope: GuideScope, windows: bool) -> Option<&'static str> {
    for (android, harmony, triggers) in INTENTS {
        let matches = triggers
            .iter()
            .any(|trigger| query.contains(compact(trigger).as_str()));
        if !matches {
            continue;
        }
        let candidate = match scope {
            GuideScope::Android => *android,
            GuideScope::Harmony => *harmony,
        };
        let Some(id) = candidate else {
            continue;
        };
        if find(id).is_some_and(|guide| guide.platforms.supports(scope, windows)) {
            return Some(id);
        }
    }
    None
}

pub fn find(id: &str) -> Option<&'static Guide> {
    GUIDES.iter().find(|guide| guide.id == id)
}

pub fn tutorials(scope: GuideScope, windows: bool) -> impl Iterator<Item = &'static Guide> {
    GUIDES
        .iter()
        .filter(move |guide| guide.tutorial && guide.platforms.supports(scope, windows))
}

pub fn quick_operations(scope: GuideScope, windows: bool) -> impl Iterator<Item = &'static Guide> {
    GUIDES
        .iter()
        .filter(move |guide| !guide.tutorial && guide.platforms.supports(scope, windows))
}

/// 页面上的建议提问，按平台给出不同入口。
pub fn suggestions(scope: GuideScope, windows: bool) -> Vec<&'static str> {
    let mut out = vec!["安装应用", "调整 DPI", "连不上设备", "无线配对"];
    if scope == GuideScope::Harmony {
        out = vec!["安装 HAP", "连接鸿蒙设备", "签名安装", "找不到 hdc"];
    }
    if windows {
        out.push("安装 USB 驱动");
    }
    out.push("截图");
    out.push("传文件");
    out
}

/// 本地检索：标题/关键词/摘要/步骤文本的加权词面匹配。
///
/// 返回按分数降序排列的教程与快捷操作。没有命中时返回空 Vec，界面负责
/// 给出建议词，绝不编造步骤。
pub fn search(query: &str, scope: GuideScope, windows: bool) -> Vec<SearchHit> {
    let query_compact = compact(query);
    if query_compact.is_empty() {
        return Vec::new();
    }

    let mut terms = tokenize(&query_compact);
    terms.retain(|term| !is_stopword(term));
    // 同义词只作为补充词，权重低于用户原词。
    let mut synonyms: Vec<String> = Vec::new();
    for (key, aliases) in SYNONYMS {
        let key_compact = compact(key);
        if query_compact.contains(&key_compact) || key_compact.contains(&query_compact) {
            for alias in *aliases {
                for term in tokenize(&compact(alias)) {
                    if !terms.contains(&term) && !synonyms.contains(&term) {
                        synonyms.push(term);
                    }
                }
            }
        }
    }
    let intent = intent_id(&query_compact, scope, windows);
    if terms.is_empty() && synonyms.is_empty() && intent.is_none() {
        return Vec::new();
    }
    // 同一查询内去重，避免重复计分。
    terms.sort();
    terms.dedup();
    synonyms.sort();
    synonyms.dedup();

    let mut hits: Vec<SearchHit> = GUIDES
        .iter()
        .filter(|guide| guide.platforms.supports(scope, windows))
        .filter_map(|guide| {
            let score = score_guide(guide, &query_compact, &terms, &synonyms, intent);
            if score < 3 {
                return None;
            }
            Some(SearchHit {
                guide,
                score,
                matched: matched_terms(guide, &query_compact, &terms, &synonyms),
            })
        })
        .collect();

    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.guide.title.cmp(right.guide.title))
    });
    hits.truncate(5);
    hits
}

fn score_guide(
    guide: &Guide,
    query: &str,
    terms: &[String],
    synonyms: &[String],
    intent: Option<&'static str>,
) -> i32 {
    let mut score = 0;
    if intent == Some(guide.id) {
        score += 60;
    }
    score += field_score(&compact(guide.title), query, terms, 8);
    score += field_score(&compact(&guide.keywords.join(" ")), query, terms, 7);
    score += field_score(&compact(guide.summary), query, terms, 3);
    score += field_score(&compact(guide.category.label()), query, terms, 2);

    let mut step_title = String::new();
    let mut step_body = String::new();
    for step in guide.steps {
        step_title.push_str(step.title);
        step_title.push(' ');
        step_body.push_str(step.body);
        step_body.push(' ');
        for item in step.checklist {
            step_body.push_str(item);
            step_body.push(' ');
        }
        for help in step.stuck {
            step_body.push_str(help.title);
            step_body.push(' ');
            step_body.push_str(help.detail);
            step_body.push(' ');
        }
    }
    score += field_score(&compact(&step_title), query, terms, 4);
    score += field_score(&compact(&step_body), query, terms, 1);

    // 同义词只用于召回：命中标题/关键词时给低权重加分。
    let headline = compact(&format!("{} {}", guide.title, guide.keywords.join(" ")));
    for term in synonyms {
        if headline.contains(term.as_str()) {
            score += 1;
        }
    }
    score
}

fn field_score(field: &str, query: &str, terms: &[String], weight: i32) -> i32 {
    if field.is_empty() {
        return 0;
    }
    let mut score = 0;
    // 完整短语命中，权重最高。
    if query.chars().count() >= 2 && field.contains(query) {
        score += weight * 3;
    }
    for term in terms {
        if field.contains(term.as_str()) {
            score += weight;
        }
    }
    score
}

fn matched_terms(guide: &Guide, query: &str, terms: &[String], synonyms: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let candidates: Vec<&str> = guide
        .keywords
        .iter()
        .copied()
        .chain(std::iter::once(guide.title))
        .collect();
    for candidate in candidates {
        let candidate_compact = compact(candidate);
        if candidate_compact.is_empty() {
            continue;
        }
        let hit = query.contains(&candidate_compact)
            || terms
                .iter()
                .any(|term| candidate_compact.contains(term.as_str()))
            || synonyms
                .iter()
                .any(|term| candidate_compact.contains(term.as_str()));
        if hit {
            let label = candidate.trim();
            if !label.is_empty() && !out.iter().any(|existing| existing == label) {
                out.push(label.to_string());
            }
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

fn compact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars().flat_map(char::to_lowercase) {
        if is_cjk(ch) || ch.is_ascii_alphanumeric() {
            out.push(ch);
        }
    }
    out
}

fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if is_cjk(ch) {
            let start = index;
            while index < chars.len() && is_cjk(chars[index]) {
                index += 1;
            }
            let run: String = chars[start..index].iter().collect();
            let run_chars: Vec<char> = run.chars().collect();
            if run_chars.len() == 1 {
                out.push(run);
            } else {
                if run_chars.len() <= 8 {
                    out.push(run.clone());
                }
                for window in run_chars.windows(2) {
                    out.push(window.iter().collect());
                }
            }
        } else if ch.is_ascii_alphanumeric() {
            let start = index;
            while index < chars.len() && chars[index].is_ascii_alphanumeric() {
                index += 1;
            }
            let token: String = chars[start..index].iter().collect();
            out.push(token.to_ascii_lowercase());
        } else {
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_id(query: &str, scope: GuideScope, windows: bool) -> Option<&'static str> {
        search(query, scope, windows)
            .into_iter()
            .next()
            .map(|hit| hit.guide.id)
    }

    #[test]
    fn chinese_install_queries_hit_apk() {
        assert_eq!(
            first_id("给手表装一个app", GuideScope::Android, false),
            Some("install_apk")
        );
        assert_eq!(
            first_id("安装应用", GuideScope::Android, false),
            Some("install_apk")
        );
        assert_eq!(
            first_id("安装 HAP", GuideScope::Harmony, false),
            Some("install_hap")
        );
    }

    #[test]
    fn dpi_aliases_hit_display_guide() {
        for query in ["dpi", "字太小", "显示太大", "怎么调密度"] {
            assert_eq!(
                first_id(query, GuideScope::Android, true),
                Some("dpi"),
                "query: {query}"
            );
        }
    }

    #[test]
    fn troubleshooting_queries_prefer_diagnostics() {
        assert_eq!(
            first_id("连不上手表", GuideScope::Android, true),
            Some("not_connecting")
        );
        assert_eq!(
            first_id("电脑认不出设备", GuideScope::Android, true),
            Some("driver")
        );
    }

    #[test]
    fn synonym_queries_find_remote_and_pairing() {
        assert_eq!(first_id("投屏", GuideScope::Android, false), Some("remote"));
        assert_eq!(
            first_id("无线配对", GuideScope::Android, false),
            Some("connect_wireless")
        );
    }

    #[test]
    fn platform_filter_hides_android_only_content() {
        let harmony = search("安装应用", GuideScope::Harmony, false);
        assert!(
            harmony.iter().all(|hit| hit.guide.platforms.harmony),
            "HarmonyOS 搜索不应返回仅 Android 的教程"
        );
        assert!(
            search("安装", GuideScope::Harmony, false)
                .iter()
                .any(|hit| hit.guide.id == "install_hap")
        );
        assert!(
            search("dpi", GuideScope::Harmony, false)
                .iter()
                .all(|hit| hit.guide.id != "dpi")
        );
    }

    #[test]
    fn windows_only_guides_are_filtered_by_host() {
        assert!(
            search("驱动", GuideScope::Android, false)
                .iter()
                .all(|hit| hit.guide.id != "driver")
        );
        assert!(
            search("驱动", GuideScope::Android, true)
                .iter()
                .any(|hit| hit.guide.id == "driver")
        );
    }

    #[test]
    fn empty_or_noise_queries_return_nothing() {
        assert!(search("", GuideScope::Android, true).is_empty());
        assert!(search("   ", GuideScope::Android, true).is_empty());
        assert!(search("qwertyuiop", GuideScope::Android, true).is_empty());
        assert!(search("!!!", GuideScope::Android, true).is_empty());
    }

    #[test]
    fn hits_are_sorted_and_limited() {
        let hits = search("连接", GuideScope::Android, true);
        assert!(!hits.is_empty());
        assert!(hits.len() <= 5);
        assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));
    }

    #[test]
    fn all_guides_are_addressable_and_nonempty() {
        for guide in GUIDES {
            assert_eq!(find(guide.id).map(|found| found.id), Some(guide.id));
            assert!(!guide.steps.is_empty(), "{} 没有步骤", guide.id);
            assert!(!guide.keywords.is_empty(), "{} 没有关键词", guide.id);
        }
    }

    #[test]
    fn device_dependent_tutorials_are_marked() {
        for id in [
            "install_apk",
            "install_hap",
            "dpi",
            "screenshot",
            "files",
            "remote",
            "op_export_apk",
            "op_port_forward",
        ] {
            let guide = find(id).expect("guide exists");
            assert!(guide.requires_device(), "{id} 应该要求设备");
        }
        for id in [
            "connect_usb",
            "connect_wireless",
            "connect_wlan",
            "connect_choose",
            "harmony_connect",
            "driver",
            "not_connecting",
            "op_update",
        ] {
            let guide = find(id).expect("guide exists");
            assert!(!guide.requires_device(), "{id} 不应该要求设备");
        }
    }

    #[test]
    fn connect_guides_all_have_art() {
        for id in [
            "connect_choose",
            "connect_usb",
            "connect_wireless",
            "connect_wlan",
        ] {
            let guide = find(id).expect("guide exists");
            for step in guide.steps {
                assert!(
                    step.art.is_some(),
                    "{} 的步骤「{}」缺少配图",
                    id,
                    step.title
                );
            }
        }
    }

    #[test]
    fn choice_steps_point_to_existing_guides() {
        let mut choice_count = 0;
        for guide in GUIDES {
            for step in guide.steps {
                for choice in step.choices {
                    choice_count += 1;
                    assert!(
                        find(choice.guide).is_some(),
                        "{} 的分支指向不存在的教程 {}",
                        guide.id,
                        choice.guide
                    );
                }
            }
        }
        assert!(choice_count >= 3, "连接方式选择至少要有 USB / 无线选项");
        let choose = find("connect_choose").expect("选择教程存在");
        assert!(
            choose.steps[0]
                .choices
                .iter()
                .any(|choice| choice.guide == "connect_wireless")
        );
        assert!(
            choose.steps[0]
                .choices
                .iter()
                .any(|choice| choice.guide == "connect_usb")
        );
    }

    #[test]
    fn short_install_query_returns_results() {
        let hits = search("装应用", GuideScope::Android, false);
        assert!(
            hits.iter().any(|hit| hit.guide.id == "install_apk"),
            "「装应用」应命中安装 APK，实际：{hits:?}"
        );
    }

    #[test]
    fn every_tutorial_step_has_a_title_and_body() {
        for guide in GUIDES.iter().filter(|guide| guide.tutorial) {
            for (index, step) in guide.steps.iter().enumerate() {
                assert!(
                    !step.title.trim().is_empty(),
                    "{} 第 {} 步没有标题",
                    guide.id,
                    index + 1
                );
                assert!(
                    !step.body.trim().is_empty(),
                    "{} 第 {} 步没有正文",
                    guide.id,
                    index + 1
                );
            }
        }
    }
}

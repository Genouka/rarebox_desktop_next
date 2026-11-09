//! Windows 上 ADB 所需的 USB 驱动（UniversalAdbDriver，源自 koush/UniversalAdbDriver）。
//!
//! ADB 本身能启动，但若 Wear OS / Android 设备未被系统识别为 USB 设备，ADB 就看不到它。
//! 这个内置驱动把 `android_winusb.inf` 对应的硬件 ID 绑定到系统自带的 `WinUSB.sys`，
//! 让设备以「Android ADB Interface」出现，从而被 `adb devices` 发现。
//!
//! 安装/卸载需要管理员权限：驱动要写入系统驱动存储，且需要信任随包提供的自签名目录文件。
//! 本程序本身无需以管理员运行——只有真正写入驱动存储的那一步会通过 UAC 提权执行
//! （仅提权这一个命令，不提权整个 GUI），与常规驱动安装器的行为一致。
//!
//! 本模块只在 Windows 上提供实质实现；其他平台一律返回「不支持」，调用方据此隐藏 UI。

#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

/// 驱动 INF 的文件名（与仓库保持一致）。
const INF_NAME: &str = "android_winusb.inf";

/// 从 `pnputil /enum-drivers` 的输出里找出 `android_winusb.inf` 对应的发布名（如 `oem12.inf`）。
///
/// `pnputil` 的标签会随系统语言本地化（中文是「已发布名称:」「原始名称:」），不能依赖英文前缀。
/// 这里只取每行最后一个形如 `*.inf` 的值：发布名（形如 `oemNN.inf`）一定出现在原始名
/// `android_winusb.inf` 之前，因此只要记住上一个 `oem*.inf`，遇到原始名时就返回它。与语言无关。
/// 纯函数，便于单元测试。
pub(crate) fn parse_published_name(text: &str) -> Option<String> {
    let mut last_inf: Option<String> = None;
    for line in text.lines() {
        // 每行的值都是末尾那个 token（如 `oem12.inf` / `android_winusb.inf`），与语言无关。
        let Some(value) = line.split_whitespace().last() else {
            continue;
        };
        if !value.to_ascii_lowercase().ends_with(".inf") {
            continue;
        }
        if value.eq_ignore_ascii_case(INF_NAME) {
            // 找到原始名：返回它前面最近的「发布名」（`oemNN.inf` 形态）。
            if let Some(published) = last_inf {
                let p = published.to_ascii_lowercase();
                if p.starts_with("oem") && p.ends_with(".inf") {
                    return Some(published);
                }
            }
        }
        last_inf = Some(value.to_string());
    }
    None
}

#[cfg(windows)]
mod imp {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{LazyLock, Mutex};

    use anyhow::{Context, Result, anyhow, bail};
    use zip::ZipArchive;

    use super::{INF_NAME, parse_published_name};

    /// 子进程不创建控制台窗口：应用是 GUI 程序，任何可见控制台都会以终端窗口形式弹出。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// 驱动是否已安装。Connect 页每次渲染都会查询，缓存后不再反复启动 `pnputil`。
    static INSTALLED: LazyLock<Mutex<Option<bool>>> = LazyLock::new(|| Mutex::new(None));

    fn installed_cache() -> std::sync::MutexGuard<'static, Option<bool>> {
        INSTALLED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 内置驱动压缩包，来自 UniversalAdbDriver 的 `usb_driver/` 目录。
    const DRIVER_ZIP: &[u8] = include_bytes!("../../../vendor/adb_driver.zip");

    /// 驱动在磁盘上展开后的目录：`<数据目录>/adb_driver`。
    fn driver_dir() -> PathBuf {
        crate::config::data_dir().join("adb_driver")
    }

    /// 确保内置驱动已展开到数据目录，返回 INF 的完整路径。
    pub fn ensure_extracted() -> Result<PathBuf> {
        let root = driver_dir();
        let inf = root.join(INF_NAME);
        if inf.exists() {
            return Ok(inf);
        }
        extract(&root)?;
        if !inf.exists() {
            bail!("内置驱动解包后缺少 {INF_NAME}");
        }
        Ok(inf)
    }

    /// 把内置 zip 解包到 `root`，过滤路径穿越，保留 amd64/i386 子目录。
    fn extract(root: &Path) -> Result<()> {
        std::fs::create_dir_all(root)
            .with_context(|| format!("无法创建驱动目录 {}", root.display()))?;
        let mut archive =
            ZipArchive::new(std::io::Cursor::new(DRIVER_ZIP)).context("内置驱动压缩包损坏")?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .with_context(|| format!("无法读取压缩包内第 {i} 项"))?;
            let raw = entry
                .enclosed_name()
                .ok_or_else(|| anyhow!("压缩包内包含非法路径"))?;
            let dest = root.join(&raw);
            // 再次确认目标确实在 root 之下，防御压缩包穿越。
            if !dest.starts_with(root) {
                bail!("压缩包内路径 {} 试图逃出目标目录", raw.display());
            }
            if entry.is_dir() {
                std::fs::create_dir_all(&dest)
                    .with_context(|| format!("无法创建目录 {}", dest.display()))?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("无法创建目录 {}", parent.display()))?;
            }
            let mut out = std::fs::File::create(&dest)
                .with_context(|| format!("无法写出文件 {}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)
                .with_context(|| format!("无法写出文件 {}", dest.display()))?;
        }
        Ok(())
    }

    /// 当前进程是否以管理员身份运行（结果缓存，避免每次都起 PowerShell）。
    pub fn is_elevated() -> bool {
        static ELEVATED: LazyLock<bool> = LazyLock::new(|| {
            let out = Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "[Security.Principal.WindowsIdentity]::GetCurrent().Groups \
                     -contains 'S-1-5-32-544'",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            match out {
                Ok(o) => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    s.trim().eq_ignore_ascii_case("true")
                }
                // 拿不到信息时保守地认为「不是管理员」，让提权路径兜底。
                Err(_) => false,
            }
        });
        *ELEVATED
    }

    /// 驱动是否已注册进系统驱动存储（无需管理员即可枚举）。
    ///
    /// 结果缓存在进程内：Connect 页每次重绘都会调用本函数，若每次都启动 `pnputil`
    /// 不仅慢，还会让子进程控制台窗口不断闪现。安装/卸载成功后会刷新缓存。
    pub fn is_installed() -> bool {
        let mut cached = installed_cache();
        if let Some(value) = *cached {
            return value;
        }
        let value = query_published_name().is_some();
        *cached = Some(value);
        value
    }

    /// 通过 `pnputil /enum-drivers` 枚举已注册驱动，返回 `android_winusb.inf` 的发布名。
    fn installed_published_name() -> Option<String> {
        query_published_name()
    }

    fn query_published_name() -> Option<String> {
        let out = Command::new("pnputil")
            .args(["/enum-drivers"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        parse_published_name(&String::from_utf8_lossy(&out.stdout))
    }

    /// 把 `pnputil` 子命令以管理员身份执行（UAC 提权），成功时返回。
    ///
    /// 非管理员进程会通过 `Start-Process -Verb RunAs` 弹出 UAC 并仅提权这一个命令；
    /// 已经是管理员时直接执行。脚本输出与退出码都记录到临时日志，便于报错。
    ///
    /// 无论是否提权，都把内部 `pnputil` 的退出码透传出来，这样命令失败时上层能收到。
    fn run_pnputil_elevated(subcommand: &[&str]) -> Result<()> {
        let root = driver_dir();
        std::fs::create_dir_all(&root).ok();
        let log = root.join("last_operation.log");

        // PowerShell 脚本：执行 pnputil 并把输出与退出码写入日志。
        let args: Vec<String> = subcommand
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect();
        let script = format!(
            "pnputil {args} *> '{log}'; exit $LASTEXITCODE",
            args = args.join(" "),
            log = log.display()
        );
        let script_path = root.join("op.ps1");
        {
            let mut f = std::fs::File::create(&script_path)
                .with_context(|| format!("无法写脚本 {}", script_path.display()))?;
            f.write_all(script.as_bytes()).context("无法写脚本内容")?;
        }

        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ]);
        if is_elevated() {
            // 已是管理员：直接运行脚本，退出码自然透传。
            cmd.arg("-File").arg(&script_path);
        } else {
            // 非管理员：通过 UAC 仅提权这一个 PowerShell 进程，并等待结束；
            // 用 $p.ExitCode 把内部 pnputil 的退出码透传给外层，避免失败被吞掉。
            let ps = format!(
                "$p = Start-Process -FilePath powershell.exe -ArgumentList \
                 '-NoProfile','-ExecutionPolicy','Bypass','-File','{}' \
                 -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode",
                script_path.display()
            );
            cmd.arg("-Command").arg(ps);
        }
        // 用 output() 而不是 status()：捕获输出会重定向子进程 stdio，配合
        // CREATE_NO_WINDOW 才能避免 Windows 终端（conhost / Windows Terminal）
        // 为继承句柄的控制台程序新开窗口。
        let output = cmd
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .with_context(|| "无法启动提权后的 PowerShell（UAC 可能被取消）")?;
        if !output.status.success() {
            let detail = std::fs::read_to_string(&log).unwrap_or_default();
            bail!(
                "驱动操作失败（退出码 {code}）：{detail}",
                code = output.status.code().unwrap_or(-1),
                detail = detail.trim()
            );
        }
        Ok(())
    }

    /// 安装驱动：展开内置驱动，再用 `pnputil /add-driver` 注册进系统存储。
    pub fn install() -> Result<()> {
        let inf = ensure_extracted()?;
        let inf_str = inf.to_string_lossy().into_owned();
        // 使用 INF 绝对路径安装；/subdirs 让 pnputil 一并拷贝 amd64/i386 下的 DLL。
        let result = run_pnputil_elevated(&["/add-driver", &inf_str, "/subdirs"]);
        // 成功即已注册；失败时清空缓存，下次重绘再按真实状态重新探测。
        *installed_cache() = if result.is_ok() { Some(true) } else { None };
        result
    }

    /// 卸载驱动：定位已注册的发布名，再用 `pnputil /delete-driver` 移除。
    pub fn uninstall() -> Result<()> {
        let result = match installed_published_name() {
            Some(published) => {
                run_pnputil_elevated(&["/delete-driver", &published, "/uninstall", "/subdirs"])
            }
            None => Ok(()),
        };
        *installed_cache() = match result {
            Ok(()) => Some(false),
            Err(_) => None,
        };
        result
    }
}

/// 非 Windows 平台的空实现，保证其它平台编译通过且 UI 自动隐藏。
#[cfg(not(windows))]
mod imp {
    use anyhow::Result;

    pub fn ensure_extracted() -> Result<std::path::PathBuf> {
        anyhow::bail!("驱动安装仅支持 Windows")
    }
    pub fn is_elevated() -> bool {
        false
    }
    pub fn is_installed() -> bool {
        false
    }
    pub fn install() -> Result<()> {
        anyhow::bail!("驱动安装仅支持 Windows")
    }
    pub fn uninstall() -> Result<()> {
        anyhow::bail!("驱动卸载仅支持 Windows")
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::parse_published_name;

    #[test]
    fn parses_published_name_for_known_inf() {
        let sample = "\
Driver packages:
   Published Name:    oem12.inf
   Original Name:     android_winusb.inf
   Provider Name:     Google, Inc.
   Class Name:        AndroidUsbDeviceClass
   Driver Version:    11.0.0.0
";
        assert_eq!(parse_published_name(sample).as_deref(), Some("oem12.inf"));
    }

    #[test]
    fn ignores_unrelated_drivers() {
        let sample = "\
   Published Name:    oem03.inf
   Original Name:     nvdia_display.inf
   Published Name:    oem04.inf
   Original Name:     usbser.inf
";
        assert_eq!(parse_published_name(sample), None);
    }

    #[test]
    fn matches_case_insensitively() {
        let sample = "Published Name: oem9.inf\nOriginal Name: ANDROID_WINUSB.INF\n";
        assert_eq!(parse_published_name(sample).as_deref(), Some("oem9.inf"));
    }

    // 复现用户中文 Windows 上 `pnputil /enum-drivers` 的真实（本地化）输出形态。
    #[test]
    fn parses_localized_chinese_output() {
        let sample = "\
已安装的驱动程序:

已发布名称:      oem12.inf
原始名称:      android_winusb.inf
提供程序名称:      Google, Inc.
类名称:      AndroidUsbDeviceClass
驱动程序日期:      06/21/2011
驱动程序版本:      11.0.0.0
签名状态:      自签名
";
        assert_eq!(parse_published_name(sample).as_deref(), Some("oem12.inf"));
    }

    #[test]
    fn parses_localized_with_leading_spaces() {
        let sample = "   已发布名称:      oem45.inf\n   原始名称:      android_winusb.inf\n";
        assert_eq!(parse_published_name(sample).as_deref(), Some("oem45.inf"));
    }
}

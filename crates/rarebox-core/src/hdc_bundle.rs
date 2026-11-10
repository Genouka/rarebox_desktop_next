//! 内置 hdc 的提取与定位。
//!
//! Windows 上的 hdc 由 `hdc.exe` 及其依赖的 `libusb-1.0.dll`、OpenSSL DLL 组成。
//! 它们随可执行文件编译进二进制，首次运行时写回数据目录，之后复用，避免每次启动
//! 都重写。这与内置 ADB、scrcpy-server 的做法一致。
//!
//! 抽取后的目录是应用数据目录下的 `hdc/<版本>`，其他模块仍然只通过 `PathBuf`
//! 使用，不需要关心 hdc 来自外部还是内置。
//!
//! 非 Windows 平台不内嵌二进制：macOS / Linux 的 hdc 随应用放在可执行文件同目录
//! （macOS `.app/Contents/MacOS/hdc`），Android 的 `libhdc.so` 放在 jniLibs 的
//! `nativeLibraryDir`，两者都不需要写数据目录。因此这里统一返回 `None`，由调用方
//! 按平台顺序继续查找。

#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use anyhow::{Context, Result, ensure};

/// 内置 hdc 的版本号，用于标记抽取目录，内容变化时仅增量更新。
#[cfg(windows)]
const HDC_VERSION: &str = "3.2.0g";

/// 需要随 `hdc.exe` 一起写出的文件：主程序在前，依赖 DLL 紧跟。
#[cfg(windows)]
const BUNDLED_FILES: &[(&str, &[u8])] = &[
    (
        "hdc.exe",
        include_bytes!("../../../vendor/hdc/windows/hdc.exe"),
    ),
    (
        "libusb-1.0.dll",
        include_bytes!("../../../vendor/hdc/windows/libusb-1.0.dll"),
    ),
    (
        "libcrypto-3-x64__.dll",
        include_bytes!("../../../vendor/hdc/windows/libcrypto-3-x64__.dll"),
    ),
    (
        "libssl-3-x64__.dll",
        include_bytes!("../../../vendor/hdc/windows/libssl-3-x64__.dll"),
    ),
];

/// 内置 hdc 的抽取目录：`数据目录/hdc/<版本>`。
#[cfg(windows)]
fn hdc_root() -> PathBuf {
    crate::config::data_dir().join("hdc").join(HDC_VERSION)
}

/// 确保内置 hdc 已写到数据目录，返回 `hdc.exe` 的路径。
///
/// 版本目录不存在时整体写出；已存在则跳过，避免重复拷贝约 10 MB 的二进制。
/// 抽取失败（例如没有写权限）返回 `None`，调用方回退到外部查找。
#[cfg(windows)]
pub fn ensure_embedded_hdc() -> Option<PathBuf> {
    let root = hdc_root();
    let exe = root.join("hdc.exe");
    if files_ready(&root) {
        return Some(exe);
    }
    match extract_to(&root) {
        Ok(()) => Some(exe),
        Err(err) => {
            eprintln!("RareBox Desktop: 无法解包内置 hdc：{err:#}");
            None
        }
    }
}

/// 已抽取的文件是否齐全且大小正确。hdc 缺少 `libusb_shared.dll` 时无法启动，
/// 因此不能只看 `hdc.exe` 是否存在。
#[cfg(windows)]
fn files_ready(root: &PathBuf) -> bool {
    BUNDLED_FILES.iter().all(|(name, bytes)| {
        std::fs::metadata(root.join(name))
            .map(|meta| meta.len() == bytes.len() as u64)
            .unwrap_or(false)
    })
}

#[cfg(windows)]
fn extract_to(root: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("无法创建内置 hdc 目录 {}", root.display()))?;
    for (name, bytes) in BUNDLED_FILES {
        let path = root.join(name);
        // 仅当大小不符时才重写，避免无谓的磁盘写入。
        let needs_write = std::fs::metadata(&path)
            .map(|meta| meta.len() != bytes.len() as u64)
            .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, bytes)
                .with_context(|| format!("无法写出内置 hdc 文件 {}", path.display()))?;
        }
    }
    // 写完后做一次自检：主程序必须存在且大小正确。
    let meta = std::fs::metadata(root.join("hdc.exe"))
        .with_context(|| format!("无法校验内置 hdc {}", root.display()))?;
    ensure!(
        meta.len() == BUNDLED_FILES[0].1.len() as u64,
        "内置 hdc 写出后大小异常：{}",
        root.display()
    );
    Ok(())
}

/// 非 Windows 平台没有内置 hdc 二进制。
#[cfg(not(windows))]
pub fn ensure_embedded_hdc() -> Option<std::path::PathBuf> {
    None
}

#[cfg(all(windows, test))]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    use super::*;

    #[test]
    fn embedded_hdc_extracts_and_is_idempotent() {
        let root = hdc_root();
        // 清理可能残留的旧目录，保证从零抽取。
        let _ = std::fs::remove_dir_all(&root);
        let first = ensure_embedded_hdc().expect("内置 hdc 应可解包");
        assert!(first.exists(), "hdc.exe 应已写出");
        assert_eq!(first, root.join("hdc.exe"));
        // 第二次调用应直接命中已抽取的文件，不再重写。
        let second = ensure_embedded_hdc().expect("二次调用不应失败");
        assert_eq!(first, second);
        // 依赖的 DLL 也必须就位，否则 hdc 无法启动。
        assert!(root.join("libusb-1.0.dll").exists());
        assert!(root.join("libssl-3-x64__.dll").exists());
        // 真正执行一次，确认解包目录中的 hdc 与其 DLL 可用。
        let output = Command::new(&first)
            .arg("-v")
            .creation_flags(0x0800_0000)
            .output()
            .expect("内置 hdc 应可执行");
        let version = String::from_utf8_lossy(&output.stdout);
        assert!(version.contains("Ver:"), "内置 hdc 版本输出异常：{version}");
        let _ = std::fs::remove_dir_all(&root);
    }
}

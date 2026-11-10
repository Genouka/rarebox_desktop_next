//! 内置 ADB 的提取与定位。
//!
//! Windows 上的 ADB 由三个文件组成：`adb.exe` 及其依赖的 `AdbWinApi.dll`、
//! `AdbWinUsbApi.dll`。它们随可执行文件编译进二进制，首次运行时写回数据目录，
//! 之后复用，避免每次启动都重写。这与内置 `scrcpy-server` 的做法一致。
//!
//! 抽取后的目录是应用数据目录下的 `adb/<版本>`，其他模块仍然只通过 `PathBuf`
//! 使用，不需要关心 ADB 来自外部还是内置。非 Windows 平台没有内置二进制，
//! 统一返回 `None`，由调用方回退到外部查找。

#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use anyhow::{Context, Result, ensure};

/// 内置 ADB 的版本号，用于标记抽取目录，内容变化时仅增量更新。
#[cfg(windows)]
const ADB_VERSION: &str = "35.0.2";

/// 需要随 `adb.exe` 一起写出的文件：主程序在前，DLL 紧跟。
#[cfg(windows)]
const BUNDLED_FILES: &[(&str, &[u8])] = &[
    (
        "adb.exe",
        include_bytes!("../../../vendor/adb/windows/adb.exe"),
    ),
    (
        "AdbWinApi.dll",
        include_bytes!("../../../vendor/adb/windows/AdbWinApi.dll"),
    ),
    (
        "AdbWinUsbApi.dll",
        include_bytes!("../../../vendor/adb/windows/AdbWinUsbApi.dll"),
    ),
];

/// 内置 ADB 的抽取目录：`数据目录/adb/<版本>`。
#[cfg(windows)]
fn adb_root() -> PathBuf {
    crate::config::data_dir().join("adb").join(ADB_VERSION)
}

/// 确保内置 ADB 已写到数据目录，返回 `adb.exe` 的路径。
///
/// 版本目录不存在时整体写出；已存在则跳过，避免重复拷贝约 6 MB 的二进制。
/// 抽取失败（例如没有写权限）返回 `None`，调用方回退到外部查找。
#[cfg(windows)]
pub fn ensure_embedded_adb() -> Option<PathBuf> {
    let root = adb_root();
    let exe = root.join("adb.exe");
    if exe.exists() {
        return Some(exe);
    }
    match extract_to(&root) {
        Ok(()) => Some(exe),
        Err(err) => {
            eprintln!("RareBox Desktop: 无法解包内置 ADB：{err:#}");
            None
        }
    }
}

#[cfg(windows)]
fn extract_to(root: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("无法创建内置 ADB 目录 {}", root.display()))?;
    for (name, bytes) in BUNDLED_FILES {
        let path = root.join(name);
        // 仅当大小不符时才重写，避免无谓的磁盘写入。
        let needs_write = std::fs::metadata(&path)
            .map(|meta| meta.len() != bytes.len() as u64)
            .unwrap_or(true);
        if needs_write {
            std::fs::write(&path, bytes)
                .with_context(|| format!("无法写出内置 ADB 文件 {}", path.display()))?;
        }
    }
    // 写完后做一次自检：主程序必须存在且大小正确。
    let meta = std::fs::metadata(root.join("adb.exe"))
        .with_context(|| format!("无法校验内置 ADB {}", root.display()))?;
    ensure!(
        meta.len() == BUNDLED_FILES[0].1.len() as u64,
        "内置 ADB 写出后大小异常：{}",
        root.display()
    );
    Ok(())
}

/// 非 Windows 平台没有内置 ADB 二进制。
#[cfg(not(windows))]
pub fn ensure_embedded_adb() -> Option<std::path::PathBuf> {
    None
}

#[cfg(all(windows, test))]
mod tests {
    use super::*;

    #[test]
    fn embedded_adb_extracts_and_is_idempotent() {
        let root = adb_root();
        // 清理可能残留的旧目录，保证从零抽取。
        let _ = std::fs::remove_dir_all(&root);
        let first = ensure_embedded_adb().expect("内置 ADB 应可解包");
        assert!(first.exists(), "adb.exe 应已写出");
        assert_eq!(first, root.join("adb.exe"));
        // 第二次调用应直接命中已抽取的文件，不再重写。
        let second = ensure_embedded_adb().expect("二次调用不应失败");
        assert_eq!(first, second);
        // 依赖的 DLL 也必须就位，否则 adb 无法启动。
        assert!(root.join("AdbWinApi.dll").exists());
        assert!(root.join("AdbWinUsbApi.dll").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}

//! 平台注入点。
//!
//! Android 宿主（GPUI 或 Kotlin/JNI）在启动时把 APK 的 `nativeLibraryDir`
//! 注入进来，内置 adb / hdc 就能像普通可执行文件一样运行。桌面端不需要设置。

use std::path::PathBuf;
use std::sync::OnceLock;

static NATIVE_LIB_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 由 Android 宿主在创建引擎前调用。
pub fn set_native_lib_dir(dir: PathBuf) {
    let _ = NATIVE_LIB_DIR.set(dir);
}

/// APK 解压原生库的目录；桌面端或尚未注入时返回 `None`。
pub fn native_lib_dir() -> Option<PathBuf> {
    NATIVE_LIB_DIR.get().cloned()
}

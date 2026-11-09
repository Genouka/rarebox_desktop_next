//! Android 入口：由 `android-activity` 在原生线程调用 `android_main`。
//!
//! 平台实现来自 `gpui-mobile`（android-activity + wgpu/Vulkan）；桌面端仍使用
//! `gpui_platform`。这里只负责初始化目录、资源与 GPUI 应用，然后复用共享 UI。

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use android_activity::AndroidApp;
use gpui::{App, Application, WindowOptions};

use crate::app::RareBox;
use crate::config::Config;

/// 应用进程的 `AndroidApp` 句柄，供 JNI 打开链接等操作使用。
static APP: OnceLock<AndroidApp> = OnceLock::new();

/// GPUI 的 Android 平台实例；根布局用它读取安全区（状态栏/导航栏）尺寸。
static PLATFORM: OnceLock<&'static std::sync::Arc<gpui_mobile::AndroidPlatform>> = OnceLock::new();

/// 状态栏、导航栏与刘海占用的逻辑像素：`(上, 下, 左, 右)`。
pub fn safe_area_insets() -> (f32, f32, f32, f32) {
    PLATFORM
        .get()
        .and_then(|platform| platform.primary_window())
        .map_or((0., 0., 0., 0.), |window| {
            let insets = window.safe_area_insets_logical();
            (insets.top, insets.bottom, insets.left, insets.right)
        })
}

/// 由 `android-activity` 在专用原生线程调用。
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("rarebox"),
    );
    gpui_mobile::android::jni::install_panic_hook();
    log::info!("android_main: entered");

    let platform = gpui_mobile::android::jni::init_platform(&app);
    let _ = PLATFORM.set(platform);
    let Some(shared) = gpui_mobile::android::jni::shared_platform() else {
        log::error!("android_main: shared_platform() returned None");
        return;
    };
    let _ = APP.set(app.clone());

    let data_dir = app
        .internal_data_path()
        .unwrap_or_else(|| PathBuf::from("/data/local/tmp/rarebox"));
    let external_dir = app.external_data_path().unwrap_or_else(|| data_dir.clone());
    crate::config::set_data_dir(data_dir);
    crate::config::set_pictures_dir(external_dir.join("Pictures"));
    crate::config::set_downloads_dir(external_dir.join("Download"));

    // 内置 adb / hdc 需要从 nativeLibraryDir 直接执行；核心引擎通过这个注入点取目录。
    if let Some(dir) = native_lib_dir() {
        rarebox_core::platform::set_native_lib_dir(dir);
    }

    log::info!("android_main: creating GPUI application");
    Application::with_platform(shared.into_rc())
        .with_assets(crate::assets::md3_assets())
        .run(|cx: &mut App| {
            material3_gpui::init(cx);

            let config = Config::load();
            crate::theme::apply(cx, config.seed, config.dark);

            let window_config = config.clone();
            if let Err(err) = cx.open_window(WindowOptions::default(), move |_, cx| {
                RareBox::new(cx, window_config.clone())
            }) {
                log::error!("android_main: open_window failed: {err}");
            }
            cx.activate(true);
        });

    log::info!("android_main: event loop exited");
}

/// APK 安装时解压原生库的目录（`ApplicationInfo.nativeLibraryDir`）。
///
/// Android 10 起应用不能在自己数据目录里执行文件，但 `nativeLibraryDir` 里的文件
/// 仍然可执行，所以随包分发的 `libadb.so` / `libhdc.so` 放在这里直接运行。
///
/// 结果只解析一次：先看 `RAREBOX_ANDROID_LIB_DIR`，再用 JNI 读
/// `Activity.getApplicationInfo().nativeLibraryDir`，最后从 `/proc/self/maps`
/// 里已加载的库反推。
pub fn native_lib_dir() -> Option<PathBuf> {
    static DIR: LazyLock<Option<PathBuf>> = LazyLock::new(resolve_native_lib_dir);
    DIR.clone()
}

fn resolve_native_lib_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("RAREBOX_ANDROID_LIB_DIR").map(PathBuf::from)
        && dir.is_dir()
    {
        return Some(dir);
    }
    if let Some(dir) = jni_native_lib_dir() {
        return Some(dir);
    }
    lib_dir_from_maps()
}

/// `getApplicationInfo().nativeLibraryDir`，需要应用进程里有 JNI 环境。
fn jni_native_lib_dir() -> Option<PathBuf> {
    use jni::objects::{JObject, JString};

    let app = APP.get()?;
    let vm = jni::JavaVM::singleton().ok()?;
    let dir = vm
        .attach_current_thread(|env| -> jni::errors::Result<String> {
            let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
            let info = env
                .call_method(
                    &activity,
                    jni::jni_str!("getApplicationInfo"),
                    jni::jni_sig!("()Landroid/content/pm/ApplicationInfo;"),
                    &[],
                )?
                .l()?;
            let raw = env
                .get_field(
                    &info,
                    jni::jni_str!("nativeLibraryDir"),
                    jni::jni_sig!("Ljava/lang/String;"),
                )?
                .l()?;
            let text = unsafe { JString::from_raw(env, raw.as_raw() as jni::sys::jstring) };
            Ok(text.to_string())
        })
        .ok()?;

    if dir.trim().is_empty() {
        return None;
    }
    let path = PathBuf::from(dir);
    path.is_dir().then_some(path)
}

/// 退路：从 `/proc/self/maps` 里已加载的原生库路径反推所在目录。
fn lib_dir_from_maps() -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    for line in maps.lines() {
        let Some(path) = line.split_whitespace().last() else {
            continue;
        };
        if !path.ends_with(".so") || !path.contains("/lib/") {
            continue;
        }
        let dir = Path::new(path).parent()?;
        if dir.is_dir() {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// 通过 `ACTION_VIEW` 打开外部链接。失败时返回 `false`，由调用方提示。
pub fn open_url(url: &str) -> bool {
    use jni::JValue;
    use jni::objects::JObject;

    let Some(app) = APP.get() else {
        return false;
    };
    let Ok(vm) = jni::JavaVM::singleton() else {
        return false;
    };

    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(
            jni::jni_str!("android/content/Intent"),
            jni::jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&action)],
        )?;
        let url = env.new_string(url)?;
        let uri = env
            .call_static_method(
                jni::jni_str!("android/net/Uri"),
                jni::jni_str!("parse"),
                jni::jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
                &[JValue::Object(&url)],
            )?
            .l()?;
        env.call_method(
            &intent,
            jni::jni_str!("setData"),
            jni::jni_sig!("(Landroid/net/Uri;)Landroid/content/Intent;"),
            &[JValue::Object(&uri)],
        )?;
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        env.call_method(
            &activity,
            jni::jni_str!("startActivity"),
            jni::jni_sig!("(Landroid/content/Intent;)V"),
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    });

    if let Err(err) = result {
        log::warn!("open_url failed: {err}");
        false
    } else {
        true
    }
}

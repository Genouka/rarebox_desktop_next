#![recursion_limit = "256"]

//! RareBox 的跨语言绑定：C ABI + Android JNI。
//!
//! 设计目标：宿主（Kotlin / Wear Compose、C/C++、其他语言）只依赖 JSON 协议，
//! 不需要理解 Rust 类型：
//!
//! - 命令：`rarebox_engine_send(handle, {"cmd":"refresh_devices"})`
//! - 事件：JSON 回调（`Event::to_protocol_json`），画面帧走独立二进制回调
//! - 状态：`rarebox_engine_state(handle)` 返回最近一次发布的状态快照 JSON
//!
//! Android 端可以用 `CoreEngine.kt`（见 `bindings/kotlin`）直接调用 JNI 版本。

use std::ffi::{CStr, CString, c_char};
use std::sync::{Arc, Mutex};

use rarebox_core::config::Config;
use rarebox_core::event::Event;
use rarebox_core::state::CoreState;
use rarebox_core::{Command, Engine};

/// JSON 事件回调。字符串在回调期间有效，宿主不应保存指针。
pub type JsonEventCallback = extern "C" fn(*const c_char);

/// 画面帧回调（BGRA）。`data` 在回调期间有效。
pub type FrameCallback =
    extern "C" fn(generation: u64, width: u32, height: u32, data: *const u8, len: usize);

/// 单一事件出口：C ABI 回调或 JNI listener 二选一，避免两路消费者抢事件。
pub(crate) type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

/// 引擎句柄。由 Rust 分配，宿主只持有裸指针。
pub struct EngineHandle {
    engine: Engine,
    sink: Arc<Mutex<Option<EventSink>>>,
}

impl EngineHandle {
    fn new(config: Config) -> Self {
        let engine = Engine::new(config);
        let sink: Arc<Mutex<Option<EventSink>>> = Arc::new(Mutex::new(None));
        spawn_event_forwarder(&engine, sink.clone());
        Self { engine, sink }
    }
}

fn spawn_event_forwarder(engine: &Engine, sink: Arc<Mutex<Option<EventSink>>>) {
    let events = engine.events();
    std::thread::Builder::new()
        .name("rarebox-ffi-events".into())
        .spawn(move || {
            while let Ok(event) = events.recv_blocking() {
                let sink = sink.lock().ok().and_then(|sink| sink.clone());
                if let Some(sink) = sink {
                    sink(event);
                }
            }
        })
        .ok();
}

fn config_from_json(json: Option<&str>) -> Config {
    let Some(json) = json else {
        return Config::load();
    };
    serde_json::from_str(json).unwrap_or_else(|err| {
        eprintln!("rarebox-ffi: 配置 JSON 无效，使用本地配置: {err}");
        Config::load()
    })
}

/// 创建引擎（使用本地配置）。
#[unsafe(no_mangle)]
pub extern "C" fn rarebox_engine_new() -> *mut EngineHandle {
    Box::into_raw(Box::new(EngineHandle::new(Config::load())))
}

/// 使用 JSON 配置创建引擎；传 `NULL` 时与 `rarebox_engine_new` 相同。
///
/// # Safety
/// `config_json` 必须是有效的 NUL 结尾 UTF-8 字符串或 `NULL`。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_new_with_config(
    config_json: *const c_char,
) -> *mut EngineHandle {
    let json = if config_json.is_null() {
        None
    } else {
        unsafe { CStr::from_ptr(config_json) }
            .to_str()
            .ok()
    };
    Box::into_raw(Box::new(EngineHandle::new(config_from_json(json))))
}

/// 销毁引擎。
///
/// # Safety
/// `handle` 必须来自 `rarebox_engine_new*`，且只能释放一次。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_free(handle: *mut EngineHandle) {
    if !handle.is_null() {
        let handle = unsafe { Box::from_raw(handle) };
        // 显式停掉核心线程；否则 JNI / C ABI 宿主反复创建销毁引擎会泄漏空闲线程。
        handle.engine.shutdown_and_wait();
        drop(handle);
    }
}

/// 投递 JSON 命令；解析失败返回 `false`。
///
/// # Safety
/// `handle` 有效，`json` 是 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_send(
    handle: *mut EngineHandle,
    json: *const c_char,
) -> bool {
    if handle.is_null() || json.is_null() {
        return false;
    }
    let handle = unsafe { &*handle };
    let Ok(json) = unsafe { CStr::from_ptr(json) }.to_str() else {
        return false;
    };
    match Command::from_json(json) {
        Ok(command) => handle.engine.command(command).is_ok(),
        Err(_) => false,
    }
}

/// 读取最近一次发布的状态快照 JSON。返回值需要用 `rarebox_string_free` 释放。
///
/// # Safety
/// `handle` 必须有效。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_state(handle: *mut EngineHandle) -> *mut c_char {
    if handle.is_null() {
        return std::ptr::null_mut();
    }
    let handle = unsafe { &*handle };
    let json = state_json(&handle.engine.state());
    CString::new(json)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

/// 设置 JSON / 画面帧回调；任一参数可传 `NULL` 取消。
///
/// # Safety
/// `handle` 有效；回调必须能在引擎工作线程上安全执行。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_set_callbacks(
    handle: *mut EngineHandle,
    json: Option<JsonEventCallback>,
    frame: Option<FrameCallback>,
) {
    if handle.is_null() {
        return;
    }
    let handle = unsafe { &*handle };
    let sink: Option<EventSink> = if json.is_none() && frame.is_none() {
        None
    } else {
        Some(Arc::new(move |event: Event| match event {
            Event::Video(frame_data) => {
                if let Some(callback) = frame {
                    callback(
                        frame_data.generation,
                        frame_data.width,
                        frame_data.height,
                        frame_data.bgra.as_ptr(),
                        frame_data.bgra.len(),
                    );
                }
            }
            other => {
                if let Some(callback) = json
                    && let Some(json) = other.to_protocol_json()
                    && let Ok(text) = CString::new(json)
                {
                    callback(text.as_ptr());
                }
            }
        }))
    };
    if let Ok(mut slot) = handle.sink.lock() {
        *slot = sink;
    }
}

/// 设置 Android APK 的 `nativeLibraryDir`（内置 adb/hdc 从该目录执行）。
///
/// 必须在 `rarebox_engine_new*` 之前调用；桌面端可忽略。
///
/// # Safety
/// `dir` 必须是有效的 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_set_native_lib_dir(dir: *const c_char) {
    if dir.is_null() {
        return;
    }
    if let Ok(dir) = unsafe { CStr::from_ptr(dir) }.to_str() {
        if !dir.is_empty() {
            rarebox_core::platform::set_native_lib_dir(std::path::PathBuf::from(dir));
        }
    }
}

/// 设置宿主应用目录：数据、图片与下载目录。
///
/// Android / Wear 的 Kotlin 宿主在创建引擎前调用（`filesDir`、
/// `getExternalFilesDir("Pictures")`、`getExternalFilesDir("Download")`）；
/// 桌面端由操作系统约定自动解析，无需调用。空指针或空字符串表示跳过。
///
/// # Safety
/// 每个非空指针都必须是有效的 NUL 结尾 UTF-8 字符串。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_engine_set_app_dirs(
    data_dir: *const c_char,
    pictures_dir: *const c_char,
    downloads_dir: *const c_char,
) {
    if let Some(dir) = unsafe { read_dir_arg(data_dir) } {
        rarebox_core::config::set_data_dir(dir);
    }
    if let Some(dir) = unsafe { read_dir_arg(pictures_dir) } {
        rarebox_core::config::set_pictures_dir(dir);
    }
    if let Some(dir) = unsafe { read_dir_arg(downloads_dir) } {
        rarebox_core::config::set_downloads_dir(dir);
    }
}

unsafe fn read_dir_arg(ptr: *const c_char) -> Option<std::path::PathBuf> {
    if ptr.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?;
    if text.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(text))
    }
}

/// 释放 `rarebox_engine_state` 返回的字符串。
///
/// # Safety
/// `text` 必须来自本库的字符串分配函数且只释放一次。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rarebox_string_free(text: *mut c_char) {
    if !text.is_null() {
        drop(unsafe { CString::from_raw(text) });
    }
}

/// 把一个能力结论编码为 `{supported, enabled, hint}`。
fn capability_json(capability: &rarebox_core::connect_support::Capability) -> serde_json::Value {
    serde_json::json!({
        "supported": capability.supported,
        "enabled": capability.enabled,
        "hint": capability.hint,
    })
}

/// 把核心状态编码为宿主可读的 JSON。
///
/// 字段命名与 Kotlin/JS 习惯一致（camelCase）；未实现的数据（安装计划等）
/// 只给出摘要，避免把大量路径/临时对象暴露给手表端。
pub fn state_json(state: &CoreState) -> String {
    use serde_json::json;

    let snapshot = state.snapshot.as_ref().map(|snapshot| {
        // 先算好能力结论，避免 `json!` 嵌套过深触发递归上限。
        let support = snapshot.debug_support();
        let wireless = capability_json(&support.wireless);
        let wlan = capability_json(&support.wlan);
        let usb = capability_json(&support.usb);
        let recommended = support.recommended().map(|transport| transport.label());
        json!({
            "serial": snapshot.serial,
            "model": snapshot.model,
            "manufacturer": snapshot.manufacturer,
            "android": snapshot.android,
            "sdk": snapshot.sdk,
            "buildId": snapshot.build_id,
            "characteristics": snapshot.characteristics,
            "patch": snapshot.patch,
            "abi": snapshot.abi,
            "ip": snapshot.ip,
            "uptimeSeconds": snapshot.uptime_seconds,
            "load": snapshot.load,
            "battery": {
                "level": snapshot.battery_level,
                "status": snapshot.battery_status,
                "temperature": snapshot.battery_temperature,
                "voltage": snapshot.battery_voltage,
                "powered": snapshot.battery_powered,
            },
            "memory": {
                "totalKb": snapshot.memory_total_kb,
                "availableKb": snapshot.memory_available_kb,
            },
            "storage": {
                "size": snapshot.storage.size,
                "used": snapshot.storage.used,
                "available": snapshot.storage.available,
                "percent": snapshot.storage.percent,
            },
            "display": {
                "physicalSize": snapshot.display.physical_size,
                "overrideSize": snapshot.display.override_size,
                "physicalDensity": snapshot.display.physical_density,
                "overrideDensity": snapshot.display.override_density,
                "rotation": snapshot.display.rotation,
            },
            "support": {
                "wireless": wireless,
                "wlan": wlan,
                "usb": usb,
                "recommended": recommended,
            },
        })
    });

    let harmony = &state.harmony;
    let harmony_snapshot = harmony.snapshot.as_ref().map(|snapshot| {
        json!({
            "key": snapshot.key,
            "model": snapshot.model,
            "brand": snapshot.brand,
            "osVersion": snapshot.os_version,
            "apiVersion": snapshot.api_version,
            "abi": snapshot.abi,
            "udid": snapshot.udid,
            "ip": snapshot.ip,
            "battery": snapshot.battery,
            "batteryStatus": snapshot.battery_status,
            "batteryTemperature": snapshot.battery_temperature_c,
            "memory": {
                "totalKb": snapshot.memory_total_kb,
                "availableKb": snapshot.memory_available_kb,
            },
            "storage": {
                "totalKb": snapshot.storage_total_kb,
                "usedKb": snapshot.storage_used_kb,
            },
            "uptimeSeconds": snapshot.uptime_seconds,
            "deviceName": snapshot.device_name,
            "screenState": snapshot.screen_state,
            "screenResolution": snapshot.screen_resolution,
            "screenRotation": snapshot.screen_rotation,
            "wifiSsid": snapshot.wifi_ssid,
        })
    });

    let signing = signing_json(&state.signing);

    let status = state.update.status.as_ref().map(|status| {
        json!({
            "updateAvailable": status.update_available,
            "currentVersion": status.current_version,
            "latestVersion": status.latest_version,
            "channel": status.channel,
            "platform": status.platform,
            "downloadUrl": status.download_url,
            "fileSize": status.file_size,
            "notes": status.notes_text(),
            "publishedAt": status.published_at,
            "forced": status.forced(),
            "reason": status.reason,
        })
    });

    let scan = json!({
        "running": state.scan.running,
        "done": state.scan.done,
        "total": state.scan.total,
        "progress": state.scan.progress(),
        "scope": state.scan.scope,
        "deep": state.scan.deep,
        "hits": state.scan.hits.iter().map(|hit| json!({
            "address": hit.address,
            "kind": hit.kind.map(|kind| match kind {
                rarebox_core::DeviceKind::Android => "android",
                rarebox_core::DeviceKind::Harmony => "harmony",
            }),
            "state": hit.state,
        })).collect::<Vec<_>>(),
    });

    let value = json!({
        "kindMode": state.kind_mode.label(),
        "autoKind": state.kind_mode.is_auto(),
        "deviceKind": match state.device_kind {
            rarebox_core::DeviceKind::Android => "android",
            rarebox_core::DeviceKind::Harmony => "harmony",
        },
        "mdns": state.mdns.iter().map(|service| json!({
            "instance": service.instance,
            "kind": service.kind.label(),
            "address": service.address,
        })).collect::<Vec<_>>(),
        "scan": scan,
        "connectError": state.connect_error.as_ref().map(|error| json!({
            "address": error.address,
            "kind": error.kind.label(),
            "detail": error.detail,
        })),
        "adb": {
            "path": state.adb_path.as_ref().map(|path| path.to_string_lossy()),
            "version": state.adb_version,
            "error": state.adb_error,
        },
        "hdc": {
            "path": harmony.path.as_ref().map(|path| path.to_string_lossy()),
            "version": harmony.version,
            "error": harmony.error,
        },
        "devices": state.devices.iter().map(|device| json!({
            "serial": device.serial,
            "state": device.state,
            "model": device.model,
            "connection": device.connection.label(),
            "online": device.is_online(),
        })).collect::<Vec<_>>(),
        "selected": state.selected_serial,
        "snapshot": snapshot,
        "harmonyDevices": harmony.devices.iter().map(|device| json!({
            "key": device.key,
            "connection": format!("{:?}", device.connection),
            "state": format!("{:?}", device.state),
            "online": device.is_online(),
            "host": device.host,
        })).collect::<Vec<_>>(),
        "harmonySelected": harmony.selected,
        "harmonySnapshot": harmony_snapshot,
        "activity": state.activity,
        "packages": state.packages.iter().map(|package| json!({
            "name": package.name,
            "label": package.label_or_name(),
            "versionName": package.version_name,
            "versionCode": package.version_code,
            "system": package.system,
        })).collect::<Vec<_>>(),
        "harmonyPackages": harmony.packages.iter().map(|package| json!({
            "name": package.name,
            "label": package.label,
        })).collect::<Vec<_>>(),
        "files": {
            "path": state.files_path,
            "selected": state.files_selected,
            "entries": state.files_entries.iter().map(file_entry_json).collect::<Vec<_>>(),
        },
        "harmonyFiles": {
            "path": harmony.files_path,
            "entries": harmony.files.iter().map(file_entry_json).collect::<Vec<_>>(),
        },
        "logs": state.logs.iter().map(|line| json!({
            "raw": line.raw,
            "time": line.time,
            "level": line.level.label(),
            "tag": line.tag,
            "message": line.message,
        })).collect::<Vec<_>>(),
        "logsAt": state.logs_at,
        "harmonyLogs": harmony.logs.iter().map(|line| json!({
            "raw": line.raw,
            "time": line.time,
            "level": line.level.label(),
            "tag": line.tag,
            "message": line.message,
        })).collect::<Vec<_>>(),
        "harmonyLogsAt": harmony.logs_at,
        "terminal": state.terminal_history.iter().map(|entry| json!({
            "command": entry.command,
            "output": entry.output,
            "success": entry.success,
            "durationMs": entry.duration_ms,
            "time": entry.time,
        })).collect::<Vec<_>>(),
        "harmonyTerminal": harmony.terminal.iter().map(|entry| json!({
            "command": entry.command,
            "output": entry.output,
            "success": entry.success,
            "durationMs": entry.duration_ms,
            "time": entry.time,
        })).collect::<Vec<_>>(),
        "screen": {
            "size": state.screen_size,
            "density": state.screen_density,
        },
        "lastScreenshot": state.last_screenshot.as_ref().map(|path| path.to_string_lossy()),
        "lastRecording": state.last_recording.as_ref().map(|path| path.to_string_lossy()),
        "lastExport": state.last_export.as_ref().map(|path| path.to_string_lossy()),
        "wireless": {
            "address": state.wireless_address,
            "pairAddress": state.pair_address,
            "pairCode": state.pair_code,
        },
        "install": {
            "scope": state.package_scope.label(),
            "query": state.package_query,
            "batchReports": state.install_batch.as_ref().map(|batch| batch.reports.iter().map(|report| json!({
                "title": report.title,
                "detail": report.detail,
                "success": report.success,
            })).collect::<Vec<_>>()),
            "planGroups": state.install_plan.as_ref().map(|plan| plan.groups.len()),
            "planFailures": state.install_plan.as_ref().map(|plan| plan.failures.len()),
        },
        "harmonyInstall": {
            "replace": harmony.install_replace,
            "shared": harmony.install_shared,
            "reports": harmony.install_reports.iter().map(|report| json!({
                "title": report.title,
                "detail": report.detail,
                "success": report.success,
            })).collect::<Vec<_>>(),
            "forwards": harmony.forwards.iter().map(|forward| json!({
                "device": forward.device,
                "first": forward.first,
                "second": forward.second,
                "reverse": forward.reverse,
            })).collect::<Vec<_>>(),
            "forwardReverse": harmony.forward_reverse,
        },
        "remote": {
            "phase": match state.remote_phase {
                rarebox_core::RemotePhase::Idle => "idle",
                rarebox_core::RemotePhase::Starting => "starting",
                rarebox_core::RemotePhase::Running => "running",
                rarebox_core::RemotePhase::Failed => "failed",
            },
            "error": state.remote_error,
            "width": state.remote_size.0,
            "height": state.remote_size.1,
            "generation": state.remote_generation,
            "frameCount": state.remote_frame_count,
        },
        "signing": signing,
        "update": {
            "checking": state.update.checking,
            "status": status,
        },
        "announcements": state.announcements.iter().map(|item| json!({
            "id": item.id,
            "title": item.title,
            "body": item.body,
            "level": item.level,
            "levelLabel": item.level_label(),
            "createdAt": item.created_at,
        })).collect::<Vec<_>>(),
        "announcementsError": state.announcements_error,
    });

    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

fn file_entry_json(entry: &rarebox_core::adb::FileEntry) -> serde_json::Value {
    use serde_json::json;
    json!({
        "name": entry.name,
        "isDir": entry.is_dir,
        "isLink": entry.is_link,
        "permissions": entry.permissions,
        "size": entry.size,
        "sizeLabel": entry.size_label,
        "modified": entry.modified,
    })
}

fn signing_json(signing: &rarebox_core::signing::SigningUi) -> serde_json::Value {
    use serde_json::json;

    let login = match &signing.login {
        rarebox_core::signing::LoginPhase::Idle => json!({ "state": "idle" }),
        rarebox_core::signing::LoginPhase::Waiting => json!({ "state": "waiting" }),
        rarebox_core::signing::LoginPhase::Failed(message) => {
            json!({ "state": "failed", "message": message })
        }
    };
    let stages: Vec<&str> = signing
        .stages
        .iter()
        .map(|status| match status {
            rarebox_core::signing::StageStatus::Pending => "pending",
            rarebox_core::signing::StageStatus::Running => "running",
            rarebox_core::signing::StageStatus::Success => "success",
            rarebox_core::signing::StageStatus::Failed => "failed",
        })
        .collect();
    let outcome = signing.outcome.as_ref().map(|outcome| {
        json!({
            "bundle": outcome.bundle,
            "signed": outcome.signed.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
            "reports": outcome.reports.iter().map(|report| json!({
                "title": report.title,
                "detail": report.detail,
                "success": report.success,
            })).collect::<Vec<_>>(),
        })
    });
    let package = signing.package.as_ref().map(|meta| {
        json!({
            "path": meta.path.to_string_lossy(),
            "bundleName": meta.bundle_name,
            "versionName": meta.version_name,
            "versionCode": meta.version_code,
            "moduleName": meta.module_name,
            "moduleType": meta.module_type,
            "acl": meta.acl,
            "permissions": meta.permissions,
        })
    });
    let service = &signing.service;

    json!({
        "busy": signing.busy,
        "initialized": signing.initialized,
        "login": login,
        "loggedIn": service.is_logged_in(),
        "account": service.auth.as_ref().map(|auth| json!({
            "nickName": auth.nick_name,
            "teamId": auth.team_id,
            "userId": auth.user_id,
        })),
        "teams": service.teams.iter().map(|team| json!({ "id": team.id, "name": team.name })).collect::<Vec<_>>(),
        "kind": match signing.kind {
            rarebox_core::signing::CertKind::Debug => "debug",
            rarebox_core::signing::CertKind::Release => "release",
        },
        "useImported": signing.use_imported,
        "importedMaterialReady": signing.imported_material().is_some(),
        "importFiles": {
            "keystore": signing.import_keystore.as_ref().map(|path| path.to_string_lossy()),
            "cert": signing.import_cert.as_ref().map(|path| path.to_string_lossy()),
            "profile": signing.import_profile.as_ref().map(|path| path.to_string_lossy()),
        },
        "block": match signing.block {
            rarebox_core::signing::SigningBlock::V2 => "v2",
            rarebox_core::signing::SigningBlock::V3 => "v3",
        },
        "stages": stages,
        "stageMessages": signing.stage_messages,
        "logs": signing.logs,
        "acl": signing.acl,
        "udid": signing.udid,
        "replace": signing.replace,
        "shared": signing.shared,
        "resetCert": signing.reset_cert,
        "refreshProfile": signing.refresh_profile,
        "inputs": signing.inputs.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>(),
        "package": package,
        "outcome": outcome,
        "lastSigned": signing.last_signed.as_ref().map(|path| path.to_string_lossy()),
        "localProfiles": signing.local_profiles.iter().map(|profile| json!({
            "packageName": profile.package_name,
            "kind": match profile.kind {
                rarebox_core::signing::CertKind::Debug => "debug",
                rarebox_core::signing::CertKind::Release => "release",
            },
            "profileId": profile.profile_id,
            "expiresAt": profile.expires_at,
        })).collect::<Vec<_>>(),
        "error": signing.error,
    })
}

// ---------------------------------------------------------------------------
// Android JNI
// ---------------------------------------------------------------------------

#[cfg(target_os = "android")]
mod android {
    use jni::objects::{Global, JObject, JString, JValue};
    use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jlong, jstring};
    use jni::{EnvUnowned, jni_sig, jni_str};

    use super::{EngineHandle, config_from_json, state_json};

    fn engine<'a>(handle: jlong) -> Option<&'a EngineHandle> {
        if handle == 0 {
            None
        } else {
            Some(unsafe { &*(handle as *const EngineHandle) })
        }
    }

    fn new_engine(json: Option<&str>) -> jlong {
        let config = config_from_json(json);
        Box::into_raw(Box::new(EngineHandle::new(config))) as jlong
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeNew<'local>(
        _env: EnvUnowned<'local>,
        _this: JObject<'local>,
    ) -> jlong {
        new_engine(None)
    }

    /// `CoreNative.setNativeLibraryDir(dir)`：必须在 `nativeNew` 前调用。
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreNative_setNativeLibraryDir<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        dir: JString<'local>,
    ) {
        let dir = env
            .with_env(|_env| -> jni::errors::Result<String> { Ok(dir.to_string()) })
            .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
        if !dir.is_empty() {
            rarebox_core::platform::set_native_lib_dir(std::path::PathBuf::from(dir));
        }
    }

    /// `CoreNative.setAppDirs(data, pictures, downloads)`：必须在 `nativeNew` 前调用。
    ///
    /// Kotlin 宿主传入 `filesDir` 与两个外部文件目录；任意参数为空字符串时跳过。
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreNative_setAppDirs<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        data_dir: JString<'local>,
        pictures_dir: JString<'local>,
        downloads_dir: JString<'local>,
    ) {
        let dirs = env
            .with_env(|_env| -> jni::errors::Result<(String, String, String)> {
                Ok((
                    data_dir.to_string(),
                    pictures_dir.to_string(),
                    downloads_dir.to_string(),
                ))
            })
            .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
        if !dirs.0.is_empty() {
            rarebox_core::config::set_data_dir(std::path::PathBuf::from(&dirs.0));
        }
        if !dirs.1.is_empty() {
            rarebox_core::config::set_pictures_dir(std::path::PathBuf::from(&dirs.1));
        }
        if !dirs.2.is_empty() {
            rarebox_core::config::set_downloads_dir(std::path::PathBuf::from(&dirs.2));
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeNewWithConfig<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        config: JString<'local>,
    ) -> jlong {
        let config = env
            .with_env(|_env| -> jni::errors::Result<String> { Ok(config.to_string()) })
            .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
        new_engine(Some(&config))
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeSend<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        handle: jlong,
        json: JString<'local>,
    ) -> jboolean {
        env.with_env(|_env| -> jni::errors::Result<jboolean> {
            let Some(engine) = engine(handle) else {
                return Ok(JNI_FALSE);
            };
            match rarebox_core::Command::from_json(&json.to_string()) {
                Ok(command) => Ok(if engine.engine.command(command).is_ok() {
                    JNI_TRUE
                } else {
                    JNI_FALSE
                }),
                Err(_) => Ok(JNI_FALSE),
            }
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeState<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        handle: jlong,
    ) -> jstring {
        env.with_env(|env| -> jni::errors::Result<jstring> {
            let Some(engine) = engine(handle) else {
                return Ok(std::ptr::null_mut());
            };
            let json = state_json(&engine.engine.state());
            let text = env.new_string(json)?;
            let object: JObject = text.into();
            Ok(object.into_raw())
        })
        .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeFree<'local>(
        _env: EnvUnowned<'local>,
        _this: JObject<'local>,
        handle: jlong,
    ) {
        if handle != 0 {
            drop(unsafe { Box::from_raw(handle as *mut EngineHandle) });
        }
    }

    /// 启动事件转发。`listener` 需要实现：
    ///
    /// ```java
    /// void onEvent(String json);
    /// void onFrame(long generation, int width, int height, byte[] bgra);
    /// ```
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_rarebox_core_CoreEngine_nativeStartEvents<'local>(
        mut env: EnvUnowned<'local>,
        _this: JObject<'local>,
        handle: jlong,
        listener: JObject<'local>,
    ) {
        let Some(engine_handle) = engine(handle) else {
            return;
        };
        let setup = env
            .with_env(
                |env| -> jni::errors::Result<Option<(jni::JavaVM, Global<JObject<'static>>)>> {
                    let vm = env.get_java_vm()?;
                    let listener = env.new_global_ref(listener)?;
                    Ok(Some((vm, listener)))
                },
            )
            .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
        let Some((vm, listener)) = setup else {
            return;
        };
        let sink: super::EventSink = std::sync::Arc::new(move |event| {
            let _ = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
                match event {
                    rarebox_core::event::Event::Video(frame) => {
                        let bytes = env.byte_array_from_slice(&frame.bgra)?;
                        let bytes: JObject = bytes.into();
                        env.call_method(
                            &listener,
                            jni_str!("onFrame"),
                            jni_sig!("(JII[B)V"),
                            &[
                                JValue::Long(frame.generation as i64),
                                JValue::Int(frame.width as i32),
                                JValue::Int(frame.height as i32),
                                JValue::Object(&bytes),
                            ],
                        )?;
                    }
                    other => {
                        if let Some(json) = other.to_protocol_json() {
                            let json = env.new_string(json)?;
                            let json: JObject = json.into();
                            env.call_method(
                                &listener,
                                jni_str!("onEvent"),
                                jni_sig!("(Ljava/lang/String;)V"),
                                &[JValue::Object(&json)],
                            )?;
                        }
                    }
                }
                if env.exception_check() {
                    env.exception_describe();
                    env.exception_clear();
                }
                Ok(())
            });
        });
        if let Ok(mut slot) = engine_handle.sink.lock() {
            *slot = Some(sink);
        }
}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_json_contains_core_fields() {
        let state = CoreState::default();
        let json: serde_json::Value = serde_json::from_str(&state_json(&state)).unwrap();
        assert_eq!(json["deviceKind"], "android");
        assert!(json["devices"].is_array());
        assert!(json["signing"]["loggedIn"].is_boolean());
    }

    #[test]
    fn commands_round_trip_through_json() {
        let command = Command::from_json(r#"{"cmd":"refresh_devices"}"#).unwrap();
        assert!(matches!(command, Command::RefreshDevices));
        let command =
            Command::from_json(r#"{"cmd":"navigate_files","path":"/sdcard"}"#).unwrap();
        match command {
            Command::NavigateFiles { path } => assert_eq!(path, "/sdcard"),
            _ => panic!("unexpected command"),
        }
    }
}

//! 引擎句柄表与事件转发（平台无关，Windows 上也会编译，便于 `cargo check` 校验）。
//!
//! ArkTS 侧用 `number` 承载句柄，只能精确表示 2^53 以内的整数，因此句柄从 1
//! 递增且永不复用；`Engine` 不是全局单例，宿主可以同时持有多个。
//!
//! 事件转发沿用 `rarebox-ffi` 的模式：独立线程消费 `Engine::events()`，再通过
//! 一个可替换的出口（鸿蒙侧是 `ThreadsafeFunction`）交回宿主线程。`Engine`
//! 自身持有的接收端永远不 `recv`，事件只会进入转发线程。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::thread::JoinHandle;

use rarebox_core::config::Config;
use rarebox_core::event::Event;
use rarebox_core::{Command, Engine};

/// 单一事件出口；同一引擎只保留一个消费者（与 `rarebox-ffi` 一致）。
pub(crate) type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

struct Entry {
    engine: Engine,
    sink: Arc<Mutex<Option<EventSink>>>,
    /// 关闭事件通道，使转发线程退出并释放 TSFN；只在 `dispose` 时调用。
    stop: Box<dyn Fn() + Send>,
    /// 转发线程；`dispose` 里 join，保证句柄摘除后线程确实不再存活。
    pump: Option<JoinHandle<()>>,
}

/// 存活的转发线程数；供泄漏回归测试断言 `create` / `dispose` 不累积线程。
static LIVE_PUMPS: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
fn live_pumps() -> usize {
    LIVE_PUMPS.load(Ordering::SeqCst)
}

/// RAII：转发线程退出时递减存活计数（panic 展开也会执行）。
struct PumpGuard;

impl Drop for PumpGuard {
    fn drop(&mut self) {
        LIVE_PUMPS.fetch_sub(1, Ordering::SeqCst);
    }
}

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static ENGINES: LazyLock<Mutex<HashMap<u64, Entry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn table() -> &'static Mutex<HashMap<u64, Entry>> {
    &ENGINES
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 创建引擎、登记句柄，并启动事件转发线程。
pub(crate) fn create(config: Config) -> u64 {
    let engine = Engine::new(config);
    let sink: Arc<Mutex<Option<EventSink>>> = Arc::new(Mutex::new(None));

    let events = engine.events();
    let pump_events = events.clone();
    let pump_sink = sink.clone();
    LIVE_PUMPS.fetch_add(1, Ordering::SeqCst);
    let pump = std::thread::Builder::new()
        .name("rarebox-ohos-events".into())
        .spawn(move || {
            let _live = PumpGuard;
            while let Ok(event) = pump_events.recv_blocking() {
                let current = lock(&pump_sink).clone();
                if let Some(sink) = current {
                    sink(event);
                }
            }
        })
        .map_err(|_| LIVE_PUMPS.fetch_sub(1, Ordering::SeqCst))
        .ok();

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    lock(table()).insert(
        handle,
        Entry {
            engine,
            sink,
            stop: Box::new(move || {
                let _ = events.close();
            }),
            pump,
        },
    );
    handle
}

/// 按句柄克隆引擎；句柄未知时返回 `None`。
pub(crate) fn engine(handle: u64) -> Option<Engine> {
    lock(table()).get(&handle).map(|entry| entry.engine.clone())
}

/// 替换（或清除）事件出口；返回句柄是否存在。
pub(crate) fn set_sink(handle: u64, sink: Option<EventSink>) -> bool {
    let table = lock(table());
    let Some(entry) = table.get(&handle) else {
        return false;
    };
    *lock(&entry.sink) = sink;
    true
}

/// 销毁引擎：先让核心线程退出并等它结束，再关闭事件通道并等待转发线程退出。
/// 返回句柄是否存在（幂等）；句柄摘除后重复调用直接返回 `false`。
pub(crate) fn dispose(handle: u64) -> bool {
    // 先摘句柄再回收，避免并发 dispose 重复处理同一引擎。
    let entry = lock(table()).remove(&handle);
    let Some(entry) = entry else {
        return false;
    };
    entry.engine.shutdown_and_wait();
    (entry.stop)();
    let Entry {
        sink, pump, engine, ..
    } = entry;
    // 先释放事件出口与引擎句柄，再 join：转发线程不会再回调 TSFN。
    drop(lock(&sink).take());
    drop(engine);
    if let Some(pump) = pump {
        // 理论上转发线程自己不会调用 dispose；防御 join(self)。
        if pump.thread().id() != std::thread::current().id() {
            let _ = pump.join();
        }
    }
    true
}

/// 把宿主传入的配置 JSON 转成 `Config`；空串或无效 JSON 回落到本地配置。
///
/// 与 `rarebox-ffi::config_from_json` 语义一致：非空 JSON 按 `Config` 反序列化，
/// 缺省字段走 `#[serde(default)]`。
pub(crate) fn config_from_json(json: &str) -> Config {
    let json = json.trim();
    if json.is_empty() {
        return Config::load();
    }
    serde_json::from_str(json).unwrap_or_else(|err| {
        eprintln!("rarebox-ohos: 配置 JSON 无效，使用本地配置: {err}");
        Config::load()
    })
}

/// 解析宿主传入的命令 JSON（`Command` 是 internally tagged：`{"cmd": ...}`）。
pub(crate) fn command_from_json(json: &str) -> Result<Command, String> {
    Command::from_json(json).map_err(|err| err.to_string())
}

/// 投递命令；引擎已停止等错误文案直接透传给宿主。
pub(crate) fn send_command(engine: &Engine, command: Command) -> Result<(), String> {
    engine.command(command).map_err(|err| err.to_string())
}

/// 状态快照 JSON（camelCase，与 C ABI / JNI 宿主共用同一份实现）。
pub(crate) fn state_json(engine: &Engine) -> String {
    rarebox_ffi::state_json(&engine.state())
}

/// `number` → 句柄。只接受正的整数值；`NaN`/`Infinity`/小数/0 都是非法句柄。
pub(crate) fn parse_handle(value: f64) -> Result<u64, String> {
    if !value.is_finite() || value.fract() != 0.0 || value < 1.0 {
        return Err(format!("invalid engine handle: {value}"));
    }
    Ok(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_positive_integer_handles() {
        assert_eq!(parse_handle(1.0), Ok(1));
        assert_eq!(parse_handle(42.0), Ok(42));
        assert_eq!(parse_handle(9_007_199_254_740_991.0), Ok(9_007_199_254_740_991));
    }

    #[test]
    fn rejects_invalid_handles() {
        for bad in [0.0, -1.0, 1.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(parse_handle(bad).is_err(), "expected rejection for {bad}");
        }
    }

    #[test]
    fn unknown_handles_are_rejected() {
        assert!(engine(9_999).is_none());
        assert!(!dispose(9_999));
        assert!(!set_sink(9_999, None));
    }

    #[test]
    fn empty_config_falls_back_to_local_config() {
        let config = config_from_json("   ");
        assert_eq!(config.adb_path, Config::load().adb_path);
    }

    #[test]
    fn config_json_overrides_defaults() {
        let config = config_from_json(r#"{"adb_path":"/opt/adb","dark":true}"#);
        assert_eq!(config.adb_path, "/opt/adb");
        assert!(config.dark);
    }

    #[test]
    fn invalid_command_json_is_reported() {
        assert!(command_from_json(r#"{"cmd":"no_such_command"}"#).is_err());
        assert!(command_from_json("not json").is_err());
    }

    /// 生命周期测试断言全局线程计数，必须串行执行。
    static LIFECYCLE_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn dispose_reclaims_core_and_forwarder_threads() {
        let _guard = lock(&LIFECYCLE_TEST_LOCK);
        let cores = rarebox_core::engine::live_core_threads();
        let pumps = live_pumps();

        for round in 0..6 {
            let handle = create(Config::default());
            assert!(engine(handle).is_some(), "round {round}");
            assert_eq!(
                rarebox_core::engine::live_core_threads(),
                cores + 1,
                "round {round}"
            );
            assert_eq!(live_pumps(), pumps + 1, "round {round}");

            assert!(dispose(handle), "round {round}");
            assert_eq!(
                rarebox_core::engine::live_core_threads(),
                cores,
                "round {round}"
            );
            assert_eq!(live_pumps(), pumps, "round {round}");

            // 句柄已摘除：重复 dispose 安全且不再回收任何东西。
            assert!(!dispose(handle), "round {round}");
            assert!(engine(handle).is_none(), "round {round}");
            assert_eq!(live_pumps(), pumps, "round {round}");
        }
    }

    #[test]
    fn disposed_engine_rejects_commands() {
        let _guard = lock(&LIFECYCLE_TEST_LOCK);
        let handle = create(Config::default());
        let engine = engine(handle).expect("handle registered");

        assert!(send_command(&engine, Command::RefreshDevices).is_ok());
        assert!(dispose(handle));
        // 已摘除句柄不再解析；克隆出来的句柄投递命令立即失败。
        assert!(super::engine(handle).is_none());
        let err = send_command(&engine, Command::RefreshDevices)
            .expect_err("disposed engine must reject commands");
        assert!(err.contains("engine stopped"), "{err}");
    }
}

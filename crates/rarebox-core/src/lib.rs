//! RareBox 的无界面核心。
//!
//! 这个 crate 不依赖 GPUI 或任何 UI 框架，桌面端、Android/Wear 客户端（JNI）
//! 与其他语言绑定都通过 `Engine` 共享同一套设备逻辑：
//!
//! ```ignore
//! let engine = Engine::new(Config::load());
//! let events = engine.events();
//! engine.command(Command::RefreshDevices)?;
//! let state = engine.state();
//! ```

pub mod adb;
pub mod adb_bundle;
pub mod adb_driver;
pub mod apk;
pub mod command;
pub mod config;
pub mod connect_support;
pub mod scan;
pub mod engine;
pub mod event;
pub mod guide;
pub mod hdc;
pub mod hdc_bundle;
pub mod install;
pub mod legal;
pub mod scrcpy;
pub mod platform;
pub mod signing;
pub mod state;
pub mod update;

mod logic;

pub use command::Command;
pub use engine::{Ctx, Engine};
pub use event::{
    DirectoryPurpose, Event, LegalDocuments, PickPurpose, VideoFrame,
};
pub use state::{
    ConfirmKind, ConfirmRequest, CoreState, DeviceKind, HdcInstallReport, HarmonyState, LogFilter,
    PackageScope, RemotePhase, TerminalEntry, ToastKind, UpdateData,
};

pub use logic::debug::ime_display;
pub use logic::logs::DISPLAY_LIMIT;
pub use logic::remote::{KEYCODE_APP_SWITCH, KEYCODE_BACK, KEYCODE_HOME, KEYCODE_POWER};
pub use logic::screen::rotation_label;

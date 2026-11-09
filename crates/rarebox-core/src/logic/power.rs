//! 重启：普通重启，以及进入 Recovery、Bootloader、Fastbootd 与 EDL。

use std::time::{Duration, Instant};

use crate::adb::RebootTarget;
use crate::engine::{Core, Ctx, Message};
use crate::state::{ConfirmKind, ToastKind};

impl Core {
    /// 普通重启直接执行；进入特殊模式会中断 ADB，先请用户确认。
    pub(crate) fn request_reboot(&mut self, target: RebootTarget, cx: &mut Ctx) {
        if !self.has_device() {
            cx.toast(ToastKind::Info, "请先连接手表再重启");
            return;
        }
        if target == RebootTarget::System {
            self.reboot_device(target, cx);
            return;
        }
        cx.confirm_kind(
            format!("重启到 {}？", target.label()),
            format!(
                "将向设备发送 adb reboot {}。{}",
                target.reboot_args()[0],
                target.outcome()
            ),
            "重启",
            ConfirmKind::Reboot(target),
        );
    }

    pub(crate) fn reboot_device(&mut self, target: RebootTarget, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };

        self.run_task(
            cx,
            format!("正在{}…", target.menu_label()),
            move || adb.reboot(&serial, target),
            move |core, result, cx| match result {
                Ok(output) => {
                    let detail = output.combined();
                    if !output.success() {
                        cx.toast(
                            ToastKind::Error,
                            format!("重启失败：{}", output.error_message()),
                        );
                        return;
                    }
                    if detail.trim().is_empty() {
                        cx.toast(
                            ToastKind::Success,
                            format!("已发送{}命令。{}", target.menu_label(), target.outcome()),
                        );
                    } else {
                        // 部分设备会把拒绝原因或警告写在退出码 0 的输出里。
                        cx.toast(
                            ToastKind::Info,
                            format!("{}：{}", target.menu_label(), detail.trim()),
                        );
                    }
                    let serial = core.state.selected_serial.clone().unwrap_or_default();
                    core.schedule_device_refresh(serial, cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("重启失败：{err:#}")),
            },
        );
    }

    /// 重启期间 ADB 会断开，设备重新上线没有固定时长（模拟器约 15 秒，真机更久）。
    /// 所以不猜时间点，而是轮询 `adb devices` 直到原先那台设备回到 `device` 状态。
    ///
    /// 掉线期间只刷新一次界面：每次轮询都刷新会把页面状态反复清空，
    /// 用户会看到列表和滚动位置不停跳变。
    fn schedule_device_refresh(&mut self, serial: String, cx: &mut Ctx) {
        let Some(adb) = self.adb_client().ok() else {
            return;
        };
        let target = serial.clone();
        let done = cx.done.clone();
        std::thread::Builder::new()
            .name("rarebox-reboot-watch".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(1_200));
                let deadline = Instant::now() + Duration::from_secs(300);
                let mut reported_down = false;
                while Instant::now() < deadline {
                    let devices = adb.devices().ok();
                    let state = devices
                        .and_then(|list| list.into_iter().find(|device| device.serial == target));
                    let online = state.as_ref().is_some_and(|device| device.is_online());
                    if online {
                        let resumed = reported_down;
                        let _ = done.send_blocking(Message::Done(Box::new(move |core, cx| {
                            if resumed {
                                cx.toast(ToastKind::Success, "设备已重新连接");
                            }
                            core.refresh_devices(cx);
                        })));
                        return;
                    }
                    if !reported_down {
                        reported_down = true;
                        let _ = done.send_blocking(Message::Done(Box::new(|core, cx| {
                            core.refresh_devices(cx);
                        })));
                    }
                    std::thread::sleep(Duration::from_millis(1_500));
                }
                let _ = done.send_blocking(Message::Done(Box::new(|core, cx| {
                    core.refresh_devices(cx);
                    cx.toast(
                        ToastKind::Info,
                        "设备还没有重新上线，可以稍后点「刷新列表」再试",
                    );
                })));
            })
            .ok();
    }
}

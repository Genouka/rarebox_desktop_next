//! 文件选择：NAPI ⇄ ArkTS `picker.DocumentViewPicker`。
//!
//! GPUI 的 `Platform::prompt_for_paths` 返回一个 `oneshot::Receiver`，调用方
//! 在背景 executor 上 await。鸿蒙的文件选择器只能从 ArkTS 调起，因此流程是：
//!
//! 1. [`prompt`] 分配 request id，登记 oneshot sender；
//! 2. 通过 `on_event` 向 ArkTS 发 `pickFiles` 事件（带 id 与选项）；
//! 3. ArkTS 打开 `picker.DocumentViewPicker`，完成后调用 NAPI
//!    `resolve_picker(requestId, pathsJson)`；
//! 4. [`resolve`] 把结果送回 oneshot。
//!
//! 渲染线程从不阻塞等待 picker；超时/取消由 ArkTS 侧调用 `resolve_picker(id, "[]")`。

use futures::channel::oneshot;
use gpui::PathPromptOptions;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

type Sender = oneshot::Sender<anyhow::Result<Option<Vec<PathBuf>>>>;

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<HashMap<u64, Sender>>> = Mutex::new(None);

fn pending() -> parking_lot::MutexGuard<'static, Option<HashMap<u64, Sender>>> {
    let mut guard = PENDING.lock();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

/// 发起一次文件选择。
pub fn prompt(options: PathPromptOptions) -> oneshot::Receiver<anyhow::Result<Option<Vec<PathBuf>>>> {
    let (sender, receiver) = oneshot::channel();
    let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    pending()
        .as_mut()
        .expect("initialized above")
        .insert(request_id, sender);

    let payload = serde_json::json!({
        "id": request_id,
        "files": options.files,
        "directories": options.directories,
        "multiple": options.multiple,
        "prompt": options.prompt.as_ref().map(|prompt| prompt.to_string()),
    });
    crate::host::emit("pickFiles", &payload.to_string());
    receiver
}

/// ArkTS 的选择结果。`paths` 为空表示用户取消。
pub fn resolve(request_id: u64, paths: Vec<PathBuf>) {
    let sender = pending()
        .as_mut()
        .and_then(|pending| pending.remove(&request_id));
    let Some(sender) = sender else {
        log::warn!("resolve_picker: unknown request {request_id}");
        return;
    };
    let result = if paths.is_empty() {
        Ok(None)
    } else {
        Ok(Some(paths))
    };
    if sender.send(result).is_err() {
        log::debug!("resolve_picker: caller dropped request {request_id}");
    }
}

/// 从 JSON 字符串解析路径数组（NAPI 入口用）。
pub fn parse_paths(json: &str) -> anyhow::Result<Vec<PathBuf>> {
    let values: Vec<String> = serde_json::from_str(json)
        .map_err(|error| anyhow::anyhow!("pathsJson must be a JSON string array: {error}"))?;
    Ok(values.into_iter().map(PathBuf::from).collect())
}

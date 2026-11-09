//! 引擎向宿主发出的事件。
//!
//! 事件是单向的：宿主只读，不修改引擎状态。需要读取完整状态时使用
//! `Engine::state()`（返回最近一次发布的可克隆快照）。


use crate::state::{ConfirmRequest, ToastKind};

/// 需要宿主弹出文件选择器的用途。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickPurpose {
    InstallApk,
    InstallHap,
    PushFiles,
    PushHdcFiles,
    SigningHap,
    SigningImportFile(crate::signing::ImportFile),
}

/// 需要宿主选择目录的用途。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryPurpose {
    Screenshot,
}

/// 从服务端拉取到的法律条款。
#[derive(Clone, Debug, Default)]
pub struct LegalDocuments {
    pub eula: Option<String>,
    pub privacy: Option<String>,
    pub error: Option<String>,
}

/// 一帧已解码的设备画面（BGRA）。
#[derive(Clone)]
pub struct VideoFrame {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoFrame")
            .field("generation", &self.generation)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("bytes", &self.bgra.len())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    /// 状态已更新；宿主可读取 `Engine::state()`。
    Changed,
    Toast {
        kind: ToastKind,
        message: String,
    },
    /// 需要宿主展示二次确认框；用户确认后由宿主发送对应命令。
    Confirm(ConfirmRequest),
    /// 需要宿主打开文件选择器。
    PickFiles(PickPurpose),
    /// 需要宿主选择目录。
    PickDirectory(DirectoryPurpose),
    /// 需要宿主用系统浏览器打开链接。
    OpenUrl(String),
    /// 需要宿主在系统文件管理器中打开路径。
    OpenPath(String),
    /// 需要宿主写入剪贴板。
    CopyText(String),
    /// 法律条款拉取完成（失败时仍带内置回退文案）。
    Legal(LegalDocuments),
    /// 远程画面的一帧。
    Video(VideoFrame),
    /// 远程会话异常结束。
    VideoEnded {
        generation: u64,
        reason: String,
    },
    /// 宿主应退出进程（例如拒绝条款）。
    Quit,
}

impl Event {
    /// 把事件编码为跨语言（JNI / C ABI）的 JSON。
    ///
    /// 画面帧不在这里传输：`Video` 用独立的二进制通道，避免每秒上百次 base64。
    pub fn to_protocol_json(&self) -> Option<String> {
        use serde_json::json;
        let value = match self {
            Event::Changed => json!({ "event": "changed" }),
            Event::Toast { kind, message } => json!({
                "event": "toast",
                "kind": match kind {
                    ToastKind::Info => "info",
                    ToastKind::Success => "success",
                    ToastKind::Error => "error",
                },
                "message": message,
            }),
            Event::Confirm(request) => json!({
                "event": "confirm",
                "title": request.title,
                "message": request.message,
                "confirmLabel": request.confirm_label,
                "kind": serde_json::to_value(&request.kind).unwrap_or_default(),
            }),
            Event::PickFiles(purpose) => json!({
                "event": "pick_files",
                "purpose": serde_json::to_value(purpose).unwrap_or_default(),
            }),
            Event::PickDirectory(purpose) => json!({
                "event": "pick_directory",
                "purpose": serde_json::to_value(purpose).unwrap_or_default(),
            }),
            Event::OpenUrl(url) => json!({ "event": "open_url", "url": url }),
            Event::OpenPath(path) => json!({ "event": "open_path", "path": path }),
            Event::CopyText(text) => json!({ "event": "copy_text", "text": text }),
            Event::Legal(docs) => json!({
                "event": "legal",
                "eula": docs.eula,
                "privacy": docs.privacy,
                "error": docs.error,
            }),
            Event::VideoEnded { generation, reason } => json!({
                "event": "video_ended",
                "generation": generation,
                "reason": reason,
            }),
            Event::Quit => json!({ "event": "quit" }),
            Event::Video(_) => return None,
        };
        Some(value.to_string())
    }
}

//! 文件传输页：浏览设备目录、拉取文件与上传本地文件。

use std::path::{Path, PathBuf};

use anyhow::bail;

use crate::config::{downloads_dir, short_path};
use crate::engine::{Core, Ctx};
use crate::event::PickPurpose;
use crate::state::ToastKind;

impl Core {
    pub(crate) fn list_files(&mut self, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let path = self.state.files_path.clone();
        self.run_task(
            cx,
            format!("正在读取 {path}…"),
            move || {
                let target = if path == "/" {
                    path.clone()
                } else {
                    format!("{path}/")
                };
                let command = format!("ls -la {}", shell_quote(&target));
                let output = adb.shell(&serial, &command)?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(crate::adb::parse_ls(&output.stdout))
            },
            move |core, result, cx| match result {
                Ok(entries) => {
                    core.state.files_entries = entries;
                    core.state.files_selected = None;
                }
                Err(err) => {
                    core.state.files_entries.clear();
                    cx.toast(ToastKind::Error, format!("读取目录失败：{err:#}"));
                }
            },
        );
    }

    pub(crate) fn navigate_files(&mut self, path: String, cx: &mut Ctx) {
        let mut target = path.trim().to_string();
        if target.is_empty() {
            target = "/sdcard".to_string();
        }
        if !target.starts_with('/') {
            target = format!("/{target}");
        }
        while target.len() > 1 && target.ends_with('/') {
            target.pop();
        }
        self.state.files_path = target;
        self.state.files_entries.clear();
        self.state.files_selected = None;
        self.list_files(cx);
    }

    pub(crate) fn go_parent_files(&mut self, cx: &mut Ctx) {
        let current = self.state.files_path.trim_end_matches('/');
        let parent = match current.rfind('/') {
            Some(0) | None => "/".to_string(),
            Some(index) => current[..index].to_string(),
        };
        self.navigate_files(parent, cx);
    }

    pub(crate) fn pull_file(&mut self, remote: String, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let name = remote
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("rarebox-file")
            .to_string();
        let task_name = name.clone();
        self.run_task(
            cx,
            format!("正在拉取 {name}…"),
            move || {
                let local = unique_path(&downloads_dir().join(&task_name));
                let local_string = local.to_string_lossy().to_string();
                let output =
                    adb.serial_command(&serial, &["pull", remote.as_str(), local_string.as_str()])?;
                if !output.success() {
                    bail!("{}", output.error_message());
                }
                Ok(local)
            },
            move |core, result, cx| match result {
                Ok(path) => {
                    core.state.last_export = Some(path.clone());
                    cx.toast(
                        ToastKind::Success,
                        format!("{name} 已保存到 {}", short_path(&path)),
                    );
                }
                Err(err) => cx.toast(ToastKind::Error, format!("拉取失败：{err:#}")),
            },
        );
    }

    pub(crate) fn pick_push_files(&mut self, cx: &mut Ctx) {
        cx.pick_files(PickPurpose::PushFiles);
    }

    pub(crate) fn push_files(&mut self, files: Vec<PathBuf>, cx: &mut Ctx) {
        let result = self.require_device();
        let Ok((adb, serial)) = result else {
            if let Err(err) = result {
                cx.toast(ToastKind::Error, format!("{err:#}"));
            }
            return;
        };
        let directory = self.state.files_path.clone();
        let count = files.len();
        self.run_task(
            cx,
            format!("正在上传 {count} 个文件…"),
            move || {
                for file in &files {
                    let source = file.to_string_lossy().to_string();
                    let output = adb
                        .serial_command(&serial, &["push", source.as_str(), directory.as_str()])?;
                    if !output.success() {
                        bail!("{}", output.error_message());
                    }
                }
                Ok(count)
            },
            move |core, result, cx| match result {
                Ok(count) => {
                    cx.toast(ToastKind::Success, format!("已上传 {count} 个文件"));
                    core.list_files(cx);
                }
                Err(err) => cx.toast(ToastKind::Error, format!("上传失败：{err:#}")),
            },
        );
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn unique_path(base: &Path) -> PathBuf {
    if !base.exists() {
        return base.to_path_buf();
    }
    let parent = base.parent().unwrap_or_else(|| Path::new("."));
    let stem = base
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".to_string());
    let extension = base
        .extension()
        .map(|extension| extension.to_string_lossy().to_string());
    for index in 1..1000 {
        let name = match &extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    base.to_path_buf()
}

//! 面向设备的便捷 API：把 ADB 服务映射成 RareBox 需要的能力。
//!
//! | 现有 `AdbClient` 调用 | 本 crate 的替换方式 |
//! |----------------------|---------------------|
//! | `devices`            | [`AdbClient::connect`] 探测端点 + [`DeviceInfo`]（CNXN banner）|
//! | `shell`              | [`AdbClient::open_shell`] / [`AdbClient::exec_out`] |
//! | `exec-out`           | [`AdbClient::exec_out`] / [`AdbClient::exec_out_stream`] |
//! | `push`               | [`AdbClient::push_bytes`] / [`AdbClient::push_file`] |
//! | `pull`               | [`AdbClient::pull`] / [`AdbClient::pull_to`] |
//! | `install`            | [`AdbClient::install`]（push 到 `/data/local/tmp` 后 `pm install`）|
//! | `reboot`             | [`AdbClient::reboot`]（`reboot:<target>` 服务）|
//! | scrcpy 的 forward    | [`AdbClient::forward_tcp`] + [`AdbClient::local_socket`] |

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::AdbKey;
use crate::conn::{ConnectOptions, Connection};
use crate::error::{Error, Result};
use crate::forward::{self, ForwardHandle};
use crate::stream::AdbStream;
use crate::sync::{DirEntry, FileStat, SyncSession};

/// 设备在 CNXN banner 里通告的信息。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    /// 原始 banner。
    pub banner: String,
    /// `::` 之前的部分：`device` / `host` / `bootloader` 等。
    pub kind: String,
    /// banner 里的 `key=value`（`features` 除外）。
    pub props: BTreeMap<String, String>,
    /// `features=` 列表。
    pub features: Vec<String>,
}

impl DeviceInfo {
    /// 解析 `device::ro.product.name=...;features=shell_v2,cmd;` 形式的 banner。
    pub fn parse(banner: &str) -> Self {
        let (kind, rest) = match banner.split_once("::") {
            Some((kind, rest)) => (kind.to_string(), rest),
            None => (String::new(), banner),
        };
        let mut props = BTreeMap::new();
        let mut features = Vec::new();
        for part in rest.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            if key == "features" {
                features = value
                    .split(',')
                    .filter(|feature| !feature.is_empty())
                    .map(str::to_string)
                    .collect();
            } else {
                props.insert(key.to_string(), value.to_string());
            }
        }
        Self {
            banner: banner.to_string(),
            kind,
            props,
            features,
        }
    }

    pub fn model(&self) -> Option<&str> {
        self.props.get("ro.product.model").map(String::as_str)
    }

    pub fn product(&self) -> Option<&str> {
        self.props.get("ro.product.name").map(String::as_str)
    }

    pub fn device(&self) -> Option<&str> {
        self.props.get("ro.product.device").map(String::as_str)
    }

    pub fn serial(&self) -> Option<&str> {
        self.props.get("ro.serialno").map(String::as_str)
    }

    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|item| item == feature)
    }
}

/// 已连接的一台设备。
#[derive(Clone)]
pub struct AdbClient {
    conn: Connection,
    info: DeviceInfo,
    serial: String,
}

impl AdbClient {
    /// 连接并完成握手（含鉴权）。
    pub fn connect(
        addr: impl ToSocketAddrs,
        key: &AdbKey,
        options: &ConnectOptions,
    ) -> Result<Self> {
        let conn = Connection::connect(addr, key, options)?;
        let info = DeviceInfo::parse(conn.banner());
        let serial = conn
            .peer_addr()
            .map(|addr| addr.to_string())
            .or_else(|| info.serial().map(str::to_string))
            .unwrap_or_default();
        Ok(Self { conn, info, serial })
    }

    /// 使用默认参数连接（15s 握手超时、1 MiB payload）。
    pub fn connect_default(addr: impl ToSocketAddrs, key: &AdbKey) -> Result<Self> {
        Self::connect(addr, key, &ConnectOptions::default())
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn device_info(&self) -> &DeviceInfo {
        &self.info
    }

    /// 本连接的端点标识（`ip:port`），可当作设备序列号使用。
    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn banner(&self) -> &str {
        self.conn.banner()
    }

    pub fn features(&self) -> &[String] {
        &self.info.features
    }

    /// 打开 `shell:<command>`，返回可增量读写的流（stdout/stderr 混在一起）。
    ///
    /// `serial` 为 `None` 时不做校验；传入时必须与 [`serial`](Self::serial)
    /// 或 banner 里的 `ro.serialno` 一致，便于上层按设备分发。
    pub fn open_shell(&self, serial: Option<&str>, command: &str) -> Result<AdbStream> {
        self.check_serial(serial)?;
        self.conn.open_service(&format!("shell:{command}"))
    }

    /// 执行命令并读取全部 stdout（二进制安全），对应 `adb exec-out`。
    pub fn exec_out(&self, command: &str) -> Result<Vec<u8>> {
        self.exec_out_stream(command)?.read_to_end_bytes()
    }

    /// 打开 `exec:<command>` 流，适合边收边处理大输出（如截屏、gzip 元数据）。
    pub fn exec_out_stream(&self, command: &str) -> Result<AdbStream> {
        self.conn.open_service(&format!("exec:{command}"))
    }

    /// 打开 `localabstract:<name>`，scrcpy 的服务端就监听在这里。
    pub fn local_socket(&self, name: &str) -> Result<AdbStream> {
        if name.contains(':') {
            self.conn.open_service(name)
        } else {
            self.conn.open_service(&format!("localabstract:{name}"))
        }
    }

    /// 打开一条 `sync:` 会话。
    pub fn sync(&self) -> Result<SyncSession> {
        SyncSession::open(&self.conn)
    }

    pub fn push_bytes(&self, remote: &str, data: &[u8], mode: u32, mtime: u32) -> Result<()> {
        self.sync()?.push(remote, mode, mtime, data)
    }

    /// 推送本地文件，mode 固定 `0o644`、mtime 取自文件系统。
    pub fn push_file(&self, local: &Path, remote: &str) -> Result<u64> {
        self.push_file_with_mode(local, remote, 0o644)
    }

    pub fn push_file_with_mode(&self, local: &Path, remote: &str, mode: u32) -> Result<u64> {
        let mut file = fs::File::open(local)?;
        let mtime = file_mtime(local);
        self.sync()?.push_reader(remote, mode, mtime, &mut file)
    }

    pub fn pull(&self, remote: &str) -> Result<Vec<u8>> {
        self.sync()?.pull(remote)
    }

    pub fn pull_to(&self, remote: &str, writer: &mut impl std::io::Write) -> Result<u64> {
        self.sync()?.pull_to(remote, writer)
    }

    pub fn stat(&self, remote: &str) -> Result<FileStat> {
        self.sync()?.stat(remote)
    }

    pub fn list(&self, remote: &str) -> Result<Vec<DirEntry>> {
        self.sync()?.list(remote)
    }

    /// 让设备重启到 `target`（`None` = 正常重启）。
    pub fn reboot(&self, target: Option<&str>) -> Result<()> {
        let service = match target {
            Some(target) if !target.is_empty() => format!("reboot:{target}"),
            _ => "reboot:".to_string(),
        };
        let stream = self.conn.open_service(&service)?;
        // 设备接受 reboot 后通常直接断开，CLSE 可能写不出去；OKAY 已代表接受。
        let _ = stream.close();
        Ok(())
    }

    /// 安装单个 APK：推送到 `/data/local/tmp`，再执行 `pm install`。
    pub fn install(&self, apk: &Path, options: &InstallOptions) -> Result<String> {
        self.install_multiple(std::slice::from_ref(&apk.to_path_buf()), options)
    }

    /// 安装一组 APK（分包安装会使用 `pm install-multiple`，顺序即传入顺序）。
    pub fn install_multiple(&self, apks: &[PathBuf], options: &InstallOptions) -> Result<String> {
        if apks.is_empty() {
            return Err(Error::Protocol("没有要安装的 APK".to_string()));
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let dir = format!("/data/local/tmp/rarebox-{}-{stamp:x}", std::process::id());

        let mut remote_paths = Vec::with_capacity(apks.len());
        for (index, apk) in apks.iter().enumerate() {
            let name = if apks.len() == 1 {
                "base.apk".to_string()
            } else {
                format!("split-{index}.apk")
            };
            let remote = format!("{dir}/{name}");
            self.push_file(apk, &remote)?;
            remote_paths.push(remote);
        }

        let quoted: Vec<String> = remote_paths.iter().map(|path| shell_quote(path)).collect();
        let command = if apks.len() == 1 {
            "pm install"
        } else {
            "pm install-multiple"
        };
        let command = format!("{command}{} {}", options.flags(), quoted.join(" "));
        let output = self.exec_out(&command);
        if options.cleanup {
            let _ = self.exec_out(&format!("rm -rf {}", shell_quote(&dir)));
        }
        let output = output?;
        let text = String::from_utf8_lossy(&output).trim().to_string();
        if !text.contains("Success") {
            return Err(Error::DeviceError {
                operation: "install".to_string(),
                reason: if text.is_empty() {
                    "pm 没有返回安装结果".to_string()
                } else {
                    text
                },
            });
        }
        Ok(text)
    }

    /// 监听本地 TCP，把连接桥接到设备的 `localabstract:<local>`。
    pub fn forward_tcp(&self, listen: SocketAddr, local: &str) -> Result<ForwardHandle> {
        forward::forward_tcp(&self.conn, listen, local)
    }

    fn check_serial(&self, serial: Option<&str>) -> Result<()> {
        if let Some(requested) = serial
            && requested != self.serial
            && Some(requested) != self.info.serial()
        {
            return Err(Error::Protocol(format!(
                "设备不匹配：本连接是 {}，请求的是 {requested}",
                self.serial
            )));
        }
        Ok(())
    }
}

/// `adb install` 的常用开关。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOptions {
    /// `-r`：覆盖安装。
    pub replace: bool,
    /// `-g`：授予全部运行时权限。
    pub grant_permissions: bool,
    /// `-t`：允许 test-only APK。
    pub allow_test: bool,
    /// `-d`：允许版本降级。
    pub allow_downgrade: bool,
    /// 安装完成后删除设备上的临时 APK。
    pub cleanup: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            replace: true,
            grant_permissions: false,
            allow_test: false,
            allow_downgrade: false,
            cleanup: true,
        }
    }
}

impl InstallOptions {
    fn flags(&self) -> String {
        let mut flags = String::new();
        if self.replace {
            flags.push_str(" -r");
        }
        if self.grant_permissions {
            flags.push_str(" -g");
        }
        if self.allow_test {
            flags.push_str(" -t");
        }
        if self.allow_downgrade {
            flags.push_str(" -d");
        }
        flags
    }
}

/// 以 POSIX shell 规则转义单个参数。
pub fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-_./:@,=+".contains(ch))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn file_mtime(path: &Path) -> u32 {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs().min(u64::from(u32::MAX)) as u32)
        .unwrap_or(0)
}

impl fmt::Debug for AdbClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdbClient")
            .field("serial", &self.serial)
            .field("model", &self.info.model())
            .finish()
    }
}

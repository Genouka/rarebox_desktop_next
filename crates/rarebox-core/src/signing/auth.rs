//! 华为开发者账号登录：打开 DevEco 授权页，在 localhost 接收 tempToken。
//!
//! AGC 的 Eco 登录没有公开 OAuth 客户端，DevEco Studio 的做法是打开固定授权页，
//! 由页面把 `tempToken` POST 回 `127.0.0.1`。这里复现该流程，token 交换在
//! `api::EcoClient` 中完成。
//!
//! 监听器只绑定回环地址，避免触发 Windows 防火墙提示；登录失败、超时或用户
//! 取消都会在 200ms 内退出。

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

const LOGIN_HOST: &str = "https://cn.devecostudio.huawei.com";
const LOGIN_APP_ID: &str = "1007";
const LOGIN_CODE: &str = "20698961dd4f420c8b44f49010c6f0cc";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(180);

pub struct LoginSession {
    listener: TcpListener,
    cancel: Arc<AtomicBool>,
    port: u16,
    timeout: Duration,
}

impl LoginSession {
    pub fn bind(cancel: Arc<AtomicBool>) -> Result<Self> {
        Self::bind_with_timeout(cancel, DEFAULT_TIMEOUT)
    }

    pub fn bind_with_timeout(cancel: Arc<AtomicBool>, timeout: Duration) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("无法绑定本地登录回调端口")?;
        listener
            .set_nonblocking(true)
            .context("无法设置登录回调端口为非阻塞")?;
        let port = listener
            .local_addr()
            .context("无法读取登录回调端口")?
            .port();
        Ok(Self {
            listener,
            cancel,
            port,
            timeout,
        })
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn login_url(&self) -> String {
        format!(
            "{LOGIN_HOST}/console/DevEcoIDE/apply?port={}&appid={LOGIN_APP_ID}&code={LOGIN_CODE}",
            self.port
        )
    }

    /// 等待浏览器回调并返回 tempToken。调用方负责在此之前打开 `login_url()`。
    pub fn wait_for_token(self) -> Result<String> {
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            if self.cancel.load(Ordering::Relaxed) {
                bail!("已取消登录");
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(token) = handle_connection(stream)? {
                        return Ok(token);
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(err) => return Err(err).context("接收登录回调失败"),
            }
        }
        bail!("登录超时，请重试")
    }

    /// 回调端口，供日志与排查使用。
    pub fn port_label(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

fn handle_connection(mut stream: TcpStream) -> Result<Option<String>> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .context("无法设置回调读取超时")?;
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if buffer.len() > 64 * 1024 {
            return Ok(None);
        }
        let read = match stream.read(&mut chunk) {
            Ok(0) => break None,
            Ok(read) => read,
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(None);
            }
            Err(_) => return Ok(None),
        };
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break Some(index);
        }
    };
    let Some(header_end) = header_end else {
        return Ok(None);
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or(target);
    let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");

    let content_length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0)
        .min(64 * 1024);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
            Err(_) => break,
        }
    }
    let body = String::from_utf8_lossy(
        &buffer[body_start..(body_start + content_length).min(buffer.len())],
    )
    .to_string();

    if path != "/callback" {
        let _ = write_response(&mut stream, 404, "Not Found", "<h1>404</h1>");
        return Ok(None);
    }

    let token = if method.eq_ignore_ascii_case("POST") {
        form_value(&body, "tempToken").or_else(|| form_value(query, "tempToken"))
    } else {
        form_value(query, "tempToken").or_else(|| form_value(&body, "tempToken"))
    };
    let Some(token) = token else {
        let _ = write_response(&mut stream, 400, "Bad Request", "<h1>缺少 tempToken</h1>");
        return Ok(None);
    };

    let html = r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>登录成功</title>
<style>body{font-family:system-ui,"Microsoft YaHei",sans-serif;background:#10151c;color:#e8eaed;display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}.card{background:#1a2029;border:1px solid #2b3441;border-radius:18px;padding:40px 48px;text-align:center;box-shadow:0 20px 60px #0008}.ok{font-size:42px;color:#58d6a9}h1{font-size:20px;margin:12px 0 8px}p{color:#9aa4b2;font-size:13px;margin:0}</style>
</head><body><div class="card"><div class="ok">✓</div><h1>登录成功</h1><p>请关闭此页面并返回 RareBox Desktop</p></div></body></html>"#;
    let _ = write_response(&mut stream, 200, "OK", html);
    Ok(Some(token))
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn write_response(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// 从 `application/x-www-form-urlencoded` 串中取字段，支持百分号编码与 `+`。
pub fn form_value(input: &str, key: &str) -> Option<String> {
    for pair in input.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name == key {
            return Some(percent_decode(value));
        }
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    output.push(high * 16 + low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_form_values() {
        assert_eq!(
            form_value("tempToken=abc%2Bdef&x=1", "tempToken").as_deref(),
            Some("abc+def")
        );
        assert_eq!(
            form_value("a=1&b=two+words", "b").as_deref(),
            Some("two words")
        );
        assert_eq!(form_value("a=1", "b"), None);
    }

    #[test]
    fn rejects_malformed_percent_escape() {
        assert_eq!(percent_decode("a%zzb"), "a%zzb");
    }

    #[test]
    fn receives_temp_token_from_localhost_callback() {
        use std::net::TcpStream;

        let cancel = Arc::new(AtomicBool::new(false));
        let session = LoginSession::bind_with_timeout(cancel, Duration::from_secs(5)).unwrap();
        let port = session.port();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let body = "tempToken=abc%2B123";
            let request = format!(
                "POST /callback HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(request.as_bytes()).unwrap();
            let mut response = [0u8; 256];
            let read = stream.read(&mut response).unwrap();
            assert!(String::from_utf8_lossy(&response[..read]).contains("200"));
        });
        let token = session.wait_for_token().unwrap();
        assert_eq!(token, "abc+123");
        client.join().unwrap();
    }
}

//! 局域网盲扫：地址/端口填错时，去「可能的值」里找设备。
//!
//! 设计要点：
//! - 只扫私有网段（RFC1918 / 链路本地 / 回环），公网地址必须先明确指定单机；
//! - 快扫：用户填的 IP + 本机 /24（可选），端口取常见调试端口与用户端口附近；
//! - 深度扫描：只对单个 IP 全端口，慢且可取消，必须用户主动开启；
//! - 端口开放不等于设备可达，协议校验（adb connect / hdc tconn）交给调用方，
//!   这里只负责并发 TCP 探测与进度。

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 常见 ADB / hdc 调试端口。
pub const COMMON_PORTS: &[u16] = &[
    5555, 5556, 5557, 5558, 5559, 5560, 5561, 5562, 5563, 5564, 5565, 8710, 8711,
];

/// 一次扫描的地址与端口集合。
#[derive(Clone, Debug)]
pub struct ScanPlan {
    pub hosts: Vec<Ipv4Addr>,
    pub ports: Vec<u16>,
    /// 用于界面展示的扫描范围描述。
    pub scope: String,
}

impl ScanPlan {
    pub fn total(&self) -> usize {
        self.hosts.len().saturating_mul(self.ports.len())
    }
}

/// 探测到的开放端口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPort {
    pub address: String,
}

/// 把用户输入拆成 host / port。支持 `1.2.3.4`、`1.2.3.4:5555`、`[::1]:5555`。
pub fn parse_endpoint(input: &str) -> (Option<String>, Option<u16>) {
    let input = input.trim();
    if input.is_empty() {
        return (None, None);
    }
    if let Some(rest) = input.strip_prefix('[') {
        // [IPv6]:port
        if let Some((host, port)) = rest.split_once("]:") {
            return (Some(host.to_string()), port.parse().ok());
        }
        return (Some(rest.trim_end_matches(']').to_string()), None);
    }
    match input.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => {
            (Some(host.trim().to_string()), port.parse().ok())
        }
        _ => (Some(input.to_string()), None),
    }
}

/// 是否属于私有/本地网段。
pub fn is_private(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

/// 本机用于访问局域网的 IPv4 地址。
///
/// 用 UDP `connect` 让系统选路（不会真的发包），避免引入网卡枚举依赖。
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:53").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// 一个 /24 网段里除自身、网络号、广播地址之外的主机。
pub fn subnet_hosts(local: Ipv4Addr) -> Vec<Ipv4Addr> {
    let octets = local.octets();
    let prefix = [octets[0], octets[1], octets[2]];
    (1..=254u8)
        .filter_map(|host| {
            let ip = Ipv4Addr::new(prefix[0], prefix[1], prefix[2], host);
            (ip != local).then_some(ip)
        })
        .collect()
}

/// 用户端口附近的候选（±10），方便「端口只差一位」的常见情况。
fn nearby_ports(port: u16) -> Vec<u16> {
    let low = port.saturating_sub(10).max(1);
    let high = port.saturating_add(10);
    (low..=high).collect()
}

/// 根据输入与模式生成扫描计划。
///
/// - `deep`：只对输入里的单个 IP 做 1..=65535；
/// - 输入带公网 IP 时不扩展到网段，只扫这一台；
/// - 输入为空时扫本机 /24。
pub fn build_plan(input: &str, deep: bool) -> ScanPlan {
    let (host, port) = parse_endpoint(input);
    let parsed = host
        .as_deref()
        .and_then(|host| host.parse::<Ipv4Addr>().ok());

    let mut ports: Vec<u16> = Vec::new();
    if deep && parsed.is_some() {
        ports.extend(1..=u16::MAX);
    } else {
        ports.extend_from_slice(COMMON_PORTS);
        if let Some(port) = port {
            for candidate in nearby_ports(port) {
                if !ports.contains(&candidate) {
                    ports.push(candidate);
                }
            }
        }
    }

    let (hosts, scope) = match parsed {
        Some(ip) if is_private(ip) && !deep => {
            // 明确私有 IP：优先扫这一台 + 同网段，覆盖「输错 IP」的情况。
            let mut hosts = vec![ip];
            for candidate in subnet_hosts(ip) {
                if !hosts.contains(&candidate) {
                    hosts.push(candidate);
                }
            }
            (hosts, format!("{ip} 与同网段"))
        }
        Some(ip) => (vec![ip], format!("{ip}")),
        None => match local_ipv4() {
            Some(local) => {
                let hosts = subnet_hosts(local);
                let scope = format!(
                    "{}.{}.{}.0/24",
                    local.octets()[0],
                    local.octets()[1],
                    local.octets()[2]
                );
                (hosts, scope)
            }
            None => (Vec::new(), "未找到本机网段".to_string()),
        },
    };

    ScanPlan {
        hosts,
        ports,
        scope,
    }
}

/// 并发探测开放端口。
///
/// - `cancel` 置位后尽快返回；
/// - `done` 每完成一个探测自增，供界面显示进度；
/// - 返回的端口按发现顺序排列。
pub fn probe(
    plan: &ScanPlan,
    timeout: Duration,
    concurrency: usize,
    cancel: &AtomicBool,
    done: &AtomicUsize,
) -> Vec<OpenPort> {
    let targets: Arc<Mutex<VecDeque<(Ipv4Addr, u16)>>> = Arc::new(Mutex::new(
        plan.hosts
            .iter()
            .flat_map(|host| plan.ports.iter().map(move |port| (*host, *port)))
            .collect(),
    ));
    let results: Arc<Mutex<Vec<OpenPort>>> = Arc::new(Mutex::new(Vec::new()));
    let concurrency = concurrency.max(1);

    std::thread::scope(|scope| {
        for _ in 0..concurrency {
            let targets = Arc::clone(&targets);
            let results = Arc::clone(&results);
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let Some((host, port)) = targets.lock().ok().and_then(|mut q| q.pop_front())
                    else {
                        return;
                    };
                    let address = SocketAddr::from((host, port));
                    if TcpStream::connect_timeout(&address, timeout).is_ok() {
                        if let Ok(mut hits) = results.lock() {
                            hits.push(OpenPort {
                                address: format!("{host}:{port}"),
                            });
                        }
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });

    Arc::try_unwrap(results)
        .map(|mutex| mutex.into_inner().unwrap_or_default())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_endpoints() {
        assert_eq!(
            parse_endpoint("192.168.1.5:5555"),
            (Some("192.168.1.5".to_string()), Some(5555))
        );
        assert_eq!(
            parse_endpoint("192.168.1.5"),
            (Some("192.168.1.5".to_string()), None)
        );
        assert_eq!(
            parse_endpoint("[fe80::1]:8710"),
            (Some("fe80::1".to_string()), Some(8710))
        );
        assert_eq!(parse_endpoint("  "), (None, None));
    }

    #[test]
    fn private_ranges() {
        assert!(is_private(Ipv4Addr::new(192, 168, 1, 4)));
        assert!(is_private(Ipv4Addr::new(10, 0, 0, 8)));
        assert!(is_private(Ipv4Addr::new(172, 16, 3, 2)));
        assert!(is_private(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_private(Ipv4Addr::new(8, 8, 8, 8)));
    }

    #[test]
    fn subnet_excludes_self_and_edges() {
        let hosts = subnet_hosts(Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(hosts.len(), 253);
        assert!(!hosts.contains(&Ipv4Addr::new(192, 168, 1, 20)));
        assert!(hosts.contains(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(hosts.contains(&Ipv4Addr::new(192, 168, 1, 254)));
    }

    #[test]
    fn plan_uses_common_ports_and_nearby() {
        let plan = build_plan("192.168.1.5:5557", false);
        assert!(plan.ports.contains(&5555));
        assert!(plan.ports.contains(&5557));
        assert!(plan.ports.contains(&8710));
        // 明确私有 IP 时同时覆盖同网段。
        assert!(plan.hosts.len() > 1);
    }

    #[test]
    fn plan_deep_scan_is_single_host_all_ports() {
        let plan = build_plan("192.168.1.5:5555", true);
        assert_eq!(plan.hosts.len(), 1);
        assert_eq!(plan.ports.len(), 65535);
    }

    #[test]
    fn plan_public_ip_stays_single_host() {
        let plan = build_plan("8.8.8.8", false);
        assert_eq!(plan.hosts, vec![Ipv4Addr::new(8, 8, 8, 8)]);
        assert!(plan.ports.contains(&5555));
    }

    #[test]
    fn probe_finds_local_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let plan = ScanPlan {
            hosts: vec![Ipv4Addr::LOCALHOST],
            ports: vec![port],
            scope: "test".to_string(),
        };
        let cancel = AtomicBool::new(false);
        let done = AtomicUsize::new(0);
        let hits = probe(&plan, Duration::from_millis(300), 4, &cancel, &done);
        assert_eq!(hits.len(), 1);
        assert_eq!(done.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn probe_respects_cancel() {
        let plan = ScanPlan {
            hosts: vec![Ipv4Addr::new(192, 0, 2, 1)],
            ports: vec![5555, 5556, 5557],
            scope: "test".to_string(),
        };
        let cancel = AtomicBool::new(true);
        let done = AtomicUsize::new(0);
        let hits = probe(&plan, Duration::from_millis(50), 4, &cancel, &done);
        assert!(hits.is_empty());
    }
}

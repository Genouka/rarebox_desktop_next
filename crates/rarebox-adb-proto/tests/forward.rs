//! 转发桥：本地 TCP ↔ `localabstract:` 双向搬运、shutdown 语义。

mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::LazyLock;
use std::time::Duration;

use rarebox_adb_proto::{AdbClient, AdbKey};
use support::{FakeAdbd, FakeConfig};

static TEST_KEY: LazyLock<AdbKey> =
    LazyLock::new(|| AdbKey::generate().expect("generate test key"));

#[test]
fn bridges_tcp_to_localabstract_and_back() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let open = session.accept_open().unwrap().expect("forward must OPEN");
        assert_eq!(open.service, "localabstract:scrcpy");
        while let Some(chunk) = session.read_some(open.local_id).unwrap() {
            session
                .send_wrte(open.local_id, open.remote_id, &chunk)
                .unwrap();
        }
    });

    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    let handle = client
        .forward_tcp("127.0.0.1:0".parse().expect("addr"), "scrcpy")
        .expect("forward_tcp");
    let addr = handle.local_addr();

    let mut tcp =
        TcpStream::connect_timeout(&addr, Duration::from_secs(2)).expect("connect bridge");
    tcp.set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout");

    tcp.write_all(b"hello scrcpy").expect("write");
    let mut echoed = [0u8; 12];
    tcp.read_exact(&mut echoed).expect("read echo");
    assert_eq!(&echoed, b"hello scrcpy");

    tcp.write_all(&[0u8, 1, 2, 3, 255]).expect("binary write");
    let mut echoed = [0u8; 5];
    tcp.read_exact(&mut echoed).expect("binary read");
    assert_eq!(echoed, [0u8, 1, 2, 3, 255]);

    handle.shutdown();

    let refused = TcpStream::connect_timeout(&addr, Duration::from_millis(500));
    assert!(refused.is_err(), "listener must be released by shutdown");

    let mut byte = [0u8; 1];
    match tcp.read(&mut byte) {
        Ok(0) | Err(_) => {}
        Ok(read) => panic!("existing connection still delivered {read} bytes after shutdown"),
    }
}

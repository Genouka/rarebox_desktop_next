//! 流层：跨 `WRTE` 重组、并发流隔离、写流控与三类错误路径。

mod support;

use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use rarebox_adb_proto::{AdbClient, AdbKey, Error};
use support::{C_CLSE, C_OPEN, C_WRTE, FakeAdbd, FakeConfig};

static TEST_KEY: LazyLock<AdbKey> =
    LazyLock::new(|| AdbKey::generate().expect("generate test key"));

fn connect(fake: &FakeAdbd) -> AdbClient {
    AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect")
}

#[test]
fn reassembles_output_split_across_wrte_packets() {
    const SIZE: usize = 200_000;
    let expected: Vec<u8> = (0..SIZE).map(|index| (index % 251) as u8).collect();
    let payload = expected.clone();

    let fake = FakeAdbd::spawn(
        FakeConfig {
            maxdata: 4096,
            ..Default::default()
        },
        move |session| {
            let open = session.accept_open().unwrap().expect("open");
            assert_eq!(open.service, "exec:cat /sdcard/big.bin");
            for chunk in payload.chunks(4096) {
                session
                    .send_wrte(open.local_id, open.remote_id, chunk)
                    .unwrap();
            }
            session
                .send_close(open.local_id, open.remote_id, "")
                .unwrap();
            let packets = payload.chunks(4096).count();
            session.expect_okays(packets).unwrap();
            assert_eq!(session.records.lock().unwrap().wrte_sent, packets);
        },
    );

    let client = connect(&fake);
    let output = client.exec_out("cat /sdcard/big.bin").expect("exec_out");
    assert_eq!(output.len(), SIZE);
    assert_eq!(output, expected);
    let expected_packets = expected.chunks(4096).count();
    assert_eq!(
        fake.records.lock().unwrap().okay_from_host,
        expected_packets,
        "host must acknowledge every WRTE exactly once"
    );
}

#[test]
fn routes_two_concurrent_streams_without_cross_talk() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let first = session.accept_open().unwrap().expect("first open");
        let second = session.accept_open().unwrap().expect("second open");
        assert_eq!(first.service, "shell:first");
        assert_eq!(second.service, "shell:second");

        for index in 0..8u8 {
            session
                .send_wrte(first.local_id, first.remote_id, &[b'A', index])
                .unwrap();
            session
                .send_wrte(second.local_id, second.remote_id, &[b'B', index])
                .unwrap();
        }
        session
            .send_close(first.local_id, first.remote_id, "")
            .unwrap();
        session
            .send_close(second.local_id, second.remote_id, "")
            .unwrap();
        session.expect_okays(16).unwrap();
    });

    let client = connect(&fake);
    let first = client.open_shell(None, "first").expect("open first");
    let second = client.open_shell(None, "second").expect("open second");
    let (first_data, second_data) = thread::scope(|scope| {
        let a = scope.spawn(|| first.read_to_end_bytes().expect("read first"));
        let b = scope.spawn(|| second.read_to_end_bytes().expect("read second"));
        (a.join().unwrap(), b.join().unwrap())
    });

    let expected_first: Vec<u8> = (0..8u8).flat_map(|index| [b'A', index]).collect();
    let expected_second: Vec<u8> = (0..8u8).flat_map(|index| [b'B', index]).collect();
    assert_eq!(first_data, expected_first);
    assert_eq!(second_data, expected_second);
    assert_eq!(fake.opens(), vec!["shell:first", "shell:second"]);
}

#[test]
fn writes_stream_data_to_device_with_flow_control() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();

    let fake = FakeAdbd::spawn(
        FakeConfig {
            maxdata: 4096,
            ..Default::default()
        },
        move |session| {
            let open = session.accept_open().unwrap().expect("open");
            let mut collected = Vec::new();
            while let Some(chunk) = session.read_some(open.local_id).unwrap() {
                collected.extend(chunk);
            }
            *sink.lock().unwrap() = collected;
        },
    );
    let client = connect(&fake);
    let stream = client
        .open_shell(None, "cat > /dev/null")
        .expect("open shell");
    let payload = vec![7u8; 100_000];
    stream.write_all(&payload).expect("write payload");
    stream.close().expect("close stream");
    drop(stream);
    drop(client);
    drop(fake);

    assert_eq!(*received.lock().unwrap(), payload);
}

#[test]
fn checksum_mismatch_is_reported() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let open = session.accept_open().unwrap().expect("open");
        session
            .send_raw_with_checksum(
                C_WRTE,
                open.remote_id,
                open.local_id,
                b"corrupt",
                0x1234_5678,
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));
    });
    let client = connect(&fake);
    let error = client.exec_out("whatever").expect_err("checksum must fail");
    assert!(
        matches!(error, Error::Checksum { .. }),
        "unexpected: {error:?}"
    );
    assert!(error.to_string().contains("校验和"), "{error}");
}

#[test]
fn unknown_message_is_reported() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let open = session.accept_open().unwrap().expect("open");
        session
            .send_raw(0xdead_beef, 0, open.local_id, b"")
            .unwrap();
        thread::sleep(Duration::from_millis(100));
    });
    let client = connect(&fake);
    let error = client
        .exec_out("whatever")
        .expect_err("unknown message must fail");
    match error {
        Error::Protocol(message) => assert!(message.contains("未预期"), "{message}"),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn premature_disconnect_is_reported() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let _open = session.accept_open().unwrap().expect("open");
        thread::sleep(Duration::from_millis(100));
        session.abort();
    });
    let client = connect(&fake);
    let error = client.exec_out("sleep 100").expect_err("must fail");
    match error {
        Error::ConnectionClosed(message) => {
            assert!(
                message.contains("断开") || message.contains("关闭"),
                "{message}"
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn close_reason_is_surfaced_from_device() {
    const REASON: &str = "shell: /system/bin/sh: nope: not found";
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let packet = session.next_packet().unwrap().expect("open packet");
        assert_eq!(packet.command, C_OPEN);
        let service =
            String::from_utf8_lossy(&packet.payload[..packet.payload.len() - 1]).to_string();
        assert_eq!(service, "shell:nope");
        session
            .send_raw(C_CLSE, 0, packet.arg0, REASON.as_bytes())
            .unwrap();
        thread::sleep(Duration::from_millis(100));
    });

    let client = connect(&fake);
    let error = client.open_shell(None, "nope").expect_err("device rejects");
    match error {
        Error::DeviceClosed { service, reason } => {
            assert_eq!(service, "shell:nope");
            assert_eq!(reason, REASON);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn rejects_serial_for_another_device() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |_session| {});
    let client = connect(&fake);
    let error = client
        .open_shell(Some("not-this-device"), "id")
        .expect_err("serial must be checked");
    assert!(matches!(error, Error::Protocol(_)), "unexpected: {error:?}");
}

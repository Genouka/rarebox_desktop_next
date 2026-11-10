//! sync 协议：push / pull / stat / list 的字节级校验与设备端错误透传。

mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rarebox_adb_proto::{AdbClient, AdbKey, Error, FileStat};
use support::{FakeAdbd, FakeConfig, FakeFs, serve_sync};

static TEST_KEY: LazyLock<AdbKey> =
    LazyLock::new(|| AdbKey::generate().expect("generate test key"));

fn pull_payload() -> Vec<u8> {
    (0..100_000usize).map(|index| (index % 239) as u8).collect()
}

fn spawn_device(fs: Arc<Mutex<FakeFs>>) -> FakeAdbd {
    FakeAdbd::spawn(FakeConfig::default(), move |session| {
        while let Some(open) = session.accept_open().unwrap() {
            if open.service == "sync:" {
                serve_sync(session, &open, &fs).unwrap();
            } else {
                session
                    .send_close(open.local_id, open.remote_id, "unexpected service")
                    .unwrap();
            }
        }
    })
}

#[test]
fn push_preserves_bytes_mode_and_mtime() {
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let fake = spawn_device(fs.clone());
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");

    let payload: Vec<u8> = (0..150_000usize).map(|index| (index % 253) as u8).collect();
    client
        .push_bytes("/sdcard/rarebox/test.bin", &payload, 0o640, 1_700_000_000)
        .expect("push");

    let fs = fs.lock().unwrap();
    let upload = fs.uploads.last().expect("device received SEND");
    assert_eq!(upload.spec, "/sdcard/rarebox/test.bin,416");
    assert_eq!(upload.mtime, 1_700_000_000);
    let file = fs
        .files
        .get("/sdcard/rarebox/test.bin")
        .expect("file stored");
    assert_eq!(file.data, payload);
    assert_eq!(file.mode, 0o100_640, "device should create a regular file");
    assert_eq!(file.mtime, 1_700_000_000);
}

#[test]
fn pull_stat_and_list_are_byte_exact() {
    let payload = pull_payload();
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    {
        let mut fs = fs.lock().unwrap();
        fs.enable_dir("/sdcard/rarebox");
        fs.insert(
            "/sdcard/rarebox/pulled.bin",
            payload.clone(),
            0o100_644,
            1_650_000_000,
        );
        fs.insert(
            "/sdcard/rarebox/a.txt",
            b"aaa".to_vec(),
            0o100_640,
            1_650_000_001,
        );
        fs.insert(
            "/sdcard/rarebox/sub/b.txt",
            b"bbb".to_vec(),
            0o100_600,
            1_650_000_002,
        );
    }
    let fake = spawn_device(fs);
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");

    let pulled = client.pull("/sdcard/rarebox/pulled.bin").expect("pull");
    assert_eq!(pulled.len(), payload.len());
    assert_eq!(pulled, payload);

    let mut streamed = Vec::new();
    let written = client
        .pull_to("/sdcard/rarebox/pulled.bin", &mut streamed)
        .expect("pull_to");
    assert_eq!(written, payload.len() as u64);
    assert_eq!(streamed, payload);

    let stat = client.stat("/sdcard/rarebox/a.txt").expect("stat");
    assert_eq!(
        stat,
        FileStat {
            mode: 0o100_640,
            size: 3,
            mtime: 1_650_000_001,
        }
    );
    assert!(stat.is_file());
    assert_eq!(stat.permissions(), 0o640);

    let entries = client.list("/sdcard/rarebox").expect("list");
    let by_name: BTreeMap<String, _> = entries
        .into_iter()
        .map(|entry| (entry.name.clone(), entry))
        .collect();
    assert_eq!(
        by_name.keys().cloned().collect::<Vec<_>>(),
        vec!["a.txt", "pulled.bin", "sub"]
    );
    assert_eq!(by_name["a.txt"].size, 3);
    assert_eq!(by_name["a.txt"].mtime, 1_650_000_001);
    assert_eq!(by_name["a.txt"].mode, 0o100_640);
    assert!(by_name["sub"].is_dir());
}

#[test]
fn push_failure_carries_device_reason() {
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let fake = spawn_device(fs);
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");

    let error = client
        .push_bytes("/sdcard/rarebox/denied.bin", b"payload", 0o644, 0)
        .expect_err("device must reject");
    match error {
        Error::DeviceError { operation, reason } => {
            assert!(operation.starts_with("push"), "{operation}");
            assert_eq!(reason, "couldn't create file: Permission denied");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn pull_and_stat_failures_carry_reasons() {
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let fake = spawn_device(fs);
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");

    let error = client
        .pull("/sdcard/missing.bin")
        .expect_err("pull must fail");
    match error {
        Error::DeviceError { operation, reason } => {
            assert!(operation.starts_with("pull"), "{operation}");
            assert!(reason.contains("No such file or directory"), "{reason}");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let error = client
        .stat("/sdcard/missing.bin")
        .expect_err("stat must fail");
    match error {
        Error::DeviceError { operation, reason } => {
            assert_eq!(operation, "stat");
            assert!(reason.contains("v1"), "{reason}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn sync_stream_handles_large_push_without_deadlock() {
    // 100 KiB 远大于假 adbd 的 4 KiB maxdata：依赖 host 的分片与 OKAY 流控。
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let fake = FakeAdbd::spawn(
        FakeConfig {
            maxdata: 4096,
            ..Default::default()
        },
        {
            let fs = fs.clone();
            move |session| {
                while let Some(open) = session.accept_open().unwrap() {
                    if open.service == "sync:" {
                        serve_sync(session, &open, &fs).unwrap();
                    }
                }
            }
        },
    );
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    let payload = vec![0x42u8; 100_000];
    client
        .push_bytes("/sdcard/large.bin", &payload, 0o644, 1)
        .expect("large push");
    let fs = fs.lock().unwrap();
    assert_eq!(fs.files["/sdcard/large.bin"].data, payload);
    drop(fs);
    // 确保连接仍然可用。
    std::thread::sleep(Duration::from_millis(20));
    assert!(fake.records.lock().unwrap().wrte_from_host > 0);
}

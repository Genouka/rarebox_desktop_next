//! 高层命令映射：reboot 服务、install（push + pm install）与串口校验。

mod support;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rarebox_adb_proto::{AdbClient, AdbKey, Error, InstallOptions};
use support::{C_CLSE, FakeAdbd, FakeConfig, FakeFs, serve_sync};

static TEST_KEY: LazyLock<AdbKey> =
    LazyLock::new(|| AdbKey::generate().expect("generate test key"));

#[test]
fn reboot_opens_device_reboot_service() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        while let Some(open) = session.accept_open().unwrap() {
            assert_eq!(open.service, "reboot:bootloader");
            session
                .send_close(open.local_id, open.remote_id, "")
                .unwrap();
        }
    });
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    client.reboot(Some("bootloader")).expect("reboot accepted");
    assert_eq!(fake.opens(), vec!["reboot:bootloader"]);
}

#[test]
fn reboot_rejection_is_surfaced() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let packet = session.next_packet().unwrap().expect("open packet");
        assert_eq!(packet.command, support::C_OPEN);
        let service =
            String::from_utf8_lossy(&packet.payload[..packet.payload.len() - 1]).to_string();
        assert_eq!(service, "reboot:invalid");
        session
            .send_raw(C_CLSE, 0, packet.arg0, b"Invalid reboot target: invalid")
            .unwrap();
        thread::sleep(Duration::from_millis(100));
    });
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");

    let error = client
        .reboot(Some("invalid"))
        .expect_err("must be rejected");
    match error {
        Error::DeviceClosed { service, reason } => {
            assert_eq!(service, "reboot:invalid");
            assert!(reason.contains("Invalid reboot target"), "{reason}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn install_pushes_apks_then_runs_pm_install() {
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let handler_fs = fs.clone();
    let fake = FakeAdbd::spawn(FakeConfig::default(), move |session| {
        while let Some(open) = session.accept_open().unwrap() {
            if open.service == "sync:" {
                serve_sync(session, &open, &handler_fs).unwrap();
            } else if open.service.starts_with("exec:pm install") {
                session
                    .send_wrte(open.local_id, open.remote_id, b"Success\n")
                    .unwrap();
                session
                    .send_close(open.local_id, open.remote_id, "")
                    .unwrap();
            } else if open.service.starts_with("exec:rm -rf") {
                session
                    .send_close(open.local_id, open.remote_id, "")
                    .unwrap();
            } else {
                session
                    .send_close(open.local_id, open.remote_id, "unexpected service")
                    .unwrap();
            }
        }
    });

    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    let apk_bytes = b"PK\x03\x04fake-apk".to_vec();
    let apk = temp_apk(&apk_bytes);

    let options = InstallOptions {
        grant_permissions: true,
        ..Default::default()
    };
    let result = client.install(&apk, &options).expect("install");
    assert_eq!(result, "Success");

    let opens = fake.opens();
    let pm = opens
        .iter()
        .find(|service| service.starts_with("exec:pm install"))
        .expect("pm install must run");
    assert!(pm.contains("pm install -r -g "), "{pm}");
    assert!(pm.contains(".apk"), "{pm}");
    assert!(
        opens
            .iter()
            .any(|service| service.starts_with("exec:rm -rf"))
    );

    let fs = fs.lock().unwrap();
    assert_eq!(fs.uploads.len(), 1);
    assert!(fs.uploads[0].spec.starts_with("/data/local/tmp/rarebox-"));
    let (path, file) = fs.files.iter().next().expect("pushed apk");
    assert!(path.ends_with("/base.apk"));
    assert_eq!(file.data, apk_bytes);

    std::fs::remove_file(&apk).ok();
}

#[test]
fn install_multiple_uses_install_multiple() {
    let fs = Arc::new(Mutex::new(FakeFs::new()));
    let handler_fs = fs.clone();
    let fake = FakeAdbd::spawn(FakeConfig::default(), move |session| {
        while let Some(open) = session.accept_open().unwrap() {
            if open.service == "sync:" {
                serve_sync(session, &open, &handler_fs).unwrap();
            } else if open.service.starts_with("exec:pm install-multiple") {
                session
                    .send_wrte(open.local_id, open.remote_id, b"Success\n")
                    .unwrap();
                session
                    .send_close(open.local_id, open.remote_id, "")
                    .unwrap();
            } else if open.service.starts_with("exec:rm -rf") {
                session
                    .send_close(open.local_id, open.remote_id, "")
                    .unwrap();
            } else {
                session
                    .send_close(open.local_id, open.remote_id, "unexpected service")
                    .unwrap();
            }
        }
    });

    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    let base = temp_apk(b"base");
    let split = temp_apk(b"split");
    let result = client
        .install_multiple(&[base.clone(), split.clone()], &InstallOptions::default())
        .expect("install-multiple");
    assert_eq!(result, "Success");
    assert_eq!(fs.lock().unwrap().uploads.len(), 2);
    assert_eq!(fs.lock().unwrap().files.len(), 2);

    std::fs::remove_file(&base).ok();
    std::fs::remove_file(&split).ok();
}

#[test]
fn serial_is_enforced() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), |session| {
        let _ = session.accept_open();
    });
    let client = AdbClient::connect_default(fake.addr, &TEST_KEY).expect("connect");
    let error = client
        .open_shell(Some("someone-else"), "id")
        .expect_err("serial mismatch");
    match error {
        Error::Protocol(message) => assert!(message.contains("设备不匹配"), "{message}"),
        other => panic!("unexpected error: {other:?}"),
    }
}

fn temp_apk(bytes: &[u8]) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "rarebox-adb-proto-{}-{stamp}.apk",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("write temp apk");
    path
}

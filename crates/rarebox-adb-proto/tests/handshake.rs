//! 握手与鉴权：CNXN banner、签名验证、首次配对、待确认、拒绝。

mod support;

use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use rarebox_adb_proto::{AdbClient, AdbKey, AuthEvent, ConnectOptions, Error};
use support::{AuthMode, FakeAdbd, FakeConfig, Session};

static TEST_KEY: LazyLock<AdbKey> =
    LazyLock::new(|| AdbKey::generate().expect("generate test key"));

fn test_key() -> &'static AdbKey {
    &TEST_KEY
}

/// 保持连接直到客户端主动断开。
fn stay(session: &mut Session) {
    while session.accept_open().expect("accept open").is_some() {}
}

#[test]
fn connects_without_auth_and_parses_banner() {
    let fake = FakeAdbd::spawn(FakeConfig::default(), stay);
    let client = AdbClient::connect_default(fake.addr, test_key()).expect("connect");

    assert_eq!(client.device_info().model(), Some("Fake Phone"));
    assert_eq!(client.device_info().product(), Some("fake_product"));
    assert_eq!(client.device_info().device(), Some("fake_device"));
    assert_eq!(client.device_info().kind, "device");
    assert!(client.device_info().has_feature("shell_v2"));
    assert!(!client.device_info().has_feature("stat_v2"));

    let records = fake.records.lock().unwrap();
    assert_eq!(records.cnxn_version, 0x0100_0000);
    assert_eq!(records.cnxn_maxdata, 1024 * 1024);
    assert_eq!(records.cnxn_banner, "host::features=cmd");
}

#[test]
fn accepts_signature_with_known_public_key() {
    let fake = FakeAdbd::spawn(
        FakeConfig {
            auth: AuthMode::KnownKey(test_key().public_key().clone()),
            ..Default::default()
        },
        stay,
    );
    let _client = AdbClient::connect_default(fake.addr, test_key()).expect("connect");
    let records = fake.records.lock().unwrap();
    assert_eq!(records.signature_attempts, 0);
    assert!(records.signature_verified);
    assert!(records.public_key.is_none());
}

#[test]
fn first_pairing_sends_public_key_after_rejected_signature() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = events.clone();
    let options = ConnectOptions {
        auth_observer: Some(Arc::new(move |event: AuthEvent| {
            observer.lock().unwrap().push(event);
        })),
        ..Default::default()
    };

    let fake = FakeAdbd::spawn(
        FakeConfig {
            auth: AuthMode::FirstPairing,
            ..Default::default()
        },
        stay,
    );
    let _client = AdbClient::connect(fake.addr, test_key(), &options).expect("pairing");

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            AuthEvent::TokenReceived,
            AuthEvent::SignatureSent,
            AuthEvent::TokenReceived,
            AuthEvent::SignatureRejected,
            AuthEvent::PublicKeySent,
        ]
    );

    let records = fake.records.lock().unwrap();
    assert_eq!(records.signature_attempts, 1);
    assert!(records.signature_verified);
    let public_key = records.public_key.as_deref().expect("public key payload");
    assert_eq!(public_key.split_whitespace().count(), 2);
    let expected_base64 = test_key().user_public_key("").expect("base64 public key");
    assert_eq!(
        public_key.split_whitespace().next(),
        Some(expected_base64.as_str())
    );
    assert!(
        public_key
            .split_whitespace()
            .nth(1)
            .is_some_and(|identity| identity.contains('@'))
    );
}

#[test]
fn reports_pending_user_confirmation() {
    let options = ConnectOptions {
        handshake_timeout: Duration::from_millis(600),
        ..Default::default()
    };
    let fake = FakeAdbd::spawn(
        FakeConfig {
            auth: AuthMode::PendingConfirmation,
            ..Default::default()
        },
        |_session| {},
    );

    let error = AdbClient::connect(fake.addr, test_key(), &options).expect_err("must not connect");
    assert!(
        matches!(error, Error::AuthPending),
        "unexpected error: {error:?}"
    );
    assert!(fake.records.lock().unwrap().signature_verified);
}

#[test]
fn maps_device_auth_rejection_to_auth_failed() {
    let fake = FakeAdbd::spawn(
        FakeConfig {
            auth: AuthMode::RejectAll,
            ..Default::default()
        },
        |_session| {},
    );

    let error = AdbClient::connect_default(fake.addr, test_key()).expect_err("must be rejected");
    match error {
        Error::AuthFailed { reason } => assert!(reason.contains("device offline"), "{reason}"),
        other => panic!("unexpected error: {other:?}"),
    }
}

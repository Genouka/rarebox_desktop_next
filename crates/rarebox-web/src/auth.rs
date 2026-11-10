//! `adb` 主机认证密钥。
//!
//! ADB 使用 RSA-2048 + PKCS#1 v1.5（SHA-1）对设备发来的 20 字节 token 签名。
//! 首次连接时如果没有被设备信任的密钥，主机还要把 Android 自定义的 524 字节
//! 公钥结构（base64）发给设备，由用户在设备上确认。
//!
//! 浏览器里密钥保存在 `localStorage`；换浏览器/清站点数据后需要重新授权。
//! 这与 `adb` 桌面端 `~/.android/adbkey` 的语义一致。

use anyhow::{Context as _, Result};
use base64::Engine as _;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use rsa::signature::{SignatureEncoding, Signer};
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPrivateKey, RsaPublicKey};
use sha1::Sha1;

/// 设备上显示的密钥名称。
pub const KEY_NAME: &str = "rarebox@web";

/// Android `adb_auth` 的公钥结构大小：4 + 4 + 256 + 256 + 4。
const ANDROID_PUBKEY_SIZE: usize = 524;
/// RSA-2048 模长的字节数。
const MODULUS_BYTES: usize = 256;
/// 模长按 32 位字计（2048 / 32）。
const MODULUS_WORDS: u32 = 64;

#[derive(Clone)]
pub struct AdbKey {
    private: RsaPrivateKey,
    /// `base64(524 字节公钥) 名称`，用于 AUTH(3) 包。
    encoded: String,
}

impl AdbKey {
    pub fn generate() -> Result<Self> {
        let mut rng = rand::rngs::OsRng;
        let private = RsaPrivateKey::new(&mut rng, 2048).context("生成 ADB RSA 密钥失败")?;
        Ok(Self::from_private(private))
    }

    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self> {
        let private = RsaPrivateKey::from_pkcs8_der(der).context("解析已保存的 ADB 密钥失败")?;
        Ok(Self::from_private(private))
    }

    pub fn to_pkcs8_der(&self) -> Result<Vec<u8>> {
        let document = self.private.to_pkcs8_der().context("导出 ADB 密钥失败")?;
        Ok(document.as_bytes().to_vec())
    }

    /// 设备端 `AUTH(3)` 需要的公钥串，已包含名称；发送时还要追加 NUL。
    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    /// 公钥，供测试验证签名。
    #[cfg(test)]
    pub(crate) fn public_key(&self) -> RsaPublicKey {
        RsaPublicKey::from(&self.private)
    }

    /// 对设备 token 签名（PKCS#1 v1.5 / SHA-1）。
    pub fn sign_token(&self, token: &[u8]) -> Result<Vec<u8>> {
        let signing_key = SigningKey::<Sha1>::new(self.private.clone());
        let signature = signing_key.sign(token);
        Ok(signature.to_vec())
    }

    fn from_private(private: RsaPrivateKey) -> Self {
        let public = RsaPublicKey::from(&private);
        let encoded = encode_android_public_key(public.n(), public.e());
        Self { private, encoded }
    }
}

/// Android 的 `RSAPublicKey` 结构（小端）：
///
/// ```text
/// u32 len;            // 模长按 32 位字计
/// u32 n0inv;          // -1 / n[0] mod 2^32
/// u8  modulus[256];   // 小端
/// u8  rr[256];        // R^2 mod n，R = 2^2048，小端
/// u32 exponent;       // 65537
/// ```
///
/// 编码后的 524 字节再做标准 base64，并追加 ` 名称`，即设备端看到的公钥。
fn encode_android_public_key(modulus: &BigUint, exponent: &BigUint) -> String {
    let modulus_le = padded_le_bytes(modulus, MODULUS_BYTES);
    let rr = (BigUint::from(1u32) << (MODULUS_BYTES * 16)) % modulus;
    let rr_le = padded_le_bytes(&rr, MODULUS_BYTES);

    let n0 = u32::from_le_bytes([modulus_le[0], modulus_le[1], modulus_le[2], modulus_le[3]]);
    let n0inv = inverse_mod_2_32(n0).wrapping_neg();

    let mut encoded = Vec::with_capacity(ANDROID_PUBKEY_SIZE);
    encoded.extend_from_slice(&MODULUS_WORDS.to_le_bytes());
    encoded.extend_from_slice(&n0inv.to_le_bytes());
    encoded.extend_from_slice(&modulus_le);
    encoded.extend_from_slice(&rr_le);
    let mut exponent_le = [0u8; 4];
    let exponent_bytes = exponent.to_bytes_le();
    let copy = exponent_bytes.len().min(exponent_le.len());
    exponent_le[..copy].copy_from_slice(&exponent_bytes[..copy]);
    encoded.extend_from_slice(&exponent_le);
    debug_assert_eq!(encoded.len(), ANDROID_PUBKEY_SIZE);

    let b64 = base64::engine::general_purpose::STANDARD.encode(encoded);
    format!("{b64} {KEY_NAME}")
}

/// 模长按小端补齐到固定长度；高字节补 0。
fn padded_le_bytes(value: &BigUint, len: usize) -> Vec<u8> {
    let bytes = value.to_bytes_le();
    let mut padded = vec![0u8; len];
    let copy = bytes.len().min(len);
    padded[..copy].copy_from_slice(&bytes[..copy]);
    padded
}

/// 奇数 `value` 在 2^32 下的乘法逆元（牛顿迭代，3→6→12→24→48 位）。
fn inverse_mod_2_32(value: u32) -> u32 {
    debug_assert!(value % 2 == 1);
    let mut inverse = value;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u32.wrapping_sub(value.wrapping_mul(inverse)));
    }
    inverse
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_is_correct_for_odd_values() {
        for value in [1u32, 3, 5, 0x1234_5679, 0xffff_fffd] {
            let inverse = inverse_mod_2_32(value);
            assert_eq!(value.wrapping_mul(inverse), 1);
        }
    }

    #[test]
    fn public_key_has_android_layout() {
        let key = AdbKey::generate().expect("generate key");
        let encoded = key.encoded();
        let (b64, name) = encoded.split_once(' ').expect("name suffix");
        assert_eq!(name, KEY_NAME);

        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64");
        assert_eq!(raw.len(), ANDROID_PUBKEY_SIZE);
        assert_eq!(&raw[0..4], &MODULUS_WORDS.to_le_bytes());

        let modulus = RsaPublicKey::from(&key.private);
        let expected = modulus.n().to_bytes_le();
        let mut actual = raw[8..8 + MODULUS_BYTES].to_vec();
        actual.truncate(expected.len());
        assert_eq!(actual, expected);

        let exponent = &raw[ANDROID_PUBKEY_SIZE - 4..];
        assert_eq!(u32::from_le_bytes(exponent.try_into().unwrap()), 65537);
    }

    #[test]
    fn signature_is_256_bytes_and_deterministic() {
        let key = AdbKey::generate().expect("generate key");
        let token = [7u8; 20];
        let first = key.sign_token(&token).expect("sign");
        let second = key.sign_token(&token).expect("sign");
        assert_eq!(first.len(), MODULUS_BYTES);
        assert_eq!(first, second);
    }
}

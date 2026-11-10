//! ADB RSA 鉴权：2048 位密钥、PKCS#1 v1.5 + SHA-1 签名、Android 公钥编码。
//!
//! Android 的 `.pub` 文件（以及 `A_AUTH`/`AUTH_RSAPUBLICKEY` 的 payload）不是
//! 标准的 X.509 SubjectPublicKeyInfo，而是 AOSP `crypto/rsa_2048.h` 里的
//! `RSAPublicKey` 结构：
//!
//! ```text
//! uint32 len;            // 524
//! uint32 n0inv;          // -1 / n[0] mod 2^32（Montgomery 参数）
//! uint8  modulus[256];   // 大端
//! uint8  rr[256];        // R^2 mod n，R = 2^2048
//! uint32 exponent;       // 小端，通常是 65537
//! ```
//!
//! 文件内容是 `base64(上述结构) + " " + user@host`。

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rand::rngs::OsRng;
use rsa::pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey, LineEnding};
use rsa::pkcs1v15::Pkcs1v15Sign;
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, RsaPrivateKey, RsaPublicKey};
use sha1::{Digest, Sha1};

use crate::error::{Error, Result};

/// Android `RSAPublicKey` 结构的长度。
pub const ANDROID_PUBKEY_LEN: usize = 524;
const MODULUS_LEN: usize = 256;
const KEY_BITS: usize = 2048;

/// 一对用于 ADB 鉴权的 RSA 密钥。
#[derive(Clone)]
pub struct AdbKey {
    private: RsaPrivateKey,
    public: RsaPublicKey,
}

impl fmt::Debug for AdbKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdbKey")
            .field("bits", &self.private.n().bits())
            .field("e", &self.public.e())
            .finish()
    }
}

impl AdbKey {
    /// 生成新的 2048 位密钥（首次使用，或设备未授权任何 key 时）。
    pub fn generate() -> Result<Self> {
        let private =
            RsaPrivateKey::new(&mut OsRng, KEY_BITS).map_err(|err| Error::AuthFailed {
                reason: format!("生成 RSA 密钥失败：{err}"),
            })?;
        Ok(Self::from_private(private))
    }

    /// 读取 AOSP 使用的 PKCS#1 PEM（`-----BEGIN RSA PRIVATE KEY-----`）。
    pub fn from_pkcs1_pem(pem: &str) -> Result<Self> {
        let private = RsaPrivateKey::from_pkcs1_pem(pem).map_err(|err| Error::AuthFailed {
            reason: format!("解析 RSA 私钥失败：{err}"),
        })?;
        if private.n().bits() != KEY_BITS {
            return Err(Error::AuthFailed {
                reason: format!(
                    "ADB 只接受 2048 位 RSA 密钥，该密钥是 {} 位",
                    private.n().bits()
                ),
            });
        }
        Ok(Self::from_private(private))
    }

    /// 转回 PKCS#1 PEM，便于持久化到 `~/.android/adbkey`。
    pub fn to_pkcs1_pem(&self) -> Result<String> {
        self.private
            .to_pkcs1_pem(LineEnding::LF)
            .map(|pem| pem.to_string())
            .map_err(|err| Error::AuthFailed {
                reason: format!("序列化 RSA 私钥失败：{err}"),
            })
    }

    pub fn from_private(private: RsaPrivateKey) -> Self {
        let public = RsaPublicKey::from(&private);
        Self { private, public }
    }

    pub fn public_key(&self) -> &RsaPublicKey {
        &self.public
    }

    /// 对设备发来的 20 字节 token 做 PKCS#1 v1.5 + SHA-1 签名。
    pub fn sign_token(&self, token: &[u8]) -> Result<Vec<u8>> {
        if token.len() != crate::proto::TOKEN_SIZE {
            return Err(Error::Protocol(format!(
                "AUTH TOKEN 应为 {} 字节，实际 {}",
                crate::proto::TOKEN_SIZE,
                token.len()
            )));
        }
        let digest = Sha1::digest(token);
        self.private
            .sign(Pkcs1v15Sign::new::<Sha1>(), &digest)
            .map_err(|err| Error::AuthFailed {
                reason: format!("签名 token 失败：{err}"),
            })
    }

    /// 524 字节的 Android `RSAPublicKey` 结构。
    pub fn android_public_key(&self) -> Result<[u8; ANDROID_PUBKEY_LEN]> {
        let modulus = fixed_be(self.public.n(), MODULUS_LEN)?;
        let (n0inv, rr) = montgomery_params(self.public.n(), &modulus)?;

        let exponent_bytes = self.public.e().to_bytes_le();
        if exponent_bytes.len() > 4 {
            return Err(Error::Unsupported("ADB 公钥指数只支持 32 位".to_string()));
        }
        let mut exponent_raw = [0u8; 4];
        exponent_raw[..exponent_bytes.len()].copy_from_slice(&exponent_bytes);
        let exponent = u32::from_le_bytes(exponent_raw);

        let mut raw = [0u8; ANDROID_PUBKEY_LEN];
        raw[0..4].copy_from_slice(&(ANDROID_PUBKEY_LEN as u32).to_le_bytes());
        raw[4..8].copy_from_slice(&n0inv.to_le_bytes());
        raw[8..8 + MODULUS_LEN].copy_from_slice(&modulus);
        raw[8 + MODULUS_LEN..8 + 2 * MODULUS_LEN].copy_from_slice(&rr);
        raw[8 + 2 * MODULUS_LEN..].copy_from_slice(&exponent.to_le_bytes());
        Ok(raw)
    }

    /// `AUTH_RSAPUBLICKEY` 的文本形式：`base64(...) user@host`。
    pub fn user_public_key(&self, identity: &str) -> Result<String> {
        let base64 = STANDARD.encode(self.android_public_key()?);
        let identity = identity.trim();
        if identity.is_empty() {
            Ok(base64)
        } else {
            Ok(format!("{base64} {identity}"))
        }
    }
}

/// 解析 `.pub` 文件内容或 `AUTH_RSAPUBLICKEY` payload，并校验 Montgomery 参数。
///
/// 这同时验证了 host 侧编码的正确性（编码与解码必须得到同一对参数）。
pub fn decode_android_public_key(text: &str) -> Result<RsaPublicKey> {
    let token = text
        .split_whitespace()
        .next()
        .ok_or_else(|| Error::AuthFailed {
            reason: "公钥字符串为空".to_string(),
        })?;
    let raw = STANDARD.decode(token).map_err(|err| Error::AuthFailed {
        reason: format!("公钥 base64 解码失败：{err}"),
    })?;
    if raw.len() != ANDROID_PUBKEY_LEN {
        return Err(Error::AuthFailed {
            reason: format!(
                "Android 公钥应为 {ANDROID_PUBKEY_LEN} 字节，实际 {}",
                raw.len()
            ),
        });
    }

    let len = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    if len as usize != ANDROID_PUBKEY_LEN {
        return Err(Error::AuthFailed {
            reason: format!("公钥结构声明长度 {len}，与固定的 {ANDROID_PUBKEY_LEN} 不符"),
        });
    }
    let n0inv = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
    let modulus = &raw[8..8 + MODULUS_LEN];
    let rr = &raw[8 + MODULUS_LEN..8 + 2 * MODULUS_LEN];
    let exponent = u32::from_le_bytes([raw[520], raw[521], raw[522], raw[523]]);

    if modulus[0] == 0 {
        return Err(Error::AuthFailed {
            reason: "公钥模数不是 2048 位".to_string(),
        });
    }
    if exponent == 0 {
        return Err(Error::AuthFailed {
            reason: "公钥指数为 0".to_string(),
        });
    }

    let (expected_n0inv, expected_rr) = montgomery_params(
        &BigUint::from_bytes_be(modulus),
        modulus.try_into().unwrap_or(&[0u8; MODULUS_LEN]),
    )?;
    if n0inv != expected_n0inv || rr != expected_rr {
        return Err(Error::AuthFailed {
            reason: "公钥的 Montgomery 参数（n0inv/rr）与模数不匹配".to_string(),
        });
    }

    RsaPublicKey::new(BigUint::from_bytes_be(modulus), BigUint::from(exponent)).map_err(|err| {
        Error::AuthFailed {
            reason: format!("公钥无效：{err}"),
        }
    })
}

/// 默认的 `user@host` 标识，附加在公钥后面（仅用于设备端展示）。
pub fn default_identity() -> String {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "rarebox".to_string());
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    format!("{user}@{host}")
}

fn fixed_be(value: &BigUint, len: usize) -> Result<[u8; MODULUS_LEN]> {
    let bytes = value.to_bytes_be();
    if bytes.len() > len {
        return Err(Error::Unsupported(format!(
            "RSA 模数为 {} 字节，超过 ADB 固定的 {len} 字节",
            bytes.len()
        )));
    }
    let mut out = [0u8; MODULUS_LEN];
    out[len - bytes.len()..].copy_from_slice(&bytes);
    Ok(out)
}

/// 计算 Montgomery 参数：`n0inv = -n^-1 mod 2^32`，`rr = 2^4096 mod n`。
fn montgomery_params(
    n: &BigUint,
    modulus_be: &[u8; MODULUS_LEN],
) -> Result<(u32, [u8; MODULUS_LEN])> {
    let n0 = u32::from_be_bytes([
        modulus_be[MODULUS_LEN - 4],
        modulus_be[MODULUS_LEN - 3],
        modulus_be[MODULUS_LEN - 2],
        modulus_be[MODULUS_LEN - 1],
    ]);
    if n0 == 0 || n0 & 1 == 0 {
        return Err(Error::AuthFailed {
            reason: "RSA 模数必须是奇数".to_string(),
        });
    }

    // Newton 迭代求 n0 的逆元（模 2^32），5 轮即可收敛到 32 位精度。
    let mut inverse = 1u32;
    for _ in 0..5 {
        inverse = inverse.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inverse)));
    }
    let n0inv = inverse.wrapping_neg();

    let rr_value = (BigUint::from(1u8) << (MODULUS_LEN * 16)) % n;
    let rr_bytes = rr_value.to_bytes_be();
    let mut rr = [0u8; MODULUS_LEN];
    rr[MODULUS_LEN - rr_bytes.len()..].copy_from_slice(&rr_bytes);
    Ok((n0inv, rr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_signs_and_encodes_android_format() {
        let key = AdbKey::generate().expect("generate key");
        let token = [0x5au8; crate::proto::TOKEN_SIZE];
        let signature = key.sign_token(&token).expect("sign");
        assert_eq!(signature.len(), 256);

        let encoded = key.user_public_key("rarebox@test").expect("encode");
        let public = decode_android_public_key(&encoded).expect("decode");
        let digest = Sha1::digest(token);
        public
            .verify(Pkcs1v15Sign::new::<Sha1>(), &digest, &signature)
            .expect("signature verifies with decoded key");
    }

    #[test]
    fn pem_roundtrip_keeps_key() {
        let key = AdbKey::generate().expect("generate key");
        let pem = key.to_pkcs1_pem().expect("pem");
        let restored = AdbKey::from_pkcs1_pem(&pem).expect("parse pem");
        assert_eq!(
            restored.android_public_key().unwrap(),
            key.android_public_key().unwrap()
        );
    }

    #[test]
    fn rejects_garbage_public_key() {
        assert!(decode_android_public_key("not-base64!!").is_err());
        let mut raw = [0u8; ANDROID_PUBKEY_LEN];
        raw[0..4].copy_from_slice(&524u32.to_le_bytes());
        let text = STANDARD.encode(raw);
        assert!(decode_android_public_key(&text).is_err());
    }
}

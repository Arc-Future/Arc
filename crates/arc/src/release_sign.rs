//! Ed25519 签名工具（发布清单 / `.aopkg` 分离签名共用）。
//!
//! 单一密钥故事：`arc release keygen` 生成的 seed（64 hex）同时用于签名发布
//! manifest 与 `.aopkg` 分发包；消费端信任锚 = 编译期内置公钥
//!（[crate::release::RELEASE_PUBLIC_KEY_HEX]），`$ARC_RELEASE_PUBKEY` 显式覆盖。
//!
//! **明确不在本切片**：PKI / 证书链 / 密钥轮换（密钥离线托管）。

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use thiserror::Error;

/// Ed25519 公钥长度。
pub const ED25519_PUBLIC_KEY_LEN: usize = 32;

/// Ed25519 签名长度。
pub const ED25519_SIGNATURE_LEN: usize = 64;

/// Ed25519 种子（SigningKey）长度。
pub const ED25519_SEED_LEN: usize = 32;

#[derive(Debug, Error)]
pub enum SignError {
    #[error("Ed25519 sign failed")]
    Sign,
    #[error("Ed25519 verify failed (tampered or wrong key)")]
    Verify,
    #[error("invalid Ed25519 public key bytes")]
    BadPublicKey,
    #[error("expected 64 hex chars ({ED25519_SEED_LEN}-byte key material), got {0} chars")]
    BadHexLen(usize),
    #[error("invalid hex character in key material")]
    BadHexChar,
}

/// 用 32 字节 seed 对消息签名，返回 `(pubkey, signature)`。
pub fn sign_message(
    seed: &[u8; ED25519_SEED_LEN],
    message: &[u8],
) -> Result<([u8; ED25519_PUBLIC_KEY_LEN], [u8; ED25519_SIGNATURE_LEN]), SignError> {
    let signing_key = SigningKey::from_bytes(seed);
    let verifying_key = signing_key.verifying_key();
    let sig = signing_key.sign(message);
    Ok((*verifying_key.as_bytes(), sig.to_bytes()))
}

/// 用公钥验签；失败 → [`SignError::Verify`]。
pub fn verify_message(
    public_key: &[u8; ED25519_PUBLIC_KEY_LEN],
    message: &[u8],
    signature: &[u8; ED25519_SIGNATURE_LEN],
) -> Result<(), SignError> {
    let vk = VerifyingKey::from_bytes(public_key).map_err(|_| SignError::BadPublicKey)?;
    let sig = Signature::from_bytes(signature);
    vk.verify(message, &sig).map_err(|_| SignError::Verify)
}

/// 小写 hex 编码（签名材料展示/落盘唯一实现）。
pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 解析 `N` 字节 hex 串（校验长度与字符集）。
pub fn parse_hex<const N: usize>(s: &str) -> Result<[u8; N], SignError> {
    let trimmed = s.trim();
    if trimmed.len() != N * 2 {
        return Err(SignError::BadHexLen(trimmed.len()));
    }
    let mut out = [0u8; N];
    for (i, chunk) in trimmed.as_bytes().chunks(2).enumerate() {
        out[i] = (hex_nibble(chunk[0])? << 4) | hex_nibble(chunk[1])?;
    }
    Ok(out)
}

/// 解析 64 hex → 32 字节（seed / 公钥共用入口）。
pub fn parse_hex32(s: &str) -> Result<[u8; 32], SignError> {
    parse_hex::<32>(s)
}

fn hex_nibble(b: u8) -> Result<u8, SignError> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(SignError::BadHexChar),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_roundtrip() {
        let seed = [0x11u8; 32];
        let msg = b"arc-release-signed-message";
        let (pk, sig) = sign_message(&seed, msg).unwrap();
        verify_message(&pk, msg, &sig).unwrap();
    }

    #[test]
    fn tampered_message_fails_verify() {
        let seed = [0x22u8; 32];
        let (pk, sig) = sign_message(&seed, b"original").unwrap();
        assert!(matches!(
            verify_message(&pk, b"tampered", &sig),
            Err(SignError::Verify)
        ));
    }

    #[test]
    fn wrong_key_fails_verify() {
        let (pk, sig) = sign_message(&[0x33u8; 32], b"msg").unwrap();
        assert!(matches!(
            verify_message(&[0x44u8; 32], b"msg", &sig),
            Err(SignError::BadPublicKey)
        ));
        let _ = pk;
    }

    #[test]
    fn parse_hex32_accepts_64() {
        let hex = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let b = parse_hex32(hex).unwrap();
        assert_eq!(b[0], 0x01);
        assert_eq!(b[31], 0xef);
        // 大小写不敏感 + 周边空白容忍。
        let b2 = parse_hex32(" 0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789ABCDEF ")
            .unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn parse_hex_rejects_bad_length_and_chars() {
        assert!(matches!(parse_hex32("abcd"), Err(SignError::BadHexLen(4))));
        assert!(matches!(parse_hex::<2>("zz11"), Err(SignError::BadHexChar)));
    }

    #[test]
    fn hex_encode_roundtrips_with_parse_hex() {
        let bytes = [0u8, 1, 0xAB, 0xFF];
        assert_eq!(hex_encode(&bytes), "0001abff");
        assert_eq!(parse_hex::<4>(&hex_encode(&bytes)).unwrap(), bytes);
    }
}

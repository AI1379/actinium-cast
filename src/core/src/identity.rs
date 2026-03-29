//! # Identity Module
//!
//! Ed25519 密钥对管理：生成、导入导出、签名与验证。
//! 私钥在 `Identity` 对象析构时会被 zeroize 安全清零。

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hex::FromHexError;
use rand::rngs::OsRng;
use thiserror::Error;
use zeroize::ZeroizeOnDrop;

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("hex 解码失败: {0}")]
    HexDecode(#[from] FromHexError),

    #[error("私钥长度不正确，需要 32 字节，实际 {0} 字节")]
    InvalidKeyLength(usize),

    #[error("公钥无效: {0}")]
    InvalidPublicKey(#[from] ed25519_dalek::SignatureError),
}

// ─── Identity ────────────────────────────────────────────────────────────────

/// 封装用户的 Ed25519 身份密钥对。
///
/// 私钥（`signing_key`）在对象析构时会被 [`ZeroizeOnDrop`] 安全清零，
/// 不会在内存中以明文形式残留。
#[derive(ZeroizeOnDrop)]
pub struct Identity {
    signing_key: SigningKey,
}

impl Identity {
    // ── 构造 ──────────────────────────────────────────────────────────────

    /// 使用系统 CSPRNG 随机生成一个新的密钥对。
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        Identity { signing_key }
    }

    /// 从 32 字节的原始私钥种子恢复身份。
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        Identity { signing_key }
    }

    /// 从十六进制字符串（64 个字符）恢复身份。
    ///
    /// # Errors
    /// - 若 hex 字符串格式非法，返回 [`IdentityError::HexDecode`]。
    /// - 若解码后长度不为 32 字节，返回 [`IdentityError::InvalidKeyLength`]。
    pub fn from_hex(hex_str: &str) -> Result<Self, IdentityError> {
        let bytes = hex::decode(hex_str)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|v: Vec<u8>| IdentityError::InvalidKeyLength(v.len()))?;
        Ok(Self::from_bytes(&arr))
    }

    // ── 导出 ──────────────────────────────────────────────────────────────

    /// 以原始字节导出私钥（32 字节）。
    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// 以小写十六进制字符串导出私钥（64 字符）。
    pub fn to_hex(&self) -> String {
        hex::encode(self.signing_key.to_bytes())
    }

    /// 获取对应的 Ed25519 公钥（`VerifyingKey`）。
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// 以原始字节导出公钥（32 字节）。
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// 以小写十六进制字符串导出公钥（64 字符）。
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing_key.verifying_key().to_bytes())
    }

    // ── 签名 ──────────────────────────────────────────────────────────────

    /// 对任意消息字节切片进行签名，返回 64 字节的 [`Signature`]。
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }
}

// ─── 独立验证函数 ─────────────────────────────────────────────────────────────

/// 使用公钥验证签名。
///
/// # 参数
/// - `public_key_bytes`: 32 字节的 Ed25519 公钥。
/// - `message`: 被签名的原始消息。
/// - `signature_bytes`: 64 字节的签名。
///
/// # 返回
/// 签名合法时返回 `true`，否则返回 `false`。
pub fn verify_signature(
    public_key_bytes: &[u8; 32],
    message: &[u8],
    signature_bytes: &[u8; 64],
) -> bool {
    match VerifyingKey::from_bytes(public_key_bytes) {
        Ok(pk) => {
            let sig = Signature::from_bytes(signature_bytes);
            pk.verify(message, &sig).is_ok()
        }
        Err(_) => false,
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_sign() {
        let identity = Identity::generate();
        let message = b"hello actinium cast";

        let sig = identity.sign(message);
        let sig_bytes = sig.to_bytes();
        let pk_bytes = identity.public_key_bytes();

        // 有效签名应验证通过
        assert!(verify_signature(&pk_bytes, message, &sig_bytes));

        // 篡改消息后验证应失败
        let tampered = b"hello actinium cast!";
        assert!(!verify_signature(&pk_bytes, tampered, &sig_bytes));
    }

    #[test]
    fn test_hex_roundtrip() {
        let id1 = Identity::generate();
        let hex = id1.to_hex();

        // hex 字符串应为 64 个小写十六进制字符
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));

        // 从 hex 恢复后，私钥与公钥应完全一致
        let id2 = Identity::from_hex(&hex).expect("from_hex should succeed");
        assert_eq!(id1.to_bytes(), id2.to_bytes());
        assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());
    }

    #[test]
    fn test_from_hex_invalid() {
        // 非法 hex 字符串
        assert!(Identity::from_hex("not-hex").is_err());
        // 长度不对（31 字节 = 62 个字符）
        assert!(Identity::from_hex(&"aa".repeat(31)).is_err());
    }

    #[test]
    fn test_public_key_hex() {
        let id = Identity::generate();
        let pk_hex = id.public_key_hex();
        assert_eq!(pk_hex.len(), 64);
        // 公钥 hex 应与 bytes 表示一致
        assert_eq!(hex::decode(&pk_hex).unwrap(), id.public_key_bytes().to_vec());
    }
}

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

/// 封装用户的 Ed25519 身份密钥对
pub struct Identity {
    signing_key: SigningKey,
}

impl Identity {
    /// 随机生成一个新的身份
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        Identity { signing_key }
    }

    /// 从 32 字节的种子（私钥）加载身份
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(bytes);
        Identity { signing_key }
    }

    /// 获取私钥字节
    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    /// 获取对应的公钥
    pub fn public_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// 获取公钥的 32 字节表示
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// 对消息进行签名
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.signing_key.sign(message)
    }
}

/// 验证签名
pub fn verify_signature(public_key_bytes: &[u8; 32], message: &[u8], signature_bytes: &[u8; 64]) -> bool {
    if let Ok(public_key) = VerifyingKey::from_bytes(public_key_bytes) {
        let signature = Signature::from_bytes(signature_bytes);
        return public_key.verify(message, &signature).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_verification() {
        let identity = Identity::generate();
        let message = b"hello actinium cast";
        
        // 签名
        let sig = identity.sign(message);
        let sig_bytes = sig.to_bytes();
        let pk_bytes = identity.public_key_bytes();

        // 验证
        assert!(verify_signature(&pk_bytes, message, &sig_bytes));

        // 伪造测试
        let tampered_message = b"hello actinium cast!";
        assert!(!verify_signature(&pk_bytes, tampered_message, &sig_bytes));
    }
}

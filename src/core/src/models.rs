//! # Data Models Module
//!
//! 定义平台的核心消息结构，遵循 BEP 44 内容格式要求。
//!
//! 所有消息均通过 [`SignedEnvelope`] 包裹，确保：
//! ```text
//! [payload, timestamp, pow_nonce, pow_hash, public_key, signature]
//! ```
//! 签名目标 = `bencode(payload) || timestamp_le || pow_nonce_le || public_key`

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("bencode 序列化失败: {0}")]
    Encode(#[from] serde_bencode::Error),
}

// ─── 业务载荷类型 ─────────────────────────────────────────────────────────────

/// 帖子（Post）载荷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Post {
    /// 帖子标题。
    pub title: String,
    /// 帖子正文内容（支持 Markdown）。
    pub content: String,
    /// 发布时使用的 PoW 难度（记录在消息中，方便网关校验）。
    pub difficulty: u8,
}

/// 评论（Comment）载荷。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Comment {
    /// 目标帖子作者的公钥 hex（作为帖子在 DHT 中的寻址键）。
    pub post_id: String,
    /// 评论内容。
    pub content: String,
}

/// 交互投票（Vote）载荷，同时支持点赞与取消。
///
/// **注意**：`positive` 使用 `u8` 而非 `bool`，因为 `serde_bencode` 不支持原生布尔类型的往返序列化。
/// 请使用 `1u8` 表示点赞，`0u8` 表示取消点赞，或调用 [`Vote::is_positive`]。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Vote {
    /// 被投票的帖子或评论的公钥 hex。
    pub target_id: String,
    /// `1` = 点赞 (like)；`0` = 取消点赞 (unlike)。
    pub positive: u8,
}

impl Vote {
    /// 语义化访问：返回是否为正向投票（点赞）。
    pub fn is_positive(&self) -> bool {
        self.positive != 0
    }
}

// ─── 签名信封 ─────────────────────────────────────────────────────────────────

/// 通用签名信封，携带任意可序列化的业务载荷 `T`。
///
/// 固定字节数组字段（`pow_hash`、`public_key`、`signature`）以 `Vec<u8>` 存储，
/// 保证与 `serde_bencode` 的兼容性，同时提供类型化的访问方法。
///
/// `T` 需满足 [`Serialize`] + [`DeserializeOwned`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "T: Serialize",
    deserialize = "T: DeserializeOwned"
))]
pub struct SignedEnvelope<T> {
    /// 业务载荷（Post / Comment / Vote）。
    pub payload: T,
    /// Unix 时间戳（秒），防止重放攻击。
    pub timestamp: i64,
    /// PoW 解的 nonce。
    pub pow_nonce: u64,
    /// PoW 最终哈希（32 字节）。
    pub pow_hash: Vec<u8>,
    /// 发布者 Ed25519 公钥（32 字节）。
    pub public_key: Vec<u8>,
    /// Ed25519 签名（64 字节）。
    pub signature: Vec<u8>,
}

impl<T: Serialize + DeserializeOwned> SignedEnvelope<T> {
    /// 从原始字节数组构造信封（类型安全的构造器）。
    pub fn new(
        payload: T,
        timestamp: i64,
        pow_nonce: u64,
        pow_hash: [u8; 32],
        public_key: [u8; 32],
        signature: [u8; 64],
    ) -> Self {
        SignedEnvelope {
            payload,
            timestamp,
            pow_nonce,
            pow_hash: pow_hash.to_vec(),
            public_key: public_key.to_vec(),
            signature: signature.to_vec(),
        }
    }

    /// 获取 PoW 哈希的固定长度数组形式。
    ///
    /// # Panics
    /// 若 `pow_hash` 字段长度不为 32 字节则 panic（数据格式破坏）。
    pub fn pow_hash_array(&self) -> [u8; 32] {
        self.pow_hash.as_slice().try_into().expect("pow_hash must be 32 bytes")
    }

    /// 获取公钥的固定长度数组形式。
    pub fn public_key_array(&self) -> [u8; 32] {
        self.public_key.as_slice().try_into().expect("public_key must be 32 bytes")
    }

    /// 获取签名的固定长度数组形式。
    pub fn signature_array(&self) -> [u8; 64] {
        self.signature.as_slice().try_into().expect("signature must be 64 bytes")
    }

    /// 序列化为 bencode 字节（用于 DHT 存储与传输）。
    pub fn to_bencode(&self) -> Result<Vec<u8>, ModelError> {
        Ok(serde_bencode::to_bytes(self)?)
    }

    /// 从 bencode 字节反序列化。
    pub fn from_bencode(bytes: &[u8]) -> Result<Self, ModelError> {
        Ok(serde_bencode::from_bytes(bytes)?)
    }

    /// 生成签名目标字节（签名时和验证时使用相同的字节序列）。
    ///
    /// 格式：`bencode(payload) || timestamp_le8 || pow_nonce_le8 || public_key`
    pub fn signing_bytes(
        payload: &T,
        timestamp: i64,
        pow_nonce: u64,
        public_key: &[u8; 32],
    ) -> Result<Vec<u8>, ModelError> {
        let mut bytes = serde_bencode::to_bytes(payload)?;
        bytes.extend_from_slice(&timestamp.to_le_bytes());
        bytes.extend_from_slice(&pow_nonce.to_le_bytes());
        bytes.extend_from_slice(public_key);
        Ok(bytes)
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_hash() -> [u8; 32] {
        [0xAAu8; 32]
    }
    fn dummy_pubkey() -> [u8; 32] {
        [0x42u8; 32]
    }
    fn dummy_sig() -> [u8; 64] {
        [0x55u8; 64]
    }

    fn make_post_envelope() -> SignedEnvelope<Post> {
        SignedEnvelope::new(
            Post {
                title: "Hello World".to_string(),
                content: "Actinium is decentralized.".to_string(),
                difficulty: 16,
            },
            1_700_000_000i64,
            42u64,
            dummy_hash(),
            dummy_pubkey(),
            dummy_sig(),
        )
    }

    /// Post bencode 往返序列化。
    #[test]
    fn test_post_envelope_roundtrip() {
        let envelope = make_post_envelope();
        let encoded = envelope.to_bencode().expect("encode should succeed");
        let decoded: SignedEnvelope<Post> =
            SignedEnvelope::from_bencode(&encoded).expect("decode should succeed");
        assert_eq!(envelope, decoded);
    }

    /// Comment bencode 往返序列化。
    #[test]
    fn test_comment_envelope_roundtrip() {
        let envelope = SignedEnvelope::new(
            Comment {
                post_id: "abcdef1234".to_string(),
                content: "Great post!".to_string(),
            },
            1_700_000_001i64,
            99u64,
            dummy_hash(),
            dummy_pubkey(),
            dummy_sig(),
        );
        let encoded = envelope.to_bencode().expect("encode");
        let decoded: SignedEnvelope<Comment> =
            SignedEnvelope::from_bencode(&encoded).expect("decode");
        assert_eq!(envelope, decoded);
    }

    /// Vote bencode 往返序列化。
    #[test]
    fn test_vote_envelope_roundtrip() {
        let envelope = SignedEnvelope::new(
            Vote {
                target_id: "deadbeef".to_string(),
                positive: 1u8,  // 1 = like
            },
            1_700_000_002i64,
            7u64,
            dummy_hash(),
            dummy_pubkey(),
            dummy_sig(),
        );
        let encoded = envelope.to_bencode().expect("encode");
        let decoded: SignedEnvelope<Vote> =
            SignedEnvelope::from_bencode(&encoded).expect("decode");
        assert_eq!(envelope, decoded);
        assert!(decoded.payload.is_positive());
    }

    /// 固定数组访问器测试。
    #[test]
    fn test_array_accessors() {
        let env = make_post_envelope();
        assert_eq!(env.pow_hash_array(), dummy_hash());
        assert_eq!(env.public_key_array(), dummy_pubkey());
        assert_eq!(env.signature_array(), dummy_sig());
    }

    /// signing_bytes 返回确定性结果（相同输入永远产生相同字节）。
    #[test]
    fn test_signing_bytes_deterministic() {
        let post = Post {
            title: "Determinism".to_string(),
            content: "Same input same output".to_string(),
            difficulty: 8,
        };
        let pk = dummy_pubkey();
        let ts = 1_700_000_000i64;
        let nonce = 12345u64;

        let b1 = SignedEnvelope::<Post>::signing_bytes(&post, ts, nonce, &pk).unwrap();
        let b2 = SignedEnvelope::<Post>::signing_bytes(&post, ts, nonce, &pk).unwrap();
        assert_eq!(b1, b2);
        assert!(!b1.is_empty());
    }
}

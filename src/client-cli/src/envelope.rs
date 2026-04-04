//! # Envelope 构建器
//!
//! 封装 SignedEnvelope 的完整构建流程：
//! PoW 计算 → 签名 → bencode 编码 → hex 编码
//!
//! 同时计算 BEP 44 签名，供网关转发到 DHT 网络。

use actinium_core::{
    Comment, Identity, Post, PowChallenge, SignedEnvelope, Vote, NETWORK_ID_LEN,
};
use mainline::{MutableItem, SigningKey as MainlineSigningKey};

/// 构建结果，包含 envelope 的 hex 编码和 BEP 44 签名 hex。
pub struct BuildResult {
    /// 完整 SignedEnvelope 的 bencode 编码后的 hex 字符串。
    pub envelope_hex: String,
    /// BEP 44 签名的 hex 字符串（64 字节 = 128 字符）。
    /// 网关用此签名将数据转发到 DHT 网络。
    pub bep44_sig_hex: String,
}

/// 提供各种消息类型 SignedEnvelope 的构建方法。
pub struct EnvelopeBuilder;

impl EnvelopeBuilder {
    /// 构建 Post envelope，返回构建结果。
    pub fn build_post(
        identity: &Identity,
        title: &str,
        content: &str,
        difficulty: u8,
        network_id: &[u8; NETWORK_ID_LEN],
    ) -> BuildResult {
        let timestamp = chrono::Utc::now().timestamp();
        let post = Post {
            title: title.to_string(),
            content: content.to_string(),
            difficulty,
        };

        let envelope = Self::sign_envelope(identity, post, timestamp, difficulty, network_id);
        Self::build_result(identity, &envelope, timestamp)
    }

    /// 构建 Comment envelope，返回构建结果。
    pub fn build_comment(
        identity: &Identity,
        post_id: &str,
        content: &str,
        difficulty: u8,
        network_id: &[u8; NETWORK_ID_LEN],
    ) -> BuildResult {
        let timestamp = chrono::Utc::now().timestamp();
        let comment = Comment {
            post_id: post_id.to_string(),
            content: content.to_string(),
        };

        let envelope = Self::sign_envelope(identity, comment, timestamp, difficulty, network_id);
        Self::build_result(identity, &envelope, timestamp)
    }

    /// 构建 Vote envelope，返回构建结果。
    pub fn build_vote(
        identity: &Identity,
        target_id: &str,
        positive: bool,
        difficulty: u8,
        network_id: &[u8; NETWORK_ID_LEN],
    ) -> BuildResult {
        let timestamp = chrono::Utc::now().timestamp();
        let vote = Vote {
            target_id: target_id.to_string(),
            positive: if positive { 1u8 } else { 0u8 },
        };

        let envelope = Self::sign_envelope(identity, vote, timestamp, difficulty, network_id);
        Self::build_result(identity, &envelope, timestamp)
    }

    /// 通用的签名 + PoW 流程。
    fn sign_envelope<T>(
        identity: &Identity,
        payload: T,
        timestamp: i64,
        difficulty: u8,
        network_id: &[u8; NETWORK_ID_LEN],
    ) -> SignedEnvelope<T>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Clone,
    {
        // 1. PoW
        let mut pow_prefix = Vec::with_capacity(40);
        pow_prefix.extend_from_slice(&identity.public_key_bytes());
        pow_prefix.extend_from_slice(&timestamp.to_le_bytes());

        let challenge = PowChallenge::new(pow_prefix, difficulty);
        let solution = challenge.solve().expect("PoW 求解失败");

        // 2. 签名（包含 network_id）
        let signing_bytes = SignedEnvelope::<T>::signing_bytes(
            network_id,
            &payload,
            timestamp,
            solution.nonce,
            &identity.public_key_bytes(),
        )
        .expect("signing_bytes 生成失败");
        let sig = identity.sign(&signing_bytes);

        // 3. 组装
        SignedEnvelope::new(
            *network_id,
            payload,
            timestamp,
            solution.nonce,
            solution.hash,
            identity.public_key_bytes(),
            sig.to_bytes(),
        )
    }

    /// 从已签名的 envelope 生成 BuildResult。
    ///
    /// 包括：
    /// - bencode 编码后的 hex 字符串
    /// - 使用同一私钥计算的 BEP 44 签名（用于 DHT 广播）
    fn build_result<T: serde::Serialize + serde::de::DeserializeOwned>(
        identity: &Identity,
        envelope: &SignedEnvelope<T>,
        timestamp: i64,
    ) -> BuildResult {
        let bencode_data = envelope.to_bencode().expect("bencode 序列化失败");
        let envelope_hex = hex::encode(&bencode_data);

        // 计算 BEP 44 签名：
        // mainline crate 的 MutableItem::new 会用 SigningKey 对 value + seq 进行签名。
        // 我们利用它来获取正确的 BEP 44 签名。
        let mainline_signing_key = MainlineSigningKey::from_bytes(&identity.to_bytes());
        let item = MutableItem::new(
            mainline_signing_key,
            &bencode_data,
            timestamp, // seq = timestamp
            None,      // no salt
        );
        let bep44_sig_hex = hex::encode(item.signature());

        BuildResult {
            envelope_hex,
            bep44_sig_hex,
        }
    }
}

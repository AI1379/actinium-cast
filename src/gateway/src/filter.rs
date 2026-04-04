//! # 数据过滤与验证模块
//!
//! 实现网关的防垃圾过滤器：
//! 1. 验证 PoW 是否满足最低难度要求。
//! 2. 验证 Ed25519 签名是否与消息内容匹配。
//!
//! 通过验证的数据才会被写入本地缓存或返回给客户端。

use actinium_core::{PowChallenge, SignedEnvelope, verify_signature};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;
use tracing::{debug, warn};

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum FilterError {
    #[error("PoW 难度不足：要求 >= {required}，实际 {actual}")]
    InsufficientDifficulty { required: u8, actual: u8 },

    #[error("PoW 验证失败：nonce/哈希与前缀不匹配")]
    InvalidPow,

    #[error("Ed25519 签名验证失败")]
    InvalidSignature,

    #[error("消息序列化失败: {0}")]
    SerializationError(String),

    #[error("时间戳异常：消息时间戳 {ts} 偏离当前时间超过 {max_drift_secs} 秒")]
    TimestampDrift { ts: i64, max_drift_secs: i64 },
}

// ─── 过滤配置 ────────────────────────────────────────────────────────────────

/// 网关过滤器的运行时配置。
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// 最低 PoW 难度（bit 前导零数量），低于此值的消息将被丢弃。
    pub min_difficulty: u8,
    /// 最大允许的时间戳偏移量（秒）。
    /// 超过此值的消息被视为重放攻击或时钟偏差。
    pub max_timestamp_drift_secs: i64,
}

impl Default for FilterConfig {
    fn default() -> Self {
        FilterConfig {
            min_difficulty: 8,            // 至少 1 字节前导零
            max_timestamp_drift_secs: 600, // ±10 分钟
        }
    }
}

// ─── 核心过滤逻辑 ─────────────────────────────────────────────────────────────

/// 对一个 `SignedEnvelope` 执行完整的防垃圾验证。
///
/// 验证顺序：
/// 1. 检查 PoW 难度是否达标。
/// 2. 检查 PoW nonce + hash 与 prefix 是否一致。
/// 3. 检查时间戳偏移是否在合理范围。
/// 4. 检查 Ed25519 签名是否合法。
pub fn validate_envelope<T: Serialize + DeserializeOwned>(
    envelope: &SignedEnvelope<T>,
    config: &FilterConfig,
    reported_difficulty: u8,
) -> Result<(), FilterError> {
    // ── 1. 难度门槛 ──
    if reported_difficulty < config.min_difficulty {
        warn!(
            required = config.min_difficulty,
            actual = reported_difficulty,
            "拒绝：PoW 难度不足"
        );
        return Err(FilterError::InsufficientDifficulty {
            required: config.min_difficulty,
            actual: reported_difficulty,
        });
    }

    // ── 2. PoW 验证 ──
    let pow_prefix = build_pow_prefix(envelope);
    let challenge = PowChallenge::new(pow_prefix, reported_difficulty);
    let pow_solution = actinium_core::PowSolution {
        nonce: envelope.pow_nonce,
        hash: envelope.pow_hash_array(),
    };
    if !challenge.verify(&pow_solution) {
        warn!("拒绝：PoW 哈希验证失败");
        return Err(FilterError::InvalidPow);
    }
    debug!("PoW 验证通过 (difficulty={})", reported_difficulty);

    // ── 3. 时间戳偏移 ──
    let now = chrono::Utc::now().timestamp();
    let drift = (now - envelope.timestamp).abs();
    if drift > config.max_timestamp_drift_secs {
        warn!(
            ts = envelope.timestamp,
            now,
            drift,
            "拒绝：时间戳偏差过大"
        );
        return Err(FilterError::TimestampDrift {
            ts: envelope.timestamp,
            max_drift_secs: config.max_timestamp_drift_secs,
        });
    }

    // ── 4. 签名验证 ──
    let signing_bytes =
        SignedEnvelope::<T>::signing_bytes(
            &envelope.payload,
            envelope.timestamp,
            envelope.pow_nonce,
            &envelope.public_key_array(),
        )
        .map_err(|e| FilterError::SerializationError(e.to_string()))?;

    if !verify_signature(
        &envelope.public_key_array(),
        &signing_bytes,
        &envelope.signature_array(),
    ) {
        warn!("拒绝：签名无效");
        return Err(FilterError::InvalidSignature);
    }
    debug!("签名验证通过");

    Ok(())
}

/// 根据 envelope 的公钥 + 时间戳构建 PoW 挑战前缀。
///
/// ```text
/// prefix = public_key (32 bytes) || timestamp_le (8 bytes)
/// ```
fn build_pow_prefix<T>(envelope: &SignedEnvelope<T>) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(40);
    prefix.extend_from_slice(&envelope.public_key);
    prefix.extend_from_slice(&envelope.timestamp.to_le_bytes());
    prefix
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use actinium_core::{Identity, PowChallenge, Post, SignedEnvelope};

    /// 辅助函数：构造一个完整、合法的 SignedEnvelope<Post>。
    fn make_valid_post_envelope(difficulty: u8) -> SignedEnvelope<Post> {
        let identity = Identity::generate();
        let timestamp = chrono::Utc::now().timestamp();
        let post = Post {
            title: "Test Post".to_string(),
            content: "Hello from filter test".to_string(),
            difficulty,
        };

        // 计算 PoW
        let mut pow_prefix = Vec::with_capacity(40);
        pow_prefix.extend_from_slice(&identity.public_key_bytes());
        pow_prefix.extend_from_slice(&timestamp.to_le_bytes());
        let challenge = PowChallenge::new(pow_prefix, difficulty);
        let solution = challenge.solve().expect("PoW solve should succeed");

        // 签名
        let signing_bytes = SignedEnvelope::<Post>::signing_bytes(
            &post,
            timestamp,
            solution.nonce,
            &identity.public_key_bytes(),
        )
        .unwrap();
        let sig = identity.sign(&signing_bytes);

        SignedEnvelope::new(
            post,
            timestamp,
            solution.nonce,
            solution.hash,
            identity.public_key_bytes(),
            sig.to_bytes(),
        )
    }

    #[test]
    fn test_valid_envelope_passes() {
        let envelope = make_valid_post_envelope(8);
        let config = FilterConfig::default();
        assert!(validate_envelope(&envelope, &config, 8).is_ok());
    }

    #[test]
    fn test_insufficient_difficulty_rejected() {
        let envelope = make_valid_post_envelope(4);
        let config = FilterConfig {
            min_difficulty: 8,
            ..Default::default()
        };
        // reported difficulty = 4，低于 min_difficulty = 8
        let result = validate_envelope(&envelope, &config, 4);
        assert!(matches!(result, Err(FilterError::InsufficientDifficulty { .. })));
    }

    #[test]
    fn test_tampered_signature_rejected() {
        let mut envelope = make_valid_post_envelope(8);
        // 篡改签名最后一个字节
        let last = envelope.signature.len() - 1;
        envelope.signature[last] ^= 0xFF;

        let config = FilterConfig::default();
        let result = validate_envelope(&envelope, &config, 8);
        assert!(matches!(result, Err(FilterError::InvalidSignature)));
    }

    #[test]
    fn test_tampered_pow_rejected() {
        let mut envelope = make_valid_post_envelope(8);
        // 篡改 PoW nonce
        envelope.pow_nonce = envelope.pow_nonce.wrapping_add(1);

        let config = FilterConfig::default();
        let result = validate_envelope(&envelope, &config, 8);
        assert!(matches!(result, Err(FilterError::InvalidPow)));
    }
}

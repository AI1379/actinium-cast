//! # Proof-of-Work Module
//!
//! 实现基于 SHA-256 的 Hashcash 变种 PoW，用于抗垃圾和限流。
//!
//! ## 算法说明
//!
//! ```text
//! hash = SHA-256(prefix || nonce_as_8_le_bytes)
//! ```
//!
//! 当 `hash` 的前 `difficulty` 个 **bit** 均为 0 时，PoW 成立。
//! 难度为 8 时表示哈希首字节为 `0x00`，难度为 16 时前两字节均为 `0x00`，以此类推。

use sha2::{Digest, Sha256};
use thiserror::Error;

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum PowError {
    #[error("nonce 耗尽 (u64::MAX)，无法在当前难度下找到解")]
    NonceExhausted,
}

// ─── 核心结构 ─────────────────────────────────────────────────────────────────

/// PoW 挑战配置。
///
/// `prefix` 通常由调用方根据业务填充，例如：
/// `public_key_bytes (32) || timestamp_le_bytes (8)`。
#[derive(Debug, Clone)]
pub struct PowChallenge {
    /// 与内容绑定的挑战前缀（通常包含公钥 + 时间戳）。
    pub prefix: Vec<u8>,
    /// 要求哈希结果前导零的 **bit** 数量（1–64 合法）。
    pub difficulty: u8,
}

/// PoW 解，包含满足难度要求的 nonce 及对应的最终哈希。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowSolution {
    /// 满足难度的随机数。
    pub nonce: u64,
    /// 最终 SHA-256(prefix || nonce) 哈希。
    pub hash: [u8; 32],
}

// ─── 实现 ─────────────────────────────────────────────────────────────────────

impl PowChallenge {
    /// 构造一个新的 PoW 挑战。
    pub fn new(prefix: Vec<u8>, difficulty: u8) -> Self {
        PowChallenge { prefix, difficulty }
    }

    /// CPU 满载暴力求解：枚举 nonce 直到哈希满足难度要求。
    ///
    /// 对于合理难度（≤ 24 bit），通常在毫秒内完成。
    ///
    /// # Errors
    /// 当 nonce 溢出 `u64::MAX` 时返回 [`PowError::NonceExhausted`]（极度罕见）。
    pub fn solve(&self) -> Result<PowSolution, PowError> {
        for nonce in 0u64..=u64::MAX {
            let hash = self.compute_hash(nonce);
            if self.meets_difficulty(&hash) {
                return Ok(PowSolution { nonce, hash });
            }
        }
        Err(PowError::NonceExhausted)
    }

    /// 快速校验一个 PoW 解是否合法（仅做一次 SHA-256）。
    pub fn verify(&self, solution: &PowSolution) -> bool {
        let expected_hash = self.compute_hash(solution.nonce);
        expected_hash == solution.hash && self.meets_difficulty(&solution.hash)
    }

    // ── 私有辅助 ──────────────────────────────────────────────────────────

    /// 计算 `SHA-256(prefix || nonce_le_bytes)`。
    fn compute_hash(&self, nonce: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.prefix);
        hasher.update(nonce.to_le_bytes());
        hasher.finalize().into()
    }

    /// 检查哈希的前 `difficulty` 个 bit 是否均为 0。
    fn meets_difficulty(&self, hash: &[u8; 32]) -> bool {
        let full_bytes = (self.difficulty / 8) as usize;
        let remainder_bits = self.difficulty % 8;

        // 检查完整的前导零字节
        for &byte in &hash[..full_bytes] {
            if byte != 0 {
                return false;
            }
        }

        // 检查剩余不足一字节的前导零 bit
        if remainder_bits > 0 {
            let mask = 0xFFu8 << (8 - remainder_bits);
            if hash[full_bytes] & mask != 0 {
                return false;
            }
        }

        true
    }
}

// ─── 便利函数 ─────────────────────────────────────────────────────────────────

/// 一步构建挑战并求解（语法糖）。
pub fn solve_pow(prefix: Vec<u8>, difficulty: u8) -> Result<PowSolution, PowError> {
    PowChallenge::new(prefix, difficulty).solve()
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 低难度（8 bit = 首字节为 0）下求解并验证。
    #[test]
    fn test_solve_and_verify_difficulty_8() {
        let prefix = b"test_prefix_data".to_vec();
        let challenge = PowChallenge::new(prefix, 8);

        let solution = challenge.solve().expect("solve should succeed");

        // 最终哈希首字节必须为 0
        assert_eq!(solution.hash[0], 0x00, "first byte must be zero for difficulty=8");
        // 哈希必须与 nonce 一致
        assert!(challenge.verify(&solution), "verify must pass for a valid solution");
    }

    /// 更高难度（16 bit）验证。
    #[test]
    fn test_solve_and_verify_difficulty_16() {
        let prefix = b"actinium_pow".to_vec();
        let challenge = PowChallenge::new(prefix, 16);

        let solution = challenge.solve().expect("solve should succeed");

        assert_eq!(solution.hash[0], 0x00);
        assert_eq!(solution.hash[1], 0x00);
        assert!(challenge.verify(&solution));
    }

    /// 验证篡改 nonce 后校验失败。
    #[test]
    fn test_verify_fails_on_tampered_nonce() {
        let challenge = PowChallenge::new(b"prefix".to_vec(), 8);
        let mut solution = challenge.solve().expect("solve should succeed");

        // 故意修改 nonce
        solution.nonce = solution.nonce.wrapping_add(1);
        assert!(
            !challenge.verify(&solution),
            "verify must fail when nonce is tampered"
        );
    }

    /// 验证篡改哈希后校验失败。
    #[test]
    fn test_verify_fails_on_tampered_hash() {
        let challenge = PowChallenge::new(b"prefix".to_vec(), 8);
        let mut solution = challenge.solve().expect("solve should succeed");

        // 把哈希首字节改成非零值，使其不满足难度要求
        solution.hash[0] = 0xFF;
        assert!(
            !challenge.verify(&solution),
            "verify must fail when hash does not meet difficulty"
        );
    }

    /// 验证 `meets_difficulty` 在非字节对齐难度下的行为（例如 difficulty=12 = 1 整字节 + 4 bit）。
    #[test]
    fn test_non_byte_aligned_difficulty() {
        let challenge = PowChallenge::new(b"align_test".to_vec(), 12);
        let solution = challenge.solve().expect("solve should succeed");

        // 首字节必须为 0x00，第二字节高 4 bit 必须为 0b0000
        assert_eq!(solution.hash[0], 0x00);
        assert_eq!(solution.hash[1] & 0xF0, 0x00);
        assert!(challenge.verify(&solution));
    }
}

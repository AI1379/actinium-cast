//! # actinium-core
//!
//! Actinium Cast 项目的核心密码学与数据模型库。
//!
//! ## 模块结构
//!
//! - [`identity`] — Ed25519 密钥对管理、签名与验证
//! - [`pow`]      — SHA-256 Hashcash 变种 Proof-of-Work
//! - [`models`]   — BEP 44 兼容的数据结构（Post、Comment、Vote、SignedEnvelope）

pub mod identity;
pub mod models;
pub mod pow;

// 便捷的顶层 re-export
pub use identity::{Identity, IdentityError, verify_signature};
pub use models::{Comment, ModelError, Post, SignedEnvelope, Vote};
pub use pow::{PowChallenge, PowError, PowSolution};

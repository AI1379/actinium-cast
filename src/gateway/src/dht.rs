//! # DHT 适配器模块
//!
//! 封装 `mainline` crate，提供 BEP 44 Mutable Data 的 `get` / `put` 操作。
//!
//! ## 设计思路
//!
//! - 网关以 **server 模式** 启动 DHT 节点，参与路由并存储数据。
//! - 所有 DHT 操作均通过 [`DhtAdapter`] 统一暴露，上层无需关心底层协议细节。
//! - 使用 `salt` 字段区分同一公钥下的不同内容类型（Post / Comment / Vote）。

use mainline::{Dht, MutableItem, SigningKey};
use std::sync::Arc;
use thiserror::Error;
use tracing::{info, warn};

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DhtError {
    #[error("DHT 初始化失败: {0}")]
    Init(#[from] std::io::Error),

    #[error("DHT put_mutable 失败: {0}")]
    PutMutable(String),

    #[error("未找到 mutable item (public_key={public_key_hex}, salt={salt:?})")]
    NotFound {
        public_key_hex: String,
        salt: Option<String>,
    },
}

// ─── DHT 适配器 ──────────────────────────────────────────────────────────────

/// 封装 `mainline::Dht`，提供项目所需的 BEP 44 高层接口。
#[derive(Clone)]
pub struct DhtAdapter {
    dht: Dht,
}

impl DhtAdapter {
    /// 以 server 模式启动 DHT 节点（会监听 UDP 端口并参与全局路由）。
    pub fn new_server() -> Result<Self, DhtError> {
        info!("正在启动 DHT server 模式节点...");
        let dht = Dht::server()?;
        let bootstrapped = dht.bootstrapped();
        if bootstrapped {
            info!("DHT 节点引导完成，已加入网络");
        } else {
            warn!("DHT 节点引导可能未完全成功，继续运行");
        }
        Ok(DhtAdapter { dht })
    }

    /// 以 client 模式启动 DHT 节点（只查询，不参与存储）。
    pub fn new_client() -> Result<Self, DhtError> {
        info!("正在启动 DHT client 模式节点...");
        let dht = Dht::client()?;
        let bootstrapped = dht.bootstrapped();
        if bootstrapped {
            info!("DHT client 节点引导完成");
        } else {
            warn!("DHT client 引导可能未完全成功");
        }
        Ok(DhtAdapter { dht })
    }

    // ── BEP 44 Mutable Data ─────────────────────────────────────────────

    /// 将已签名的 bencode 数据发布到 DHT。
    ///
    /// # 参数
    /// - `signing_key`: mainline 的 `SigningKey`（32 字节种子）。
    /// - `value`: bencode 编码后的完整 `SignedEnvelope` 字节。
    /// - `seq`: BEP 44 序列号，用于版本管理。
    /// - `salt`: 可选的 salt（用于区分同公钥下的不同内容槽位）。
    pub fn put_mutable(
        &self,
        signing_key: &SigningKey,
        value: &[u8],
        seq: i64,
        salt: Option<&[u8]>,
    ) -> Result<(), DhtError> {
        let item = MutableItem::new(signing_key.clone(), value, seq, salt);
        self.dht
            .put_mutable(item, None)
            .map_err(|e| DhtError::PutMutable(format!("{e:?}")))?;
        info!(seq, "成功发布 mutable item 到 DHT");
        Ok(())
    }

    /// 从 DHT 获取最新的 mutable item。
    ///
    /// # 参数
    /// - `public_key`: 32 字节 Ed25519 公钥。
    /// - `salt`: 可选的 salt。
    ///
    /// # 返回
    /// 成功时返回 `(value_bytes, seq)`。
    pub fn get_mutable(
        &self,
        public_key: &[u8; 32],
        salt: Option<&[u8]>,
    ) -> Result<(Vec<u8>, i64), DhtError> {
        match self.dht.get_mutable_most_recent(public_key, salt) {
            Some(item) => {
                let value = item.value().to_vec();
                let seq = item.seq();
                info!(seq, value_len = value.len(), "从 DHT 获取到 mutable item");
                Ok((value, seq))
            }
            None => Err(DhtError::NotFound {
                public_key_hex: hex::encode(public_key),
                salt: salt.map(|s| hex::encode(s)),
            }),
        }
    }

    /// 获取底层 DHT 节点引用（用于高级操作）。
    pub fn inner(&self) -> &Dht {
        &self.dht
    }
}

/// 将 `DhtAdapter` 包装为线程安全的 `Arc`，方便在 `axum` State 中共享。
pub type SharedDht = Arc<DhtAdapter>;

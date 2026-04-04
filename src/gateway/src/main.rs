//! # Actinium Cast Gateway
//!
//! 网关服务的入口点。
//!
//! ## 启动流程
//!
//! 1. 初始化日志系统。
//! 2. 解析网络标识符。
//! 3. 打开本地 SQLite 缓存。
//! 4. (可选) 启动 DHT 节点。
//! 5. 构建 HTTP API 路由。
//! 6. 启动 HTTP 服务器。

mod api;
mod cache;
mod dht;
mod filter;

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use actinium_core::NETWORK_ID_LEN;
use crate::api::{AppState, build_router};
use crate::cache::Cache;
use crate::dht::DhtAdapter;
use crate::filter::FilterConfig;

/// 从环境变量 `ACTINIUM_NETWORK_ID` 解析 network_id（64 个 hex 字符 = 32 字节）。
/// 若未设置，则使用固定种子生成一个开发用网络 ID。
fn resolve_network_id() -> [u8; NETWORK_ID_LEN] {
    if let Ok(hex_str) = std::env::var("ACTINIUM_NETWORK_ID") {
        let hex_str = hex_str.trim();
        let bytes = hex::decode(hex_str).unwrap_or_else(|e| {
            eprintln!("❌ ACTINIUM_NETWORK_ID hex 解码失败: {e}");
            eprintln!("   期望 {} 个 hex 字符 (= {} 字节)", NETWORK_ID_LEN * 2, NETWORK_ID_LEN);
            std::process::exit(1);
        });
        if bytes.len() != NETWORK_ID_LEN {
            eprintln!(
                "❌ ACTINIUM_NETWORK_ID 长度错误: 期望 {} 字节, 实际 {} 字节",
                NETWORK_ID_LEN,
                bytes.len()
            );
            std::process::exit(1);
        }
        let mut arr = [0u8; NETWORK_ID_LEN];
        arr.copy_from_slice(&bytes);
        arr
    } else {
        // 未设置环境变量，使用固定种子生成开发用网络 ID
        // SHA-256("actinium-cast-dev-network-v1")
        use sha2::{Sha256, Digest};
        let hash = Sha256::digest(b"actinium-cast-dev-network-v1");
        let mut arr = [0u8; NETWORK_ID_LEN];
        arr.copy_from_slice(&hash);
        arr
    }
}

#[tokio::main]
async fn main() {
    // ── 1. 日志初始化 ──
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("gateway=info,actinium_core=info")),
        )
        .init();

    info!("🚀 Actinium Cast Gateway 正在启动...");

    // ── 2. 网络标识符 ──
    let network_id = resolve_network_id();
    let network_id_hex = hex::encode(network_id);
    if std::env::var("ACTINIUM_NETWORK_ID").is_ok() {
        info!(network_id = %network_id_hex, "🔗 使用自定义网络 ID");
    } else {
        info!(network_id = %network_id_hex, "🔗 使用开发默认网络 ID (设置 ACTINIUM_NETWORK_ID 环境变量来自定义)");
    }

    // ── 3. 本地缓存 ──
    let cache = Cache::open("gateway_cache.db").expect("无法打开缓存数据库");
    info!("📦 本地缓存已就绪");

    // ── 4. DHT 节点 ──
    // 默认尝试以 client 模式启动，失败时跳过（允许离线开发/测试）
    let dht = match DhtAdapter::new_client() {
        Ok(adapter) => {
            info!("🌐 DHT 节点已启动（client 模式）");
            Some(adapter)
        }
        Err(e) => {
            tracing::warn!("⚠️ DHT 连接失败，将以离线模式运行: {e}");
            None
        }
    };

    // ── 5. 过滤配置 ──
    let filter_config = FilterConfig::with_network_id(network_id);
    info!(
        min_difficulty = filter_config.min_difficulty,
        max_drift = filter_config.max_timestamp_drift_secs,
        "🛡️ 过滤器配置完成"
    );

    // ── 6. 构建应用状态 ──
    let state = Arc::new(AppState {
        cache,
        dht,
        filter_config,
    });

    // ── 7. 构建路由 ──
    let app = build_router(state);

    // ── 8. 启动 HTTP 服务器 ──
    let bind_addr = "0.0.0.0:3000";
    info!("🌍 HTTP API 服务监听于 http://{bind_addr}");

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("无法绑定端口");

    axum::serve(listener, app)
        .await
        .expect("HTTP 服务器异常退出");
}

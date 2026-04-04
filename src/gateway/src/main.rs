//! # Actinium Cast Gateway
//!
//! 网关服务的入口点。
//!
//! ## 启动流程
//!
//! 1. 初始化日志系统。
//! 2. 打开本地 SQLite 缓存。
//! 3. (可选) 启动 DHT 节点。
//! 4. 构建 HTTP API 路由。
//! 5. 启动 HTTP 服务器。

mod api;
mod cache;
mod dht;
mod filter;

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::api::{AppState, build_router};
use crate::cache::Cache;
use crate::dht::DhtAdapter;
use crate::filter::FilterConfig;

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

    // ── 2. 本地缓存 ──
    let cache = Cache::open("gateway_cache.db").expect("无法打开缓存数据库");
    info!("📦 本地缓存已就绪");

    // ── 3. DHT 节点 ──
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

    // ── 4. 过滤配置 ──
    let filter_config = FilterConfig::default();
    info!(
        min_difficulty = filter_config.min_difficulty,
        max_drift = filter_config.max_timestamp_drift_secs,
        "🛡️ 过滤器配置完成"
    );

    // ── 5. 构建应用状态 ──
    let state = Arc::new(AppState {
        cache,
        dht,
        filter_config,
    });

    // ── 6. 构建路由 ──
    let app = build_router(state);

    // ── 7. 启动 HTTP 服务器 ──
    let bind_addr = "0.0.0.0:3000";
    info!("🌍 HTTP API 服务监听于 http://{bind_addr}");

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("无法绑定端口");

    axum::serve(listener, app)
        .await
        .expect("HTTP 服务器异常退出");
}

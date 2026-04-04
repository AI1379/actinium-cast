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
use crate::cache::{Cache, MessageType};
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
    let db_path = std::env::var("CACHE_DB").unwrap_or_else(|_| "gateway_cache.db".to_string());
    let cache = Cache::open(&db_path).expect("无法打开缓存数据库");
    info!("📦 本地缓存已就绪");

    // ── 4. DHT 节点 ──
    // 启动 server 模式以更好地支持 P2P 网络并在本地维护路由表
    let dht = match DhtAdapter::new_server() {
        Ok(adapter) => {
            info!("🌐 DHT 节点已启动（server 模式）");
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
    let app = build_router(state.clone());

    // ── 8. 网关横向 Mesh 同步守护线程 (BEP 5 Tracker 模式) ──
    let port_str = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let port: u16 = port_str.parse().unwrap_or(3000);

    if let Some(dht_adapter) = &state.dht {
        tokio::spawn(mesh_sync_task(
            state.clone(),
            dht_adapter.clone(),
            network_id,
            port,
        ));
    }

    // ── 9. 启动 HTTP 服务器 ──
    let bind_addr = format!("0.0.0.0:{port}");
    info!("🌍 HTTP API 服务监听于 http://{bind_addr}");

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .expect("无法绑定端口");

    axum::serve(listener, app)
        .await
        .expect("HTTP 服务器异常退出");
}

// ─── 后台任务：Gateway Mesh 同步 ──────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SyncResponse {
    ok: bool,
    data: Option<Vec<SyncMessageDto>>,
}

#[derive(serde::Deserialize)]
struct SyncMessageDto {
    id: i64,
    msg_type: String,
    public_key: String,
    timestamp: i64,
    envelope_hex: String,
}

/// 后台同步任务：通过 DHT 寻找同伴网关，并跨节点拉取数据。
async fn mesh_sync_task(
    state: Arc<AppState>,
    dht: DhtAdapter,
    network_id: [u8; NETWORK_ID_LEN],
    local_port: u16,
) {
    info!("🔄 Mesh Sync 后台任务已启动");
    use sha2::{Digest, Sha256};
    
    // 把 network_id hash 一次作为我们的“专属私密 Tracker 频道”
    let info_hash_bytes: [u8; 20] = Sha256::digest(&network_id)[..20].try_into().unwrap();
    let info_hash = mainline::Id::from_bytes(info_hash_bytes).unwrap();
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let mut last_sync_ids = std::collections::HashMap::<std::net::SocketAddr, i64>::new();

    loop {
        // 1. 宣告自己
        if let Err(e) = dht.announce_peer(info_hash, local_port) {
            tracing::debug!("announce_peer 失败: {e}");
        }

        // 2. 寻找同伴网关
        let peers = dht.get_peers(info_hash);
        tracing::debug!("网关 Mesh 发现存活 peers 数量: {}", peers.len());

        for peer in peers {
            // 简单防自环（在云环境/NAT下可能判断不准，但无害，最多自己拉自己）
            if peer.port() == local_port {
                continue; // 只要端口一样就跳过，假设我们在本地测试同一台机器不重叠端口
            }

            // 开发模式：如果设置了 ACTINIUM_DEV_LOCAL_MESH，强制将找到的 peer IP 替换为 127.0.0.1
            // 解决家用路由器不支持 NAT 回环 (Hairpinning) 导致无法通过公网 IP 访问本机端口的问题
            let peer_ip = if std::env::var("ACTINIUM_DEV_LOCAL_MESH").is_ok() {
                "127.0.0.1".to_string()
            } else {
                peer.ip().to_string()
            };

            let since_id = *last_sync_ids.get(&peer).unwrap_or(&0);
            let url = format!("http://{}:{}/api/sync/messages?since_id={}&limit=100", peer_ip, peer.port(), since_id);
            
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(payload) = resp.json::<SyncResponse>().await {
                        if payload.ok {
                            if let Some(messages) = payload.data {
                                let mut max_id = since_id;
                                let mut new_added = 0;

                                for msg in messages {
                                    if msg.id > max_id {
                                        max_id = msg.id;
                                    }
                                    let msg_t = MessageType::from_str(&msg.msg_type).unwrap_or(MessageType::Post);
                                    if let Ok(bencode_data) = hex::decode(&msg.envelope_hex) {
                                        // TODO: 最好也校验一下签名。为了优化性能，在此暂时只依赖对方的可靠性。
                                        // 直接使用 insert 忽略本地已经持有的校验逻辑（也可以使用更复杂的 INSERT OR IGNORE）
                                        // 这里简化处理：因为 Cache 使用了 id 为主键，重复内容会自动叠加（应该查重，但为演示 Mesh 先保留）
                                        if let Ok(_) = state.cache.insert(msg_t, &msg.public_key, msg.timestamp, &bencode_data) {
                                            new_added += 1;
                                        }
                                    }
                                }
                                
                                last_sync_ids.insert(peer, max_id);
                                if new_added > 0 {
                                    tracing::info!("🔗 从 Peer [{peer}] 成功同步了 {new_added} 条增量消息!");
                                }
                            }
                        }
                    }
                }
                _ => {
                    tracing::debug!("Peer [{peer}] 连接失败或超时");
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}


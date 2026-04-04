//! # RESTful API 模块
//!
//! 基于 `axum` 构建网关的 HTTP API 层。
//!
//! ## 路由
//!
//! | 方法   | 路径                               | 说明                   |
//! |--------|------------------------------------|-----------------------|
//! | GET    | `/api/posts`                       | 获取帖子列表           |
//! | GET    | `/api/posts/:id`                   | 获取单个帖子           |
//! | GET    | `/api/posts/:public_key/comments`  | 获取某帖子的评论       |
//! | GET    | `/api/votes/:target_id`            | 获取投票统计           |
//! | POST   | `/api/publish/post`                | 提交并广播一条帖子     |
//! | POST   | `/api/publish/comment`             | 提交并广播一条评论     |
//! | POST   | `/api/publish/vote`                | 提交并广播一条投票     |
//! | GET    | `/api/status`                      | 网关状态               |

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use actinium_core::{Comment, Post, SignedEnvelope, Vote};
use crate::cache::{Cache, CachedMessage, MessageType};
use crate::dht::DhtAdapter;
use crate::filter::{FilterConfig, validate_envelope};

// ─── 应用状态 ────────────────────────────────────────────────────────────────

/// 跨请求共享的应用状态。
pub struct AppState {
    pub cache: Cache,
    pub dht: Option<DhtAdapter>,
    pub filter_config: FilterConfig,
}

pub type SharedState = Arc<AppState>;

// ─── 请求/响应 DTO ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        ApiResponse {
            ok: true,
            data: Some(data),
            error: None,
        }
    }
}

impl ApiResponse<()> {
    pub fn error(msg: impl ToString) -> Self {
        ApiResponse {
            ok: false,
            data: None,
            error: Some(msg.to_string()),
        }
    }
}

/// 从缓存记录中提取的帖子摘要。
#[derive(Debug, Serialize)]
pub struct PostSummary {
    pub id: i64,
    pub public_key: String,
    pub timestamp: i64,
    pub title: String,
    pub content: String,
    pub difficulty: u8,
    pub created_at: String,
}

/// 评论摘要。
#[derive(Debug, Serialize)]
pub struct CommentSummary {
    pub id: i64,
    pub public_key: String,
    pub post_id: String,
    pub timestamp: i64,
    pub content: String,
    pub created_at: String,
}

/// 投票统计。
#[derive(Debug, Serialize)]
pub struct VoteStats {
    pub target_id: String,
    pub likes: i64,
    pub unlikes: i64,
}

/// 网关状态信息。
#[derive(Debug, Serialize)]
pub struct GatewayStatus {
    pub network_id: String,
    pub total_posts: i64,
    pub total_comments: i64,
    pub total_votes: i64,
    pub dht_connected: bool,
}

/// 发布请求体（bencode 编码的 SignedEnvelope 的 hex 表示）。
#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    /// 完整 SignedEnvelope 的 bencode 编码后再 hex encode 的字符串。
    pub envelope_hex: String,
}

// ─── 路由构建 ────────────────────────────────────────────────────────────────

/// 构建完整的 axum Router。
pub fn build_router(state: SharedState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // 读取类路由
        .route("/api/posts", get(list_posts))
        .route("/api/posts/{id}", get(get_post))
        .route("/api/posts/{public_key}/comments", get(list_comments))
        .route("/api/votes/{target_id}", get(get_votes))
        // 写入类路由
        .route("/api/publish/post", post(publish_post))
        .route("/api/publish/comment", post(publish_comment))
        .route("/api/publish/vote", post(publish_vote))
        // 状态路由
        .route("/api/status", get(status))
        .layer(cors)
        .with_state(state)
}

// ─── 处理器实现 ──────────────────────────────────────────────────────────────

/// GET /api/posts — 获取帖子列表
async fn list_posts(
    State(state): State<SharedState>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);

    match state.cache.list_by_type(MessageType::Post, limit, offset) {
        Ok(messages) => {
            let summaries: Vec<PostSummary> = messages
                .into_iter()
                .filter_map(|m| decode_post_summary(m))
                .collect();
            Json(ApiResponse::success(summaries)).into_response()
        }
        Err(e) => {
            error!("查询帖子列表失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(format!("内部错误: {e}"))),
            )
                .into_response()
        }
    }
}

/// GET /api/posts/:id — 获取单个帖子
async fn get_post(
    State(state): State<SharedState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match state.cache.get_by_id(id) {
        Ok(Some(msg)) => {
            if let Some(summary) = decode_post_summary(msg) {
                Json(ApiResponse::success(summary)).into_response()
            } else {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiResponse::<()>::error("无法解码帖子数据")),
                )
                    .into_response()
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error("帖子不存在")),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(format!("内部错误: {e}"))),
        )
            .into_response(),
    }
}

/// GET /api/posts/:public_key/comments — 获取某帖子的评论
async fn list_comments(
    State(state): State<SharedState>,
    Path(public_key): Path<String>,
    Query(params): Query<PaginationParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    match state
        .cache
        .list_comments_for_post(&public_key, limit, offset)
    {
        Ok(messages) => {
            let summaries: Vec<CommentSummary> = messages
                .into_iter()
                .filter_map(|m| decode_comment_summary(m))
                .collect();
            Json(ApiResponse::success(summaries)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(format!("内部错误: {e}"))),
        )
            .into_response(),
    }
}

/// GET /api/votes/:target_id — 获取投票统计
async fn get_votes(
    State(state): State<SharedState>,
    Path(target_id): Path<String>,
) -> impl IntoResponse {
    // 从所有 Vote 类型消息中统计
    match state.cache.list_by_type(MessageType::Vote, 10000, 0) {
        Ok(messages) => {
            let mut likes: i64 = 0;
            let mut unlikes: i64 = 0;
            for msg in messages {
                if let Some(envelope) = try_decode_envelope::<Vote>(&msg.bencode_data) {
                    if envelope.payload.target_id == target_id {
                        if envelope.payload.is_positive() {
                            likes += 1;
                        } else {
                            unlikes += 1;
                        }
                    }
                }
            }
            Json(ApiResponse::success(VoteStats {
                target_id,
                likes,
                unlikes,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(format!("内部错误: {e}"))),
        )
            .into_response(),
    }
}

/// POST /api/publish/post — 提交帖子
async fn publish_post(
    State(state): State<SharedState>,
    Json(req): Json<PublishRequest>,
) -> impl IntoResponse {
    publish_envelope::<Post>(state, req.envelope_hex, MessageType::Post).await
}

/// POST /api/publish/comment — 提交评论
async fn publish_comment(
    State(state): State<SharedState>,
    Json(req): Json<PublishRequest>,
) -> impl IntoResponse {
    publish_envelope::<Comment>(state, req.envelope_hex, MessageType::Comment).await
}

/// POST /api/publish/vote — 提交投票
async fn publish_vote(
    State(state): State<SharedState>,
    Json(req): Json<PublishRequest>,
) -> impl IntoResponse {
    publish_envelope::<Vote>(state, req.envelope_hex, MessageType::Vote).await
}

/// GET /api/status — 网关状态
async fn status(State(state): State<SharedState>) -> impl IntoResponse {
    let total_posts = state.cache.count(Some(MessageType::Post)).unwrap_or(0);
    let total_comments = state.cache.count(Some(MessageType::Comment)).unwrap_or(0);
    let total_votes = state.cache.count(Some(MessageType::Vote)).unwrap_or(0);

    Json(ApiResponse::success(GatewayStatus {
        network_id: state.filter_config.network_id_hex(),
        total_posts,
        total_comments,
        total_votes,
        dht_connected: state.dht.is_some(),
    }))
}

// ─── 通用发布逻辑 ────────────────────────────────────────────────────────────

/// 通用的发布流程：hex 解码 → bencode 解析 → 过滤验证 → 入库 → (可选) DHT 广播。
async fn publish_envelope<T>(
    state: SharedState,
    envelope_hex: String,
    msg_type: MessageType,
) -> impl IntoResponse
where
    T: Serialize + serde::de::DeserializeOwned + std::fmt::Debug,
{
    // 1. hex → bytes
    let bencode_bytes = match hex::decode(&envelope_hex) {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<()>::error(format!("hex 解码失败: {e}"))),
            )
                .into_response();
        }
    };

    // 2. bencode → SignedEnvelope<T>
    let envelope: SignedEnvelope<T> = match SignedEnvelope::from_bencode(&bencode_bytes) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<()>::error(format!("bencode 解析失败: {e}"))),
            )
                .into_response();
        }
    };

    // 3. 提取 reported_difficulty（对 Post 类型从 payload 中读取，其他类型使用 min_difficulty）
    let reported_difficulty = extract_difficulty::<T>(&envelope, &state.filter_config);

    // 4. 过滤验证
    if let Err(e) = validate_envelope(&envelope, &state.filter_config, reported_difficulty) {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse::<()>::error(format!("验证失败: {e}"))),
        )
            .into_response();
    }

    // 5. 入库
    let public_key_hex = hex::encode(&envelope.public_key);
    match state
        .cache
        .insert(msg_type, &public_key_hex, envelope.timestamp, &bencode_bytes)
    {
        Ok(id) => {
            info!(id, msg_type = msg_type.as_str(), "消息已入库");

            // 6. (可选) DHT 广播 — 如果 DHT 适配器已连接
            if let Some(dht) = &state.dht {
                let signing_key = mainline::SigningKey::from_bytes(&envelope.public_key_array());
                if let Err(e) = dht.put_mutable(
                    &signing_key,
                    &bencode_bytes,
                    envelope.timestamp, // 使用时间戳作为 seq
                    None,
                ) {
                    // DHT 广播失败不阻止入库成功
                    error!("DHT 广播失败: {e}");
                }
            }

            Json(ApiResponse::success(serde_json::json!({
                "id": id,
                "message": "发布成功"
            })))
            .into_response()
        }
        Err(e) => {
            error!("入库失败: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(format!("入库失败: {e}"))),
            )
                .into_response()
        }
    }
}

// ─── 辅助函数 ────────────────────────────────────────────────────────────────

/// 尝试从 bencode 字节解码 SignedEnvelope<T>。
fn try_decode_envelope<T: Serialize + serde::de::DeserializeOwned>(
    data: &[u8],
) -> Option<SignedEnvelope<T>> {
    SignedEnvelope::from_bencode(data).ok()
}

/// 将缓存记录解码为 PostSummary。
fn decode_post_summary(msg: CachedMessage) -> Option<PostSummary> {
    let envelope = try_decode_envelope::<Post>(&msg.bencode_data)?;
    Some(PostSummary {
        id: msg.id,
        public_key: msg.public_key,
        timestamp: envelope.timestamp,
        title: envelope.payload.title,
        content: envelope.payload.content,
        difficulty: envelope.payload.difficulty,
        created_at: msg.created_at,
    })
}

/// 将缓存记录解码为 CommentSummary。
fn decode_comment_summary(msg: CachedMessage) -> Option<CommentSummary> {
    let envelope = try_decode_envelope::<Comment>(&msg.bencode_data)?;
    Some(CommentSummary {
        id: msg.id,
        public_key: msg.public_key,
        post_id: envelope.payload.post_id,
        timestamp: envelope.timestamp,
        content: envelope.payload.content,
        created_at: msg.created_at,
    })
}

/// 从 envelope 中提取声称的 PoW 难度。
///
/// 对于 Post 载荷，difficulty 记录在 payload 中。
/// 对于其他类型，回退到 filter_config.min_difficulty。
fn extract_difficulty<T: Serialize>(envelope: &SignedEnvelope<T>, config: &FilterConfig) -> u8 {
    // 我们无法在泛型中直接访问 payload.difficulty，
    // 所以尝试从 bencode 重新解析为 Post 来提取。
    // 如果失败则回退到配置的最低难度。
    if let Ok(bencode) = serde_bencode::to_bytes(&envelope.payload) {
        if let Ok(post) = serde_bencode::from_bytes::<Post>(&bencode) {
            return post.difficulty;
        }
    }
    config.min_difficulty
}

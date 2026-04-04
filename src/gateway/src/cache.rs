//! # 本地高速缓存模块
//!
//! 使用 SQLite 存储从 DHT 拉取或客户端提交的 `SignedEnvelope` 数据。
//!
//! ## 存储模型
//!
//! 每条消息在数据库中存储为 bencode 原始字节，同时记录以下元数据索引：
//! - `id`            — 自增主键
//! - `msg_type`      — 消息类型（post / comment / vote）
//! - `public_key`    — 发布者公钥 hex
//! - `timestamp`     — 消息时间戳（Unix 秒）
//! - `bencode_data`  — 完整的 bencode 编码的 SignedEnvelope 字节
//! - `created_at`    — 入库时间

use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::info;

// ─── 错误类型 ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("SQLite 错误: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// ─── 消息类型 ────────────────────────────────────────────────────────────────

/// 缓存中的消息类型标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Post,
    Comment,
    Vote,
}

impl MessageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::Post => "post",
            MessageType::Comment => "comment",
            MessageType::Vote => "vote",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "post" => Some(MessageType::Post),
            "comment" => Some(MessageType::Comment),
            "vote" => Some(MessageType::Vote),
            _ => None,
        }
    }
}

// ─── 缓存记录 ────────────────────────────────────────────────────────────────

/// 从数据库反序列化的缓存记录。
#[derive(Debug, Clone)]
pub struct CachedMessage {
    pub id: i64,
    pub msg_type: String,
    pub public_key: String,
    pub timestamp: i64,
    pub bencode_data: Vec<u8>,
    pub created_at: String,
}

// ─── 缓存实现 ────────────────────────────────────────────────────────────────

/// 基于 SQLite 的本地高速缓存。
///
/// 内部通过 `Arc<Mutex<Connection>>` 保证多线程与跨任务共享的安全。
#[derive(Clone)]
pub struct Cache {
    conn: Arc<Mutex<Connection>>,
}

impl Cache {
    /// 打开（或创建）SQLite 数据库并初始化表结构。
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, CacheError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous  = NORMAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS messages (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                msg_type     TEXT    NOT NULL,
                public_key   TEXT    NOT NULL,
                timestamp    INTEGER NOT NULL,
                bencode_data BLOB    NOT NULL,
                created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
            );

            CREATE INDEX IF NOT EXISTS idx_messages_type
                ON messages(msg_type);
            CREATE INDEX IF NOT EXISTS idx_messages_pubkey
                ON messages(public_key);
            CREATE INDEX IF NOT EXISTS idx_messages_timestamp
                ON messages(timestamp DESC);
            ",
        )?;
        info!("本地缓存数据库已初始化");
        Ok(Cache {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// 在内存中创建临时缓存（测试用）。
    pub fn open_in_memory() -> Result<Self, CacheError> {
        Self::open(":memory:")
    }

    // ── 写入 ──────────────────────────────────────────────────────────────

    /// 将一条消息写入缓存。
    ///
    /// # 参数
    /// - `msg_type`: 消息类型。
    /// - `public_key_hex`: 发布者公钥的十六进制字符串。
    /// - `timestamp`: 消息时间戳（Unix 秒）。
    /// - `bencode_data`: 完整的 bencode 编码字节。
    pub fn insert(
        &self,
        msg_type: MessageType,
        public_key_hex: &str,
        timestamp: i64,
        bencode_data: &[u8],
    ) -> Result<i64, CacheError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages (msg_type, public_key, timestamp, bencode_data)
             VALUES (?1, ?2, ?3, ?4)",
            params![msg_type.as_str(), public_key_hex, timestamp, bencode_data],
        )?;
        Ok(conn.last_insert_rowid())
    }

    // ── 读取 ──────────────────────────────────────────────────────────────

    /// 按类型分页查询消息，按时间戳降序（最新优先）。
    pub fn list_by_type(
        &self,
        msg_type: MessageType,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CachedMessage>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, msg_type, public_key, timestamp, bencode_data, created_at
             FROM messages
             WHERE msg_type = ?1
             ORDER BY timestamp DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![msg_type.as_str(), limit, offset], |row| {
            Ok(CachedMessage {
                id: row.get(0)?,
                msg_type: row.get(1)?,
                public_key: row.get(2)?,
                timestamp: row.get(3)?,
                bencode_data: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按 ID 获取单条消息。
    pub fn get_by_id(&self, id: i64) -> Result<Option<CachedMessage>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, msg_type, public_key, timestamp, bencode_data, created_at
             FROM messages
             WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(CachedMessage {
                id: row.get(0)?,
                msg_type: row.get(1)?,
                public_key: row.get(2)?,
                timestamp: row.get(3)?,
                bencode_data: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(Ok(msg)) => Ok(Some(msg)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// 按发布者公钥查询所有消息。
    pub fn list_by_public_key(
        &self,
        public_key_hex: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CachedMessage>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, msg_type, public_key, timestamp, bencode_data, created_at
             FROM messages
             WHERE public_key = ?1
             ORDER BY timestamp DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![public_key_hex, limit, offset], |row| {
            Ok(CachedMessage {
                id: row.get(0)?,
                msg_type: row.get(1)?,
                public_key: row.get(2)?,
                timestamp: row.get(3)?,
                bencode_data: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 查询指定帖子的评论（通过 public_key 关联的 Comment 类型消息）。
    pub fn list_comments_for_post(
        &self,
        post_public_key_hex: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<CachedMessage>, CacheError> {
        // Comment 消息中 post_id 字段存储目标帖子的公钥 hex，
        // 但我们只能在 bencode_data 中搜索。
        // 这里使用简单的 LIKE 匹配（生产环境应考虑增加 post_id 索引列）。
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, msg_type, public_key, timestamp, bencode_data, created_at
             FROM messages
             WHERE msg_type = 'comment'
             ORDER BY timestamp DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(CachedMessage {
                id: row.get(0)?,
                msg_type: row.get(1)?,
                public_key: row.get(2)?,
                timestamp: row.get(3)?,
                bencode_data: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;

        // 过滤出 bencode_data 中包含目标 post_id 的评论
        let all: Vec<CachedMessage> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(all
            .into_iter()
            .filter(|m| {
                // 尝试在 bencode 中检查是否包含目标公钥
                // 简单的字节包含检查（bencode 字典中 post_id 值一定包含该 hex 串）
                let pk_bytes = post_public_key_hex.as_bytes();
                m.bencode_data
                    .windows(pk_bytes.len())
                    .any(|w| w == pk_bytes)
            })
            .collect())
    }

    /// 获取消息总数（可选按类型过滤）。
    pub fn count(&self, msg_type: Option<MessageType>) -> Result<i64, CacheError> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = match msg_type {
            Some(t) => conn.query_row(
                "SELECT COUNT(*) FROM messages WHERE msg_type = ?1",
                params![t.as_str()],
                |row| row.get(0),
            )?,
            None => {
                conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row.get(0))?
            }
        };
        Ok(count)
    }

    /// 获取所有大于指定 ID 的消息（用于 P2P 增量同步）。
    pub fn list_all_messages_since(
        &self,
        since_id: i64,
        limit: i64,
    ) -> Result<Vec<CachedMessage>, CacheError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, msg_type, public_key, timestamp, bencode_data, created_at
             FROM messages
             WHERE id > ?1
             ORDER BY id ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since_id, limit], |row| {
            Ok(CachedMessage {
                id: row.get(0)?,
                msg_type: row.get(1)?,
                public_key: row.get(2)?,
                timestamp: row.get(3)?,
                bencode_data: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

// ─── 测试 ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_list() {
        let cache = Cache::open_in_memory().unwrap();

        let id = cache
            .insert(
                MessageType::Post,
                "aabbccdd",
                1_700_000_000,
                b"fake bencode data",
            )
            .unwrap();
        assert!(id > 0);

        let posts = cache.list_by_type(MessageType::Post, 10, 0).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].public_key, "aabbccdd");
        assert_eq!(posts[0].bencode_data, b"fake bencode data");
    }

    #[test]
    fn test_get_by_id() {
        let cache = Cache::open_in_memory().unwrap();
        let id = cache
            .insert(MessageType::Comment, "1122", 1_700_000_001, b"comment")
            .unwrap();

        let msg = cache.get_by_id(id).unwrap().expect("should find by id");
        assert_eq!(msg.msg_type, "comment");

        let none = cache.get_by_id(9999).unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn test_count() {
        let cache = Cache::open_in_memory().unwrap();
        cache.insert(MessageType::Post, "aa", 1, b"d1").unwrap();
        cache.insert(MessageType::Post, "bb", 2, b"d2").unwrap();
        cache.insert(MessageType::Vote, "cc", 3, b"d3").unwrap();

        assert_eq!(cache.count(None).unwrap(), 3);
        assert_eq!(cache.count(Some(MessageType::Post)).unwrap(), 2);
        assert_eq!(cache.count(Some(MessageType::Vote)).unwrap(), 1);
        assert_eq!(cache.count(Some(MessageType::Comment)).unwrap(), 0);
    }
}

# Actinium Cast — 运行与测试指南

本文档说明如何编译、运行和手动测试整个 Actinium Cast 项目（Phase 1 + Phase 2），覆盖从底层密码学库到网关 HTTP API 的完整链路。

---

## 目录

- [前置条件](#前置条件)
- [项目结构](#项目结构)
- [编译项目](#编译项目)
- [运行单元测试](#运行单元测试)
- [启动网关服务](#启动网关服务)
- [手动 API 测试](#手动-api-测试httpie--curl)
  - [检查网关状态](#1-检查网关状态)
  - [构造并发布帖子](#2-构造并发布帖子)
  - [查询帖子列表](#3-查询帖子列表)
  - [查询单条帖子](#4-查询单条帖子)
  - [发布评论](#5-发布评论)
  - [发布投票](#6-发布投票)
  - [查询投票统计](#7-查询投票统计)
  - [验证拒绝非法数据](#8-验证拒绝非法数据)
- [使用测试脚本进行集成测试](#使用测试脚本进行集成测试)
- [日志与调试](#日志与调试)
- [常见问题](#常见问题)

---

## 前置条件

| 工具 | 最低版本 | 说明 |
|------|---------|------|
| **Rust** | 1.85+ (edition 2024) | `rustup update` 确保最新 |
| **Cargo** | 随 Rust 安装 | 构建系统 |
| **curl** 或 **HTTPie** | 任意版本 | API 手动测试 |
| **Python 3** | 3.8+ | 运行集成测试脚本（可选） |

> [!NOTE]
> 项目使用 `rusqlite` 的 `bundled` feature，SQLite 会被自动编译，**无需**系统级安装 SQLite。

---

## 项目结构

```
actinium-cast/
├── Cargo.toml                  # 工作区根配置
├── docs/
│   ├── ARCHITECTURE.md         # 架构设计文档
│   └── ROADMAP.md              # 路线图（含进度）
└── src/
    ├── core/                   # Phase 1: 核心密码学库
    │   └── src/
    │       ├── lib.rs          # 公开模块入口
    │       ├── identity.rs     # Ed25519 密钥管理
    │       ├── pow.rs          # SHA-256 Hashcash PoW
    │       └── models.rs       # BEP 44 数据结构
    ├── gateway/                # Phase 2: 网关服务
    │   └── src/
    │       ├── main.rs         # 入口点（tokio async）
    │       ├── dht.rs          # DHT 适配器（mainline BEP 44）
    │       ├── filter.rs       # 防垃圾过滤器
    │       ├── cache.rs        # SQLite 本地缓存
    │       └── api.rs          # axum REST API
    ├── client-cli/             # Phase 3（占位）
    └── client-wasm/            # Phase 3（占位）
```

---

## 编译项目

```powershell
# 编译整个工作区（首次编译约 2-4 分钟）
cargo build

# 仅编译特定 crate
cargo build -p actinium-core
cargo build -p gateway
```

---

## 运行单元测试

```powershell
# 运行全部测试（21 个）
cargo test -p actinium-core -p gateway

# 仅运行核心密码学测试（14 个）
cargo test -p actinium-core

# 仅运行网关测试（7 个）
cargo test -p gateway

# 运行特定测试并查看详细输出
cargo test -p gateway -- --nocapture filter::tests::test_valid_envelope_passes
```

期望结果：
```
test result: ok. 14 passed (core)
test result: ok.  7 passed (gateway)
```

---

## 启动网关服务

```powershell
# 默认启动（会尝试连接 DHT 网络，失败时降级为离线模式）
cargo run -p gateway
```

成功启动后你会看到类似输出：
```
INFO gateway: 🚀 Actinium Cast Gateway 正在启动...
INFO gateway: 📦 本地缓存已就绪
INFO gateway: 🌐 DHT 节点已启动（client 模式）   # 或 "⚠️ DHT 连接失败..."
INFO gateway: 🛡️ 过滤器配置完成 min_difficulty=8 max_drift=600
INFO gateway: 🌍 HTTP API 服务监听于 http://0.0.0.0:3000
```

> [!TIP]
> 如果你的网络环境无法访问 BitTorrent DHT（如企业防火墙），网关会自动降级为离线模式。
> 所有本地 API（发布、查询）仍然可以正常工作，只是不会广播到全球 DHT 网络。

服务启动后，数据库文件 `gateway_cache.db` 会自动在项目根目录创建。

---

## 手动 API 测试（HTTPie / curl）

以下所有示例假设网关运行在 `http://localhost:3000`。

### 1. 检查网关状态

```powershell
curl http://localhost:3000/api/status
```

期望响应：
```json
{
  "ok": true,
  "data": {
    "total_posts": 0,
    "total_comments": 0,
    "total_votes": 0,
    "dht_connected": true
  }
}
```

### 2. 构造并发布帖子

发布帖子需要一个完整的、经过 PoW 计算和 Ed25519 签名的 `SignedEnvelope<Post>`。由于这个过程涉及密码学运算，手动构造比较复杂。推荐使用下文的 [集成测试脚本](#使用测试脚本进行集成测试)。

发布 API 接受的请求体格式：

```json
{
  "envelope_hex": "<SignedEnvelope 的 bencode 编码再 hex encode 后的字符串>"
}
```

其中 `envelope_hex` 的生成流程如下：

```
1. 生成 Ed25519 密钥对
2. 构造 Post { title, content, difficulty }
3. 获取当前 Unix 时间戳
4. 计算 PoW:
   prefix = public_key_bytes(32) || timestamp_le_bytes(8)
   solve SHA-256 Hashcash with difficulty bits
5. 计算签名:
   signing_bytes = bencode(post) || timestamp_le(8) || pow_nonce_le(8) || public_key(32)
   signature = Ed25519.sign(signing_bytes)
6. 组装 SignedEnvelope { payload, timestamp, pow_nonce, pow_hash, public_key, signature }
7. envelope_hex = hex(bencode(SignedEnvelope))
```

```powershell
# 使用 curl 发布（envelope_hex 需替换为实际值）
curl -X POST http://localhost:3000/api/publish/post `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "<见下方集成测试脚本输出>"}'
```

成功响应：
```json
{
  "ok": true,
  "data": { "id": 1, "message": "发布成功" }
}
```

### 3. 查询帖子列表

```powershell
# 默认查询（最新 20 条）
curl http://localhost:3000/api/posts

# 分页查询
curl "http://localhost:3000/api/posts?limit=5&offset=0"
```

### 4. 查询单条帖子

```powershell
curl http://localhost:3000/api/posts/1
```

### 5. 发布评论

与发布帖子类似，但 payload 为 `Comment { post_id, content }`：

```powershell
curl -X POST http://localhost:3000/api/publish/comment `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "<comment_envelope_hex>"}'
```

### 6. 发布投票

payload 为 `Vote { target_id, positive }`（positive: 1 = 点赞, 0 = 取消）：

```powershell
curl -X POST http://localhost:3000/api/publish/vote `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "<vote_envelope_hex>"}'
```

### 7. 查询投票统计

```powershell
# target_id 为被投票对象的公钥 hex
curl http://localhost:3000/api/votes/<target_public_key_hex>
```

### 8. 验证拒绝非法数据

```powershell
# 发送无效的 hex 字符串（应返回 400）
curl -X POST http://localhost:3000/api/publish/post `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "not_valid_hex!!!"}'

# 发送篡改过的数据（应返回 403 - 验证失败）
curl -X POST http://localhost:3000/api/publish/post `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "aabbccdd"}'
```

---

## 使用测试脚本进行集成测试

由于手动构造 `SignedEnvelope` 需要密码学运算，我们提供了一个 Rust 集成测试程序来自动完成完整的端对端测试流程。

### 运行集成测试

确保网关已在 `localhost:3000` 运行，然后在**另一个终端**执行：

```powershell
cargo test -p gateway --test integration_test -- --nocapture
```

该测试会自动执行以下流程：

```mermaid
graph LR
    A[生成 Ed25519 身份] --> B[构造 Post]
    B --> C[计算 PoW]
    C --> D[签名 → SignedEnvelope]
    D --> E[bencode → hex]
    E --> F[POST /api/publish/post]
    F --> G[GET /api/posts]
    G --> H[GET /api/posts/:id]
    H --> I[构造 Comment → POST]
    I --> J[构造 Vote → POST]
    J --> K[GET /api/votes/:id]
    K --> L[验证非法数据被拒绝]
```

### 集成测试文件

测试文件位于 `src/gateway/tests/integration_test.rs`，它会：

1. **生成身份** — 随机生成 Ed25519 密钥对
2. **发布帖子** — 计算 PoW → 签名 → 编码 → HTTP POST → 验证 201
3. **查询帖子** — GET /api/posts 验证刚发布的帖子出现在列表中
4. **发布评论** — 针对帖子作者公钥创建 Comment
5. **发布投票** — 对帖子点赞
6. **查询统计** — 验证投票数量正确
7. **拒绝测试** — 篡改签名/PoW 的 envelope 被网关 HTTP 403 拒绝

---

## 日志与调试

### 调整日志级别

通过 `RUST_LOG` 环境变量控制日志详细程度：

```powershell
# 查看所有 DEBUG 级别日志
$env:RUST_LOG="gateway=debug"; cargo run -p gateway

# 查看 filter 模块的 debug + 其他保持 info
$env:RUST_LOG="gateway::filter=debug,gateway=info"; cargo run -p gateway

# 查看 DHT 连接的详细日志
$env:RUST_LOG="gateway=debug,mainline=debug"; cargo run -p gateway

# 查看所有 trace 级别日志（非常详细）
$env:RUST_LOG="trace"; cargo run -p gateway
```

### 数据库检查

可以直接用 SQLite 工具查看缓存数据库：

```powershell
# 安装 sqlite3 CLI（如已有则跳过）
# Windows: winget install SQLite.SQLite

sqlite3 gateway_cache.db

# 在 sqlite3 shell 中：
.tables
SELECT id, msg_type, public_key, timestamp, created_at FROM messages ORDER BY id DESC LIMIT 10;
SELECT COUNT(*) FROM messages GROUP BY msg_type;
.quit
```

### 清空测试数据

```powershell
# 删除缓存数据库以重置状态
Remove-Item gateway_cache.db -ErrorAction SilentlyContinue

# 然后重新启动网关
cargo run -p gateway
```

---

## 常见问题

### Q: 编译报错 `ed25519-dalek` 版本冲突？

**A:** 这是正常的。`actinium-core` 使用 `ed25519-dalek 2.1`，而 `mainline` 使用 `ed25519-dalek 3.0.0-pre.1`。Cargo 会自动编译两个版本并隔离它们的类型系统。**不影响运行**。

### Q: 启动网关时提示 "DHT 连接失败"？

**A:** 这表示当前网络环境无法访问 BitTorrent DHT 网络（常见于企业防火墙环境）。网关会自动降级为离线模式：
- ✅ 所有 API 端点正常工作
- ✅ 本地发布、查询、过滤功能完整
- ❌ 数据不会广播到全球 DHT 网络

### Q: 端口 3000 被占用怎么办？

**A:** 目前端口硬编码为 `3000`。可以暂时杀掉占用进程：
```powershell
netstat -ano | Select-String "3000"
Stop-Process -Id <PID>
```

### Q: `gateway_cache.db` 可以在多实例间共享吗？

**A:** 不推荐。SQLite 在多进程并发写入时可能出现锁争用。每个网关实例应使用独立的数据库文件。

### Q: PoW 计算太慢？

**A:** PoW 计算时间与难度指数相关：
| 难度 (bits) | 期望计算次数 | 大约耗时 |
|-------------|-------------|---------|
| 8           | ~256        | < 1ms   |
| 16          | ~65,536     | ~5ms    |
| 20          | ~1,048,576  | ~50ms   |
| 24          | ~16,777,216 | ~800ms  |

默认最低难度为 `8`（1 字节前导零），适合开发测试。生产环境建议 `16-20`。

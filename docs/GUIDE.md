# Actinium Cast — 运行与测试指南

本文档说明如何编译、运行和测试整个 Actinium Cast 项目（Phase 1 + Phase 2），覆盖从底层密码学库到网关 HTTP API 的完整链路。

---

## 目录

- [前置条件](#前置条件)
- [项目结构](#项目结构)
- [编译项目](#编译项目)
- [运行单元测试](#运行单元测试)
- [启动网关服务](#启动网关服务)
- [CLI 客户端使用](#cli-客户端使用)
  - [查看帮助](#查看帮助)
  - [查询网关状态](#查询网关状态)
  - [生成身份](#生成身份)
  - [发布帖子](#发布帖子)
  - [查询帖子列表](#查询帖子列表)
  - [查询单条帖子](#查询单条帖子)
  - [发布评论](#发布评论)
  - [发布投票](#发布投票)
  - [查询投票统计](#查询投票统计)
- [端对端冒烟测试](#端对端冒烟测试)
- [手动 API 测试（curl）](#手动-api-测试curl)
- [日志与调试](#日志与调试)
- [常见问题](#常见问题)

---

## 前置条件

| 工具      | 最低版本              | 说明                        |
| --------- | --------------------- | --------------------------- |
| **Rust**  | 1.85+ (edition 2024)  | `rustup update` 确保最新    |
| **Cargo** | 随 Rust 安装          | 构建系统                    |
| **curl**  | 任意版本（可选）      | 手动 API 测试               |

> [!NOTE]
> 项目使用 `rusqlite` 的 `bundled` feature，SQLite 会被自动编译，**无需**系统级安装 SQLite。

---

## 项目结构

```text
actinium-cast/
├── Cargo.toml                  # 工作区根配置
├── docs/
│   ├── ARCHITECTURE.md         # 架构设计文档
│   ├── GUIDE.md                # 本文档
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
    ├── client-cli/             # CLI 客户端（含冒烟测试）
    │   └── src/
    │       ├── main.rs         # 入口点 + clap 子命令
    │       ├── envelope.rs     # SignedEnvelope 构建器
    │       └── gateway.rs      # HTTP 客户端封装
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
cargo build -p client-cli
```

---

## 运行单元测试

```powershell
# 运行全部单元测试（21 个）
cargo test -p actinium-core -p gateway

# 仅运行核心密码学测试（14 个）
cargo test -p actinium-core

# 仅运行网关测试（7 个）
cargo test -p gateway

# 运行特定测试并查看详细输出
cargo test -p gateway -- --nocapture filter::tests::test_valid_envelope_passes
```

期望结果：

```text
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

```text
INFO gateway: 🚀 Actinium Cast Gateway 正在启动...
INFO gateway: 🔗 使用开发默认网络 ID network_id=4c8afc12...
INFO gateway: 📦 本地缓存已就绪
INFO gateway: 🌐 DHT 节点已启动（client 模式）   # 或 "⚠️ DHT 连接失败..."
INFO gateway: 🛡️ 过滤器配置完成 min_difficulty=8 max_drift=600
INFO gateway: 🌍 HTTP API 服务监听于 http://0.0.0.0:3000
```

### 网络隔离（Network ID）

每个独立部署的 Actinium Cast 网络通过一个 32 字节的 **Network ID** 进行隔离。不同网络的消息互不干扰。

- **默认行为**：未配置时，使用固定种子生成的开发网络 ID，网关和 CLI 自动匹配。
- **自定义网络**：通过环境变量设置：

```powershell
# 设置网关的网络 ID（64 个 hex 字符 = 32 字节）
$env:ACTINIUM_NETWORK_ID="your_64_hex_chars_here"
cargo run -p gateway
```

> [!IMPORTANT]
> 网关和客户端必须使用相同的 Network ID，否则网关会拒绝接受客户端的消息。

> [!TIP]
> 如果你的网络环境无法访问 BitTorrent DHT（如企业防火墙），网关会自动降级为离线模式。
> 所有本地 API（发布、查询）仍然可以正常工作，只是不会广播到全球 DHT 网络。

服务启动后，数据库文件 `gateway_cache.db` 会自动在项目根目录创建。

---

## CLI 客户端使用

CLI 客户端（`client-cli`）是与网关交互的主要工具，负责身份管理、PoW 计算、签名和 HTTP 通信。

### 全局参数

| 参数             | 默认值                    | 说明                   |
| ---------------- | ------------------------- | ---------------------- |
| `--gateway`      | `http://localhost:3000`   | 网关地址               |
| `--difficulty`   | `8`                       | PoW 难度 (bit 前导零)  |
| `--network-id`   | 开发默认值               | 网络标识符 (64 hex 字符) |

### 查看帮助

```powershell
cargo run -p client-cli -- --help
```

### 查询网关状态

```powershell
cargo run -p client-cli -- status
```

输出示例：

```text
📊 网关状态:
   帖子数:   3
   评论数:   1
   投票数:   2
   DHT连接:  true
```

### 生成身份

```powershell
cargo run -p client-cli -- identity generate
```

输出示例：

```text
🔑 新身份已生成:
   公钥 (public_key):  a1b2c3d4...（64 字符 hex）
   私钥 (secret_key):  e5f6a7b8...（64 字符 hex）

⚠️  请安全保存私钥！丢失后无法恢复。
```

### 发布帖子

```powershell
# 使用临时身份（自动生成）
cargo run -p client-cli -- post --title "Hello World" --content "我的第一篇帖子"

# 使用指定身份
cargo run -p client-cli -- post --title "Hello" --content "World" --secret-key <私钥hex>

# 指定更高的 PoW 难度
cargo run -p client-cli -- --difficulty 16 post --title "高难度帖子" --content "需要更多算力"

# 使用自定义网络 ID 发帖
cargo run -p client-cli -- --network-id <64_hex_chars> post --title "Hello" --content "World"
```

### 查询帖子列表

```powershell
# 默认查询（最新 20 条）
cargo run -p client-cli -- list-posts

# 分页查询
cargo run -p client-cli -- list-posts --limit 5 --offset 0
```

### 查询单条帖子

```powershell
cargo run -p client-cli -- get-post 1
```

### 发布评论

```powershell
cargo run -p client-cli -- comment --post-id <帖子作者公钥hex> --content "非常棒的帖子！"
```

### 发布投票

```powershell
# 点赞
cargo run -p client-cli -- vote --target-id <目标公钥hex> --positive true

# 取消点赞
cargo run -p client-cli -- vote --target-id <目标公钥hex> --positive false
```

### 查询投票统计

```powershell
cargo run -p client-cli -- get-votes <目标公钥hex>
```

---

## 端对端冒烟测试

`smoke-test` 子命令是集成测试的首选方式。它会自动执行完整的业务工作流并验证每一步结果。

### 运行方式

**在一个终端启动网关：**

```powershell
cargo run -p gateway
```

**在另一个终端运行冒烟测试：**

```powershell
cargo run -p client-cli -- smoke-test
```

> [!IMPORTANT]
> `client-cli` 是独立于 `gateway` 的二进制文件，不会触发对 `gateway.exe` 的重编译，
> 因此可以在网关运行时随时编译和执行，不存在 Windows 文件锁定问题。

### 测试流程

冒烟测试会自动执行以下 10 步，每步附带 ✅/❌ 结果：

```mermaid
graph LR
    A[检查网关在线] --> B[生成 Alice/Bob 身份]
    B --> C[Alice 发布帖子]
    C --> D[查询帖子列表]
    D --> E[查询单条帖子]
    E --> F[Bob 发布评论]
    F --> G[Bob 点赞]
    G --> H[查询投票统计]
    H --> I[验证非法数据被拒绝]
    I --> J[最终状态验证]
```

### 期望输出

```text
════════════════════════════════════════════════════════════
  Actinium Cast 端对端冒烟测试
════════════════════════════════════════════════════════════

📡 步骤 0: 检查网关连接...
   ✅ 网关在线

🔑 步骤 1: 生成身份...
   Alice: a1b2c3d4...e5f6a7b8
   Bob:   11223344...55667788

📝 步骤 2: Alice 发布帖子...
   ✅ 帖子发布成功
   帖子 ID: 1

📋 步骤 3: 查询帖子列表...
   ✅ 帖子列表包含刚发布的帖子

🔍 步骤 4: 查询单条帖子...
   ✅ 帖子详情正确

💬 步骤 5: Bob 发布评论...
   ✅ 评论发布成功

👍 步骤 6: Bob 点赞...
   ✅ 投票发布成功

📊 步骤 7: 查询投票统计...
   ✅ 投票统计正确 (likes≥1, unlikes=0)

🛡️ 步骤 8: 验证安全过滤...
   ✅ 无效 hex 被拒绝 (400)
   ✅ 无效 bencode 被拒绝 (400)
   ✅ 篡改数据被拒绝 (400/403)

📊 步骤 9: 最终状态...
   ✅ 数据库有数据 (posts≥1, comments≥1, votes≥1)

════════════════════════════════════════════════════════════
  ✅ 全部 12 项检查通过！
════════════════════════════════════════════════════════════
```

---

## 手动 API 测试（curl）

如果你更喜欢直接使用 curl，以下是各端点的示例。需要注意 POST 接口需要完整的密码学 envelope，建议使用 CLI 客户端代替。

```powershell
# 查询状态
curl http://localhost:3000/api/status

# 查询帖子列表
curl "http://localhost:3000/api/posts?limit=5&offset=0"

# 查询单条帖子
curl http://localhost:3000/api/posts/1

# 查询投票
curl http://localhost:3000/api/votes/<target_public_key_hex>

# 验证拒绝无效数据 (应返回 400)
curl -X POST http://localhost:3000/api/publish/post `
  -H "Content-Type: application/json" `
  -d '{"envelope_hex": "not_valid_hex"}'
```

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
| ----------- | ------------ | -------- |
| 8           | ~256         | < 1ms    |
| 16          | ~65,536      | ~5ms     |
| 20          | ~1,048,576   | ~50ms    |
| 24          | ~16,777,216  | ~800ms   |

默认最低难度为 `8`（1 字节前导零），适合开发测试。生产环境建议 `16-20`。

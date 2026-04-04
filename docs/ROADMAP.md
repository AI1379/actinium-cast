# Actinium Cast - 项目路线图 (Roadmap)

本项目旨在构建一个纯匿名、抗审查、利用 BitTorrent DHT 网络和轻量级区块链的内容分发平台。由于规模较大且模块间依赖清晰，整个开发周期建议分为 **5 个核心阶段**，从底层的核心库算法逐步向上构建。

---

## 📌 节点总览与状态

- [x] **Phase 1: 核心密码学与基础组件 (Core)**
- [x] **Phase 2: P2P 桥接与网关服务 (Gateway)**
- [ ] **Phase 3: 客户端与 WebAssembly 执行层 (Client & Wasm)**
- [ ] **Phase 4: 链上多签与全局控制层 (Blockchain)**
- [ ] **Phase 5: 综合前端与网关索引服务 (Frontend & Assembly)**
- [ ] **Phase 6: 家庭化部署与高级网络穿透 (NAT Traversal)**

---

## 🛠️ 各阶段详细设计 (TODO List)

### 🧱 Phase 1: 核心密码学与基础组件 (`src/core`)

本阶段构建项目的信任根基与抗垃圾手段，全部为纯 Rust API。

- [x] **1.1 身份管理 (Identity)**
  - [x] 实现 Ed25519 密钥对生成、导出与加载。
  - [x] 封装针对任意消息的私钥签名、公钥验证接口。
- [x] **1.2 客户端 Proof-of-Work (PoW)**
  - [x] 设计基于 SHA-256 的 Hashcash 算法，支持动态难度。
  - [x] 实现 CPU 满载 PoW 计算器。
  - [x] 实现 PoW 结果的高效验证（网关和客户端用）。
- [x] **1.3 数据结构化设计 (Data Models)**
  - [x] 依照 BEP 44 设计「帖子 (Post)」、「评论 (Comment)」、「交互消息 (Vote/Like/Unlike)」的序列化/反序列化（例如使用 `serde_bencode`）。
  - [x] 确保每条打包好的消息包含：`[内容, 时间戳, PoW解, 用户身份公钥, 签名]`。

---

### 🌐 Phase 2: P2P 桥接与网关服务 (`src/gateway`)

网关负责对接 DHT 生态，拉取、过滤、缓存并广播数据。

- [x] **2.1 DHT 网络适配与 BEP 44 寻址**
  - [x] 接入并测试可在 Rust 环境运行的 BitTorrent DHT 协议客户端。
  - [x] 实现 BEP 44 的 Mutable Data `get` 和 `put` 操作。
- [x] **2.2 数据中继及过滤 (Relay & Filter)**
  - [x] 建立网关的本地临时高速缓存（如 SQLite / Sled），用于存储常用帖子的摘要，减少频繁直接查询 DHT 的开销。
  - [x] **防垃圾垃圾过滤器**：从 DHT 发现新数据后，第一步校验 PoW 和签名签名，剔除无效数据。
- [x] **2.3 RESTful / WebSocket API 开发**
  - [x] 开放 HTTP 接口，允许外端查询：`获取列表 / 查看单帖 / 评论聚合`。
  - [x] 开放 POST 接口，由 Gateway 将外来的签名包推给全球 DHT。
- [x] **2.4 网关互联与冷启动恢复 (Mesh Syncing)**
  - [x] 通过 BEP 5 协议向 DHT 广播当前网关节点位置。
  - [x] 设立内部 HTTP 数据拉取端点 (`/api/sync/keys` 或 `/api/sync/messages`)。
  - [x] 启动后台进程跨网关同步未知的公钥与帖子，保证灾难恢复能力。

---

### 📦 Phase 3: 客户端与 WebAssembly 执行层

让浏览器或 TUI 拥有本地绝对的控制权，不接触任何私钥给服务器。

- [ ] **3.1 WASM 打包与绑定 (`src/client-wasm`)**
  - [ ] 使用 `wasm-bindgen` 导出 `core` 里的 Ed25519 计算与 PoW 计算接口。
  - [ ] 开发 Wasm 多线程或 Rust async 调度以保证浏览器计算 PoW 时不会卡死页面渲染。
- [ ] **3.2 命令行工具集成调试 (`src/client-cli`)**
  - [x] 为研发开发一个 CLI。不需要前端也能：`生成身份 -> 发帖度算PoW -> HTTP发给网关 -> 全局路由发贴`。
  - [ ] 实现 CLI 消息轮询订阅，看自己的贴子广播状态。

---

### ⛓️ Phase 4: 链上多签与全局层 (`contracts`)

不记业务，只管管理员特权，抗击由于系统极度匿名带来的“极端内容”。

- [ ] **4.1 智能合约主体编写 (Solidity / Substrate)**
  - [ ] 编写动态多签门槛验证（ threshold signature ）合约。
  - [ ] 管理员席位记录（支持：提议、多方签署、动态撤销/新增阈值管理员）。
- [ ] **4.2 屏蔽字典与 Tombstone (墓碑记录)**
  - [ ] 合约内置全局 TOMBSTONE 路由。即合集 N/M 多签同意后，链上抛出 `GlobalBan(PostHash)` 事件。
- [ ] **4.3 网关链上监听服务**
  - [ ] 在 `gateway` 节点中跑一个轻量的 RPC 监听，监控特定合约事件，自动下架本地和拒绝路由对应的 Hash。
- [ ] **4.4 归档事件备份 (Archival Index Log - 可选)**
  - [ ] 在合约中预留无状态的事件接口，例如：`Event NewPublicKeys(bytes32[])`。
  - [ ] 开发可选的“归档服务器插件”，允许官方节点花费少量 Gas，通过定时批量上链确立全网公钥的最强韧性备份。

---

### 🎨 Phase 5: 综合前端与网关索引服务 (`frontend`)

给正常用户极佳和极简的使用体验。

- [ ] **5.1 首屏加速逻辑**
  - [ ] 用户进入页面，Gateway 极速渲染目前缓存的 Top 帖子。
- [ ] **5.2 本地安全沙箱**
  - [ ] 前端利用 IndexDB 或者 localStorage 加密保存用户本次分配或保留的私钥，绝对不在 HTTP 流量中泄露。
- [ ] **5.3 前端视觉反馈**
  - [ ] 点击发送时，UI 出现 “正在计算 PoW 计算...” 的优雅进度反馈。
- [ ] **5.4 点赞与状态同步**
  - [ ] 类似 Nostr 协议的异步渲染：帖子加载完后，异步拉取并渲染和该 Post 挂钩的点赞（Vote）统计。

---

### 🌐 Phase 6: 家庭化部署与高级网络穿透 (NAT Traversal)

赋予网关超越传统 HTTP Server 的强劲网络连通潜力，允许被部署在严格防火墙或家庭大内网之下。

- [ ] **6.1 家用路由器自动穿透 (Auto Port Forwarding)**
  - [ ] 引入 `igd` / UPnP 和 NAT-PMP 协议。
  - [ ] 当网关在本机启动时，主动联系路由器映射网关端口，让光猫自动接受外部 TCP 敲门。
- [ ] **6.2 引入打洞与局域网发现 (Hole Punching & LPD)**
  - [ ] 加入 BEP 14 (Local Peer Discovery) 广播，确保同一局域网下的多台设备快速组建 Mesh 互连。
  - [ ] 在双边 NAT 的终极降级情况下，设计或引入 UDP（类似 uTP）打洞机制来确保 Mesh Sync HTTP 请求能在云端与大内网之间双向触达。


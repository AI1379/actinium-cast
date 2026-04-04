//! # Actinium Cast CLI 客户端
//!
//! 提供与网关交互的命令行工具，用于：
//! - 身份管理（生成密钥对）
//! - 发布帖子、评论、投票
//! - 查询帖子列表、详情、投票统计
//! - 端对端集成测试（`smoke-test` 子命令）
//!
//! ## 使用方式
//!
//! ```bash
//! # 查看帮助
//! cargo run -p client-cli -- --help
//!
//! # 查询网关状态
//! cargo run -p client-cli -- status
//!
//! # 生成身份
//! cargo run -p client-cli -- identity generate
//!
//! # 发布帖子
//! cargo run -p client-cli -- post --title "Hello" --content "World"
//!
//! # 使用自定义网络 ID 发帖
//! cargo run -p client-cli -- --network-id <hex> post --title "Hello" --content "World"
//!
//! # 端对端冒烟测试
//! cargo run -p client-cli -- smoke-test
//! ```

mod envelope;
mod gateway;

use clap::{Parser, Subcommand};

use crate::envelope::EnvelopeBuilder;
use crate::gateway::GatewayClient;
use actinium_core::{Identity, NETWORK_ID_LEN};

// ─── CLI 定义 ────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "actinium-cli",
    about = "Actinium Cast 命令行客户端",
    version,
    author
)]
struct Cli {
    /// 网关地址（默认 http://localhost:3000）
    #[arg(long, default_value = "http://localhost:3000", global = true)]
    gateway: String,

    /// PoW 难度（bit 前导零数量，默认 8）
    #[arg(long, default_value_t = 8, global = true)]
    difficulty: u8,

    /// 网络标识符（64 个 hex 字符 = 32 字节）。
    /// 未指定时使用默认的开发网络 ID。
    #[arg(long, global = true)]
    network_id: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 查询网关状态
    Status,

    /// 身份管理
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },

    /// 发布帖子（自动生成临时身份）
    Post {
        /// 帖子标题
        #[arg(long)]
        title: String,

        /// 帖子内容
        #[arg(long)]
        content: String,

        /// 私钥 hex（可选，不提供则自动生成临时身份）
        #[arg(long)]
        secret_key: Option<String>,
    },

    /// 发布评论
    Comment {
        /// 目标帖子作者的公钥 hex
        #[arg(long)]
        post_id: String,

        /// 评论内容
        #[arg(long)]
        content: String,

        /// 私钥 hex（可选）
        #[arg(long)]
        secret_key: Option<String>,
    },

    /// 发布投票
    Vote {
        /// 目标对象 ID（公钥 hex）
        #[arg(long)]
        target_id: String,

        /// 是否点赞（true / false）
        #[arg(long, default_value_t = true)]
        positive: bool,

        /// 私钥 hex（可选）
        #[arg(long)]
        secret_key: Option<String>,
    },

    /// 查询帖子列表
    ListPosts {
        /// 分页大小
        #[arg(long, default_value_t = 20)]
        limit: i64,

        /// 偏移量
        #[arg(long, default_value_t = 0)]
        offset: i64,
    },

    /// 查询单条帖子
    GetPost {
        /// 帖子 ID
        id: i64,
    },

    /// 查询投票统计
    GetVotes {
        /// 目标对象 ID
        target_id: String,
    },

    /// 端对端冒烟测试（自动执行完整工作流）
    SmokeTest,
}

#[derive(Subcommand)]
enum IdentityAction {
    /// 生成新的 Ed25519 密钥对
    Generate,
}

// ─── 网络 ID 解析 ────────────────────────────────────────────────────────────

/// 解析 network_id：优先使用 CLI 参数，其次环境变量，最后使用默认开发网络 ID。
fn resolve_network_id(cli_value: Option<&str>) -> [u8; NETWORK_ID_LEN] {
    // 1. CLI 参数
    if let Some(hex_str) = cli_value {
        return parse_network_id_hex(hex_str);
    }

    // 2. 环境变量
    if let Ok(hex_str) = std::env::var("ACTINIUM_NETWORK_ID") {
        return parse_network_id_hex(hex_str.trim());
    }

    // 3. 默认开发网络 ID（与 gateway 使用相同的种子）
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(b"actinium-cast-dev-network-v1");
    let mut arr = [0u8; NETWORK_ID_LEN];
    arr.copy_from_slice(&hash);
    arr
}

fn parse_network_id_hex(hex_str: &str) -> [u8; NETWORK_ID_LEN] {
    let bytes = hex::decode(hex_str).unwrap_or_else(|e| {
        eprintln!("❌ network_id hex 解码失败: {e}");
        eprintln!("   期望 {} 个 hex 字符 (= {} 字节)", NETWORK_ID_LEN * 2, NETWORK_ID_LEN);
        std::process::exit(1);
    });
    if bytes.len() != NETWORK_ID_LEN {
        eprintln!(
            "❌ network_id 长度错误: 期望 {} 字节, 实际 {} 字节",
            NETWORK_ID_LEN,
            bytes.len()
        );
        std::process::exit(1);
    }
    let mut arr = [0u8; NETWORK_ID_LEN];
    arr.copy_from_slice(&bytes);
    arr
}

// ─── 入口 ────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let gw = GatewayClient::new(&cli.gateway);
    let difficulty = cli.difficulty;
    let network_id = resolve_network_id(cli.network_id.as_deref());

    match cli.command {
        Command::Status => cmd_status(&gw),
        Command::Identity { action } => match action {
            IdentityAction::Generate => cmd_identity_generate(),
        },
        Command::Post {
            title,
            content,
            secret_key,
        } => cmd_post(&gw, difficulty, &title, &content, secret_key.as_deref(), &network_id),
        Command::Comment {
            post_id,
            content,
            secret_key,
        } => cmd_comment(&gw, difficulty, &post_id, &content, secret_key.as_deref(), &network_id),
        Command::Vote {
            target_id,
            positive,
            secret_key,
        } => cmd_vote(&gw, difficulty, &target_id, positive, secret_key.as_deref(), &network_id),
        Command::ListPosts { limit, offset } => cmd_list_posts(&gw, limit, offset),
        Command::GetPost { id } => cmd_get_post(&gw, id),
        Command::GetVotes { target_id } => cmd_get_votes(&gw, &target_id),
        Command::SmokeTest => cmd_smoke_test(&gw, difficulty, &network_id),
    }
}

// ─── 命令实现 ────────────────────────────────────────────────────────────────

fn cmd_status(gw: &GatewayClient) {
    match gw.get("/api/status") {
        Ok((_, body)) => {
            let data = &body["data"];
            println!("📊 网关状态:");
            println!("   帖子数:   {}", data["total_posts"]);
            println!("   评论数:   {}", data["total_comments"]);
            println!("   投票数:   {}", data["total_votes"]);
            println!("   DHT连接:  {}", data["dht_connected"]);
        }
        Err(e) => eprintln!("❌ 无法连接网关: {e}"),
    }
}

fn cmd_identity_generate() {
    let id = Identity::generate();
    let pk_hex = id.public_key_hex();
    let sk_hex = id.to_hex();
    println!("🔑 新身份已生成:");
    println!("   公钥 (public_key):  {pk_hex}");
    println!("   私钥 (secret_key):  {sk_hex}");
    println!();
    println!("⚠️  请安全保存私钥！丢失后无法恢复。");
    println!("   使用 --secret-key 参数传入私钥来以此身份发布内容。");
}

fn resolve_identity(secret_key: Option<&str>) -> Identity {
    match secret_key {
        Some(sk) => {
            Identity::from_hex(sk).unwrap_or_else(|e| {
                eprintln!("❌ 私钥解析失败: {e}");
                std::process::exit(1);
            })
        }
        None => {
            let id = Identity::generate();
            println!("ℹ️  未指定私钥，已自动生成临时身份");
            println!("   公钥: {}", id.public_key_hex());
            id
        }
    }
}

fn cmd_post(gw: &GatewayClient, difficulty: u8, title: &str, content: &str, secret_key: Option<&str>, network_id: &[u8; NETWORK_ID_LEN]) {
    let identity = resolve_identity(secret_key);
    println!("⛏️  正在计算 PoW (difficulty={difficulty})...");
    let result = EnvelopeBuilder::build_post(&identity, title, content, difficulty, network_id);
    println!("📤 正在发布帖子...");
    match gw.publish("/api/publish/post", &result.envelope_hex, Some(&result.bep44_sig_hex)) {
        Ok((status, body)) => {
            if body["ok"] == true {
                println!("✅ 帖子发布成功! id={}", body["data"]["id"]);
            } else {
                eprintln!("❌ 发布失败 (HTTP {status}): {}", body["error"]);
            }
        }
        Err(e) => eprintln!("❌ 请求失败: {e}"),
    }
}

fn cmd_comment(gw: &GatewayClient, difficulty: u8, post_id: &str, content: &str, secret_key: Option<&str>, network_id: &[u8; NETWORK_ID_LEN]) {
    let identity = resolve_identity(secret_key);
    println!("⛏️  正在计算 PoW (difficulty={difficulty})...");
    let result = EnvelopeBuilder::build_comment(&identity, post_id, content, difficulty, network_id);
    println!("📤 正在发布评论...");
    match gw.publish("/api/publish/comment", &result.envelope_hex, Some(&result.bep44_sig_hex)) {
        Ok((status, body)) => {
            if body["ok"] == true {
                println!("✅ 评论发布成功! id={}", body["data"]["id"]);
            } else {
                eprintln!("❌ 发布失败 (HTTP {status}): {}", body["error"]);
            }
        }
        Err(e) => eprintln!("❌ 请求失败: {e}"),
    }
}

fn cmd_vote(gw: &GatewayClient, difficulty: u8, target_id: &str, positive: bool, secret_key: Option<&str>, network_id: &[u8; NETWORK_ID_LEN]) {
    let identity = resolve_identity(secret_key);
    let label = if positive { "👍 点赞" } else { "👎 取消点赞" };
    println!("⛏️  正在计算 PoW (difficulty={difficulty})...");
    let result = EnvelopeBuilder::build_vote(&identity, target_id, positive, difficulty, network_id);
    println!("📤 正在发布{label}...");
    match gw.publish("/api/publish/vote", &result.envelope_hex, Some(&result.bep44_sig_hex)) {
        Ok((status, body)) => {
            if body["ok"] == true {
                println!("✅ {label}发布成功! id={}", body["data"]["id"]);
            } else {
                eprintln!("❌ 发布失败 (HTTP {status}): {}", body["error"]);
            }
        }
        Err(e) => eprintln!("❌ 请求失败: {e}"),
    }
}

fn cmd_list_posts(gw: &GatewayClient, limit: i64, offset: i64) {
    match gw.get(&format!("/api/posts?limit={limit}&offset={offset}")) {
        Ok((_, body)) => {
            let posts = body["data"].as_array();
            match posts {
                Some(arr) if !arr.is_empty() => {
                    println!("📋 帖子列表 (共 {} 条):\n", arr.len());
                    for p in arr {
                        println!("  ┌─ #{} | {} | difficulty={}",
                            p["id"], p["created_at"], p["difficulty"]);
                        println!("  │  作者: {}", p["public_key"]);
                        println!("  │  标题: {}", p["title"].as_str().unwrap_or(""));
                        println!("  │  内容: {}", p["content"].as_str().unwrap_or(""));
                        println!("  └──────────────────────────────────────");
                    }
                }
                _ => println!("📭 暂无帖子"),
            }
        }
        Err(e) => eprintln!("❌ 查询失败: {e}"),
    }
}

fn cmd_get_post(gw: &GatewayClient, id: i64) {
    match gw.get(&format!("/api/posts/{id}")) {
        Ok((status, body)) => {
            if body["ok"] == true {
                let p = &body["data"];
                println!("🔍 帖子详情:");
                println!("   ID:       {}", p["id"]);
                println!("   标题:     {}", p["title"]);
                println!("   内容:     {}", p["content"]);
                println!("   作者:     {}", p["public_key"]);
                println!("   难度:     {}", p["difficulty"]);
                println!("   时间戳:   {}", p["timestamp"]);
                println!("   入库时间: {}", p["created_at"]);
            } else {
                eprintln!("❌ 帖子不存在 (HTTP {status})");
            }
        }
        Err(e) => eprintln!("❌ 查询失败: {e}"),
    }
}

fn cmd_get_votes(gw: &GatewayClient, target_id: &str) {
    match gw.get(&format!("/api/votes/{target_id}")) {
        Ok((_, body)) => {
            let data = &body["data"];
            println!("📊 投票统计:");
            println!("   目标:   {}", data["target_id"]);
            println!("   👍 赞:  {}", data["likes"]);
            println!("   👎 踩:  {}", data["unlikes"]);
        }
        Err(e) => eprintln!("❌ 查询失败: {e}"),
    }
}

// ─── 冒烟测试 ────────────────────────────────────────────────────────────────

fn cmd_smoke_test(gw: &GatewayClient, difficulty: u8, network_id: &[u8; NETWORK_ID_LEN]) {
    println!("\n{}", "═".repeat(60));
    println!("  Actinium Cast 端对端冒烟测试");
    println!("{}\n", "═".repeat(60));

    println!("🔗 网络 ID: {}", hex::encode(network_id));

    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! check {
        ($label:expr, $cond:expr) => {
            if $cond {
                println!("   ✅ {}", $label);
                passed += 1;
            } else {
                println!("   ❌ {} — 检查未通过", $label);
                failed += 1;
            }
        };
    }

    // ── 0. 网关在线 ──
    println!("📡 步骤 0: 检查网关连接...");
    let (status, body) = match gw.get("/api/status") {
        Ok(r) => r,
        Err(e) => {
            eprintln!("❌ 无法连接网关: {e}");
            eprintln!("   请先在另一个终端运行: cargo run -p gateway");
            std::process::exit(1);
        }
    };
    check!("网关在线", status == 200 && body["ok"] == true);
    println!("   DHT 连接: {}", body["data"]["dht_connected"]);

    // ── 1. 生成身份 ──
    println!("\n🔑 步骤 1: 生成身份...");
    let alice = Identity::generate();
    let alice_pk = alice.public_key_hex();
    println!("   Alice: {}...{}", &alice_pk[..8], &alice_pk[56..]);

    let bob = Identity::generate();
    let bob_pk = bob.public_key_hex();
    println!("   Bob:   {}...{}", &bob_pk[..8], &bob_pk[56..]);
    passed += 1; // 身份生成总是成功

    // ── 2. Alice 发布帖子 ──
    println!("\n📝 步骤 2: Alice 发布帖子...");
    let post_result = EnvelopeBuilder::build_post(
        &alice,
        "冒烟测试帖子",
        "这是 Actinium Cast 的端对端冒烟测试 🎉",
        difficulty,
        network_id,
    );
    let (status, body) = gw.publish("/api/publish/post", &post_result.envelope_hex, Some(&post_result.bep44_sig_hex))
        .expect("发布帖子请求失败");
    check!("帖子发布成功", status == 200 && body["ok"] == true);
    let post_id = body["data"]["id"].as_i64().unwrap_or(-1);
    println!("   帖子 ID: {post_id}");

    // ── 3. 查询帖子列表 ──
    println!("\n📋 步骤 3: 查询帖子列表...");
    let (status, body) = gw.get("/api/posts").expect("查询帖子失败");
    let posts = body["data"].as_array();
    let has_our_post = posts.map_or(false, |arr| {
        arr.iter().any(|p| p["title"] == "冒烟测试帖子" && p["public_key"] == alice_pk)
    });
    check!("帖子列表包含刚发布的帖子", status == 200 && has_our_post);

    // ── 4. 查询单条帖子 ──
    println!("\n🔍 步骤 4: 查询单条帖子 (id={post_id})...");
    let (status, body) = gw.get(&format!("/api/posts/{post_id}"))
        .expect("查询帖子详情失败");
    check!(
        "帖子详情正确",
        status == 200
            && body["data"]["id"] == post_id
            && body["data"]["content"] == "这是 Actinium Cast 的端对端冒烟测试 🎉"
    );

    // ── 5. Bob 发布评论 ──
    println!("\n💬 步骤 5: Bob 发布评论...");
    let comment_result = EnvelopeBuilder::build_comment(
        &bob, &alice_pk, "非常棒的帖子！", difficulty, network_id,
    );
    let (status, body) = gw.publish("/api/publish/comment", &comment_result.envelope_hex, Some(&comment_result.bep44_sig_hex))
        .expect("发布评论失败");
    check!("评论发布成功", status == 200 && body["ok"] == true);

    // ── 6. Bob 对 Alice 的帖子点赞 ──
    println!("\n👍 步骤 6: Bob 点赞...");
    let vote_result = EnvelopeBuilder::build_vote(&bob, &alice_pk, true, difficulty, network_id);
    let (status, body) = gw.publish("/api/publish/vote", &vote_result.envelope_hex, Some(&vote_result.bep44_sig_hex))
        .expect("发布投票失败");
    check!("投票发布成功", status == 200 && body["ok"] == true);

    // ── 7. 查询投票统计 ──
    println!("\n📊 步骤 7: 查询投票统计...");
    let (status, body) = gw.get(&format!("/api/votes/{alice_pk}"))
        .expect("查询投票失败");
    check!(
        "投票统计正确 (likes≥1, unlikes=0)",
        status == 200
            && body["data"]["likes"].as_i64().unwrap_or(0) >= 1
            && body["data"]["unlikes"].as_i64().unwrap_or(-1) == 0
    );

    // ── 8. 安全过滤验证 ──
    println!("\n🛡️ 步骤 8: 验证安全过滤...");

    // 8a. 无效 hex
    let (status, body) = gw.publish("/api/publish/post", "not_valid_hex!!!", None)
        .expect("请求应成功发送");
    check!("无效 hex 被拒绝 (400)", status == 400 && body["ok"] == false);

    // 8b. 合法 hex 但无效 bencode
    let (status, body) = gw.publish("/api/publish/post", "aabbccdd", None)
        .expect("请求应成功发送");
    check!("无效 bencode 被拒绝 (400)", status == 400 && body["ok"] == false);

    // 8c. 篡改数据
    let tampered_result = EnvelopeBuilder::build_post(
        &alice, "篡改测试", "这条消息会被篡改", difficulty, network_id,
    );
    let mut tampered_hex = tampered_result.envelope_hex;
    let len = tampered_hex.len();
    let last = u8::from_str_radix(&tampered_hex[len - 2..], 16).unwrap_or(0);
    tampered_hex.replace_range(len - 2.., &format!("{:02x}", last ^ 0xFF));
    let (status, body) = gw.publish("/api/publish/post", &tampered_hex, None)
        .expect("请求应成功发送");
    check!(
        "篡改数据被拒绝 (400/403)",
        (status == 400 || status == 403) && body["ok"] == false
    );

    // ── 9. 最终状态 ──
    println!("\n📊 步骤 9: 最终状态...");
    let (_, body) = gw.get("/api/status").expect("状态查询失败");
    let data = &body["data"];
    check!(
        "数据库有数据 (posts≥1, comments≥1, votes≥1)",
        data["total_posts"].as_i64().unwrap_or(0) >= 1
            && data["total_comments"].as_i64().unwrap_or(0) >= 1
            && data["total_votes"].as_i64().unwrap_or(0) >= 1
    );

    // ── 汇总 ──
    println!("\n{}", "═".repeat(60));
    if failed == 0 {
        println!("  ✅ 全部 {passed} 项检查通过！");
    } else {
        println!("  ⚠️  通过 {passed} 项, 失败 {failed} 项");
    }
    println!("{}\n", "═".repeat(60));

    if failed > 0 {
        std::process::exit(1);
    }
}

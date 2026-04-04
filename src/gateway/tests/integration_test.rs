//! # 端对端集成测试
//!
//! 在**网关已运行**（`cargo run -p gateway`，监听 `localhost:3000`）的前提下，
//! 本测试通过 HTTP 调用完整验证从身份生成到帖子发布再到查询的全链路。
//!
//! ## 运行方式
//!
//! ```bash
//! # 1. 终端 A：启动网关
//! cargo run -p gateway
//!
//! # 2. 终端 B：运行集成测试
//! cargo test -p gateway --test integration_test -- --nocapture
//! ```

use actinium_core::{Comment, Identity, Post, PowChallenge, SignedEnvelope, Vote};

// ─── 辅助：构造合法 SignedEnvelope ──────────────────────────────────────────

/// 构造一个完整的、密码学合法的 SignedEnvelope<Post>，
/// 返回 (bencode_hex, public_key_hex)。
fn build_post_envelope(
    identity: &Identity,
    title: &str,
    content: &str,
    difficulty: u8,
) -> (String, String) {
    let timestamp = chrono::Utc::now().timestamp();
    let post = Post {
        title: title.to_string(),
        content: content.to_string(),
        difficulty,
    };

    // PoW prefix = public_key(32) || timestamp_le(8)
    let mut pow_prefix = Vec::with_capacity(40);
    pow_prefix.extend_from_slice(&identity.public_key_bytes());
    pow_prefix.extend_from_slice(&timestamp.to_le_bytes());

    let challenge = PowChallenge::new(pow_prefix, difficulty);
    let solution = challenge.solve().expect("PoW 应当能在合理时间内求解");

    // 签名
    let signing_bytes = SignedEnvelope::<Post>::signing_bytes(
        &post,
        timestamp,
        solution.nonce,
        &identity.public_key_bytes(),
    )
    .expect("signing_bytes 不应失败");
    let sig = identity.sign(&signing_bytes);

    let envelope = SignedEnvelope::new(
        post,
        timestamp,
        solution.nonce,
        solution.hash,
        identity.public_key_bytes(),
        sig.to_bytes(),
    );

    let bencode = envelope.to_bencode().expect("bencode 序列化不应失败");
    let hex_str = hex::encode(&bencode);
    let pk_hex = identity.public_key_hex();
    (hex_str, pk_hex)
}

/// 构造一个合法的 SignedEnvelope<Comment>。
fn build_comment_envelope(
    identity: &Identity,
    post_id: &str,
    content: &str,
    difficulty: u8,
) -> String {
    let timestamp = chrono::Utc::now().timestamp();
    let comment = Comment {
        post_id: post_id.to_string(),
        content: content.to_string(),
    };

    let mut pow_prefix = Vec::with_capacity(40);
    pow_prefix.extend_from_slice(&identity.public_key_bytes());
    pow_prefix.extend_from_slice(&timestamp.to_le_bytes());

    let challenge = PowChallenge::new(pow_prefix, difficulty);
    let solution = challenge.solve().expect("PoW solve");

    let signing_bytes = SignedEnvelope::<Comment>::signing_bytes(
        &comment,
        timestamp,
        solution.nonce,
        &identity.public_key_bytes(),
    )
    .unwrap();
    let sig = identity.sign(&signing_bytes);

    let envelope = SignedEnvelope::new(
        comment,
        timestamp,
        solution.nonce,
        solution.hash,
        identity.public_key_bytes(),
        sig.to_bytes(),
    );

    hex::encode(envelope.to_bencode().unwrap())
}

/// 构造一个合法的 SignedEnvelope<Vote>。
fn build_vote_envelope(
    identity: &Identity,
    target_id: &str,
    positive: bool,
    difficulty: u8,
) -> String {
    let timestamp = chrono::Utc::now().timestamp();
    let vote = Vote {
        target_id: target_id.to_string(),
        positive: if positive { 1u8 } else { 0u8 },
    };

    let mut pow_prefix = Vec::with_capacity(40);
    pow_prefix.extend_from_slice(&identity.public_key_bytes());
    pow_prefix.extend_from_slice(&timestamp.to_le_bytes());

    let challenge = PowChallenge::new(pow_prefix, difficulty);
    let solution = challenge.solve().expect("PoW solve");

    let signing_bytes = SignedEnvelope::<Vote>::signing_bytes(
        &vote,
        timestamp,
        solution.nonce,
        &identity.public_key_bytes(),
    )
    .unwrap();
    let sig = identity.sign(&signing_bytes);

    let envelope = SignedEnvelope::new(
        vote,
        timestamp,
        solution.nonce,
        solution.hash,
        identity.public_key_bytes(),
        sig.to_bytes(),
    );

    hex::encode(envelope.to_bencode().unwrap())
}

// ─── HTTP 辅助 ──────────────────────────────────────────────────────────────

const BASE_URL: &str = "http://localhost:3000";

fn http_get(path: &str) -> Result<(u16, serde_json::Value), String> {
    let url = format!("{BASE_URL}{path}");
    let resp = reqwest::blocking::get(&url)
        .map_err(|e| format!("GET {url} 连接失败 (网关是否已启动?): {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| format!("读取响应体失败: {e}"))?;
    let body: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败: {e}, 原始响应: {text}"))?;
    Ok((status, body))
}

fn http_post(path: &str, envelope_hex: &str) -> Result<(u16, serde_json::Value), String> {
    let url = format!("{BASE_URL}{path}");
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "envelope_hex": envelope_hex }))
        .send()
        .map_err(|e| format!("POST {url} 连接失败 (网关是否已启动?): {e}"))?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| format!("读取响应体失败: {e}"))?;
    let body: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("JSON 解析失败: {e}, 原始响应: {text}"))?;
    Ok((status, body))
}

// ─── 集成测试 ────────────────────────────────────────────────────────────────

#[test]
fn test_full_workflow() {
    println!("\n{}", "=".repeat(60));
    println!("  Actinium Cast 端对端集成测试");
    println!("{}\n", "=".repeat(60));

    // ── 0. 检查网关是否在线 ──
    println!("📡 步骤 0: 检查网关连接...");
    let (status, body) =
        http_get("/api/status").expect("无法连接网关！请确保已运行 `cargo run -p gateway`");
    assert_eq!(status, 200, "网关状态端点应返回 200");
    assert_eq!(body["ok"], true);
    println!(
        "   ✅ 网关在线: dht_connected={}",
        body["data"]["dht_connected"]
    );

    // ── 1. 生成身份 ──
    println!("\n🔑 步骤 1: 生成 Ed25519 身份...");
    let alice = Identity::generate();
    let alice_pk_hex = alice.public_key_hex();
    println!(
        "   ✅ Alice 公钥: {}...{}",
        &alice_pk_hex[..8],
        &alice_pk_hex[56..]
    );

    let bob = Identity::generate();
    let bob_pk_hex = bob.public_key_hex();
    println!(
        "   ✅ Bob   公钥: {}...{}",
        &bob_pk_hex[..8],
        &bob_pk_hex[56..]
    );

    // ── 2. Alice 发布帖子 ──
    println!("\n📝 步骤 2: Alice 发布帖子...");
    let (post_hex, _) = build_post_envelope(
        &alice,
        "我的第一篇帖子",
        "这是 Actinium Cast 的集成测试帖子 🎉",
        8,
    );
    println!("   envelope_hex 长度: {} 字符", post_hex.len());

    let (status, body) = http_post("/api/publish/post", &post_hex).expect("发布帖子请求失败");
    assert_eq!(status, 200, "发布帖子应返回 200");
    assert_eq!(body["ok"], true, "发布应成功");
    let post_id = body["data"]["id"].as_i64().expect("应返回帖子 ID");
    println!("   ✅ 帖子发布成功, id={post_id}");

    // ── 3. 查询帖子列表 ──
    println!("\n📋 步骤 3: 查询帖子列表...");
    let (status, body) = http_get("/api/posts").expect("查询帖子失败");
    assert_eq!(status, 200);
    let posts = body["data"].as_array().expect("data 应为数组");
    assert!(!posts.is_empty(), "帖子列表不应为空");
    println!("   ✅ 获取到 {} 条帖子", posts.len());

    // 验证最新帖子的内容
    let latest = &posts[0];
    assert_eq!(latest["title"], "我的第一篇帖子");
    assert_eq!(latest["public_key"], alice_pk_hex);
    println!("   ✅ 最新帖子标题: {}", latest["title"]);

    // ── 4. 查询单条帖子 ──
    println!("\n🔍 步骤 4: 查询单条帖子 (id={post_id})...");
    let (status, body) = http_get(&format!("/api/posts/{post_id}")).expect("查询帖子详情失败");
    assert_eq!(status, 200);
    assert_eq!(body["data"]["id"], post_id);
    assert_eq!(
        body["data"]["content"],
        "这是 Actinium Cast 的集成测试帖子 🎉"
    );
    println!("   ✅ 帖子内容验证通过");

    // ── 5. Bob 发布评论 ──
    println!("\n💬 步骤 5: Bob 发布评论...");
    let comment_hex = build_comment_envelope(&bob, &alice_pk_hex, "非常棒的帖子！", 8);
    let (status, body) = http_post("/api/publish/comment", &comment_hex).expect("发布评论失败");
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    let comment_id = body["data"]["id"].as_i64().unwrap();
    println!("   ✅ 评论发布成功, id={comment_id}");

    // ── 6. Bob 对 Alice 的帖子点赞 ──
    println!("\n👍 步骤 6: Bob 对 Alice 的帖子点赞...");
    let vote_hex = build_vote_envelope(&bob, &alice_pk_hex, true, 8);
    let (status, body) = http_post("/api/publish/vote", &vote_hex).expect("发布投票失败");
    assert_eq!(status, 200);
    assert_eq!(body["ok"], true);
    println!("   ✅ 点赞发布成功");

    // ── 7. 查询投票统计 ──
    println!("\n📊 步骤 7: 查询投票统计...");
    let (status, body) = http_get(&format!("/api/votes/{alice_pk_hex}")).expect("查询投票失败");
    assert_eq!(status, 200);
    assert_eq!(body["data"]["likes"], 1, "应有 1 个点赞");
    assert_eq!(body["data"]["unlikes"], 0, "不应有取消点赞");
    println!(
        "   ✅ 投票统计: likes={}, unlikes={}",
        body["data"]["likes"], body["data"]["unlikes"]
    );

    // ── 8. 验证网关拒绝非法数据 ──
    println!("\n🛡️ 步骤 8: 验证安全过滤...");

    // 8a. 无效 hex
    let (status, body) =
        http_post("/api/publish/post", "not_valid_hex!!!").expect("请求应成功发送");
    assert_eq!(status, 400, "无效 hex 应返回 400");
    assert_eq!(body["ok"], false);
    println!(
        "   ✅ 无效 hex 被拒绝 (400): {}",
        body["error"].as_str().unwrap_or("")
    );

    // 8b. 合法 hex 但无效 bencode
    let (status, body) = http_post("/api/publish/post", "aabbccdd").expect("请求应成功发送");
    assert_eq!(status, 400, "无效 bencode 应返回 400");
    assert_eq!(body["ok"], false);
    println!(
        "   ✅ 无效 bencode 被拒绝 (400): {}",
        body["error"].as_str().unwrap_or("")
    );

    // 8c. 篡改签名
    let (mut tampered_hex, _) =
        build_post_envelope(&alice, "篡改测试", "这条消息的签名会被篡改", 8);
    // 将 hex 字符串最后 2 个字符翻转（篡改 bencode 尾部）
    let len = tampered_hex.len();
    let last_byte = u8::from_str_radix(&tampered_hex[len - 2..], 16).unwrap_or(0);
    let flipped = last_byte ^ 0xFF;
    tampered_hex.replace_range(len - 2.., &format!("{:02x}", flipped));

    let (status, body) = http_post("/api/publish/post", &tampered_hex).expect("请求应成功发送");
    // 篡改后可能触发 400（bencode 解析失败）或 403（验证失败）
    assert!(
        status == 400 || status == 403,
        "篡改数据应被拒绝, 实际状态码: {status}"
    );
    assert_eq!(body["ok"], false);
    println!(
        "   ✅ 篡改数据被拒绝 ({status}): {}",
        body["error"].as_str().unwrap_or("")
    );

    // ── 9. 最终状态验证 ──
    println!("\n📊 步骤 9: 最终状态验证...");
    let (_, body) = http_get("/api/status").expect("状态查询失败");
    let data = &body["data"];
    println!("   总帖子数: {}", data["total_posts"]);
    println!("   总评论数: {}", data["total_comments"]);
    println!("   总投票数: {}", data["total_votes"]);
    assert!(
        data["total_posts"].as_i64().unwrap() >= 1,
        "至少应有 1 个帖子"
    );
    assert!(
        data["total_comments"].as_i64().unwrap() >= 1,
        "至少应有 1 条评论"
    );
    assert!(
        data["total_votes"].as_i64().unwrap() >= 1,
        "至少应有 1 个投票"
    );

    println!("\n{}", "=".repeat(60));
    println!("  ✅ 全部集成测试通过！");
    println!("{}\n", "=".repeat(60));
}

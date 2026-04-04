//! # Gateway HTTP 客户端
//!
//! 封装与网关 REST API 交互的 HTTP 请求逻辑。

use serde_json::Value;

/// 与网关交互的 HTTP 客户端。
pub struct GatewayClient {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl GatewayClient {
    /// 创建客户端，base_url 如 "http://localhost:3000"。
    pub fn new(base_url: &str) -> Self {
        GatewayClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::blocking::Client::new(),
        }
    }

    /// 发送 GET 请求，返回 (HTTP状态码, JSON Body)。
    pub fn get(&self, path: &str) -> Result<(u16, Value), String> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("GET {url} 连接失败 (网关是否已启动?): {e}"))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .map_err(|e| format!("读取响应体失败: {e}"))?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|e| format!("JSON 解析失败: {e}, 原始响应: {text}"))?;
        Ok((status, body))
    }

    /// 发送 POST /api/publish/* 请求，附带 BEP 44 签名。
    pub fn publish(
        &self,
        path: &str,
        envelope_hex: &str,
        bep44_sig_hex: Option<&str>,
    ) -> Result<(u16, Value), String> {
        let url = format!("{}{}", self.base_url, path);
        let mut body = serde_json::json!({ "envelope_hex": envelope_hex });
        if let Some(sig) = bep44_sig_hex {
            body["bep44_sig_hex"] = serde_json::Value::String(sig.to_string());
        }
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| format!("POST {url} 连接失败 (网关是否已启动?): {e}"))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .map_err(|e| format!("读取响应体失败: {e}"))?;
        let body: Value = serde_json::from_str(&text)
            .map_err(|e| format!("JSON 解析失败: {e}, 原始响应: {text}"))?;
        Ok((status, body))
    }
}


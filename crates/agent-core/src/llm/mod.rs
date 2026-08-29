//! OpenAI 兼容接口的流式客户端。

pub mod accumulator;
pub mod types;

use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};

use crate::config::LlmConfig;
use crate::error::{Error, Result};
use types::{ApiErrorEnvelope, ChatChunk, ChatRequest, Message, StreamOptions, ToolDef};

/// 从流中解析出的一个事件。
#[derive(Debug)]
pub enum StreamItem {
    Chunk(ChatChunk),
    /// 收到 `[DONE]` 哨兵。
    Done,
}

pub struct LlmClient {
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            // 流式请求不能设整体超时（会在长回答中途掐断），只约束建连阶段。
            .connect_timeout(std::time::Duration::from_secs(20))
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .user_agent(concat!("coding-agent/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http })
    }

    /// 发起流式对话请求。
    ///
    /// 返回的 Stream 每项是一个已解析的 chunk；解析失败的单帧会被跳过并记日志，
    /// 而不是终止整条流 —— 兼容网关偶尔会插入非标准帧（如心跳注释）。
    pub async fn stream_chat(
        &self,
        config: &LlmConfig,
        messages: &[Message],
        tools: &[ToolDef],
    ) -> Result<impl Stream<Item = StreamItem>> {
        let body = ChatRequest {
            model: &config.model,
            messages,
            stream: true,
            stream_options: Some(StreamOptions {
                include_usage: true,
            }),
            temperature: config.temperature,
            tools,
            // 无工具时整个字段都不发：部分网关见到 tool_choice 就要求 tools 非空
            tool_choice: (!tools.is_empty()).then_some("auto"),
        };

        let mut req = self.http.post(config.endpoint()).json(&body);
        if !config.api_key.trim().is_empty() {
            req = req.bearer_auth(config.api_key.trim());
        }

        tracing::debug!(
            messages = messages.len(),
            tools = tools.len(),
            "发起流式请求"
        );

        let resp = req.send().await?;
        let status = resp.status();

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            let described = describe_api_error(status, &text);
            // 错误也要落日志：只发给 UI 的话，排查时什么线索都没有
            tracing::warn!("请求失败：{described}");
            // 把请求体一并记下 —— 4xx 十有八九是消息序列不合协议
            if let Ok(json) = serde_json::to_string(&body) {
                tracing::debug!("失败请求的消息体：{}", truncate(&json, 4000));
            }
            return Err(Error::Config(described));
        }

        let stream = resp
            .bytes_stream()
            .eventsource()
            .filter_map(|event| async move {
                let event = match event {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::warn!("SSE 帧读取失败: {e}");
                        return None;
                    }
                };

                if event.data.trim() == "[DONE]" {
                    return Some(StreamItem::Done);
                }

                match serde_json::from_str::<ChatChunk>(&event.data) {
                    Ok(chunk) => Some(StreamItem::Chunk(chunk)),
                    Err(e) => {
                        tracing::warn!("跳过无法解析的 chunk: {e}; 原始内容: {}", event.data);
                        None
                    }
                }
            });

        Ok(stream)
    }

    /// 连通性探测：发一次极小的请求，读到第一帧即算成功。
    ///
    /// 刻意走与真实对话相同的流式路径，而不是打 `/models` —— 后者能通不代表
    /// chat 端点和模型名是对的，那种"测试通过但用不了"最误导人。
    pub async fn probe(&self, config: &LlmConfig, timeout: std::time::Duration) -> Result<Probe> {
        let messages = [Message::user("hi")];
        let stream = self.stream_chat(config, &messages, &[]).await?;
        tokio::pin!(stream);

        match tokio::time::timeout(timeout, stream.next()).await {
            Ok(Some(StreamItem::Chunk(_))) => Ok(Probe::Ok),
            Ok(Some(StreamItem::Done)) => Ok(Probe::EmptyStream),
            Ok(None) => Ok(Probe::ClosedImmediately),
            Err(_) => Ok(Probe::Timeout),
        }
    }
}

/// 连通性探测的结果。文案由宿主决定，core 只给事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// 收到了内容帧。
    Ok,
    /// 连上了，但服务端一个内容帧都没给就结束了。
    EmptyStream,
    /// 连上后立即关闭数据流。
    ClosedImmediately,
    /// 等待首帧超时。
    Timeout,
}

/// 把服务端错误体翻译成人能看懂的一句话。///
/// 只给 HTTP 状态码对用户毫无帮助 —— 401 到底是 key 错了还是地址填成了别家，
/// 得看 body 里的 message。
fn describe_api_error(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<ApiErrorEnvelope>(body)
        .map(|e| e.error.message)
        .unwrap_or_else(|_| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "服务端未返回错误详情".to_string()
            } else {
                trimmed.chars().take(300).collect()
            }
        });

    let hint = match status.as_u16() {
        401 | 403 => "（API Key 无效，或该 Key 无权访问此模型）",
        404 => "（地址或模型名不存在，检查 base_url 是否需要 /v1 前缀）",
        429 => "（触发限流或余额不足）",
        500..=599 => "（服务端故障，可稍后重试）",
        _ => "",
    };

    format!("HTTP {status} {hint}：{detail}")
}

/// 截断长文本用于日志。
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max).collect();
    format!("{head}…[已截断]")
}

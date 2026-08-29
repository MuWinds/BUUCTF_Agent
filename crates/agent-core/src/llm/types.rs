//! OpenAI 兼容协议的线格式定义。
//!
//! 所有响应侧字段一律 `Option` —— 兼容网关（vLLM / DeepSeek / Qwen / 各类中转）
//! 对规范的遵守程度参差不齐，缺字段是常态，不该导致整条流解析失败。

use serde::{Deserialize, Serialize};

// ---------- 请求 ----------

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [Message],
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub tools: &'a [ToolDef],
    /// 仅在有工具时发送。固定 `"auto"` —— 由模型自主决定何时调用。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamOptions {
    /// 要求服务端在流末尾附带 usage。不支持的网关会忽略该字段（而非报错）。
    pub include_usage: bool,
}

/// 工具定义，发给模型的部分。
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolDef {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function",
            function: FunctionDef {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// assistant 消息里模型请求的工具调用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// tool 消息里对应的调用 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 一次完整的工具调用。流式场景下由 [`crate::llm::accumulator`] 组装得到。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// 未解析的 JSON 字符串 —— 协议如此规定，模型也可能生成非法 JSON。
    pub arguments: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(Role::System, content)
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(Role::User, content)
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(Role::Assistant, content)
    }

    /// 模型请求工具调用时的 assistant 消息。
    ///
    /// `content` 可能为空（模型只调工具不说话），但历史里必须保留这条消息，
    /// 否则后续的 tool 结果消息会找不到对应的调用而被服务端拒绝。
    pub fn tool_calls(content: String, calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: (!content.is_empty()).then_some(content),
            tool_calls: Some(calls),
            tool_call_id: None,
        }
    }

    /// 工具执行结果，回灌给模型。
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
        }
    }

    fn text(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

// ---------- 流式响应 ----------

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// 仅在最后一帧出现（且需服务端支持 include_usage）。
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub delta: Delta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Delta {
    #[serde(default)]
    pub content: Option<String>,
    /// 思维链。非标准字段，DeepSeek-R1 / Qwen 等使用。
    #[serde(default)]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ToolCallDelta>>,
}

/// 工具调用的一个分片。
///
/// 字段全 `Option` 不是保守，是必需：规范只保证 `index` 稳定，
/// `id` 与 `name` 通常只在该 index 的首帧出现，之后各帧只带 `arguments` 片段。
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallDelta {
    /// 少数网关不发 index（隐含单个调用），缺失时按 0 处理。
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub function: Option<FunctionDelta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FunctionDelta {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub total_tokens: u32,
}

// ---------- 错误响应 ----------

/// 非 2xx 时服务端返回的 JSON。用于把 `{"error":{"message":...}}` 里的
/// 真实原因提取出来给用户，而不是只显示一个 HTTP 状态码。
#[derive(Debug, Deserialize)]
pub struct ApiErrorEnvelope {
    pub error: ApiError,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub message: String,
}

//! 前后端事件协议 —— 唯一真源。
//!
//! 前端的 `src/lib/events.ts` 必须与本文件手工保持一致。
//!
//! 事件通过 Tauri v2 的 `Channel` 推送而非全局 `emit`：
//! Channel 天然按调用隔离，前端无需按事件名路由、也无需管理 unlisten。

use serde::{Deserialize, Serialize};

/// 一次对话轮次中推送给前端的所有事件。
///
/// 序列化为 `{ "type": "assistant_delta", ... }` 的外部标签形式，
/// 前端用 discriminated union 做穷尽匹配。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// 轮次开始。前端据此创建一条空的 assistant 消息占位。
    TurnStart { turn_id: String, model: String },

    /// 助手正文增量。**已在 Rust 侧按帧聚合**，不是逐 token。
    AssistantDelta { turn_id: String, text: String },

    /// 思维链增量（DeepSeek / Qwen 等的 `reasoning_content` 字段）。
    /// 前端折叠灰显，与正文分开渲染。
    ReasoningDelta { turn_id: String, text: String },

    /// 模型开始请求一个工具调用。**此时参数往往还没到齐** ——
    /// 先发出来让 UI 立刻画出卡片头，观感上比等参数齐了再画好得多。
    ToolCallStart {
        turn_id: String,
        call_id: String,
        name: String,
    },

    /// 参数已到齐，可以渲染完整的卡片头了。
    ToolCallReady {
        turn_id: String,
        call_id: String,
        name: String,
        args: serde_json::Value,
        /// 折叠态显示的一行摘要，由工具自己生成 —— 前端不该懂工具语义。
        preview: String,
    },

    /// 工具执行期间的增量输出。**已按帧节流**，不是每读到一点就发一次。
    ToolProgress {
        turn_id: String,
        call_id: String,
        stream: &'static str,
        chunk: String,
    },

    /// 工具执行完毕。
    ToolResult {
        turn_id: String,
        call_id: String,
        ok: bool,
        duration_ms: u64,
        result: ToolResultBody,
    },

    /// 用量统计。由服务端 `stream_options.include_usage` 返回，
    /// 部分兼容网关不支持，此时该事件不会出现。
    Usage {
        turn_id: String,
        /// 本轮所有请求累计的输入 token（用于估算成本）。
        prompt_tokens: u32,
        completion_tokens: u32,
        total_tokens: u32,
        /// 最后一次请求的输入 token，即当前上下文的实际占用。
        context_used: u32,
        /// 模型的上下文窗口，用户配置值。
        context_limit: u32,
        elapsed_ms: u64,
        /// 每秒输出 token 数，由 completion_tokens / elapsed 计算。
        tps: f64,
    },

    /// 轮次结束。`finish_reason` 直接透传服务端值（stop / length / tool_calls 等）。
    TurnEnd {
        turn_id: String,
        finish_reason: String,
        elapsed_ms: u64,
    },

    /// 轮次因错误终止。与 command 返回的 Err 区分：
    /// 流已经开始后出错只能走事件，因为 command 早已返回。
    Error {
        turn_id: String,
        code: String,
        message: String,
        retryable: bool,
    },
}

/// 工具结果的结构化载荷。
///
/// 按类型分开而不是统一成字符串，前端才能差异化渲染 —— 文件内容带行号、
/// 匹配列表可点击、diff 上色。
///
/// 实现 `Deserialize` 是因为它会随 [`crate::session::Session`] 一起落盘：
/// 恢复会话时要能还原出原样的卡片，而不是退化成纯文本。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolResultBody {
    /// 纯文本结果（Read / Glob / Grep）。
    Text {
        content: String,
        /// 内容是否被截断过。前端据此显示"仅显示前 N 行"之类的提示。
        truncated: bool,
    },
    /// 文件改动（Write / Edit）。
    Diff {
        path: String,
        hunks: Vec<DiffHunk>,
        added: usize,
        removed: usize,
    },
    /// 命令执行结果（Bash）。
    Exec {
        command: String,
        exit_code: Option<i32>,
        /// 合并后的输出尾部。完整输出可能极长，只留末尾。
        output: String,
        truncated: bool,
        timed_out: bool,
        killed: bool,
    },
    /// 执行失败。
    Error { message: String },
}

/// 一段连续的改动及其上下文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffTag {
    Eq,
    Del,
    Ins,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub tag: DiffTag,
    /// 原文件行号，插入行为 None。
    pub old_line: Option<usize>,
    /// 新文件行号，删除行为 None。
    pub new_line: Option<usize>,
    /// 行内分段。
    pub segments: Vec<DiffSegment>,
}

/// 行内的一段文本。
///
/// **刻意用分段而不是 (start, end) 索引区间**：Rust 按字节/字符计数，
/// JS 字符串按 UTF-16 码元计数，中文和 emoji 会让索引对不上。
/// 直接传切好的片段，前端只管拼接上色，不做任何位置计算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSegment {
    pub text: String,
    /// 是否是本行实际变化的部分。UI 对这些片段做更强的高亮。
    pub emphasis: bool,
}

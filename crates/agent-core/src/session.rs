//! 会话记录 —— 单一数据源。
//!
//! 早先的做法是分别持有「给模型的消息」和「给 UI 的展示数据」两份。
//! 运行时那样分离是对的（UI 要 diff 结构体，模型只要一句摘要），
//! 但落到存储层就成了两份可能不一致的数据。
//!
//! 这里改成只存一份 [`Session`]：它同时承载两种信息，
//! 发给模型的消息列表由 [`Session::to_messages`] **投影**得出。
//! 一份数据，一致性天然成立。

use serde::{Deserialize, Serialize};

use crate::events::ToolResultBody;
use crate::llm::types::{FunctionCall, Message, ToolCall};

/// 一次完整的会话。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Entry {
    System {
        text: String,
    },
    User {
        text: String,
    },
    /// 自动压缩产生的历史摘要。
    ///
    /// 投影时作为一条 user 消息发给模型 —— 语义就是「此前发生过这些」，
    /// 模型继续读得到关键信息，窗口占用却大幅下降。UI 把它渲染成可展开
    /// 的提示气泡，而不是普通对话。
    Summary {
        text: String,
    },
    Assistant {
        /// 按到达顺序排列的片段。文本与工具调用交错，顺序即真相。
        segments: Vec<Segment>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reasoning: String,
        /// 每次请求的思维链，按请求顺序各存一段。
        ///
        /// DeepSeek 思考模式要求**按条**原样回传 reasoning_content，
        /// 拼接的整段会被 API 拒绝。`reasoning` 字段继续保留累加值
        /// （UI 折叠显示用），投影时以这里为准。
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        reasoning_rounds: Vec<String>,
        #[serde(default)]
        status: Status,
        /// 本次请求发出时模型的上下文占用（服务端返回的 prompt_tokens）。
        ///
        /// 用于自动压缩的阈值判断：比起把整段历史重新估算一遍，
        /// 服务端报的真实值才是「当前到底占了多少」的权威答案。
        /// 多轮工具调用里每次请求都会覆盖，保留的是最后一次请求的值。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_used: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Reasoning { text: String },
    Text { text: String },
    Tool { call: ToolRecord },
}

/// 一次工具调用的完整记录。
///
/// `llm_text` 与 `ui` 并存正是这个结构存在的意义：
/// 前者用于投影出发给模型的消息，后者用于还原界面上的卡片。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    pub call_id: String,
    pub name: String,
    pub args: serde_json::Value,
    /// 折叠态摘要。
    pub preview: String,
    /// 回灌给模型的文本。
    pub llm_text: String,
    /// 展示给用户的结构化结果。
    pub ui: ToolResultBody,
    pub ok: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Done,
    Cancelled,
    Error,
}

impl Session {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn push_system(&mut self, text: impl Into<String>) {
        self.entries.push(Entry::System { text: text.into() });
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.entries.push(Entry::User { text: text.into() });
    }

    /// 开一条新的助手条目，后续的文本与工具都追加到它上面。
    pub fn start_assistant(&mut self) {
        self.entries.push(Entry::Assistant {
            segments: Vec::new(),
            reasoning: String::new(),
            reasoning_rounds: Vec::new(),
            status: Status::Done,
            context_used: None,
        });
    }

    /// 记录最后一次请求时服务端报告的上下文占用。
    ///
    /// 同一轮里可能多次请求（多轮工具调用），每次覆盖 —— 保留的是
    /// 最后那次请求的 prompt_tokens，那才是「这轮结束时占了多少」。
    pub fn record_context_used(&mut self, prompt_tokens: u32) {
        if let Some(Entry::Assistant { context_used, .. }) = self.entries.last_mut() {
            *context_used = Some(prompt_tokens);
        }
    }

    /// 追加思维链片段。并入最后一个思维链片段；末尾是工具或正文则另起一段。
    pub fn push_reasoning_segment(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(segments) = self.current_segments() else {
            return;
        };
        match segments.last_mut() {
            Some(Segment::Reasoning { text: existing }) => existing.push_str(text),
            _ => segments.push(Segment::Reasoning {
                text: text.to_string(),
            }),
        }
    }

    /// 追加助手正文。并入最后一个文本片段；末尾是工具则另起一段。
    pub fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(segments) = self.current_segments() else {
            return;
        };
        match segments.last_mut() {
            Some(Segment::Text { text: existing }) => existing.push_str(text),
            _ => segments.push(Segment::Text {
                text: text.to_string(),
            }),
        }
    }

    pub fn push_reasoning(&mut self, text: &str) {
        if let Some(Entry::Assistant { reasoning, .. }) = self.entries.last_mut() {
            reasoning.push_str(text);
        }
    }

    /// 记录一次请求的思维链。与 `push_text` / `push_tool` 按相同节奏调用
    /// （每次 [`crate::turn`] 请求产出一段），投影时按条回传给服务端。
    pub fn push_reasoning_round(&mut self, text: &str) {
        if let Some(Entry::Assistant {
            reasoning_rounds, ..
        }) = self.entries.last_mut()
        {
            reasoning_rounds.push(text.to_string());
        }
    }

    pub fn push_tool(&mut self, record: ToolRecord) {
        if let Some(segments) = self.current_segments() {
            segments.push(Segment::Tool { call: record });
        }
    }

    pub fn set_status(&mut self, value: Status) {
        if let Some(Entry::Assistant { status, .. }) = self.entries.last_mut() {
            *status = value;
        }
    }

    /// 末尾的助手条目若什么都没产出就丢掉，避免历史里留下空壳。
    pub fn drop_empty_assistant(&mut self) {
        let empty = matches!(
            self.entries.last(),
            Some(Entry::Assistant { segments, reasoning, .. })
                if segments.is_empty() && reasoning.is_empty()
        );
        if empty {
            self.entries.pop();
        }
    }

    fn current_segments(&mut self) -> Option<&mut Vec<Segment>> {
        match self.entries.last_mut() {
            Some(Entry::Assistant { segments, .. }) => Some(segments),
            _ => None,
        }
    }

    /// 回退/分叉：保留到 `index`（含）为止的条目，丢弃之后的所有内容。
    ///
    /// `index` 是 [`entries`](Self::entries) 的绝对索引（含开头的 system）。
    /// 返回 `false` 表示 `index` 已超出范围或指向最后一条 —— 没有可丢弃的
    /// 内容，调用方不应改变任何状态。
    pub fn truncate_to(&mut self, index: usize) -> bool {
        if index >= self.entries.len() || index + 1 == self.entries.len() {
            return false;
        }
        self.entries.truncate(index + 1);
        true
    }

    /// 粗估投影消息占用的 token 数。
    ///
    /// 用于自动压缩的阈值判断，不需要精确 —— 差 30% 只是早一点或晚一点触发。
    /// 按序列化长度/4 估算（OpenAI 的 1 token ≈ 4 字符经验值），JSON 序列化
    /// 会把中文按 UTF-8 原样输出，中文文本的 1 字 ≈ 1-3 字节，落在同一量级。
    pub fn estimate_tokens(&self) -> usize {
        let json = serde_json::to_string(&self.to_messages()).unwrap_or_default();
        json.len() / 4 + 1
    }

    /// 投影成发给模型的消息序列。
    ///
    /// 协议要求带 `tool_calls` 的 assistant 消息后必须**紧跟**对应的 tool 消息，
    /// 所以交错的片段要拆成「文本 + 紧随其后的工具组」若干批，
    /// 而不能把所有文本拼一起、所有工具堆一起 —— 那样服务端会直接拒绝。
    pub fn to_messages(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        for entry in &self.entries {
            match entry {
                Entry::System { text } => messages.push(Message::system(text)),
                Entry::User { text } => messages.push(Message::user(text)),
                // 压缩摘要对模型而言就是一段历史背景，按 user 消息发过去即可
                Entry::Summary { text } => messages.push(Message::user(text)),
                Entry::Assistant {
                    segments,
                    reasoning,
                    reasoning_rounds,
                    ..
                } => project_assistant(segments, reasoning, reasoning_rounds, &mut messages),
            }
        }

        messages
    }
}

/// 取某批（第 `batch` 次请求）应回传的思维链。
///
/// 优先按 `reasoning_rounds`（新数据按请求分段）；旧数据只有拼接的
/// `reasoning`，不管哪一批都回传它 —— 反正 DeepSeek 只校验字段存在性，
/// 缺了直接 400，带个值（哪怕是整段拼接）总比缺字段强。
///
/// **空串也返回 `Some`**：DeepSeek 校验的是字段存在性而不是内容，模型某轮
/// 没输出 thinking（如纯工具调用轮）时这里存的是空串，仍要保留字段，
/// 否则下一轮请求 400。
fn reasoning_for<'a>(
    reasoning: &'a str,
    reasoning_rounds: &'a [String],
    batch: usize,
) -> Option<&'a str> {
    if !reasoning_rounds.is_empty() {
        return reasoning_rounds.get(batch).map(String::as_str);
    }
    Some(reasoning)
}

fn project_assistant(
    segments: &[Segment],
    reasoning: &str,
    reasoning_rounds: &[String],
    messages: &mut Vec<Message>,
) {
    let mut text = String::new();
    let mut pending: Vec<&ToolRecord> = Vec::new();
    // 批次从 0 递增：每次 flush 收口一批，对应一次请求的产出
    let mut batch = 0usize;

    let flush = |batch: &mut usize,
                 text: &mut String,
                 pending: &mut Vec<&ToolRecord>,
                 messages: &mut Vec<Message>| {
        let round = reasoning_for(reasoning, reasoning_rounds, *batch);
        *batch += 1;

        if pending.is_empty() {
            if !text.is_empty() {
                messages.push(Message::assistant_with_reasoning(
                    std::mem::take(text),
                    round,
                ));
            }
            return;
        }

        let calls = pending
            .iter()
            .map(|record| ToolCall {
                id: record.call_id.clone(),
                kind: "function".to_string(),
                function: FunctionCall {
                    name: record.name.clone(),
                    arguments: record.args.to_string(),
                },
            })
            .collect();

        messages.push(Message::tool_calls_with_reasoning(
            std::mem::take(text),
            calls,
            round,
        ));

        for record in pending.drain(..) {
            messages.push(Message::tool_result(&record.call_id, &record.llm_text));
        }
    };

    for segment in segments {
        match segment {
            Segment::Reasoning { .. } => {
                // 工具组之后出现思维链，说明进入了下一轮，先把上一轮收口
                if !pending.is_empty() {
                    flush(&mut batch, &mut text, &mut pending, messages);
                }
            }
            Segment::Text { text: chunk } => {
                // 工具组之后又出现文本，说明进入了下一轮，先把上一轮收口
                if !pending.is_empty() {
                    flush(&mut batch, &mut text, &mut pending, messages);
                }
                text.push_str(chunk);
            }
            Segment::Tool { call } => pending.push(call),
        }
    }

    flush(&mut batch, &mut text, &mut pending, messages);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Role;

    fn record(id: &str, name: &str, llm_text: &str) -> ToolRecord {
        ToolRecord {
            call_id: id.into(),
            name: name.into(),
            args: serde_json::json!({ "path": "a.rs" }),
            preview: format!("{name}(a.rs)"),
            llm_text: llm_text.into(),
            ui: ToolResultBody::Text {
                content: llm_text.into(),
                truncated: false,
            },
            ok: true,
            duration_ms: 5,
        }
    }

    fn roles(messages: &[Message]) -> Vec<Role> {
        messages.iter().map(|m| m.role).collect()
    }

    #[test]
    fn projects_plain_conversation() {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("hi");
        session.start_assistant();
        session.push_text("hello");

        let messages = session.to_messages();
        assert_eq!(
            roles(&messages),
            vec![Role::System, Role::User, Role::Assistant]
        );
        assert_eq!(messages[2].content.as_deref(), Some("hello"));
        assert!(messages[2].tool_calls.is_none());
    }

    /// 带工具调用时，assistant 消息后必须紧跟 tool 消息。
    #[test]
    fn projects_tool_call_pairs() {
        let mut session = Session::default();
        session.push_user("读一下");
        session.start_assistant();
        session.push_text("我看看");
        session.push_tool(record("c1", "Read", "文件内容"));
        session.push_text("看完了");

        let messages = session.to_messages();
        assert_eq!(
            roles(&messages),
            vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant]
        );

        let calls = messages[1].tool_calls.as_ref().expect("应当带 tool_calls");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(messages[2].content.as_deref(), Some("文件内容"));
    }

    /// 多轮工具调用要拆成多组，不能把工具全堆在一条消息上。
    #[test]
    fn splits_multiple_tool_rounds() {
        let mut session = Session::default();
        session.push_user("查一下");
        session.start_assistant();
        session.push_tool(record("c1", "Glob", "找到 3 个文件"));
        session.push_text("再看看内容");
        session.push_tool(record("c2", "Read", "内容如下"));

        let messages = session.to_messages();
        assert_eq!(
            roles(&messages),
            vec![
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::Assistant,
                Role::Tool
            ],
            "两轮工具调用应当拆成两组 assistant+tool"
        );
    }

    /// 同一轮里的并行调用应当挂在同一条 assistant 消息上。
    #[test]
    fn groups_parallel_calls_together() {
        let mut session = Session::default();
        session.start_assistant();
        session.push_tool(record("c1", "Read", "a"));
        session.push_tool(record("c2", "Read", "b"));

        let messages = session.to_messages();
        assert_eq!(
            roles(&messages),
            vec![Role::Assistant, Role::Tool, Role::Tool]
        );
        assert_eq!(messages[0].tool_calls.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn text_merges_into_last_segment() {
        let mut session = Session::default();
        session.start_assistant();
        session.push_text("abc");
        session.push_text("def");

        let Some(Entry::Assistant { segments, .. }) = session.entries.last() else {
            panic!("应当是助手条目");
        };
        assert_eq!(segments.len(), 1, "连续文本应当合并成一段");
    }

    /// 工具之后的文本要另起一段，否则顺序信息就丢了。
    #[test]
    fn text_after_tool_starts_new_segment() {
        let mut session = Session::default();
        session.start_assistant();
        session.push_text("before");
        session.push_tool(record("c1", "Read", "x"));
        session.push_text("after");

        let Some(Entry::Assistant { segments, .. }) = session.entries.last() else {
            panic!("应当是助手条目");
        };
        assert_eq!(segments.len(), 3);
    }

    /// 思维链与文本和工具交错时，必须保留在 segments 时间线上且投影正确。
    #[test]
    fn reasoning_segments_interleave_with_tools_and_text() {
        let mut session = Session::default();
        session.start_assistant();
        session.push_reasoning_segment("思考 1");
        session.push_text("说话 1");
        session.push_tool(record("c1", "Read", "x"));
        session.push_reasoning_segment("思考 2");
        session.push_text("说话 2");

        let Some(Entry::Assistant { segments, .. }) = session.entries.last() else {
            panic!("应当是助手条目");
        };
        assert_eq!(segments.len(), 5);
        assert!(matches!(&segments[0], Segment::Reasoning { text } if text == "思考 1"));
        assert!(matches!(&segments[1], Segment::Text { text } if text == "说话 1"));
        assert!(matches!(&segments[2], Segment::Tool { .. }));
        assert!(matches!(&segments[3], Segment::Reasoning { text } if text == "思考 2"));
        assert!(matches!(&segments[4], Segment::Text { text } if text == "说话 2"));

        let messages = session.to_messages();
        assert_eq!(
            roles(&messages),
            vec![Role::Assistant, Role::Tool, Role::Assistant]
        );
    }

    #[test]
    fn drops_empty_assistant_entry() {
        let mut session = Session::default();
        session.push_user("hi");
        session.start_assistant();
        session.drop_empty_assistant();

        assert_eq!(session.entries.len(), 1, "空的助手条目应当被丢弃");
    }

    #[test]
    fn keeps_assistant_with_content() {
        let mut session = Session::default();
        session.start_assistant();
        session.push_text("x");
        session.drop_empty_assistant();

        assert_eq!(session.entries.len(), 1);
    }

    /// 序列化往返后投影结果必须一致 —— 这正是「只存一份」要保证的。
    #[test]
    fn survives_serialization_round_trip() {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("hi");
        session.start_assistant();
        session.push_reasoning("想一想");
        session.push_reasoning_round("想一想");
        session.push_text("先查一下");
        session.push_tool(record("c1", "Glob", "3 个文件"));
        session.push_text("好了");

        let json = serde_json::to_string(&session).expect("应当可序列化");
        let restored: Session = serde_json::from_str(&json).expect("应当可反序列化");

        assert_eq!(
            roles(&session.to_messages()),
            roles(&restored.to_messages()),
            "恢复后投影出的消息序列必须与原始一致"
        );

        let Some(Entry::Assistant {
            segments,
            reasoning,
            reasoning_rounds,
            ..
        }) = restored.entries.last()
        else {
            panic!("应当是助手条目");
        };
        assert_eq!(reasoning, "想一想");
        assert_eq!(
            reasoning_rounds,
            &["想一想".to_string()],
            "按请求分段的思维链必须随会话一起落盘，否则重启后无法回传"
        );
        // 回传的内容也不能丢：恢复后的投影要能带上思维链
        // （索引 2 是第一条 assistant 消息，前面是 system 和 user）
        assert_eq!(
            restored.to_messages()[2].reasoning_content.as_deref(),
            Some("想一想")
        );

        // UI 侧信息也要完整保留，否则恢复后卡片就退化成纯文本了
        let has_ui = segments.iter().any(|s| {
            matches!(
                s,
                Segment::Tool { call } if matches!(call.ui, ToolResultBody::Text { .. })
            )
        });
        assert!(has_ui, "工具的 UI 结果没有被保留");
    }

    /// DeepSeek 思考模式要求按条原样回传 reasoning_content，缺了下一轮请求 400。
    #[test]
    fn projects_reasoning_back_per_request() {
        let mut session = Session::default();
        session.push_user("查一下");
        session.start_assistant();
        session.push_reasoning("先想一步");
        session.push_reasoning_round("先想一步");
        session.push_tool(record("c1", "Read", "内容"));

        let messages = session.to_messages();
        assert_eq!(
            messages[1].reasoning_content.as_deref(),
            Some("先想一步"),
            "工具调用消息必须带该次请求的思维链"
        );
    }

    /// 两轮工具调用时，各轮的思维链要各回各的，不能拼接成一段 ——
    /// 拼接的内容 DeepSeek 会拒绝（must be passed back 校验的是原文）。
    #[test]
    fn reasoning_is_split_across_tool_rounds() {
        let mut session = Session::default();
        session.push_user("查");
        session.start_assistant();
        session.push_reasoning("第一轮思考");
        session.push_reasoning_round("第一轮思考");
        session.push_tool(record("c1", "Glob", "a"));
        session.push_text("继续");
        session.push_reasoning("第二轮思考");
        session.push_reasoning_round("第二轮思考");
        session.push_tool(record("c2", "Read", "b"));

        let messages = session.to_messages();
        let with_calls: Vec<_> = messages.iter().filter(|m| m.tool_calls.is_some()).collect();
        assert_eq!(with_calls.len(), 2);
        assert_eq!(
            with_calls[0].reasoning_content.as_deref(),
            Some("第一轮思考"),
            "第一轮的思维链要挂在自己那条消息上"
        );
        assert_eq!(
            with_calls[1].reasoning_content.as_deref(),
            Some("第二轮思考"),
            "第二轮不能带上第一轮的思维链"
        );
    }

    /// 纯文本回答同样要回传思维链（thinking 模式的普通轮次也校验）。
    #[test]
    fn plain_text_carries_reasoning_when_present() {
        let mut session = Session::default();
        session.push_user("想");
        session.start_assistant();
        session.push_reasoning("思考中");
        session.push_reasoning_round("思考中");
        session.push_text("答案");

        let messages = session.to_messages();
        assert_eq!(messages[1].reasoning_content.as_deref(), Some("思考中"));
    }

    /// 工具调用轮模型可能完全没输出 reasoning（空串），字段仍然必须带上 ——
    /// DeepSeek 校验的是字段存在性，缺失直接 400。
    #[test]
    fn tool_round_without_reasoning_still_keeps_field() {
        let mut session = Session::default();
        session.push_user("查");
        session.start_assistant();
        session.push_reasoning_round("");
        session.push_tool(record("c1", "Read", "x"));

        let messages = session.to_messages();
        let with_calls: Vec<_> = messages.iter().filter(|m| m.tool_calls.is_some()).collect();
        assert_eq!(with_calls.len(), 1);
        assert_eq!(
            with_calls[0].reasoning_content.as_deref(),
            Some(""),
            "空串也必须带字段，否则下一轮请求 400"
        );
    }

    /// 多轮工具调用，其中一轮没输出 reasoning，该轮字段同样不能丢。
    #[test]
    fn reasoning_fields_exist_for_every_tool_round() {
        let mut session = Session::default();
        session.push_user("查");
        session.start_assistant();
        session.push_reasoning("第一轮思考");
        session.push_reasoning_round("第一轮思考");
        session.push_tool(record("c1", "Glob", "a"));
        session.push_text("继续");
        session.push_reasoning_round(""); // 第二轮没思考
        session.push_tool(record("c2", "Read", "b"));

        let messages = session.to_messages();
        let with_calls: Vec<_> = messages.iter().filter(|m| m.tool_calls.is_some()).collect();
        assert_eq!(with_calls.len(), 2);
        assert_eq!(
            with_calls[0].reasoning_content.as_deref(),
            Some("第一轮思考")
        );
        assert_eq!(
            with_calls[1].reasoning_content.as_deref(),
            Some(""),
            "第二轮字段必须存在（空串）"
        );
    }

    /// 旧版本持久化的会话没有 reasoning_rounds（只有拼接的 reasoning），
    /// 退化成挂到第一批 —— 总比不带强，旧会话至少不会直接 400。
    #[test]
    fn legacy_reasoning_attaches_to_first_batch() {
        let mut session = Session::default();
        session.push_user("查");
        session.start_assistant();
        session.push_reasoning("旧版思维链");
        session.push_tool(record("c1", "Read", "x"));

        if let Some(Entry::Assistant {
            reasoning_rounds, ..
        }) = session.entries.last_mut()
        {
            reasoning_rounds.clear();
        }

        let messages = session.to_messages();
        assert_eq!(messages[1].reasoning_content.as_deref(), Some("旧版思维链"));
    }

    /// 摘要条目投影成 user 消息 —— 模型把摘要当历史背景读，UI 另有气泡展示。
    #[test]
    fn summary_projects_as_user_message() {
        let mut session = Session::default();
        session.push_system("sys");
        session.entries.push(Entry::Summary {
            text: "（摘要）讨论了架构".into(),
        });
        session.push_user("继续");

        let messages = session.to_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, Role::User);
        assert_eq!(messages[1].content.as_deref(), Some("（摘要）讨论了架构"));
    }

    /// 回退到某条消息：保留它之前的内容，截断之后的所有条目。
    #[test]
    fn truncates_after_given_entry() {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("q1");
        session.start_assistant();
        session.push_text("a1");
        session.push_user("q2");
        session.start_assistant();
        session.push_text("a2");

        // 回退到第 1 条 user（索引 1）：保留 system + q1，丢 a1/q2/a2
        assert!(session.truncate_to(1));
        assert_eq!(session.entries.len(), 2);
        assert!(matches!(session.entries[1], Entry::User { .. }));

        // 指向最后一条时没有可丢弃内容，返回 false 且不变更
        let len = session.entries.len();
        assert!(!session.truncate_to(len - 1));
        assert_eq!(session.entries.len(), len);
    }

    /// 截断点越界时不应 panic，返回 false 让调用方自己处理。
    #[test]
    fn truncate_out_of_range_is_safe_noop() {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("q1");
        assert!(!session.truncate_to(5));
        assert_eq!(session.entries.len(), 2);
    }

    /// token 估算随消息量单调增长 —— 压缩的触发判断依赖相对大小。
    #[test]
    fn estimate_tokens_grows_with_history() {
        let mut short = Session::default();
        short.push_user("hi");
        let mut long = short.clone();
        for i in 0..20 {
            long.push_user(format!("这是第 {i} 条很长的消息"));
            long.start_assistant();
            long.push_text("这是答复，也比较长一些");
        }
        assert!(long.estimate_tokens() > short.estimate_tokens());
    }
}

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
    Assistant {
        /// 按到达顺序排列的片段。文本与工具调用交错，顺序即真相。
        segments: Vec<Segment>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reasoning: String,
        #[serde(default)]
        status: Status,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
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
            status: Status::Done,
        });
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
                Entry::Assistant { segments, .. } => project_assistant(segments, &mut messages),
            }
        }

        messages
    }
}

fn project_assistant(segments: &[Segment], messages: &mut Vec<Message>) {
    let mut text = String::new();
    let mut pending: Vec<&ToolRecord> = Vec::new();

    let flush = |text: &mut String, pending: &mut Vec<&ToolRecord>, messages: &mut Vec<Message>| {
        if pending.is_empty() {
            if !text.is_empty() {
                messages.push(Message::assistant(std::mem::take(text)));
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

        messages.push(Message::tool_calls(std::mem::take(text), calls));

        for record in pending.drain(..) {
            messages.push(Message::tool_result(&record.call_id, &record.llm_text));
        }
    };

    for segment in segments {
        match segment {
            Segment::Text { text: chunk } => {
                // 工具组之后又出现文本，说明进入了下一轮，先把上一轮收口
                if !pending.is_empty() {
                    flush(&mut text, &mut pending, messages);
                }
                text.push_str(chunk);
            }
            Segment::Tool { call } => pending.push(call),
        }
    }

    flush(&mut text, &mut pending, messages);
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
            ..
        }) = restored.entries.last()
        else {
            panic!("应当是助手条目");
        };
        assert_eq!(reasoning, "想一想");

        // UI 侧信息也要完整保留，否则恢复后卡片就退化成纯文本了
        let has_ui = segments.iter().any(|s| {
            matches!(
                s,
                Segment::Tool { call } if matches!(call.ui, ToolResultBody::Text { .. })
            )
        });
        assert!(has_ui, "工具的 UI 结果没有被保留");
    }
}

//! 长对话的自动压缩。
//!
//! 会话越长，投影给模型的上下文越大，接近窗口上限时要么被服务端拒绝、
//! 要么回答质量急剧下降。这里的策略：把最老的一批条目折叠成一段摘要，
//! 作为 [`Entry::Summary`] 存回会话 —— 模型继续读得到关键信息（决策、
//! 结论、待办），窗口却腾出了空间。数据结构不变，还是同一个 `Vec<Entry>`。

use futures_util::StreamExt;

use crate::config::LlmConfig;
use crate::error::Result;
use crate::llm::types::Message;
use crate::llm::{LlmClient, StreamItem};
use crate::session::{Entry, Session};

/// 压缩后保留的最近非 system 条目数（约 4 轮对话）。
///
/// 太大会让压缩毫无意义（腾不出空间），太小会让模型丢失最近的上下文
/// 衔接。4 轮是「刚发生过的事」与「历史」之间的实用分界。
pub const KEEP_ENTRIES: usize = 8;

/// 摘要请求里最后一条 user 消息的前缀。fake-llm 靠它把摘要请求和正常
/// 对话区分开（正常请求带 tools，摘要请求不带，但 fake-llm 靠 match 选
/// 场景，前缀比字段有无更不容易误判）。
pub const COMPACT_MARKER: &str = "[COMPACT]";

/// 一次压缩的结果，供调用方发事件、更新 UI。
#[derive(Debug, Clone)]
pub struct Compaction {
    /// 被压缩掉的条目数（= 替换成一条 Summary 的条目数）。
    pub removed_entries: usize,
    /// 生成的摘要文本。
    pub summary: String,
}

/// 估算当前上下文的 token 占用。
///
/// 优先用**服务端返回的真实占用**（最后一条 assistant 记录的
/// `context_used`）：那是模型实际看到的上下文大小，比任何字符估算都准。
/// 之后又追加的条目（比如上一轮结束后新增的用户消息）没有对应的真实值，
/// 按与 [`Session::estimate_tokens`] 相同的消息口径估算补上。完全没有真实
/// 记录时（全新会话、或网关没回 usage），退回到纯估算 —— 宁可保守触发
/// 也不放过超限。
pub fn estimate_context_used(session: &Session) -> usize {
    let mut last_known = 0usize;
    let mut tail_from: Option<usize> = None;

    for (i, entry) in session.entries.iter().enumerate() {
        if let Entry::Assistant {
            context_used: Some(used),
            ..
        } = entry
        {
            // 记录到「该条 assistant 请求时」为止的占用；
            // 这一条内部的内容（回复正文、工具结果）不在请求里，
            // 从下一条起按字符估算补上。
            last_known = *used as usize;
            tail_from = Some(i + 1);
        }
    }

    match tail_from {
        // 有真实记录：真实占用 + 其后新条目的估算
        // （i 越界说明 record 之后没有追加任何条目，直接返回真实值）
        Some(i) if i < session.entries.len() => {
            let tail = Session {
                entries: session.entries[i..].to_vec(),
            };
            last_known + tail.estimate_tokens()
        }
        Some(_) => last_known,
        // 没有任何真实记录：整段退回字符估算
        None => session.estimate_tokens(),
    }
}

/// 是否需要压缩：估算 token 占用超过窗口的一定比例。
pub fn should_compact(session: &Session, config: &LlmConfig) -> bool {
    let threshold = config.effective_compact_threshold();
    estimate_context_used(session) as f64 >= config.context_limit as f64 * threshold
}

/// 找出要压缩的条目范围（左闭右开）。
///
/// 范围永远从第一条非 system 条目开始 —— 系统提示词是产品的固定人格，
/// 不参与压缩，必须原样保留。压缩到「只剩最近 `keep_entries` 条非 system
/// 条目」为止。非 system 条目不足时返回 `None`（没得压缩，或压缩后
/// 剩不下什么）。
pub fn compression_range(session: &Session, keep_entries: usize) -> Option<(usize, usize)> {
    let non_system: Vec<usize> = session
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !matches!(e, Entry::System { .. }))
        .map(|(i, _)| i)
        .collect();
    if non_system.len() <= keep_entries {
        return None;
    }
    let end = non_system[non_system.len() - keep_entries];
    let start = non_system[0];
    if start >= end {
        return None;
    }
    Some((start, end))
}

/// 把 `entries[start..end]` 折叠成一条 [`Entry::Summary`]。返回被移除的条目数。
pub fn apply_compaction(session: &mut Session, range: (usize, usize), summary: &str) -> usize {
    let (start, end) = range;
    let removed = end - start;
    session.entries.splice(
        start..end,
        [Entry::Summary {
            text: summary.to_string(),
        }],
    );
    removed
}

/// 压缩会话：超限则折叠最老条目为摘要，返回摘要内容；未超限返回 `None`。
///
/// 摘要请求是一次不带工具的普通对话请求（无工具定义、无 tool_choice），
/// 读完整条流把正文拼出来。调用方（应用层）在轮次开始前调用，得到结果
/// 后发 [`crate::events::AgentEvent::ContextCompressed`] 通知 UI。
pub async fn maybe_compact(
    client: &LlmClient,
    config: &LlmConfig,
    session: &mut Session,
) -> Result<Option<Compaction>> {
    if !should_compact(session, config) {
        return Ok(None);
    }
    let Some(range) = compression_range(session, KEEP_ENTRIES) else {
        return Ok(None);
    };

    // 只投影被压缩的那段 —— 摘要请求不该把完整历史再发一遍，那样压缩
    // 反而让窗口更紧张。子 Session 复用 to_messages，工具配对天然成立。
    let sub = Session {
        entries: session.entries[range.0..range.1].to_vec(),
    };
    let history = sub.to_messages();
    if history.is_empty() {
        return Ok(None);
    }

    let summary = summarize(client, config, &history).await?;
    let removed_entries = apply_compaction(session, range, &summary);
    Ok(Some(Compaction {
        removed_entries,
        summary,
    }))
}

/// 把一段历史消息压缩成摘要文本。
///
/// 摘要指令带固定前缀 [`COMPACT_MARKER`]，fake-llm 据此识别并回放
/// 固定的摘要内容，端到端测试才确定。
pub async fn summarize(
    client: &LlmClient,
    config: &LlmConfig,
    history: &[Message],
) -> Result<String> {
    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(Message::system(
        "你是对话压缩器。把用户提供的历史对话压缩成一段简洁的中文摘要：\
         保留关键决策、结论、未完成的事项和工具执行结果，省略寒暄与重复细节。\
         不要输出任何解释，直接给摘要正文。",
    ));
    messages.extend(history.iter().cloned());
    messages.push(Message::user(format!(
        "{COMPACT_MARKER} 请把以上历史压缩成一段摘要。"
    )));

    let stream = client.stream_chat(config, &messages, &[]).await?;
    tokio::pin!(stream);
    let mut summary = String::new();
    while let Some(item) = stream.next().await {
        match item {
            StreamItem::Done => break,
            StreamItem::Chunk(chunk) => {
                let Some(choice) = chunk.choices.into_iter().next() else {
                    continue;
                };
                if let Some(text) = choice.delta.content {
                    summary.push_str(&text);
                }
            }
        }
    }
    Ok(summary.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::ToolResultBody;
    use crate::session::ToolRecord;

    fn tool_record(llm_text: &str) -> ToolRecord {
        ToolRecord {
            call_id: "c1".into(),
            name: "Read".into(),
            args: serde_json::json!({ "path": "a.rs" }),
            preview: "Read(a.rs)".into(),
            llm_text: llm_text.into(),
            ui: ToolResultBody::Text {
                content: llm_text.into(),
                truncated: false,
            },
            ok: true,
            duration_ms: 5,
        }
    }

    fn sample_session(rounds: usize) -> Session {
        let mut session = Session::default();
        session.push_system("sys");
        for i in 0..rounds {
            session.push_user(format!("第 {i} 轮的问题"));
            session.start_assistant();
            session.push_text(format!("第 {i} 轮的答复").as_str());
        }
        session
    }

    fn config(limit: u32, threshold: f64) -> LlmConfig {
        LlmConfig {
            context_limit: limit,
            compact_threshold: threshold,
            ..Default::default()
        }
    }

    /// 压缩范围必须从第一条非 system 条目开始 —— 系统提示词不参与压缩。
    #[test]
    fn range_starts_after_system_prompt() {
        let session = sample_session(6);
        let (start, end) = compression_range(&session, 4).expect("应当有可压缩范围");
        assert_eq!(start, 1, "system 位于 index 0，压缩必须从 index 1 开始");
        // 12 条非 system，保留最近 4 条 → 压缩前 8 条 → end = 1 + 8
        assert_eq!(end, 9);
    }

    #[test]
    fn no_range_when_entries_are_few() {
        let session = sample_session(2);
        assert!(compression_range(&session, 4).is_none(), "条目太少不该压缩");
    }

    #[test]
    fn no_range_when_everything_would_be_kept() {
        let session = sample_session(4);
        assert!(
            compression_range(&session, 8).is_none(),
            "全部保留时没有可压缩内容"
        );
    }

    #[test]
    fn should_compact_only_when_over_threshold() {
        let mut session = Session::default();
        session.push_system("sys");
        // 一条很长的消息：估算 token 必然超过窗口的 70%
        session.push_user("长".repeat(2000));
        assert!(
            should_compact(&session, &config(100, 0.7)),
            "估算 token 应远超 70 的上限"
        );

        let short = sample_session(1);
        assert!(
            !should_compact(&short, &config(100_000, 0.7)),
            "短对话不该触发压缩"
        );
    }

    /// 阈值越低越早触发：同样的历史，0.3 阈值压、0.9 阈值不压。
    #[test]
    fn lower_threshold_triggers_earlier() {
        let mut session = Session::default();
        session.push_system("sys");
        // 100 个中文字符 ≈ 300 字节 ≈ 75 token：夹在 200*0.3=60 与 200*0.9=180 之间
        session.push_user("长".repeat(100));

        assert!(
            should_compact(&session, &config(200, 0.3)),
            "0.3 阈值应当触发"
        );
        assert!(
            !should_compact(&session, &config(200, 0.9)),
            "0.9 阈值不应当触发"
        );
    }

    /// 服务端返回的真实占用优先于字符估算：即便字符串很短（估不超），
    /// 只要真实 context_used 超过阈值就要压缩。
    #[test]
    fn real_usage_overrides_estimate() {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("短问题");
        session.start_assistant();
        session.push_text("短答复");
        // 模拟 turn 记录的服务端占用：内容短但占用已经很大
        session.record_context_used(90);

        assert_eq!(estimate_context_used(&session), 90, "真实占用直接采用");
        assert!(
            should_compact(&session, &config(100, 0.8)),
            "90 > 100*0.8 应触发压缩"
        );
    }

    /// 真实占用之后又追加的条目要按字符估算补上 —— 新消息还没被服务端数过。
    #[test]
    fn trailing_entries_are_estimated_on_top_of_real_usage() {
        let mut session = Session::default();
        session.push_system("sys");
        // 历史做得足够长：前面这些条目的估算必须超过真实占用 50，
        // 否则「真实值 + 尾差」反而比整段估算大，断言就失去意义。
        session.push_user("问题一".repeat(60));
        session.start_assistant();
        session.push_text("答复一".repeat(60).as_str());
        session.record_context_used(50);
        let appended = "追加的长问题".repeat(50);
        session.push_user(&appended);

        let used = estimate_context_used(&session);
        assert!(used > 50, "新消息应叠加在真实占用之上：{used}");
        // 新消息的估算至少贡献了其 UTF-8 字节数的四分之一
        assert!(
            used >= 50 + appended.len() / 4,
            "叠加量不应少于新消息的估算：{used}"
        );
        // 有真实记录时，系统提示词与旧历史不再重复计数 —— 比整段估算小
        assert!(
            used < session.estimate_tokens(),
            "真实占用优先应避免重复计入旧历史：used={used}, estimate={}",
            session.estimate_tokens()
        );
    }

    /// 完全没有任何 usage 记录时退回整段估算 —— 全新会话或网关没回 usage。
    #[test]
    fn falls_back_to_estimate_without_usage() {
        let session = sample_session(3);
        assert_eq!(
            estimate_context_used(&session),
            session.estimate_tokens(),
            "无 usage 记录时应当退回纯估算"
        );
    }

    /// 多轮工具调用时，context_used 每次覆盖，取最后一次请求的值。
    #[test]
    fn record_context_used_keeps_the_latest_request() {
        let mut session = Session::default();
        session.start_assistant();
        session.record_context_used(10);
        session.record_context_used(200);
        assert_eq!(estimate_context_used(&session), 200, "应保留最后一次的值");
    }

    /// 压缩后：被压缩条目消失、摘要条目就位、投影消息显著变短。
    #[test]
    fn apply_compaction_replaces_range_with_summary() {
        let mut session = sample_session(6);
        let before = session.entries.len();
        let range = compression_range(&session, 4).expect("应当有可压缩范围");
        let removed = apply_compaction(&mut session, range, "（摘要）");

        assert_eq!(removed, 8);
        assert_eq!(session.entries.len(), before - 8 + 1, "8 条换 1 条摘要");
        assert!(
            matches!(session.entries[1], Entry::Summary { .. }),
            "摘要应紧跟 system"
        );
        assert!(
            matches!(session.entries[0], Entry::System { ref text } if text == "sys"),
            "system 原样保留"
        );
    }

    /// 压缩掉的条目里有工具调用时，投影的子 Session 也要保证工具配对完整。
    #[test]
    fn compression_keeps_tool_pairs_intact() {
        let mut session = Session::default();
        session.push_system("sys");
        for i in 0..3 {
            session.push_user(format!("问题 {i}"));
            session.start_assistant();
            session.push_tool(tool_record("工具结果"));
            session.push_text("继续");
        }

        let range = compression_range(&session, 2).expect("应当有可压缩范围");
        let sub = Session {
            entries: session.entries[range.0..range.1].to_vec(),
        };
        let messages = sub.to_messages();
        // 压缩段内有工具调用 → 投影必须产出 assistant+tool 配对，不能只出 assistant
        let tool_messages = messages.iter().filter(|m| m.tool_call_id.is_some()).count();
        assert!(tool_messages > 0, "工具结果消息不能丢：{messages:?}");
        // 每条 tool 结果前面必须有带 tool_calls 的 assistant 消息
        let with_calls = messages.iter().filter(|m| m.tool_calls.is_some()).count();
        assert_eq!(
            with_calls, tool_messages,
            "assistant 调用与 tool 结果必须成对出现"
        );
    }

    /// 摘要条目在序列化往返后依然可投影 —— 压缩后的会话要能落盘恢复。
    #[test]
    fn summary_survives_serialization() {
        let mut session = sample_session(6);
        let range = compression_range(&session, 4).unwrap();
        apply_compaction(&mut session, range, "（摘要内容）");

        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert!(matches!(restored.entries[1], Entry::Summary { .. }));
        let messages = restored.to_messages();
        assert!(
            messages
                .iter()
                .any(|m| m.content.as_deref() == Some("（摘要内容）")),
            "摘要应按 user 消息发给模型"
        );
    }
}

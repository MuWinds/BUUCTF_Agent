//! 流式 `tool_calls` 的增量组装。
//!
//! OpenAI 的流式协议里，一次工具调用被拆成多帧：
//!
//! ```text
//! {index:0, id:"call_abc", function:{name:"Read", arguments:""}}
//! {index:0, function:{arguments:"{\"file"}}
//! {index:0, function:{arguments:"_path\":\"a.rs\"}"}}
//! ```
//!
//! 规范只保证 `index` 稳定。实际接入的兼容网关（vLLM / DeepSeek / Qwen /
//! 各类中转）还会有这些行为，组装器必须全部容忍：
//!
//! - 每帧重复发送完整的 `id` 和 `name`
//! - `name` 本身也被分片
//! - 不发 `index`（隐含只有一个调用）
//! - 多个不同的调用共用 `index: 0`，仅靠 `id` 区分
//! - `arguments` 一次性给完，不分片

use std::collections::BTreeMap;

use crate::llm::types::{FunctionCall, ToolCall, ToolCallDelta};

/// 新出现的工具调用。上层据此立刻画出卡片头 —— 此时参数往往还没到齐。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub call_id: String,
    pub name: String,
    pub index: u32,
}

#[derive(Debug, Default)]
struct Slot {
    id: Option<String>,
    name: String,
    arguments: String,
    /// 是否已经作为 `Started` 上报过。
    announced: bool,
}

/// 把分片攒成完整的工具调用。
///
/// 用 `BTreeMap` 而非 `HashMap`：产出顺序必须稳定，否则并行工具调用的
/// 执行顺序会随机变化，同样的对话跑两次结果不同。
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    slots: BTreeMap<u32, Slot>,
}

impl ToolCallAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 吃进一帧里的所有分片，返回本帧新出现的调用。
    pub fn push(&mut self, deltas: &[ToolCallDelta]) -> Vec<Started> {
        let mut started = Vec::new();

        for delta in deltas {
            let index = self.resolve_index(delta);
            let slot = self.slots.entry(index).or_default();

            if let Some(id) = &delta.id {
                // get_or_insert 而非覆盖：防止「每帧重发 id」把已有值反复改写
                slot.id.get_or_insert_with(|| id.clone());
            }

            if let Some(function) = &delta.function {
                if let Some(name) = &function.name {
                    // 相等说明是重发，不是分片
                    if !name.is_empty() && *name != slot.name {
                        slot.name.push_str(name);
                    }
                }
                if let Some(args) = &function.arguments {
                    slot.arguments.push_str(args);
                }
            }

            // 名字到齐就上报，不等参数 —— 让 UI 能立刻显示"正在调用 Read"
            if !slot.announced && !slot.name.is_empty() {
                slot.announced = true;
                started.push(Started {
                    call_id: slot.id.clone().unwrap_or_else(|| fallback_id(index)),
                    name: slot.name.clone(),
                    index,
                });
            }
        }

        started
    }

    /// 决定这个分片属于哪个槽位。
    ///
    /// 通常直接用 `index`。但有的网关对多个调用都发 `index: 0`，只靠 `id` 区分
    /// —— 此时若发现 id 与该槽位已有的 id 不同，就另开一个槽位，
    /// 否则两次调用会被拼成一个损坏的 JSON。
    fn resolve_index(&self, delta: &ToolCallDelta) -> u32 {
        let index = delta.index.unwrap_or(0);

        let (Some(incoming), Some(slot)) = (&delta.id, self.slots.get(&index)) else {
            return index;
        };
        let Some(existing) = &slot.id else {
            return index;
        };

        if existing == incoming {
            index
        } else {
            // 已有同 id 的槽位（乱序重入）就回到那个，否则开新槽
            self.slots
                .iter()
                .find(|(_, s)| s.id.as_ref() == Some(incoming))
                .map(|(i, _)| *i)
                .unwrap_or_else(|| self.slots.keys().max().map_or(0, |m| m + 1))
        }
    }

    /// 收口，产出完整的调用列表。
    ///
    /// 参数里的 JSON 会尝试修复常见的截断问题；修不好就原样返回 ——
    /// 让执行层把解析错误回灌给模型重试，比在这里吞掉要好。
    pub fn finish(self) -> Vec<ToolCall> {
        self.slots
            .into_iter()
            .map(|(index, slot)| ToolCall {
                id: slot.id.unwrap_or_else(|| fallback_id(index)),
                kind: "function".to_string(),
                function: FunctionCall {
                    name: slot.name,
                    arguments: repair_json(&slot.arguments),
                },
            })
            .collect()
    }
}

/// 网关没给 id 时自造一个。id 只用于把结果和调用配对，本地生成即可。
fn fallback_id(index: u32) -> String {
    format!("call_local_{index}")
}

/// 修复被截断的 JSON 参数。
///
/// 只处理两类确定安全的情况：尾部多余的逗号、缺失的收尾括号。
/// 修不好就原样返回。
fn repair_json(raw: &str) -> String {
    let trimmed = raw.trim();

    // 空参数是合法的（无参工具），归一成 {}
    if trimmed.is_empty() {
        return "{}".to_string();
    }

    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_string();
    }

    let mut fixed = strip_trailing_commas(trimmed);
    for closer in missing_closers(&fixed) {
        fixed.push(closer);
    }

    if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
        tracing::debug!("修复了被截断的工具参数 JSON");
        fixed
    } else {
        trimmed.to_string()
    }
}

/// 去掉多余的逗号：闭合括号之前的，以及整串末尾的。
///
/// `{"a":1,}` 里那个逗号在 `}` 前面而不是串尾，光 trim 尾部字符是碰不到的。
/// 字符串字面量里的逗号不动。
fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut in_string = false;
    let mut escaped = false;

    for (i, &ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            out.push(ch);
            continue;
        }
        match ch {
            '\\' if in_string => {
                escaped = true;
                out.push(ch);
            }
            '"' => {
                in_string = !in_string;
                out.push(ch);
            }
            ',' if !in_string => {
                let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
                // 后面就是闭合括号或已到末尾 —— 这个逗号是多余的
                if !matches!(next, Some('}') | Some(']') | None) {
                    out.push(ch);
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

/// 扫描出还没闭合的括号，按需补上。字符串字面量内的括号不算。
fn missing_closers(s: &str) -> Vec<char> {
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => stack.push('}'),
            '[' if !in_string => stack.push(']'),
            '}' | ']' if !in_string => {
                stack.pop();
            }
            _ => {}
        }
    }

    // 栈里是按开启顺序压入的闭合符，补全要逆序 —— 最后开启的最先闭合
    stack.reverse();

    // 字符串没闭合的话，引号是最内层，必须排在所有括号之前
    if in_string {
        stack.insert(0, '"');
    }

    stack
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::FunctionDelta;

    fn delta(
        index: Option<u32>,
        id: Option<&str>,
        name: Option<&str>,
        args: Option<&str>,
    ) -> ToolCallDelta {
        ToolCallDelta {
            index,
            id: id.map(String::from),
            function: (name.is_some() || args.is_some()).then(|| FunctionDelta {
                name: name.map(String::from),
                arguments: args.map(String::from),
            }),
        }
    }

    /// 标准形态：id 和 name 只在首帧，之后只有参数分片。
    #[test]
    fn assembles_standard_stream() {
        let mut acc = ToolCallAccumulator::new();

        let started = acc.push(&[delta(Some(0), Some("call_a"), Some("Read"), Some(""))]);
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].name, "Read");
        assert_eq!(started[0].call_id, "call_a");

        // 参数还没到就已经上报了，这正是我们要的
        acc.push(&[delta(Some(0), None, None, Some("{\"path\""))]);
        acc.push(&[delta(Some(0), None, None, Some(":\"a.rs\"}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_a");
        assert_eq!(calls[0].function.name, "Read");
        assert_eq!(calls[0].function.arguments, "{\"path\":\"a.rs\"}");
    }

    /// 有的网关每帧都重发完整的 id 和 name。
    #[test]
    fn tolerates_repeated_id_and_name() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[delta(Some(0), Some("call_a"), Some("Grep"), Some("{"))]);
        acc.push(&[delta(
            Some(0),
            Some("call_a"),
            Some("Grep"),
            Some("\"q\":1"),
        )]);
        acc.push(&[delta(Some(0), Some("call_a"), Some("Grep"), Some("}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "Grep", "name 被重复拼接了");
        assert_eq!(calls[0].function.arguments, "{\"q\":1}");
    }

    /// name 本身被分片。
    #[test]
    fn assembles_fragmented_name() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[delta(Some(0), Some("c"), Some("Re"), None)]);
        acc.push(&[delta(Some(0), None, Some("ad"), None)]);

        assert_eq!(acc.finish()[0].function.name, "Read");
    }

    /// 并行调用：两个 index 各自独立组装，且产出顺序稳定。
    #[test]
    fn assembles_parallel_calls() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[
            delta(Some(0), Some("a"), Some("Read"), Some("{\"p\":1}")),
            delta(Some(1), Some("b"), Some("Glob"), Some("{\"p\":2}")),
        ]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function.name, "Read");
        assert_eq!(calls[1].function.name, "Glob");
    }

    /// 不发 index 的网关，按单个调用处理。
    #[test]
    fn defaults_missing_index_to_zero() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[delta(None, Some("a"), Some("Read"), Some("{}"))]);
        acc.push(&[delta(None, None, None, None)]);

        assert_eq!(acc.finish().len(), 1);
    }

    /// 多个调用共用 index 0，靠 id 区分 —— 不能被拼成一个。
    #[test]
    fn splits_distinct_ids_sharing_an_index() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[delta(Some(0), Some("a"), Some("Read"), Some("{\"p\":1}"))]);
        acc.push(&[delta(Some(0), Some("b"), Some("Glob"), Some("{\"p\":2}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 2, "两个不同 id 的调用被合并了");
        assert_eq!(calls[0].function.arguments, "{\"p\":1}");
        assert_eq!(calls[1].function.arguments, "{\"p\":2}");
    }

    /// 同 id 的分片乱序回到 index 0 之后，仍应落回原槽位。
    #[test]
    fn routes_late_fragments_back_by_id() {
        let mut acc = ToolCallAccumulator::new();
        acc.push(&[delta(Some(0), Some("a"), Some("Read"), Some("{\"p\""))]);
        acc.push(&[delta(Some(0), Some("b"), Some("Glob"), Some("{}"))]);
        // 又回到 a
        acc.push(&[delta(Some(0), Some("a"), None, Some(":1}"))]);

        let calls = acc.finish();
        assert_eq!(calls.len(), 2);
        let a = calls.iter().find(|c| c.id == "a").expect("槽位 a 丢失");
        assert_eq!(a.function.arguments, "{\"p\":1}");
    }

    /// 没有 id 时自造一个，保证结果能和调用配对。
    #[test]
    fn synthesizes_missing_id() {
        let mut acc = ToolCallAccumulator::new();
        let started = acc.push(&[delta(Some(2), None, Some("Read"), Some("{}"))]);

        assert_eq!(started[0].call_id, "call_local_2");
        assert_eq!(acc.finish()[0].id, "call_local_2");
    }

    #[test]
    fn empty_arguments_become_empty_object() {
        assert_eq!(repair_json(""), "{}");
        assert_eq!(repair_json("   "), "{}");
    }

    #[test]
    fn repairs_truncated_json() {
        assert_eq!(repair_json("{\"a\":1"), "{\"a\":1}");
        assert_eq!(repair_json("{\"a\":[1,2"), "{\"a\":[1,2]}");
        assert_eq!(repair_json("{\"a\":1,}"), "{\"a\":1}");
        assert_eq!(repair_json("{\"a\":[1,2,]}"), "{\"a\":[1,2]}");
        assert_eq!(repair_json("{\"a\":1,"), "{\"a\":1}");
        assert_eq!(repair_json("{\"a\":\"unclosed"), "{\"a\":\"unclosed\"}");
    }

    /// 字符串里的逗号不该被当成结构性逗号删掉。
    #[test]
    fn keeps_commas_inside_strings() {
        assert_eq!(repair_json("{\"a\":\"x,\"}"), "{\"a\":\"x,\"}");
    }

    /// 括号出现在字符串字面量里时不该被当成结构。
    #[test]
    fn ignores_braces_inside_strings() {
        assert_eq!(repair_json("{\"a\":\"{[\"}"), "{\"a\":\"{[\"}");
        assert_eq!(repair_json("{\"a\":\"x\\\"y\"}"), "{\"a\":\"x\\\"y\"}");
    }

    /// 修不好就原样返回，交给上层报错给模型重试。
    #[test]
    fn leaves_unrepairable_json_alone() {
        assert_eq!(repair_json("not json at all"), "not json at all");
    }
}

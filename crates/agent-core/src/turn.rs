//! 轮次驱动：请求 → 执行工具 → 回灌结果 → 再请求，直到模型不再调用工具。
//!
//! 消息历史通过 `&mut Vec<Message>` 传入而非由 core 持有：调用方通常把历史
//! 放在锁里，让 core 持有会强迫它跨整个轮次持锁，配置读取之类的操作就会被阻塞。

use std::time::Instant;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::config::LlmConfig;
use crate::error::Error;
use crate::events::{AgentEvent, ToolResultBody};
use crate::llm::accumulator::ToolCallAccumulator;
use crate::llm::types::{Message, ToolCall};
use crate::llm::{LlmClient, StreamItem};
use crate::session::{Session, Status, ToolRecord};
use crate::sink::{ProgressReporter, ThrottledSink};
use crate::tools::{Registry, ToolCtx, ToolEnv};

/// 单轮内最多允许模型调用几次工具就必须收口。
///
/// 防的是模型陷入「调用 → 失败 → 换个参数再调」的死循环。触顶后把情况
/// 如实告诉模型，让它给出结论而不是静默截断。
const MAX_TOOL_ROUNDS: usize = 25;

/// 一个轮次的产出。
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    /// 服务端给的结束原因；被取消时为 `cancelled`，出错时为 `error`。
    pub finish_reason: String,
}

impl TurnOutcome {
    pub fn is_cancelled(&self) -> bool {
        self.finish_reason == "cancelled"
    }
}

/// 跨多轮工具调用累计的用量。
///
/// 一个轮次里模型可能被请求很多次，每次都会返回自己那次的 usage。
/// 只报最后一次会严重低估真实消耗，所以这里累加。
#[derive(Debug, Default, Clone, Copy)]
struct UsageTally {
    prompt: u32,
    completion: u32,
    total: u32,
    /// **最后一次**请求的 prompt tokens。
    ///
    /// 上下文占用要用这个而不是累加值：每次请求都重发完整历史，
    /// 累加起来轻易就超过窗口大小，拿去算占比会得出 300% 这种荒谬数字。
    last_prompt: u32,
    /// 服务端是否真的返回过 usage。没有的话不发事件 —— 显示 0 会让人
    /// 误以为真的没消耗 token。
    reported: bool,
}

impl UsageTally {
    fn add(&mut self, u: &crate::llm::types::Usage) {
        self.prompt += u.prompt_tokens;
        self.completion += u.completion_tokens;
        self.total += u.total_tokens;
        self.last_prompt = u.prompt_tokens;
        self.reported = true;
    }
}

/// 执行一个轮次。
///
/// `session` 传入时应已包含用户消息；返回时包含本轮产生的全部内容。
/// 发给模型的消息由 [`Session::to_messages`] 投影得出 —— 会话只有一份数据。
///
/// 错误不向上抛而是转成 `AgentEvent::Error` —— 流一旦开始，调用方的入口函数
/// 往往早已返回，UI 只能通过事件流感知失败。
pub async fn run(
    client: &LlmClient,
    config: &LlmConfig,
    session: &mut Session,
    registry: &Registry,
    env: &ToolEnv,
    sink: &mut ThrottledSink,
    cancel: CancellationToken,
) -> TurnOutcome {
    let started = Instant::now();

    sink.emit(AgentEvent::TurnStart {
        turn_id: sink.turn_id().to_string(),
        model: config.model.clone(),
    });

    let definitions = registry.definitions();
    let mut usage = UsageTally::default();

    session.start_assistant();

    for round in 0..MAX_TOOL_ROUNDS {
        let messages = session.to_messages();

        let step = match request(
            client,
            config,
            &messages,
            &definitions,
            sink,
            &cancel,
            &mut usage,
        )
        .await
        {
            Ok(step) => step,
            Err(reason) => {
                session.set_status(status_for(&reason));
                session.drop_empty_assistant();
                let outcome = TurnOutcome {
                    finish_reason: reason,
                };
                return finish(sink, started, usage, config, outcome);
            }
        };

        session.push_reasoning(&step.reasoning);
        session.push_text(&step.answer);

        if step.calls.is_empty() {
            session.set_status(Status::Done);
            session.drop_empty_assistant();
            let outcome = TurnOutcome {
                finish_reason: step.finish_reason,
            };
            return finish(sink, started, usage, config, outcome);
        }

        for call in &step.calls {
            if cancel.is_cancelled() {
                session.set_status(Status::Cancelled);
                let outcome = TurnOutcome {
                    finish_reason: "cancelled".into(),
                };
                return finish(sink, started, usage, config, outcome);
            }

            let (record, fatal) = execute(call, registry, env, &cancel, sink).await;
            session.push_tool(record);

            if let Some(message) = fatal {
                emit_error(sink, "tool", &message, false);
                session.set_status(Status::Error);
                let outcome = TurnOutcome {
                    finish_reason: "error".into(),
                };
                return finish(sink, started, usage, config, outcome);
            }
        }

        if round + 1 == MAX_TOOL_ROUNDS {
            session.push_user(format!(
                "已连续调用工具 {MAX_TOOL_ROUNDS} 次，达到本轮上限。\
                 请基于目前掌握的信息直接给出结论，不要再调用工具。"
            ));
            session.start_assistant();
        }
    }

    session.set_status(Status::Done);
    session.drop_empty_assistant();
    let outcome = TurnOutcome {
        finish_reason: "tool_limit".into(),
    };
    finish(sink, started, usage, config, outcome)
}

fn status_for(reason: &str) -> Status {
    match reason {
        "cancelled" => Status::Cancelled,
        _ => Status::Error,
    }
}

/// 一次 LLM 请求的结果。
struct Step {
    answer: String,
    reasoning: String,
    calls: Vec<ToolCall>,
    finish_reason: String,
}

/// 发一次请求并消费完整条流。
///
/// `Err` 携带的是这一轮的结束原因（`error` / `cancelled`）。
async fn request(
    client: &LlmClient,
    config: &LlmConfig,
    messages: &[Message],
    definitions: &[crate::llm::types::ToolDef],
    sink: &mut ThrottledSink,
    cancel: &CancellationToken,
    usage: &mut UsageTally,
) -> Result<Step, String> {
    let stream = match client.stream_chat(config, messages, definitions).await {
        Ok(s) => s,
        Err(e) => {
            emit_llm_error(sink, &e);
            return Err("error".into());
        }
    };
    tokio::pin!(stream);

    let mut answer = String::new();
    let mut reasoning = String::new();
    let mut finish_reason = "stop".to_string();
    let mut accumulator = ToolCallAccumulator::new();

    loop {
        let item = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                sink.flush();
                return Err("cancelled".into());
            }
            item = stream.next() => item,
        };

        let Some(item) = item else { break };

        let chunk = match item {
            StreamItem::Done => break,
            StreamItem::Chunk(c) => c,
        };

        if let Some(u) = chunk.usage {
            usage.add(&u);
        }

        // usage-only 帧的 choices 为空，属正常情况
        let Some(choice) = chunk.choices.into_iter().next() else {
            continue;
        };

        if let Some(reason) = choice.finish_reason {
            finish_reason = reason;
        }
        if let Some(text) = choice.delta.reasoning_content {
            reasoning.push_str(&text);
            sink.push_reasoning(&text);
        }
        if let Some(text) = choice.delta.content {
            answer.push_str(&text);
            sink.push_text(&text);
        }
        if let Some(deltas) = choice.delta.tool_calls {
            for started in accumulator.push(&deltas) {
                sink.emit(AgentEvent::ToolCallStart {
                    turn_id: sink.turn_id().to_string(),
                    call_id: started.call_id,
                    name: started.name,
                });
            }
        }
    }

    sink.flush();

    Ok(Step {
        answer,
        reasoning,
        calls: accumulator.finish(),
        finish_reason,
    })
}

/// 执行一个工具调用，返回 (完整记录, 致命错误信息)。
///
/// 记录同时含 `llm_text` 与 `ui`，直接进 `Session` —— 落盘与投影都用它。
async fn execute(
    call: &ToolCall,
    registry: &Registry,
    env: &ToolEnv,
    cancel: &CancellationToken,
    sink: &mut ThrottledSink,
) -> (ToolRecord, Option<String>) {
    let turn_id = sink.turn_id().to_string();
    let started = Instant::now();

    let mut record = ToolRecord {
        call_id: call.id.clone(),
        name: call.function.name.clone(),
        args: serde_json::Value::Null,
        preview: call.function.name.clone(),
        llm_text: String::new(),
        ui: ToolResultBody::Error {
            message: String::new(),
        },
        ok: false,
        duration_ms: 0,
    };

    /// 失败收口：事件与记录用同一份文本，两边不会说不一样的话。
    macro_rules! fail {
        ($record:expr, $message:expr) => {{
            let message: String = $message;
            $record.ui = ToolResultBody::Error {
                message: message.clone(),
            };
            $record.llm_text = message;
            $record.duration_ms = started.elapsed().as_millis() as u64;
            emit_tool_result(
                sink,
                &turn_id,
                &call.id,
                false,
                $record.duration_ms,
                $record.ui.clone(),
            );
        }};
    }

    let Some(tool) = registry.get(&call.function.name) else {
        fail!(record, registry.unknown_tool_message(&call.function.name));
        return (record, None);
    };

    // 参数非法不是致命错误：把解析失败原样告诉模型，它通常能自己改对
    let args = match serde_json::from_str::<serde_json::Value>(&call.function.arguments) {
        Ok(v) => v,
        Err(e) => {
            fail!(
                record,
                format!(
                    "工具 `{}` 的参数不是合法 JSON：{e}。收到的内容：{}。请重新生成完整的参数。",
                    call.function.name, call.function.arguments
                )
            );
            return (record, None);
        }
    };

    record.args = args.clone();
    record.preview = tool.preview(&args);

    sink.emit(AgentEvent::ToolCallReady {
        turn_id: turn_id.clone(),
        call_id: call.id.clone(),
        name: call.function.name.clone(),
        preview: record.preview.clone(),
        args: args.clone(),
    });

    // 每次调用派生独立的上下文：进度上报器要绑定到本次调用的卡片
    let ctx = ToolCtx {
        workspace_root: env.workspace_root.clone(),
        cancel: cancel.clone(),
        progress: ProgressReporter::new(sink.raw(), turn_id.clone(), call.id.clone()),
    };

    match tool.execute(args, &ctx).await {
        Ok(outcome) => {
            record.ok = true;
            record.llm_text = outcome.llm_text;
            record.ui = outcome.ui;
            record.duration_ms = started.elapsed().as_millis() as u64;
            emit_tool_result(
                sink,
                &turn_id,
                &call.id,
                true,
                record.duration_ms,
                record.ui.clone(),
            );
            (record, None)
        }
        Err(error) => {
            let fatal = error.is_fatal().then(|| error.to_string());
            fail!(record, error.to_string());
            (record, fatal)
        }
    }
}

fn emit_tool_result(
    sink: &ThrottledSink,
    turn_id: &str,
    call_id: &str,
    ok: bool,
    duration_ms: u64,
    result: ToolResultBody,
) {
    sink.emit(AgentEvent::ToolResult {
        turn_id: turn_id.to_string(),
        call_id: call_id.to_string(),
        ok,
        duration_ms,
        result,
    });
}

fn finish(
    sink: &mut ThrottledSink,
    started: Instant,
    usage: UsageTally,
    config: &LlmConfig,
    outcome: TurnOutcome,
) -> TurnOutcome {
    sink.flush();

    let elapsed = started.elapsed();
    if usage.reported {
        let secs = elapsed.as_secs_f64();
        sink.emit(AgentEvent::Usage {
            turn_id: sink.turn_id().to_string(),
            prompt_tokens: usage.prompt,
            completion_tokens: usage.completion,
            total_tokens: usage.total,
            context_used: usage.last_prompt,
            context_limit: config.context_limit,
            elapsed_ms: elapsed.as_millis() as u64,
            tps: if secs > 0.0 {
                f64::from(usage.completion) / secs
            } else {
                0.0
            },
        });
    }

    sink.emit(AgentEvent::TurnEnd {
        turn_id: sink.turn_id().to_string(),
        finish_reason: outcome.finish_reason.clone(),
        elapsed_ms: elapsed.as_millis() as u64,
    });
    outcome
}

fn emit_llm_error(sink: &ThrottledSink, error: &Error) {
    sink.emit(AgentEvent::Error {
        turn_id: sink.turn_id().to_string(),
        code: error.code().to_string(),
        message: error.to_string(),
        retryable: error.retryable(),
    });
}

fn emit_error(sink: &ThrottledSink, code: &str, message: &str, retryable: bool) {
    sink.emit(AgentEvent::Error {
        turn_id: sink.turn_id().to_string(),
        code: code.to_string(),
        message: message.to_string(),
        retryable,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::Usage;

    fn usage(prompt: u32, completion: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        }
    }

    /// 累计值用于成本，最后一次的 prompt 用于上下文占用 —— 两者语义不同。
    #[test]
    fn tally_separates_cumulative_from_context() {
        let mut tally = UsageTally::default();
        tally.add(&usage(1000, 50));
        tally.add(&usage(1600, 40));
        tally.add(&usage(2300, 30));

        assert_eq!(tally.prompt, 4900, "累计输入应当把每次请求都算上");
        assert_eq!(tally.completion, 120);
        assert_eq!(
            tally.last_prompt, 2300,
            "上下文占用应当只看最后一次请求，否则占比会超过 100%"
        );
    }

    #[test]
    fn tally_starts_unreported() {
        let tally = UsageTally::default();
        assert!(
            !tally.reported,
            "没收到 usage 时不该上报，显示 0 会误导用户"
        );

        let mut tally = tally;
        tally.add(&usage(10, 10));
        assert!(tally.reported);
    }
}

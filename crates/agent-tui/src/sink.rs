//! 把 core 的事件出口接到 TUI 主循环的 mpsc channel 上。
//!
//! 与 Tauri 版的 `ChannelSink` 同构：agent 跑在独立 task 里，
//! 通过这条 channel 把 `AgentEvent` 送回主循环更新界面。

use agent_core::{AgentEvent, EventSink};
use tokio::sync::mpsc;

/// 主循环从 channel 收到的消息。
///
/// `TurnFinished` 不是 `AgentEvent` —— 它是任务级信号：轮次已结束、
/// 会话已写回。UI 据此把状态从「忙碌」切回「可输入」，并取回会话。
#[derive(Debug)]
pub enum UiMessage {
    Agent(AgentEvent),
    /// 轮次彻底结束，送回最终会话。
    TurnFinished {
        turn_id: String,
        session: agent_core::Session,
    },
}

/// 把 `AgentEvent` 转发到主循环的 sink。
pub struct ChannelSink {
    tx: mpsc::UnboundedSender<UiMessage>,
}

impl ChannelSink {
    pub fn new(tx: mpsc::UnboundedSender<UiMessage>) -> Self {
        Self { tx }
    }
}

impl EventSink for ChannelSink {
    fn emit(&self, event: AgentEvent) {
        // 发送失败只记日志：主循环关闭时（退出中）channel 断开，
        // 此时丢弃事件没有意义 —— 进程都要结束了。
        if let Err(e) = self.tx.send(UiMessage::Agent(event)) {
            tracing::debug!("事件推送失败（主循环可能已关闭）: {e}");
        }
    }
}

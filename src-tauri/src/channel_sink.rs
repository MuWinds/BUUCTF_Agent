//! 把 core 的事件出口接到 Tauri 的 IPC channel 上。

use agent_core::{AgentEvent, EventSink};
use tauri::ipc::Channel;

/// 用 Tauri v2 的 `Channel` 而非全局 `emit`：
/// Channel 天然按调用隔离，前端无需按事件名路由，也不必管理 unlisten。
pub struct ChannelSink(pub Channel<AgentEvent>);

impl EventSink for ChannelSink {
    fn emit(&self, event: AgentEvent) {
        // 发送失败只记日志：前端窗口关闭时 channel 会断开，此时中止整轮没有意义
        // —— 调用方通过 cancel token 感知取消，而不是靠这里的错误。
        if let Err(e) = self.0.send(event) {
            tracing::debug!("事件推送失败（前端可能已关闭）: {e}");
        }
    }
}

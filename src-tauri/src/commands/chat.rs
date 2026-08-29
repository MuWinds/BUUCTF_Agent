//! 对话相关命令。

use agent_core::{turn, AgentEvent, Error, Result, Session, ThrottledSink, ToolEnv};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::channel_sink::ChannelSink;
use crate::state::{AppState, SYSTEM_PROMPT};

/// 发送一条用户消息并驱动一个轮次。
///
/// 该命令直到轮次结束才返回，期间所有输出通过 `on_event` 推送。
/// `cancel_turn` 是独立命令，Tauri 并发处理命令，因此取消能在本命令
/// 仍在执行时进来。
#[tauri::command]
pub async fn send_message(
    state: State<'_, AppState>,
    text: String,
    on_event: Channel<AgentEvent>,
) -> Result<()> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(Error::Config("消息不能为空".into()));
    }

    let config = {
        let config = state.config.read().await;
        config.validate().map_err(Error::Config)?;
        config.clone()
    };

    tracing::info!(model = %config.model, endpoint = %config.endpoint(), "开始轮次");

    // 同一时刻只允许一个轮次
    state.cancel_active().await;

    // 取出会话独占使用：轮次期间要持续往里追加内容
    let mut session = {
        let mut guard = state.session.lock().await;
        if guard.is_empty() {
            guard.push_system(SYSTEM_PROMPT);
        }
        guard.push_user(&text);
        guard.clone()
    };

    let cancel = CancellationToken::new();
    *state.active_turn.lock().await = Some(cancel.clone());

    let env = ToolEnv {
        workspace_root: state.workspace_root.read().await.clone(),
    };

    let turn_id = uuid::Uuid::new_v4().to_string();
    let mut sink = ThrottledSink::new(Arc::new(ChannelSink(on_event)), turn_id);

    let outcome = turn::run(
        &state.client,
        &config,
        &mut session,
        &state.registry,
        &env,
        &mut sink,
        cancel,
    )
    .await;

    tracing::info!(
        finish_reason = %outcome.finish_reason,
        entries = session.entries.len(),
        "轮次结束"
    );

    // 写回并落盘。被取消时同样要保留 —— 用户看得见的内容，模型下一轮也该看得见。
    *state.session.lock().await = session;
    *state.active_turn.lock().await = None;
    state.persist().await;

    Ok(())
}

/// 读取已保存的会话，供前端启动时还原界面。
#[tauri::command]
pub async fn get_session(state: State<'_, AppState>) -> Result<Session> {
    Ok(state.session.lock().await.clone())
}

/// 中止当前轮次。已生成的部分会保留在会话中。
#[tauri::command]
pub async fn cancel_turn(state: State<'_, AppState>) -> Result<()> {
    state.cancel_active().await;
    Ok(())
}

/// 清空会话，开始新对话。
#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> Result<()> {
    state.cancel_active().await;
    state.session.lock().await.clear();
    state.persist().await;
    Ok(())
}

//! 对话相关命令。

use agent_core::{compact, turn, AgentEvent, Error, Result, Session, ThrottledSink, ToolEnv};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::channel_sink::ChannelSink;
use crate::state::{ActiveTurn, AppState, DEFAULT_SESSION_ID};
use agent_host::persist::{self, SessionList};
use agent_host::system_prompt;

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

    // 旧行为：新消息到来时中止进行中的轮次。随后再拿串行锁，
    // 等被中止的轮次真正写回会话后才继续 —— 不会读到半成品。
    state.cancel_active().await;
    let _turn_guard = state.turn_gate.lock().await;

    // 先取工作区：系统提示词要读它及其父目录的 AGENTS.md / CLAUDE.md
    let workspace_root = state.workspace_root.read().await.clone();

    // 取出会话独占使用：轮次期间要持续往里追加内容
    let mut session = {
        let mut guard = state.session.lock().await;
        if guard.is_empty() {
            guard.push_system(system_prompt(&workspace_root, &state.registry));
        }
        guard.push_user(&text);
        guard.clone()
    };

    let turn_id = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    let preempt = Arc::new(AtomicBool::new(false));
    *state.active_turn.lock().await = Some(ActiveTurn {
        turn_id: turn_id.clone(),
        cancel: cancel.clone(),
        preempt: preempt.clone(),
    });

    let env = ToolEnv { workspace_root };

    let mut sink = ThrottledSink::new(Arc::new(ChannelSink(on_event)), turn_id.clone());

    // 长对话自动压缩：历史接近窗口上限时，先把最老的一段折叠成摘要，
    // 再开始正式轮次 —— 否则请求本身就可能放不进窗口。压缩失败不阻断
    // 对话（记日志继续），它只是保命措施。
    match compact::maybe_compact(&state.client, &config, &mut session).await {
        Ok(Some(compaction)) => {
            tracing::info!(removed = compaction.removed_entries, "已自动压缩上下文");
            sink.emit(AgentEvent::ContextCompacted {
                turn_id: turn_id.clone(),
                removed_entries: compaction.removed_entries,
                summary: compaction.summary,
            });
            // 压缩结果立即写回并落盘 —— UI 恢复时看到的会话必须与
            // 模型实际读到的上下文一致，否则两边各说各话。
            *state.session.lock().await = session.clone();
            state.persist().await;
        }
        Ok(None) => {}
        Err(e) => tracing::warn!("自动压缩失败，继续原样对话：{e}"),
    }

    let outcome = turn::run(
        &state.client,
        &config,
        &mut session,
        &state.registry,
        &env,
        &mut sink,
        cancel,
        &preempt,
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

/// 列出当前工作区下的全部会话摘要。
#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<SessionList> {
    let workspace = state.workspace_root.read().await.clone();
    let sessions = persist::list(&state.app_data, &workspace).await;
    let current_id = state.session_id.read().await.clone();
    Ok(SessionList {
        current_id,
        sessions,
    })
}

/// 切换到指定会话。会话属于当前工作区，切到别的会话前请先切换工作区。
#[tauri::command]
pub async fn switch_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let workspace = state.workspace_root.read().await.clone();
    let session = persist::load(&state.app_data, &workspace, &id)
        .await
        .ok_or_else(|| Error::Config(format!("会话 {id} 不存在")))?;

    state.cancel_active().await;
    let _turn_guard = state.turn_gate.lock().await;
    *state.session.lock().await = session;
    *state.session_id.write().await = id;
    // 会话随工作区走，读取记录同样是工作区级的，切换会话不需要清它
    Ok(())
}

/// 新建一个空会话并切换到它。旧的会话继续留在磁盘上。
#[tauri::command]
pub async fn new_session(state: State<'_, AppState>) -> Result<()> {
    state.cancel_active().await;
    let _turn_guard = state.turn_gate.lock().await;
    *state.session.lock().await = Session::default();
    *state.session_id.write().await = persist::new_id();
    Ok(())
}

/// 删除指定会话文件。删除当前会话时清空界面并退回默认会话。
#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<()> {
    let workspace = state.workspace_root.read().await.clone();
    persist::delete(&state.app_data, &workspace, &id)
        .await
        .map_err(|e| Error::Internal(format!("删除会话失败：{e}")))?;

    let current = state.session_id.read().await.clone();
    if current == id {
        state.cancel_active().await;
        let _turn_guard = state.turn_gate.lock().await;
        *state.session.lock().await = Session::default();
        *state.session_id.write().await = DEFAULT_SESSION_ID.to_string();
    }
    Ok(())
}

/// 中止当前轮次。已生成的部分会保留在会话中。
#[tauri::command]
pub async fn cancel_turn(state: State<'_, AppState>) -> Result<()> {
    state.cancel_active().await;
    Ok(())
}

/// 通知当前轮次「插队」：在当前 tool call 结束后停下，把位置让给新消息。
///
/// 与 `cancel_turn` 不同，它不打断正在执行的工具，只阻止轮次继续进入
/// 下一轮请求。排队/插队的消息由前端在轮次结束后重新触发 `send_message`。
/// `turn_id` 由前端从 `turn_start` 事件里取回，防止迟到信号误伤下一轮。
#[tauri::command]
pub async fn preempt_turn(state: State<'_, AppState>, turn_id: String) -> Result<()> {
    state.preempt_active(&turn_id).await;
    Ok(())
}

/// 回退/分叉：把当前会话截断到第 `entry_index` 条消息（含），
/// 丢弃之后的所有内容，然后从这里重新开始。
///
/// 被截断掉的部分不会就此消失 —— 截断前完整会话已另存为新 id，
/// 旧分支保留在会话列表里，随时可以切回去。这也正是「分叉」的语义：
/// 从同一个起点走出两条不同的路，而不是把走过的路抹掉。
///
/// `entry_index` 是 `Session.entries` 的绝对索引（含开头的 system），
/// 前端在渲染消息时记录每条消息对应的索引。指向最后一条或越界时
/// 返回错误 —— 没有可丢弃的内容，回退没有意义。
#[tauri::command]
pub async fn rewind_session(state: State<'_, AppState>, entry_index: usize) -> Result<SessionList> {
    let workspace = state.workspace_root.read().await.clone();
    let model = state.config.read().await.model.clone();
    let session_id = state.session_id.read().await.clone();

    let mut full = state.session.lock().await.clone();
    if !full.truncate_to(entry_index) {
        return Err(Error::Config(
            "回退点无效：指向最后一条消息或已超出范围".into(),
        ));
    }

    // 截断前先把完整会话另存为新 id —— 旧分支不能丢
    persist::save(
        &state.app_data,
        &workspace,
        &model,
        &persist::new_id(),
        &state.session.lock().await.clone(),
    )
    .await
    .map_err(|e| Error::Internal(format!("保存旧分支失败：{e}")))?;

    state.cancel_active().await;
    let _turn_guard = state.turn_gate.lock().await;
    *state.session.lock().await = full;
    // 当前会话 id 不变 —— 用户还在同一段历史里，只是倒回去重走
    state.persist().await;

    let sessions = persist::list(&state.app_data, &workspace).await;
    Ok(SessionList {
        current_id: session_id,
        sessions,
    })
}

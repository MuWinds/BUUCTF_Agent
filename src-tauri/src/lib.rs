//! 应用装配入口。
//!
//! 这一层只做三件事：把 core 的能力接到 Tauri 的 command/channel 上、
//! 持有会话状态、初始化日志。业务逻辑在 `agent-core`，工具与存储等
//! 共享实现在 `agent-host`。

// MSVC 链接器在中文系统上会往 stdout 打一行「正在创建库…」，
// Rust 1.98 的 linker_messages lint 会把它当成警告。纯环境噪音，
// 留着只会淹没真正的警告。
#![allow(linker_messages)]

mod channel_sink;
mod commands;
mod state;
// 对外可见仅为了让 `tests/e2e.rs` 能装配一份真实的工具注册表。
// 端到端测试若改用桩工具就失去意义，而工具实现按架构约定留在宿主层。
pub use agent_host::tools;

/// 启动 Tauri 应用。
///
/// # Panics
///
/// 当状态初始化或 Tauri 运行时启动失败时 panic —— 此时应用无法继续，
/// 没有可恢复的路径。
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;

            let app_data = app.path().app_data_dir()?;
            let state = state::AppState::new(app_data.clone())?;

            // 恢复当前工作区最近一次会话（并迁移旧版单一会话文件）。
            // 放在 setup 里同步等待，避免前端在会话加载完成前就把界面画成空的。
            tauri::async_runtime::block_on(async {
                let workspace = state.workspace_root.read().await.clone();
                let model = state.config.read().await.model.clone();
                let (session, session_id) =
                    agent_host::persist::bootstrap(&app_data, &workspace, &model).await;
                *state.session.lock().await = session;
                *state.session_id.write().await = session_id;
            });

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::chat::send_message,
            commands::chat::cancel_turn,
            commands::chat::preempt_turn,
            commands::chat::rewind_session,
            commands::chat::get_session,
            commands::chat::list_sessions,
            commands::chat::switch_session,
            commands::chat::new_session,
            commands::chat::delete_session,
            commands::config::get_llm_config,
            commands::config::set_llm_config,
            commands::config::clear_api_key,
            commands::config::test_connection,
            commands::workspace::get_workspace,
            commands::workspace::set_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}

/// 初始化日志。可通过环境变量 `CODING_AGENT_LOG` 调整级别，默认 `info`。
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_env("CODING_AGENT_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,coding_agent_lib=debug,agent_core=debug"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
